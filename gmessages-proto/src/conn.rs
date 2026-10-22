//! HTTP layer: hosts, headers, cookies, encodings, errors, configuration, token refresh
//! (spec §1 and §3).

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::codec::{self, Proto};
use crate::config::Config;
use crate::crypto;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::session::{Session, now_us};
use crate::wire;

pub(crate) const API: &str = "https://instantmessaging-pa.googleapis.com";
pub(crate) const ACCOUNT: &str = "https://instantmessaging-pa.clients6.google.com";
pub(crate) const WEB_CONFIG: &str = "https://messages.google.com/web/config";
pub(crate) const UPLOAD: &str = "https://instantmessaging-pa.googleapis.com/upload";

pub(crate) const NET_BUGLE: &str = "Bugle";
pub(crate) const NET_GDITTO: &str = "GDitto";

/// Builds the RPC URL of a service method.
pub(crate) fn rpc_url(host: &str, service: &str, method: &str) -> String {
    format!("{host}/$rpc/google.internal.communications.instantmessaging.v1.{service}/{method}")
}

/// Body encoding of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Binary,
    PbLite,
}

impl Encoding {
    fn content_type(self) -> &'static str {
        match self {
            Encoding::Binary => "application/x-protobuf",
            Encoding::PbLite => "application/json+protobuf",
        }
    }
}

/// Which header set to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderSet {
    /// RPC calls.
    Rpc,
    /// `GET /web/config`.
    Config,
    /// Upload service and plain downloads: no api key, no x-user-agent, no cookies.
    Media,
}

/// Decodes a body by its content type: binary or pblite.
pub(crate) fn decode_body<M: Proto>(content_type: &str, body: &[u8]) -> Result<M> {
    let ct = content_type.to_ascii_lowercase();
    let pblite = if ct.starts_with("application/x-protobuf") {
        false
    } else if ct.starts_with("application/json") || ct.starts_with("text/plain") {
        true
    } else {
        body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[')
    };
    if pblite {
        Ok(codec::from_pblite_slice(body)?)
    } else {
        Ok(codec::from_binary(body)?)
    }
}

/// Parses a `Set-Cookie` value: `Some((name, Some(value)))` to set,
/// `Some((name, None))` to delete.
pub(crate) fn parse_set_cookie(v: &str) -> Option<(String, Option<String>)> {
    let mut parts = v.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut delete = false;
    for attr in parts {
        let (k, val) = attr.split_once('=').unwrap_or((attr, ""));
        if k.trim().eq_ignore_ascii_case("max-age")
            && val.trim().parse::<i64>().is_ok_and(|n| n <= 0)
        {
            delete = true;
        }
    }
    Some((
        name.to_string(),
        if delete {
            None
        } else {
            Some(value.trim().to_string())
        },
    ))
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The HTTP side of a client: configuration, session, reqwest client.
pub(crate) struct Conn {
    http: reqwest::Client,
    pub config: Config,
    session: Mutex<Session>,
    events: Option<UnboundedSender<Event>>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl Conn {
    pub fn new(
        config: Config,
        session: Session,
        events: Option<UnboundedSender<Event>>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()?;
        Ok(Conn {
            http,
            config,
            session: Mutex::new(session),
            events,
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Session {
        self.session().clone()
    }

    /// Reports a session change to the application.
    pub fn session_changed(&self) {
        if let Some(tx) = &self.events {
            let _ = tx.send(Event::SessionChanged(Box::new(self.snapshot())));
        }
    }

    pub fn is_account(&self) -> bool {
        self.session().is_google_account()
    }

    /// Relay host: `ACCOUNT` for Google account sessions, `API` otherwise.
    pub fn relay_host(&self) -> &'static str {
        if self.is_account() { ACCOUNT } else { API }
    }

    /// `network` of credentials for non-pairing requests.
    pub fn network(&self) -> &'static str {
        if self.is_account() { NET_GDITTO } else { "" }
    }

    pub fn version(&self) -> wire::ClientVersion {
        let (y, m, d) = self.config.client_version;
        wire::ClientVersion {
            year: y,
            month: m,
            day: d,
            minor_a: 4,
            minor_b: 6,
        }
    }

    pub fn browser_details(&self) -> wire::BrowserDetails {
        wire::BrowserDetails {
            user_agent: self.config.user_agent.clone(),
            browser: self.config.browser_kind,
            os: self.config.device_name.clone(),
            device: self.config.device_kind,
        }
    }

    /// Credentials with the given request id and network, and the current token if any.
    pub fn credentials(&self, request_id: impl Into<String>, network: &str) -> wire::Credentials {
        wire::Credentials {
            request_id: request_id.into(),
            network: network.to_string(),
            token: self.session().token.clone(),
            version: Some(self.version()),
        }
    }

    pub fn headers(&self, set: HeaderSet, content_type: Option<&str>) -> HeaderMap {
        let c = &self.config;
        let mut h = HeaderMap::new();
        let mut put = |k: &'static str, v: &str| {
            if let Ok(v) = HeaderValue::from_str(v) {
                h.insert(HeaderName::from_static(k), v);
            }
        };
        if let Some(ct) = content_type {
            put("content-type", ct);
        }
        put("accept", "*/*");
        if set == HeaderSet::Rpc || set == HeaderSet::Config {
            put("x-goog-api-key", &c.api_key);
        }
        if set == HeaderSet::Rpc {
            put("x-user-agent", "grpc-web-javascript/0.1");
        }
        put("user-agent", &c.user_agent);
        put("sec-ch-ua", &c.sec_ch_ua);
        put("sec-ch-ua-mobile", &c.sec_ch_ua_mobile);
        put("sec-ch-ua-platform", &c.sec_ch_ua_platform);
        if set != HeaderSet::Config {
            put("origin", "https://messages.google.com");
        }
        put("referer", "https://messages.google.com/");
        put(
            "sec-fetch-site",
            if set == HeaderSet::Config {
                "same-origin"
            } else {
                "cross-site"
            },
        );
        put("sec-fetch-mode", "cors");
        put("sec-fetch-dest", "empty");
        put("accept-language", "en-US,en;q=0.9");
        if set != HeaderSet::Media {
            self.add_cookie_headers(&mut h);
        }
        h
    }

    /// `Cookie` and `Authorization: SAPISIDHASH` when the session holds cookies.
    fn add_cookie_headers(&self, h: &mut HeaderMap) {
        let s = self.session();
        if s.cookies.is_empty() {
            return;
        }
        let cookie = s
            .cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            h.insert(reqwest::header::COOKIE, v);
        }
        if let Some(sapisid) = s.cookies.get("SAPISID")
            && let Ok(v) = HeaderValue::from_str(&crypto::sapisid_hash(unix_secs(), sapisid))
        {
            h.insert(reqwest::header::AUTHORIZATION, v);
        }
    }

    /// Applies `Set-Cookie` headers of a response to the held cookies (only when the
    /// session holds cookies, i.e. Google account sessions).
    pub fn apply_set_cookies(&self, headers: &HeaderMap) {
        let mut changed = false;
        {
            let mut s = self.session();
            if s.cookies.is_empty() {
                return;
            }
            for v in headers.get_all(reqwest::header::SET_COOKIE) {
                let Some((name, value)) = v.to_str().ok().and_then(parse_set_cookie) else {
                    continue;
                };
                match value {
                    Some(v) => {
                        if s.cookies.get(&name) != Some(&v) {
                            s.cookies.insert(name, v);
                            changed = true;
                        }
                    }
                    None => changed |= s.cookies.remove(&name).is_some(),
                }
            }
        }
        if changed {
            debug!("cookies updated");
            self.session_changed();
        }
    }

    /// Turns a non-2xx response into an error.
    pub async fn error_from(&self, resp: reqwest::Response) -> Error {
        let status = resp.status().as_u16();
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.bytes().await.unwrap_or_default();
        match decode_body::<wire::RpcError>(&ct, &body) {
            Ok(e) if e.code != 0 || !e.message.is_empty() => Error::Server {
                status,
                code: Some(e.code),
                message: e.message,
                details: e.details.into_iter().map(|d| d.type_url).collect(),
            },
            _ => Error::Server {
                status,
                code: None,
                message: String::from_utf8_lossy(&body).chars().take(300).collect(),
                details: Vec::new(),
            },
        }
    }

    /// Sends a request; 5xx answers are retried (3 attempts in all, waiting 1 s then 2 s)
    /// when `retry` is set. Returns the successful response.
    pub async fn send(
        &self,
        rb: reqwest::RequestBuilder,
        retry: bool,
    ) -> Result<reqwest::Response> {
        let attempts = if retry { 3 } else { 1 };
        let mut attempt = 0;
        loop {
            attempt += 1;
            let req = rb
                .try_clone()
                .ok_or_else(|| Error::InvalidInput("request not clonable".into()))?;
            let resp = req.send().await?;
            self.apply_set_cookies(resp.headers());
            if resp.status().is_success() {
                return Ok(resp);
            }
            let err = self.error_from(resp).await;
            let server_err = matches!(&err, Error::Server { status, .. } if *status >= 500);
            if server_err && attempt < attempts {
                warn!(%err, attempt, "server error, retrying");
                tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
                continue;
            }
            return Err(err);
        }
    }

    /// Posts an RPC and decodes its reply.
    pub async fn rpc<Req: Proto, Resp: Proto>(
        &self,
        host: &str,
        service: &str,
        method: &str,
        enc: Encoding,
        req: &Req,
    ) -> Result<Resp> {
        let body = match enc {
            Encoding::Binary => codec::to_binary(req),
            Encoding::PbLite => codec::to_pblite_string(req).into_bytes(),
        };
        let rb = self
            .http
            .post(rpc_url(host, service, method))
            .headers(self.headers(HeaderSet::Rpc, Some(enc.content_type())))
            .body(body);
        let resp = self.send(rb, true).await?;
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await?;
        if bytes.is_empty() {
            return Ok(Resp::default());
        }
        decode_body(&ct, &bytes)
    }

    /// Posts an RPC whose reply body carries nothing useful (calls, acks).
    pub async fn post_rpc<Req: Proto>(
        &self,
        host: &str,
        service: &str,
        method: &str,
        enc: Encoding,
        req: &Req,
    ) -> Result<()> {
        let body = match enc {
            Encoding::Binary => codec::to_binary(req),
            Encoding::PbLite => codec::to_pblite_string(req).into_bytes(),
        };
        let rb = self
            .http
            .post(rpc_url(host, service, method))
            .headers(self.headers(HeaderSet::Rpc, Some(enc.content_type())))
            .body(body);
        let resp = self.send(rb, true).await?;
        let _ = resp.bytes().await;
        Ok(())
    }

    /// Opens the event stream; returns the streaming response.
    pub async fn open_stream(&self, stream_id: &str) -> Result<reqwest::Response> {
        let req = wire::ReceiveMessagesRequest {
            credentials: Some(self.credentials(stream_id, self.network())),
            unknown4: Some(wire::ReceiveUnknown {
                empty: Some(wire::Empty {}),
            }),
        };
        let rb = self
            .http
            .post(rpc_url(self.relay_host(), "Messaging", "ReceiveMessages"))
            .headers(self.headers(HeaderSet::Rpc, Some(Encoding::PbLite.content_type())))
            .body(codec::to_pblite_string(&req));
        self.send(rb, false).await
    }

    /// `GET /web/config`.
    pub async fn fetch_config(&self) -> Result<wire::WebConfig> {
        let rb = self
            .http
            .get(WEB_CONFIG)
            .headers(self.headers(HeaderSet::Config, None));
        let resp = self.send(rb, true).await?;
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.bytes().await?;
        // The configuration is pblite whatever its content type.
        match codec::from_pblite_slice(&body) {
            Ok(c) => Ok(c),
            Err(_) => decode_body(&ct, &body),
        }
    }

    /// Builds the token refresh request (exposed for tests).
    pub fn refresh_request(
        &self,
        request_id: &str,
        timestamp: i64,
    ) -> wire::RegisterRefreshRequest {
        let s = self.session();
        let sig = s
            .refresh_key
            .sign(crypto::refresh_signature_input(request_id, timestamp).as_bytes());
        wire::RegisterRefreshRequest {
            credentials: Some(wire::Credentials {
                request_id: request_id.to_string(),
                network: if s.is_google_account() {
                    NET_GDITTO.into()
                } else {
                    String::new()
                },
                token: s.token.clone(),
                version: Some(self.version()),
            }),
            browser: Some((&s.browser).into()),
            timestamp,
            signature: sig,
            options: Some(wire::RefreshOptions {
                always: Some(wire::Empty {}),
                push: None,
            }),
            unknown16: 2,
        }
    }

    /// Refreshes the relay token if it expires within an hour (or if `force`).
    pub async fn ensure_token(&self, force: bool) -> Result<()> {
        let _g = self.refresh_lock.lock().await;
        let left = self.session().token_expiry_us - now_us();
        if !force && left > 3600 * 1_000_000 {
            return Ok(());
        }
        debug!(left_s = left / 1_000_000, "refreshing relay token");
        let rid = uuid::Uuid::new_v4().to_string();
        let ts = (now_us() / 1000) * 1000;
        let req = self.refresh_request(&rid, ts);
        let res: Result<wire::RegisterRefreshResponse> = self
            .rpc(
                ACCOUNT,
                "Registration",
                "RegisterRefresh",
                Encoding::PbLite,
                &req,
            )
            .await;
        match res {
            Ok(r) => {
                let Some(t) = r.token.filter(|t| !t.token.is_empty()) else {
                    return Err(Error::Decode("token refresh reply without token".into()));
                };
                self.session().set_token(&t);
                self.session_changed();
                Ok(())
            }
            Err(e) => {
                let gone = e.is_session_gone()
                    || e.is_http_status(401)
                    || e.is_http_status(403)
                    || e.is_http_status(404)
                    || matches!(&e, Error::Server { code: Some(5), .. });
                if gone {
                    Err(Error::SessionGone(e.to_string()))
                } else {
                    Err(e)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::RefreshKey;

    #[test]
    fn set_cookie_parsing() {
        assert_eq!(
            parse_set_cookie("OSID=abc; Path=/; Secure; HttpOnly"),
            Some(("OSID".into(), Some("abc".into())))
        );
        assert_eq!(
            parse_set_cookie("X=; Max-Age=0; Path=/"),
            Some(("X".into(), None))
        );
        assert_eq!(parse_set_cookie("garbage"), None);
    }

    #[test]
    fn urls() {
        assert_eq!(
            rpc_url(API, "Messaging", "SendMessage"),
            "https://instantmessaging-pa.googleapis.com/$rpc/google.internal.communications.instantmessaging.v1.Messaging/SendMessage"
        );
    }

    #[test]
    fn headers_and_cookies() {
        let mut s = Session::blank(RefreshKey::generate());
        let conn = Conn::new(Config::default(), s.clone(), None).unwrap();
        let h = conn.headers(HeaderSet::Rpc, Some("application/json+protobuf"));
        assert_eq!(h["x-user-agent"], "grpc-web-javascript/0.1");
        assert!(h.get("cookie").is_none());
        let h = conn.headers(HeaderSet::Config, None);
        assert!(h.get("x-user-agent").is_none() && h.get("origin").is_none());
        assert_eq!(h["sec-fetch-site"], "same-origin");
        let h = conn.headers(HeaderSet::Media, None);
        assert!(h.get("x-goog-api-key").is_none());

        s.cookies.insert("SAPISID".into(), "sap".into());
        s.cookies.insert("SID".into(), "sid".into());
        let conn = Conn::new(Config::default(), s, None).unwrap();
        let h = conn.headers(HeaderSet::Rpc, None);
        assert_eq!(h["cookie"], "SAPISID=sap; SID=sid");
        assert!(
            h["authorization"]
                .to_str()
                .unwrap()
                .starts_with("SAPISIDHASH ")
        );
        let h = conn.headers(HeaderSet::Media, None);
        assert!(h.get("cookie").is_none());
    }

    #[test]
    fn refresh_request_signature() {
        use p256::ecdsa::signature::Verifier;
        let mut s = Session::blank(RefreshKey::generate());
        s.token = vec![1, 2, 3];
        let key = s.refresh_key.clone();
        let conn = Conn::new(Config::default(), s, None).unwrap();
        let rid = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let ts = 1_700_000_000_123_000;
        let r = conn.refresh_request(rid, ts);
        assert_eq!(r.timestamp, ts);
        assert_eq!(r.unknown16, 2);
        assert!(r.options.as_ref().unwrap().always.is_some());
        let sig = p256::ecdsa::Signature::from_der(&r.signature).unwrap();
        key.verifying_key()
            .verify(format!("{rid}:{ts}").as_bytes(), &sig)
            .unwrap();
        // pblite shape: credentials first, timestamp at index 2, options at 12, 2 at 15
        let j = r.to_pblite_for_test();
        let a = j.as_array().unwrap();
        assert_eq!(a.len(), 16);
        assert_eq!(a[0][0], rid);
        assert_eq!(a[2], ts);
        assert_eq!(a[12].as_array().unwrap().len(), 23);
        assert_eq!(a[12][8], serde_json::json!([]));
        assert_eq!(a[15], 2);
    }

    trait ForTest {
        fn to_pblite_for_test(&self) -> serde_json::Value;
    }
    impl<T: crate::codec::PbLite> ForTest for T {
        fn to_pblite_for_test(&self) -> serde_json::Value {
            self.to_pblite()
        }
    }
}
