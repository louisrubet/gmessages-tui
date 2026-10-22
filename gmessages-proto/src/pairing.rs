//! Pairing (spec §4): QR code, Google account, unpairing.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::{Notify, mpsc};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::codec::{self, B64};
use crate::config::Config;
use crate::conn::{ACCOUNT, API, Conn, Encoding, NET_BUGLE, NET_GDITTO};
use crate::crypto::{RefreshKey, RequestKeys};
use crate::emoji;
use crate::error::{Error, PairingError, Result};
use crate::event::Event;
use crate::relay::{CallOpts, Mode, PAIRING_TTL_US, Relay, Ttl, action};
use crate::session::{Device, Session, now_us};
use crate::types::PairingErrorCode;
use crate::ukey2::{self, Ukey2Client};
use crate::wire;

/// Interval between QR codes.
pub const QR_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
/// Number of QR codes shown before giving up.
pub const QR_MAX_CODES: usize = 6;
/// Delay after pairing before connecting, so the phone saves the pairing first.
pub const POST_PAIRING_DELAY: Duration = Duration::from_secs(2);
const STATEMENT: &str = "This is an undocumented API. Use or access of undocumented Google APIs without express authorization is prohibited per the Google API Terms of Service (https://developers.google.com/terms).";

/// Cancels a pairing in progress. Clone it, give one copy to the pairing function, call
/// [`cancel`](Self::cancel) on another.
#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<CancelInner>);

#[derive(Default, Debug)]
struct CancelInner {
    flag: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    /// A new, not cancelled token.
    pub fn new() -> Self {
        Self::default()
    }
    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.flag.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.flag.load(Ordering::SeqCst)
    }
    /// Resolves once cancellation is requested.
    pub async fn cancelled(&self) {
        let fut = self.0.notify.notified();
        tokio::pin!(fut);
        fut.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        fut.await
    }
}

/// The emoji to show during Google account pairing; the user taps the same one on the
/// phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmojiPrompt {
    /// The emoji.
    pub emoji: &'static str,
    /// Index value derived from the handshake (before `mod len`).
    pub index_value: u32,
    /// Emoji list version confirmed by the phone.
    pub emoji_version: i32,
    /// URL of an SVG image of the emoji (Noto), for terminals that cannot render it.
    pub image_url: String,
}

/// Options of Google account pairing.
#[derive(Debug, Clone, Default)]
pub struct AccountPairingOptions {
    /// Which phone of the account to pair with, by recency of activity (0: the most
    /// recently active). Use 1, 2… to try the next one if the chosen phone does not answer.
    pub phone_index: usize,
}

/// Builds the URL to render as a QR code.
pub fn qr_url(pairing_key: &[u8], keys: &RequestKeys) -> String {
    let data = wire::QrCodeData {
        pairing_key: pairing_key.to_vec(),
        aes_key: keys.aes.to_vec(),
        hmac_key: keys.hmac.to_vec(),
    };
    format!(
        "https://support.google.com/messages/?p=web_computer#?c={}",
        B64.encode(codec::to_binary(&data))
    )
}

struct RelayGuard(Arc<Relay>);
impl Drop for RelayGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn fatal_of(ev: Option<Event>) -> Option<Error> {
    match ev {
        Some(Event::LoggedOut(r)) => Some(Error::SessionGone(format!("{r:?}"))),
        None => Some(Error::Closed),
        _ => None,
    }
}

/// Waits for an event that ends the pairing (the relay's stream failed for good).
async fn next_fatal(rx: &mut mpsc::UnboundedReceiver<Event>) -> Error {
    loop {
        if let Some(e) = fatal_of(rx.recv().await) {
            return e;
        }
    }
}

/// Pairs by QR code.
///
/// `on_qr` is called with each URL to render as a QR code (a new one every 30 s, up to 6).
/// Returns the paired session, to save and pass to [`Client::connect`](crate::Client::connect).
/// The function waits the 2 s the phone needs to save the pairing before returning.
pub async fn pair_with_qr(
    config: Config,
    cancel: &CancelToken,
    mut on_qr: impl FnMut(&str) + Send,
) -> Result<Session> {
    let refresh_key = RefreshKey::generate();
    let keys = RequestKeys::generate();
    let conn = Arc::new(Conn::new(
        config,
        Session::blank(refresh_key.clone()),
        None,
    )?);

    match conn.fetch_config().await {
        Ok(c) => {
            if let Some(id) = c.device.and_then(|d| Uuid::parse_str(&d.device_id).ok()) {
                conn.session().web_device_id = Some(id);
            }
        }
        Err(e) => warn!(%e, "configuration fetch failed; continuing"),
    }

    let req = wire::RegisterPhoneRelayRequest {
        credentials: Some(wire::Credentials {
            request_id: Uuid::new_v4().to_string(),
            network: NET_BUGLE.into(),
            token: Vec::new(),
            version: Some(conn.version()),
        }),
        browser: Some(conn.browser_details()),
        keys: Some(wire::KeyRegistration {
            key: Some(wire::PublicKeyEntry {
                kind: 2,
                public_key: refresh_key.public_key_der(),
            }),
        }),
    };
    let reg: wire::RegisterPhoneRelayResponse = conn
        .rpc(API, "Pairing", "RegisterPhoneRelay", Encoding::Binary, &req)
        .await?;
    {
        let mut s = conn.session();
        if let Some(b) = reg.browser.clone() {
            s.browser = b.into();
        }
        if let Some(t) = &reg.token {
            s.set_token(t);
        }
    }
    let mut pairing_key = reg.pairing_key;

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let (p_tx, mut p_rx) = mpsc::unbounded_channel();
    let relay = Relay::new(conn.clone(), Mode::Pairing, ev_tx);
    relay.set_pairing_sink(p_tx);
    relay.start();
    let _guard = RelayGuard(relay.clone());

    let mut shown = 0usize;
    let paired = loop {
        on_qr(&qr_url(&pairing_key, &keys));
        shown += 1;
        let deadline = tokio::time::sleep(QR_REFRESH_INTERVAL);
        tokio::pin!(deadline);
        let outcome = loop {
            tokio::select! {
                _ = &mut deadline => break None,
                _ = cancel.cancelled() => return Err(PairingError::Cancelled.into()),
                ev = p_rx.recv() => match ev {
                    Some(wire::PairingEvent { paired: Some(p), .. }) => break Some(Ok(p)),
                    Some(wire::PairingEvent { revoked: Some(_), .. }) => break Some(Err(PairingError::Revoked.into())),
                    Some(_) => {}
                    None => break Some(Err(Error::Closed)),
                },
                ev = ev_rx.recv() => if let Some(e) = fatal_of(ev) { break Some(Err(e)) },
            }
        };
        match outcome {
            Some(r) => break r?,
            None if shown >= QR_MAX_CODES => return Err(PairingError::QrExpired.into()),
            None => {
                let req = wire::RefreshPhoneRelayRequest {
                    credentials: Some(conn.credentials(Uuid::new_v4().to_string(), NET_BUGLE)),
                };
                let r: wire::RefreshPhoneRelayResponse = conn
                    .rpc(API, "Pairing", "RefreshPhoneRelay", Encoding::Binary, &req)
                    .await?;
                if !r.pairing_key.is_empty() {
                    pairing_key = r.pairing_key;
                }
            }
        }
    };

    info!("paired by QR code");
    {
        let mut s = conn.session();
        if let Some(p) = paired.phone {
            s.phone = p.into();
        }
        if let Some(b) = paired.browser {
            s.browser = b.into();
        }
        if let Some(t) = &paired.token {
            s.set_token(t);
        }
        s.aes_key = keys.aes;
        s.hmac_key = keys.hmac;
    }
    let _ = relay.flush_acks().await;
    relay.shutdown();
    tokio::time::sleep(POST_PAIRING_DELAY).await;
    Ok(conn.snapshot())
}

/// Chooses the destination among the account's registrations: role 1, most recently seen
/// first; returns the `index`-th.
pub(crate) fn choose_destination(devices: &wire::SignInDevices, index: usize) -> Option<String> {
    let mut phones: Vec<(i64, &str)> = devices
        .registrations
        .iter()
        .filter(|r| r.role == 1 && !r.id.is_empty())
        .map(|r| {
            let seen = devices
                .seen
                .iter()
                .filter(|s| s.id == r.id)
                .map(|s| s.seen)
                .max()
                .unwrap_or(0);
            (seen, r.id.as_str())
        })
        .collect();
    phones.sort_by_key(|p| std::cmp::Reverse(p.0));
    phones.get(index).map(|(_, id)| id.to_string())
}

/// `messages-web-` followed by the 32 lowercase hex digits of the web device id.
pub(crate) fn web_device_ref(id: &Uuid) -> String {
    format!("messages-web-{}", id.simple())
}

fn pairing_opts(class: i32, timeout: Duration) -> CallOpts {
    CallOpts {
        encrypt: false,
        class,
        ttl: Ttl::Fixed(PAIRING_TTL_US),
        request_id: None,
        expect_reply: true,
        timeout,
        pairing: true,
        urgent: false,
    }
}

async fn send_pairing_cancel(relay: &Arc<Relay>, attempt_id: &Uuid) {
    let mut o = pairing_opts(20, Duration::from_secs(10)).no_reply();
    o.request_id = Some(attempt_id.to_string());
    if let Err(e) = relay.call(action::PAIRING_CANCEL, None, o).await {
        warn!(%e, "pairing cancel failed");
    }
}

fn check_reply(inc: Option<crate::relay::Incoming>) -> Result<wire::PairingReply> {
    let inc = inc.ok_or_else(|| Error::Decode("no pairing reply".into()))?;
    let reply: wire::PairingReply = codec::from_binary(&inc.clear)?;
    if reply.error_kind != 0 || reply.error != 0 {
        return Err(PairingError::from_code(PairingErrorCode(reply.error)).into());
    }
    Ok(reply)
}

/// Pairs through the Google account signed in on both sides.
///
/// `cookies` are the Google cookies of `messages.google.com` (`OSID`, `SID`, `HSID`, `SSID`,
/// `APISID`, `SAPISID`, optionally `__Secure-1PSIDTS`). `on_emoji` is called once with the
/// emoji to show; the user taps the same one on the phone. Errors of the phone are mapped
/// to [`PairingError`] (wrong emoji, cancelled, timed out, no phone…).
pub async fn pair_with_google_account(
    config: Config,
    cookies: BTreeMap<String, String>,
    options: AccountPairingOptions,
    cancel: &CancelToken,
    mut on_emoji: impl FnMut(&EmojiPrompt) + Send,
) -> Result<Session> {
    for c in ["SAPISID", "SID"] {
        if !cookies.contains_key(c) {
            return Err(PairingError::BadCookies(format!("cookie {c} missing")).into());
        }
    }
    let refresh_key = RefreshKey::generate();
    let mut blank = Session::blank(refresh_key.clone());
    blank.cookies = cookies;
    let conn = Arc::new(Conn::new(config, blank, None)?);
    let cookie_err = |e: Error| -> Error {
        if e.is_http_status(401) || e.is_http_status(403) || e.is_session_gone() {
            PairingError::BadCookies(e.to_string()).into()
        } else {
            e
        }
    };

    // Configuration: the web device id.
    let cfg = conn.fetch_config().await.map_err(cookie_err)?;
    let web_device_id = cfg
        .device
        .and_then(|d| Uuid::parse_str(&d.device_id).ok())
        .ok_or_else(|| {
            PairingError::BadCookies("configuration has no device id (not signed in?)".into())
        })?;
    conn.session().web_device_id = Some(web_device_id);

    // 1. Sign in.
    let req = wire::SignInGaiaRequest {
        credentials: Some(wire::Credentials {
            request_id: Uuid::new_v4().to_string(),
            network: NET_GDITTO.into(),
            token: Vec::new(),
            version: Some(conn.version()),
        }),
        body: Some(wire::SignInBody {
            device: Some(wire::WebDeviceRef {
                kind: 3,
                id: web_device_ref(&web_device_id),
            }),
            key: Some(wire::SignInKey {
                public_key: refresh_key.public_key_der(),
            }),
        }),
        network: NET_GDITTO.into(),
    };
    let resp: wire::SignInGaiaResponse = conn
        .rpc(
            ACCOUNT,
            "Registration",
            "SignInGaia",
            Encoding::PbLite,
            &req,
        )
        .await
        .map_err(cookie_err)?;
    let devices = resp.devices.unwrap_or_default();
    let browser: Device = devices
        .this
        .as_ref()
        .and_then(|t| t.device.clone())
        .ok_or_else(|| Error::Decode("sign-in reply without a browser device".into()))?
        .into();
    let mut phone = browser.clone();
    phone.source_id = phone.source_id.to_lowercase();
    let dest = choose_destination(&devices, options.phone_index).ok_or(PairingError::NoPhone)?;
    let dest = Uuid::parse_str(dest.trim())
        .map_err(|e| Error::Decode(format!("destination id {dest:?}: {e}")))?;
    {
        let mut s = conn.session();
        s.browser = browser;
        s.phone = phone;
        s.destination_id = Some(dest);
        if let Some(t) = &resp.token {
            s.set_token(t);
        }
    }
    debug!(%dest, "signed in; destination chosen");

    // 2. Listen, and wait until the stream is open.
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let relay = Relay::new(conn.clone(), Mode::Pairing, ev_tx);
    let mut opened = relay.opened();
    relay.start();
    let _guard = RelayGuard(relay.clone());
    tokio::select! {
        r = opened.wait_for(|n| *n > 0) => { r.map_err(|_| Error::Closed)?; }
        _ = cancel.cancelled() => return Err(PairingError::Cancelled.into()),
        _ = tokio::time::sleep(Duration::from_secs(60)) => return Err(PairingError::TimedOut.into()),
        e = next_fatal(&mut ev_rx) => return Err(e),
    }

    // 3. Handshake.
    let ukey = Ukey2Client::new();
    let attempt_id = Uuid::new_v4();
    let started_ms = now_us() / 1000;
    let browser_details = conn.browser_details();

    let init_req = wire::PairingRequest {
        attempt_id: attempt_id.to_string(),
        browser: Some(browser_details.clone()),
        started: started_ms,
        data: ukey.init.clone(),
        emoji_version: 1,
        key_version: 1,
        statement: String::new(),
    };
    let init_call = relay.call(
        action::PAIRING_INIT,
        Some(codec::to_binary(&init_req)),
        pairing_opts(20, Duration::from_secs(20)),
    );
    let init_reply = tokio::select! {
        r = init_call => r,
        _ = cancel.cancelled() => Err(PairingError::Cancelled.into()),
    };
    let init_reply = match init_reply.and_then(check_reply) {
        Ok(r) => r,
        Err(e) => {
            send_pairing_cancel(&relay, &attempt_id).await;
            return Err(match e {
                Error::Timeout => PairingError::TimedOut.into(),
                e => e,
            });
        }
    };
    let hs = match ukey.handle_server_init(&init_reply.data) {
        Ok(h) => h,
        Err(e) => {
            send_pairing_cancel(&relay, &attempt_id).await;
            return Err(e.into());
        }
    };
    let n = ukey2::emoji_index(&hs.auth);
    let prompt = EmojiPrompt {
        emoji: emoji::emoji_at(n, init_reply.emoji_version),
        index_value: n,
        emoji_version: init_reply.emoji_version,
        image_url: emoji::noto_svg_url(emoji::emoji_at(n, init_reply.emoji_version)),
    };
    info!(emoji = prompt.emoji, "showing pairing emoji");
    on_emoji(&prompt);

    let finish_req = wire::PairingRequest {
        attempt_id: attempt_id.to_string(),
        browser: Some(browser_details),
        started: started_ms,
        data: ukey.finish.clone(),
        emoji_version: 0,
        key_version: 0,
        statement: STATEMENT.into(),
    };
    let finish_call = relay.call(
        action::PAIRING_FINISH,
        Some(codec::to_binary(&finish_req)),
        pairing_opts(2, Duration::from_secs(600)),
    );
    let finish = tokio::select! {
        r = finish_call => r,
        _ = cancel.cancelled() => Err(PairingError::Cancelled.into()),
        e = next_fatal(&mut ev_rx) => Err(e),
    };
    if let Err(e) = finish.and_then(check_reply) {
        send_pairing_cancel(&relay, &attempt_id).await;
        return Err(match e {
            Error::Timeout => PairingError::TimedOut.into(),
            e => e,
        });
    }

    let keys = ukey2::derive_request_keys(&hs.next, init_reply.key_version);
    {
        let mut s = conn.session();
        s.aes_key = keys.aes;
        s.hmac_key = keys.hmac;
        s.pairing_attempt_id = Some(attempt_id);
    }
    info!("paired through the Google account");
    let _ = relay.flush_acks().await;
    relay.shutdown();
    tokio::time::sleep(POST_PAIRING_DELAY).await;
    Ok(conn.snapshot())
}

/// Unpairs a session without connecting it (see also
/// [`Client::unpair`](crate::Client::unpair)).
///
/// Google account sessions: action 46 with the pairing attempt id. QR sessions:
/// `RevokeRelayPairing`.
pub async fn unpair(config: Config, session: Session) -> Result<()> {
    let (tx, _rx) = mpsc::unbounded_channel();
    let conn = Arc::new(Conn::new(config, session, None)?);
    // Not started: used only to post the call (no activation to wait for).
    let relay = Relay::new(conn, Mode::Pairing, tx);
    unpair_with(&relay).await
}

pub(crate) async fn unpair_with(relay: &Arc<Relay>) -> Result<()> {
    let conn = &relay.conn;
    let s = conn.snapshot();
    if s.is_google_account() {
        let attempt = s
            .pairing_attempt_id
            .ok_or_else(|| Error::InvalidInput("session has no pairing attempt id".into()))?;
        let req = wire::UnpairRequest {
            attempt_id: attempt.to_string(),
        };
        relay
            .call_no_reply(action::ACCOUNT_UNPAIR, Some(&req))
            .await
    } else {
        let req = wire::RevokeRelayPairingRequest {
            credentials: Some(conn.credentials(Uuid::new_v4().to_string(), "")),
            browser: Some((&s.browser).into()),
        };
        let _: wire::RevokeRelayPairingResponse = conn
            .rpc(API, "Pairing", "RevokeRelayPairing", Encoding::Binary, &req)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_url_format() {
        let keys = RequestKeys {
            aes: [1; 32],
            hmac: [2; 32],
        };
        let url = qr_url(&[9, 9, 9], &keys);
        let b = url
            .strip_prefix("https://support.google.com/messages/?p=web_computer#?c=")
            .unwrap();
        let data: wire::QrCodeData = codec::from_binary(&B64.decode(b).unwrap()).unwrap();
        assert_eq!(data.pairing_key, vec![9, 9, 9]);
        assert_eq!(data.aes_key, vec![1; 32]);
        assert_eq!(data.hmac_key, vec![2; 32]);
    }

    #[test]
    fn web_device_ref_format() {
        let id = Uuid::parse_str("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap();
        assert_eq!(
            web_device_ref(&id),
            "messages-web-0f8fad5bd9cb469fa16570867728950e"
        );
    }

    #[test]
    fn destination_choice() {
        let reg = |id: &str, role| wire::Registration {
            id: id.into(),
            role,
            ..Default::default()
        };
        let seen = |id: &str, t| wire::RegistrationSeen {
            id: id.into(),
            seen: t,
            ..Default::default()
        };
        let d = wire::SignInDevices {
            this: None,
            registrations: vec![reg("a", 1), reg("b", 6), reg("c", 1)],
            seen: vec![seen("a", 10), seen("b", 99), seen("c", 20)],
        };
        assert_eq!(choose_destination(&d, 0).as_deref(), Some("c"));
        assert_eq!(choose_destination(&d, 1).as_deref(), Some("a"));
        assert_eq!(choose_destination(&d, 2), None);
    }

    #[test]
    fn pairing_error_mapping() {
        assert_eq!(
            PairingError::from_code(PairingErrorCode(5)),
            PairingError::WrongEmoji
        );
        assert_eq!(
            PairingError::from_code(PairingErrorCode(7)),
            PairingError::CancelledOnPhone
        );
        assert_eq!(
            PairingError::from_code(PairingErrorCode(27)),
            PairingError::CancelledOnPhone
        );
        for c in [2, 3, 6] {
            assert_eq!(
                PairingError::from_code(PairingErrorCode(c)),
                PairingError::TimedOut
            );
        }
        assert_eq!(
            PairingError::from_code(PairingErrorCode(11)),
            PairingError::Phone(PairingErrorCode(11))
        );
    }

    #[tokio::test]
    async fn cancel_token() {
        let t = CancelToken::new();
        let t2 = t.clone();
        let h = tokio::spawn(async move { t2.cancelled().await });
        tokio::task::yield_now().await;
        t.cancel();
        h.await.unwrap();
        t.cancelled().await; // already cancelled: returns at once
        assert!(t.is_cancelled());
    }
}
