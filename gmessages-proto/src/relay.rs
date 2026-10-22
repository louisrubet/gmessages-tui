//! The relay engine (spec §5): event stream, dispatch, acknowledgements, calls to the
//! phone, activation, liveness and reconnection.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rand::Rng;
use tokio::sync::{Notify, mpsc::UnboundedSender, oneshot, watch};
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::codec::{self, Proto};
use crate::conn::{Conn, Encoding};
use crate::crypto::sha256;
use crate::error::{Error, Result};
use crate::event::{Event, LogoutReason};
use crate::longpoll::LongPollParser;
use crate::types;
use crate::wire;

/// Action codes used internally.
pub(crate) mod action {
    pub const LIST_CONVERSATIONS: i32 = 1;
    pub const LIST_MESSAGES: i32 = 2;
    pub const SEND_MESSAGE: i32 = 3;
    pub const LIST_CONTACTS: i32 = 6;
    pub const GET_OR_CREATE_CONVERSATION: i32 = 9;
    pub const MARK_READ: i32 = 10;
    pub const TYPING: i32 = 12;
    pub const SETTINGS_UPDATE: i32 = 13;
    pub const UPDATE_CONVERSATION: i32 = 15;
    pub const UPDATES: i32 = 16;
    pub const PRESENCE_ACK: i32 = 17;
    pub const CONVERSATION_TYPE: i32 = 21;
    pub const PING: i32 = 22;
    pub const DELETE_MESSAGE: i32 = 23;
    pub const RESEND_MESSAGE: i32 = 25;
    pub const TOP_CONTACTS: i32 = 28;
    pub const CONTACT_THUMBNAILS: i32 = 29;
    pub const DEFAULT_SMS_APP: i32 = 31;
    pub const FULL_SIZE_IMAGE: i32 = 36;
    pub const PARTICIPANT_THUMBNAILS: i32 = 37;
    pub const REACTION: i32 = 38;
    pub const GET_CONVERSATION: i32 = 43;
    pub const PAIRING_INIT: i32 = 44;
    pub const PAIRING_FINISH: i32 = 45;
    pub const ACCOUNT_UNPAIR: i32 = 46;
    pub const PAIRING_CANCEL: i32 = 47;
    pub const PREFETCH_CONTACTS: i32 = 52;
    pub const BLOCKED_CONTACTS: i32 = 53;
}

const ROUTE_DATA: i32 = 19;
const ROUTE_PAIRING: i32 = 14;
const ROUTE_ACCOUNT_PAIRING: i32 = 7;
const STREAM_IDLE: Duration = Duration::from_secs(60);
const ACK_INTERVAL: Duration = Duration::from_secs(5);
const ACK_CAP: usize = 1024;
const DEDUP_SIZE: usize = 8;
pub(crate) const PAIRING_TTL_US: i64 = 300_000_000;

/// Whether the relay serves a pairing in progress or a paired client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// No pings, no activation, no update events.
    Pairing,
    /// Full client.
    Paired,
}

/// A reply item from the phone.
#[derive(Debug, Clone)]
pub(crate) struct Incoming {
    #[allow(dead_code)]
    pub action: i32,
    pub clear: Vec<u8>,
    pub decrypted: Option<Vec<u8>>,
}

/// TTL of a call.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Ttl {
    /// The stored token TTL.
    Token,
    /// No TTL field.
    Absent,
    /// A fixed value, µs.
    Fixed(i64),
}

/// Options of a call to the phone.
#[derive(Debug, Clone)]
pub(crate) struct CallOpts {
    pub encrypt: bool,
    pub class: i32,
    pub ttl: Ttl,
    pub request_id: Option<String>,
    pub expect_reply: bool,
    pub timeout: Duration,
    /// Pairing calls: `Call.session` left empty.
    pub pairing: bool,
    /// Ping the phone after 5 s without reply.
    pub urgent: bool,
}

impl CallOpts {
    pub fn normal(timeout: Duration) -> Self {
        CallOpts {
            encrypt: true,
            class: 2,
            ttl: Ttl::Token,
            request_id: None,
            expect_reply: true,
            timeout,
            pairing: false,
            urgent: true,
        }
    }
    pub fn no_reply(mut self) -> Self {
        self.expect_reply = false;
        self.urgent = false;
        self
    }
}

#[derive(Debug)]
struct State {
    stream_id: String,
    active_session_id: String,
    activated_stream: Option<String>,
    backlog_remaining: u32,
    dedup: VecDeque<(String, [u8; 32])>,
    last_phone_activity: Instant,
    missed_pings: u32,
    last_default_sms_check: Option<Instant>,
    logged_out: bool,
}

enum BodyEnd {
    Normal,
    Idle,
    Error(String),
    Reconnect,
    Shutdown,
}

/// The relay engine shared by pairing and the client.
pub(crate) struct Relay {
    pub conn: Arc<Conn>,
    mode: Mode,
    events: UnboundedSender<Event>,
    pending: Mutex<HashMap<String, oneshot::Sender<Incoming>>>,
    acks: Mutex<Vec<String>>,
    st: Mutex<State>,
    pairing_tx: Mutex<Option<UnboundedSender<wire::PairingEvent>>>,
    opened: watch::Sender<u64>,
    activated: watch::Sender<bool>,
    reconnect: Notify,
    urgent_ping: Notify,
    connect_ping: Notify,
    alive: Notify,
    shutdown: watch::Sender<bool>,
    pub first_list_done: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Stream retry delay: 10–15 s, then 5 s more per consecutive failure (capped at 5 min).
fn backoff(failures: u32) -> Duration {
    let base = 10_000 + 5_000 * u64::from(failures.saturating_sub(1));
    let jitter = rand::thread_rng().gen_range(0..5_000);
    Duration::from_millis((base + jitter).min(300_000))
}

impl Relay {
    pub fn new(conn: Arc<Conn>, mode: Mode, events: UnboundedSender<Event>) -> Arc<Self> {
        Arc::new(Relay {
            conn,
            mode,
            events,
            pending: Mutex::new(HashMap::new()),
            acks: Mutex::new(Vec::new()),
            st: Mutex::new(State {
                stream_id: Uuid::new_v4().to_string(),
                active_session_id: String::new(),
                activated_stream: None,
                backlog_remaining: 0,
                dedup: VecDeque::new(),
                last_phone_activity: Instant::now(),
                missed_pings: 0,
                last_default_sms_check: None,
                logged_out: false,
            }),
            pairing_tx: Mutex::new(None),
            opened: watch::channel(0).0,
            activated: watch::channel(false).0,
            reconnect: Notify::new(),
            urgent_ping: Notify::new(),
            connect_ping: Notify::new(),
            alive: Notify::new(),
            shutdown: watch::channel(false).0,
            first_list_done: AtomicBool::new(false),
        })
    }

    /// Starts the background tasks.
    pub fn start(self: &Arc<Self>) {
        tokio::spawn(self.clone().stream_loop());
        tokio::spawn(self.clone().ack_loop());
        if self.mode == Mode::Paired {
            tokio::spawn(self.clone().liveness_loop());
        }
    }

    fn st(&self) -> MutexGuard<'_, State> {
        lock(&self.st)
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    pub fn set_pairing_sink(&self, tx: UnboundedSender<wire::PairingEvent>) {
        *lock(&self.pairing_tx) = Some(tx);
    }

    /// A receiver of the number of times the stream opened.
    pub fn opened(&self) -> watch::Receiver<u64> {
        self.opened.subscribe()
    }

    pub fn active_session_id(&self) -> String {
        self.st().active_session_id.clone()
    }

    /// Stops all tasks; pending calls fail with [`Error::Closed`].
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
        lock(&self.pending).clear();
    }

    pub fn is_shut(&self) -> bool {
        *self.shutdown.borrow()
    }

    async fn wait_shutdown(&self) {
        let mut rx = self.shutdown.subscribe();
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// Sleeps; returns false if shut down meanwhile.
    async fn sleep(&self, d: Duration) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(d) => !self.is_shut(),
            _ = self.wait_shutdown() => false,
        }
    }

    fn fatal(&self, reason: LogoutReason) {
        {
            let mut st = self.st();
            if st.logged_out {
                return;
            }
            st.logged_out = true;
        }
        warn!(?reason, "session ended");
        self.emit(Event::LoggedOut(reason));
        self.shutdown();
    }

    /// Asks the stream task to reconnect with a new stream id.
    pub fn force_reconnect(&self) {
        self.reconnect.notify_one();
    }

    // ---------------------------------------------------------------- stream

    async fn stream_loop(self: Arc<Self>) {
        let mut failures: u32 = 0;
        let mut down_since: Option<Instant> = None;
        let mut ever_connected = false;
        loop {
            if self.is_shut() {
                break;
            }
            if self.mode == Mode::Paired {
                match self.conn.ensure_token(false).await {
                    Ok(()) => {}
                    Err(Error::SessionGone(m)) => {
                        self.fatal(LogoutReason::CredentialsRejected(m));
                        break;
                    }
                    Err(e) => warn!(%e, "token refresh failed; trying with the current token"),
                }
            }
            let stream_id = self.st().stream_id.clone();
            debug!(%stream_id, "opening event stream");
            let res = tokio::select! {
                r = self.conn.open_stream(&stream_id) => r,
                _ = self.wait_shutdown() => break,
            };
            let resp = match res {
                Ok(r) => r,
                Err(e) => {
                    if e.is_http_status(401) || e.is_http_status(403) || e.is_session_gone() {
                        self.fatal(LogoutReason::CredentialsRejected(e.to_string()));
                        break;
                    }
                    failures += 1;
                    down_since.get_or_insert_with(Instant::now);
                    let delay = backoff(failures);
                    warn!(%e, ?delay, "event stream failed to open");
                    self.emit(Event::Disconnected {
                        error: e.to_string(),
                        retry_in: delay,
                    });
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.reconnect.notified() => self.new_stream_id(),
                        _ = self.wait_shutdown() => break,
                    }
                    continue;
                }
            };
            failures = 0;
            let was_down = down_since.take();
            if !ever_connected || was_down.is_some() {
                info!("event stream connected");
                self.emit(Event::Connected);
            }
            ever_connected = true;
            self.st().backlog_remaining = 0;
            self.opened.send_modify(|n| *n += 1);
            if self.mode == Mode::Paired {
                let needs_activation = self.st().activated_stream.as_deref() != Some(&stream_id);
                if needs_activation {
                    tokio::spawn(self.clone().activation_task(stream_id.clone()));
                } else if was_down.is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
                    let me = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = me.catch_up().await {
                            warn!(%e, "catch-up failed");
                        }
                    });
                }
            }
            match self.read_body(resp).await {
                BodyEnd::Normal => debug!("event stream ended; reopening"),
                BodyEnd::Idle => {
                    warn!("nothing received for 60 s; reopening the event stream");
                    down_since = Some(Instant::now() - STREAM_IDLE);
                }
                BodyEnd::Error(e) => {
                    failures += 1;
                    down_since = Some(Instant::now());
                    let delay = backoff(failures);
                    warn!(%e, ?delay, "event stream broke");
                    self.emit(Event::Disconnected {
                        error: e,
                        retry_in: delay,
                    });
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.reconnect.notified() => self.new_stream_id(),
                        _ = self.wait_shutdown() => break,
                    }
                }
                BodyEnd::Reconnect => {
                    info!("full reconnect requested");
                    self.new_stream_id();
                }
                BodyEnd::Shutdown => break,
            }
        }
        debug!("stream task stopped");
    }

    fn new_stream_id(&self) {
        let mut st = self.st();
        st.stream_id = Uuid::new_v4().to_string();
        st.activated_stream = None;
    }

    async fn read_body(self: &Arc<Self>, mut resp: reqwest::Response) -> BodyEnd {
        let mut parser = LongPollParser::new();
        let mut items = 0usize;
        loop {
            let chunk = tokio::select! {
                r = tokio::time::timeout(STREAM_IDLE, resp.chunk()) => r,
                _ = self.reconnect.notified() => return BodyEnd::Reconnect,
                _ = self.wait_shutdown() => return BodyEnd::Shutdown,
            };
            match chunk {
                Err(_) => return BodyEnd::Idle,
                Ok(Err(e)) => return BodyEnd::Error(e.to_string()),
                Ok(Ok(None)) => {
                    return if parser.is_done() || items > 0 {
                        BodyEnd::Normal
                    } else {
                        BodyEnd::Error("event stream closed without data".into())
                    };
                }
                Ok(Ok(Some(bytes))) => match parser.feed(&bytes) {
                    Ok(list) => {
                        for raw in list {
                            items += 1;
                            self.handle_item(&raw);
                        }
                    }
                    Err(e) => return BodyEnd::Error(e.to_string()),
                },
            }
        }
    }

    fn handle_item(self: &Arc<Self>, raw: &[u8]) {
        let item: wire::StreamItem = match codec::from_pblite_slice(raw) {
            Ok(i) => i,
            Err(e) => {
                warn!(%e, "undecodable stream item");
                return;
            }
        };
        if item.heartbeat.is_some() {
            trace!("heartbeat");
        }
        if item.start.is_some() {
            trace!("stream start");
        }
        if let Some(b) = item.backlog {
            let n = b.count.unwrap_or(0).max(0) as u32;
            debug!(n, "backlog announced");
            self.st().backlog_remaining = n;
        }
        if let Some(env) = item.envelope {
            if !env.id.is_empty() {
                self.queue_ack(env.id.clone());
            }
            self.dispatch(env);
        }
    }

    fn dispatch(self: &Arc<Self>, env: wire::IncomingEnvelope) {
        match env.route {
            ROUTE_DATA => {
                let data: wire::DataPayload = match codec::from_binary(&env.payload) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(%e, "undecodable data payload");
                        return;
                    }
                };
                let keys = self.conn.session().request_keys();
                let decrypted = if !data.encrypted.is_empty() && !keys.is_empty() {
                    match keys.decrypt(&data.encrypted) {
                        Ok(p) => Some(p),
                        Err(e) => {
                            warn!(%e, action = data.action, "cannot decrypt payload");
                            None
                        }
                    }
                } else {
                    None
                };
                let intermediate = self.conn.is_account()
                    && !data.clear.is_empty()
                    && data.encrypted.is_empty()
                    && data.action != action::PAIRING_INIT
                    && data.action != action::PAIRING_FINISH;
                if intermediate && lock(&self.pending).contains_key(&data.request_id) {
                    trace!(action = data.action, "intermediate item before a reply");
                    return;
                }
                if !data.request_id.is_empty() && !intermediate {
                    let tx = lock(&self.pending).remove(&data.request_id);
                    if let Some(tx) = tx {
                        self.phone_seen();
                        let _ = tx.send(Incoming {
                            action: data.action,
                            clear: data.clear,
                            decrypted,
                        });
                        return;
                    }
                }
                let old = {
                    let mut st = self.st();
                    if st.backlog_remaining > 0 {
                        st.backlog_remaining -= 1;
                        true
                    } else {
                        false
                    }
                };
                if data.action == action::UPDATES {
                    if data.encrypted.is_empty() && data.clear == [0x72, 0x00] {
                        let me = self.clone();
                        tokio::spawn(async move {
                            let _ = me.flush_acks().await;
                            me.fatal(LogoutReason::AccountLoggedOut);
                        });
                        return;
                    }
                    if self.mode == Mode::Paired {
                        self.phone_seen();
                        if let Some(p) = decrypted {
                            self.handle_update(&p, old);
                        }
                    }
                } else {
                    debug!(action = data.action, rid = %data.request_id, "unexpected data item");
                }
            }
            ROUTE_PAIRING => {
                let ev: wire::PairingEvent = match codec::from_binary(&env.payload) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(%e, "undecodable pairing event");
                        return;
                    }
                };
                if let Some(tx) = lock(&self.pairing_tx).as_ref() {
                    let _ = tx.send(ev.clone());
                }
                if ev.revoked.is_some() && self.mode == Mode::Paired {
                    self.fatal(LogoutReason::Unpaired);
                }
            }
            ROUTE_ACCOUNT_PAIRING => debug!("account pairing notice"),
            r => debug!(route = r, "item on unknown route"),
        }
    }

    fn phone_seen(&self) {
        let mut st = self.st();
        st.last_phone_activity = Instant::now();
        if st.missed_pings > 0 {
            self.alive.notify_one();
        }
    }

    fn handle_update(self: &Arc<Self>, plain: &[u8], old: bool) {
        let up: wire::Update = match codec::from_binary(plain) {
            Ok(u) => u,
            Err(e) => {
                warn!(%e, "undecodable update");
                return;
            }
        };
        // Dedup on (thing id, SHA-256 of the decrypted payload).
        let thing = up
            .conversations
            .as_ref()
            .and_then(|c| c.conversations.first().map(|c| c.id.clone()))
            .or_else(|| {
                up.messages
                    .as_ref()
                    .and_then(|m| m.messages.first().map(|m| m.id.clone()))
            })
            .unwrap_or_default();
        let key = (thing, sha256(plain));
        {
            let mut st = self.st();
            if st.dedup.contains(&key) {
                trace!("duplicate update dropped");
                return;
            }
            st.dedup.push_back(key);
            while st.dedup.len() > DEDUP_SIZE {
                st.dedup.pop_front();
            }
        }
        if let Some(c) = up.conversations
            && !old
        {
            for conv in c.conversations {
                self.emit(Event::ConversationUpdated(Box::new(conv.into())));
            }
        }
        if let Some(m) = up.messages {
            for msg in m.messages {
                self.emit(Event::MessageUpserted {
                    message: Box::new(msg.into()),
                    old,
                });
            }
        }
        if let Some(t) = up.typing.and_then(|t| t.typing)
            && !old
        {
            self.emit(Event::Typing(t.into()));
        }
        if let Some(s) = up.settings {
            self.emit(Event::Settings(Box::new(s.into())));
        }
        if let Some(a) = up.alert
            && !old
        {
            let alert = types::Alert::from(a.alert);
            if alert.is_browser_inactive() {
                info!(?alert, "this browser session became inactive");
            }
            self.emit(Event::Alert(alert));
        }
        if up.presence_check.is_some() && !old {
            let me = self.clone();
            tokio::spawn(async move {
                let o = CallOpts::normal(me.conn.config.call_timeout).no_reply();
                if let Err(e) = me.call(action::PRESENCE_ACK, None, o).await {
                    warn!(%e, "presence acknowledgement failed");
                }
            });
        }
        if let Some(a) = up.account {
            self.emit(Event::AccountChanged(a.into()));
        }
    }

    // ---------------------------------------------------------------- acks

    fn queue_ack(&self, id: String) {
        let mut a = lock(&self.acks);
        a.push(id);
        if a.len() > ACK_CAP {
            let n = a.len() - ACK_CAP;
            a.drain(..n);
        }
    }

    /// Sends the queued acknowledgements; on failure they are queued again.
    pub async fn flush_acks(&self) -> Result<()> {
        let ids = std::mem::take(&mut *lock(&self.acks));
        if ids.is_empty() {
            return Ok(());
        }
        let browser: wire::Device = (&self.conn.session().browser).into();
        let req = wire::AckMessagesRequest {
            credentials: Some(
                self.conn
                    .credentials(Uuid::new_v4().to_string(), self.conn.network()),
            ),
            empty: Some(wire::Empty {}),
            acks: ids
                .iter()
                .map(|id| wire::Ack {
                    id: id.clone(),
                    browser: Some(browser.clone()),
                })
                .collect(),
        };
        let res = self
            .conn
            .post_rpc(
                self.conn.relay_host(),
                "Messaging",
                "AckMessages",
                Encoding::PbLite,
                &req,
            )
            .await;
        if let Err(e) = &res {
            warn!(%e, n = ids.len(), "acknowledgement failed; requeued");
            let mut a = lock(&self.acks);
            let mut merged = ids;
            merged.append(&mut a);
            let excess = merged.len().saturating_sub(ACK_CAP);
            merged.drain(..excess);
            *a = merged;
        } else {
            trace!(n = ids.len(), "acknowledged");
        }
        res
    }

    async fn ack_loop(self: Arc<Self>) {
        while self.sleep(ACK_INTERVAL).await {
            let _ = self.flush_acks().await;
        }
        let _ = self.flush_acks().await;
    }

    // ---------------------------------------------------------------- calls

    /// Calls the phone. Returns the reply when `opts.expect_reply`.
    pub async fn call(
        self: &Arc<Self>,
        action: i32,
        payload: Option<Vec<u8>>,
        opts: CallOpts,
    ) -> Result<Option<Incoming>> {
        if self.is_shut() {
            return Err(Error::Closed);
        }
        if self.mode == Mode::Paired && !opts.pairing && action != action::UPDATES {
            // Calls made right after connecting wait (a little) for the first activation,
            // so that they carry the active session id.
            let mut rx = self.activated.subscribe();
            tokio::select! {
                _ = tokio::time::timeout(Duration::from_secs(15), rx.wait_for(|a| *a)) => {}
                _ = self.wait_shutdown() => return Err(Error::Closed),
            }
        }
        let rid = opts
            .request_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let session_id = if opts.pairing {
            String::new()
        } else {
            self.st().active_session_id.clone()
        };
        let (keys, phone, token, token_ttl, dest) = {
            let s = self.conn.session();
            (
                s.request_keys(),
                wire::Device::from(&s.phone),
                s.token.clone(),
                s.token_ttl_us,
                s.destination_id,
            )
        };
        let (clear, encrypted) = match payload {
            None => (Vec::new(), Vec::new()),
            Some(p) if opts.encrypt => (Vec::new(), keys.encrypt(&p)),
            Some(p) => (p, Vec::new()),
        };
        let call = wire::Call {
            request_id: rid.clone(),
            action,
            clear,
            encrypted,
            session: session_id,
        };
        let ttl = match opts.ttl {
            Ttl::Token => token_ttl,
            Ttl::Absent => 0,
            Ttl::Fixed(t) => t,
        };
        let req = wire::SendMessageRequest {
            phone: Some(phone),
            routing: Some(wire::Routing {
                request_id: rid.clone(),
                route: ROUTE_DATA,
                call: codec::to_binary(&call),
                class: Some(wire::MessageClass {
                    empty: Some(wire::Empty {}),
                    class: opts.class,
                }),
            }),
            auth: Some(wire::CallAuth {
                request_id: rid.clone(),
                token,
                version: Some(self.conn.version()),
            }),
            ttl,
            destinations: dest.map(|d| vec![d.to_string()]).unwrap_or_default(),
        };
        let rx = if opts.expect_reply {
            let (tx, rx) = oneshot::channel();
            lock(&self.pending).insert(rid.clone(), tx);
            Some(rx)
        } else {
            None
        };
        trace!(action, %rid, "calling the phone");
        if let Err(e) = self
            .conn
            .post_rpc(
                self.conn.relay_host(),
                "Messaging",
                "SendMessage",
                Encoding::PbLite,
                &req,
            )
            .await
        {
            lock(&self.pending).remove(&rid);
            return Err(e);
        }
        let Some(rx) = rx else { return Ok(None) };
        if opts.urgent && self.mode == Mode::Paired {
            let me = self.clone();
            let rid2 = rid.clone();
            tokio::spawn(async move {
                if me.sleep(Duration::from_secs(5)).await && lock(&me.pending).contains_key(&rid2) {
                    me.urgent_ping.notify_one();
                }
            });
        }
        match tokio::time::timeout(opts.timeout, rx).await {
            Ok(Ok(inc)) => Ok(Some(inc)),
            Ok(Err(_)) => Err(Error::Closed),
            Err(_) => {
                lock(&self.pending).remove(&rid);
                Err(Error::Timeout)
            }
        }
    }

    /// Calls the phone with an encrypted request and decodes the encrypted reply.
    pub async fn call_typed<Req: Proto, Resp: Proto>(
        self: &Arc<Self>,
        action: i32,
        req: Option<&Req>,
    ) -> Result<Resp> {
        let opts = CallOpts::normal(self.conn.config.call_timeout);
        self.call_typed_with(action, req, opts).await
    }

    pub async fn call_typed_with<Req: Proto, Resp: Proto>(
        self: &Arc<Self>,
        action: i32,
        req: Option<&Req>,
        opts: CallOpts,
    ) -> Result<Resp> {
        let inc = self
            .call(action, req.map(codec::to_binary), opts)
            .await?
            .ok_or_else(|| Error::Decode("no reply".into()))?;
        match inc.decrypted {
            Some(p) => Ok(codec::from_binary(&p)?),
            None => Ok(Resp::default()),
        }
    }

    /// Calls the phone without waiting for a reply.
    pub async fn call_no_reply<Req: Proto>(
        self: &Arc<Self>,
        action: i32,
        req: Option<&Req>,
    ) -> Result<()> {
        let opts = CallOpts::normal(self.conn.config.call_timeout).no_reply();
        self.call(action, req.map(codec::to_binary), opts)
            .await
            .map(|_| ())
    }

    // ---------------------------------------------------------------- activation

    async fn activation_task(self: Arc<Self>, stream_id: String) {
        if !self.sleep(Duration::from_secs(2)).await {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.st().backlog_remaining > 0 && Instant::now() < deadline {
            if !self.sleep(Duration::from_millis(100)).await {
                return;
            }
        }
        let _ = self.flush_acks().await;
        if !self.sleep(Duration::from_secs(1)).await {
            return;
        }
        if self.st().stream_id != stream_id {
            return;
        }
        if let Err(e) = self.activate().await {
            warn!(%e, "activation failed");
            return;
        }
        self.st().activated_stream = Some(stream_id);
        self.activated.send_replace(true);
        self.emit(Event::Active);
        self.connect_ping.notify_one();
        let me = self.clone();
        tokio::spawn(async move { me.check_default_sms().await });
    }

    /// Takes the active role with a new active session id (action 16).
    pub async fn activate(self: &Arc<Self>) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        self.st().active_session_id = id.clone();
        info!(session = %id, "activating");
        let mut o = CallOpts::normal(self.conn.config.call_timeout).no_reply();
        o.request_id = Some(id);
        o.ttl = Ttl::Absent;
        self.call(action::UPDATES, None, o).await.map(|_| ())
    }

    /// Catch-up: action 16 with the current active session id.
    pub async fn catch_up(self: &Arc<Self>) -> Result<()> {
        let id = self.active_session_id();
        if id.is_empty() {
            return Ok(());
        }
        debug!("sending catch-up");
        let mut o = CallOpts::normal(self.conn.config.call_timeout).no_reply();
        o.request_id = Some(id);
        o.ttl = Ttl::Absent;
        self.call(action::UPDATES, None, o).await.map(|_| ())
    }

    async fn check_default_sms(self: &Arc<Self>) {
        {
            let mut st = self.st();
            if st
                .last_default_sms_check
                .is_some_and(|t| t.elapsed() < Duration::from_secs(3600))
            {
                return;
            }
            st.last_default_sms_check = Some(Instant::now());
        }
        match self
            .call_typed::<wire::Empty, wire::DefaultSmsResponse>(action::DEFAULT_SMS_APP, None)
            .await
        {
            Ok(r) => self.emit(Event::DefaultSmsApp(r.is_default)),
            Err(e) => debug!(%e, "default SMS app check failed"),
        }
    }

    // ---------------------------------------------------------------- liveness

    async fn ping(self: &Arc<Self>, timeout: Duration) -> Result<()> {
        let mut o = CallOpts::normal(timeout);
        o.urgent = false;
        let req = wire::PingRequest { value: true };
        self.call(action::PING, Some(codec::to_binary(&req)), o)
            .await
            .map(|_| ())
    }

    async fn liveness_loop(self: Arc<Self>) {
        const MINUTE: Duration = Duration::from_secs(60);
        let mut missed: u32 = 0;
        let mut reported = false;
        let mut first_after_connect = false;
        let mut delay = MINUTE;
        let mut last_ping: Option<Instant> = None;
        let mut last_extra_catchup = Instant::now();
        loop {
            let mut urgent = false;
            let mut alive_signal = false;
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = self.urgent_ping.notified() => urgent = true,
                _ = self.connect_ping.notified() => first_after_connect = true,
                _ = self.alive.notified() => alive_signal = true,
                _ = self.wait_shutdown() => return,
            }
            let activated = self.st().activated_stream.is_some();
            if !activated {
                delay = MINUTE;
                continue;
            }
            if alive_signal {
                if missed >= 2 {
                    let _ = self.catch_up().await;
                }
                if reported {
                    info!("phone responding again");
                    self.emit(Event::PhoneResponding);
                }
                missed = 0;
                reported = false;
                delay = MINUTE;
                self.st().missed_pings = 0;
                continue;
            }
            if let Some(t) = last_ping {
                let since = t.elapsed();
                if since < Duration::from_secs(30) {
                    if urgent {
                        continue;
                    }
                    if !self.sleep(Duration::from_secs(30) - since).await {
                        return;
                    }
                }
            }
            match self.conn.ensure_token(false).await {
                Err(Error::SessionGone(m)) => {
                    self.fatal(LogoutReason::CredentialsRejected(m));
                    return;
                }
                Err(e) => debug!(%e, "token refresh failed"),
                Ok(()) => {}
            }
            let quiet = self.st().last_phone_activity.elapsed();
            if quiet > Duration::from_secs(3 * 3600)
                && last_extra_catchup.elapsed() > Duration::from_secs(300)
            {
                last_extra_catchup = Instant::now();
                let _ = self.catch_up().await;
            }
            {
                let me = self.clone();
                tokio::spawn(async move { me.check_default_sms().await });
            }
            let sent = Instant::now();
            last_ping = Some(sent);
            let timeout = if urgent {
                Duration::from_secs(10)
            } else {
                MINUTE
            };
            let ok = self.ping(timeout).await.is_ok();
            if self.is_shut() {
                return;
            }
            let alive = ok || self.st().last_phone_activity > sent;
            if alive {
                if missed >= 2 {
                    let _ = self.catch_up().await;
                }
                if reported {
                    info!("phone responding again");
                    self.emit(Event::PhoneResponding);
                }
                missed = 0;
                reported = false;
                delay = MINUTE;
            } else {
                missed += 1;
                warn!(missed, "ping missed");
                match missed {
                    2 => {
                        if let Err(e) = self.activate().await {
                            warn!(%e, "re-activation failed");
                        }
                    }
                    3 => self.force_reconnect(),
                    _ => {}
                }
                if (missed >= 4 || first_after_connect) && !reported {
                    self.emit(Event::PhoneNotResponding);
                    reported = true;
                }
                delay = MINUTE * 2u32.pow((missed - 1).min(6));
            }
            first_after_connect = false;
            self.st().missed_pings = missed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::crypto::{RefreshKey, RequestKeys};
    use crate::session::Session;
    use tokio::sync::mpsc;

    fn setup(account: bool) -> (Arc<Relay>, mpsc::UnboundedReceiver<Event>, RequestKeys) {
        let keys = RequestKeys::generate();
        let mut s = Session::blank(RefreshKey::generate());
        s.aes_key = keys.aes;
        s.hmac_key = keys.hmac;
        if account {
            s.destination_id = Some(Uuid::new_v4());
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = Arc::new(Conn::new(Config::default(), s, None).unwrap());
        (Relay::new(conn, Mode::Paired, tx), rx, keys)
    }

    fn item(id: &str, route: i32, payload: Vec<u8>) -> Vec<u8> {
        let it = wire::StreamItem {
            envelope: Some(wire::IncomingEnvelope {
                id: id.into(),
                route,
                payload,
                ..Default::default()
            }),
            ..Default::default()
        };
        codec::to_pblite_string(&it).into_bytes()
    }

    fn data_item(id: &str, d: &wire::DataPayload) -> Vec<u8> {
        item(id, ROUTE_DATA, codec::to_binary(d))
    }

    fn message_update(keys: &RequestKeys, msg_id: &str) -> wire::DataPayload {
        let up = wire::Update {
            messages: Some(wire::MessageList {
                messages: vec![wire::Message {
                    id: msg_id.into(),
                    conversation_id: "1".into(),
                    status: Some(wire::MessageStatusInfo {
                        status: 100,
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };
        wire::DataPayload {
            request_id: Uuid::new_v4().to_string(),
            action: action::UPDATES,
            encrypted: keys.encrypt(&codec::to_binary(&up)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn updates_backlog_dedup_and_acks() {
        let (relay, mut rx, keys) = setup(false);
        relay.handle_item(&data_item("e1", &message_update(&keys, "m1")));
        match rx.try_recv().unwrap() {
            Event::MessageUpserted { message, old } => {
                assert_eq!(message.id, "m1");
                assert!(!old);
                assert!(message.is_incoming());
            }
            e => panic!("unexpected {e:?}"),
        }
        // the same decrypted payload again (different encryption, new envelope): dropped
        relay.handle_item(&data_item("e2", &message_update(&keys, "m1")));
        relay.handle_item(&data_item("e3", &message_update(&keys, "m1")));
        assert!(rx.try_recv().is_err(), "identical updates dropped");
        // backlog: the next 1 non-reply item is old
        relay.handle_item(b"[null,null,null,[1]]");
        relay.handle_item(&data_item("e4", &message_update(&keys, "m2")));
        relay.handle_item(&data_item("e5", &message_update(&keys, "m3")));
        let olds: Vec<bool> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|e| match e {
                Event::MessageUpserted { old, .. } => old,
                e => panic!("unexpected {e:?}"),
            })
            .collect();
        assert_eq!(olds, vec![true, false]);
        assert_eq!(
            lock(&relay.acks).clone(),
            vec!["e1", "e2", "e3", "e4", "e5"]
        );
    }

    #[tokio::test]
    async fn replies_are_matched() {
        let (relay, mut rx, keys) = setup(false);
        let (tx, mut rrx) = oneshot::channel();
        lock(&relay.pending).insert("rid-1".into(), tx);
        let reply = wire::DataPayload {
            request_id: "rid-1".into(),
            action: action::PING,
            encrypted: keys.encrypt(b"\x08\x01"),
            ..Default::default()
        };
        relay.handle_item(&data_item("e1", &reply));
        let inc = rrx.try_recv().unwrap();
        assert_eq!(inc.decrypted.as_deref(), Some(&b"\x08\x01"[..]));
        assert!(rx.try_recv().is_err());
        assert!(lock(&relay.pending).is_empty());
    }

    #[tokio::test]
    async fn account_intermediate_items_are_not_replies() {
        let (relay, _rx, _keys) = setup(true);
        let (tx, mut rrx) = oneshot::channel();
        lock(&relay.pending).insert("rid-2".into(), tx);
        let inter = wire::DataPayload {
            request_id: "rid-2".into(),
            action: action::LIST_CONVERSATIONS,
            clear: vec![1, 2],
            ..Default::default()
        };
        relay.handle_item(&data_item("e1", &inter));
        assert!(rrx.try_recv().is_err());
        // a pairing reply in the clear is a reply
        let pairing = wire::DataPayload {
            action: action::PAIRING_INIT,
            ..inter
        };
        relay.handle_item(&data_item("e2", &pairing));
        assert_eq!(rrx.try_recv().unwrap().clear, vec![1, 2]);
    }

    #[tokio::test]
    async fn logged_out_notice_and_revocation() {
        let (relay, mut rx, _keys) = setup(true);
        let notice = wire::DataPayload {
            action: action::UPDATES,
            clear: vec![0x72, 0x00],
            ..Default::default()
        };
        relay.handle_item(&data_item("", &notice));
        let ev = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap();
        assert!(matches!(
            ev,
            Some(Event::LoggedOut(LogoutReason::AccountLoggedOut))
        ));
        assert!(relay.is_shut());

        let (relay, mut rx, _keys) = setup(false);
        let ev = wire::PairingEvent {
            revoked: Some(wire::Revoked::default()),
            ..Default::default()
        };
        relay.handle_item(&item("p1", ROUTE_PAIRING, codec::to_binary(&ev)));
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::LoggedOut(LogoutReason::Unpaired))
        ));
    }

    #[test]
    fn backoff_policy() {
        let a = backoff(1);
        assert!(a >= Duration::from_secs(10) && a < Duration::from_secs(15));
        let b = backoff(3);
        assert!(b >= Duration::from_secs(20) && b < Duration::from_secs(25));
        assert!(backoff(1000) <= Duration::from_secs(300));
    }
}
