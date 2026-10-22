//! Public data types: conversations, messages, participants, contacts, settings…
//!
//! These are built from the wire messages ([`crate::wire`]). Enumerations keep unknown raw
//! values (`Other(i32)` variants, or newtypes over `i32` with associated constants).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::wire;

/// A timestamp in **microseconds** since the Unix epoch (the protocol's unit).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// From microseconds.
    pub fn from_micros(us: i64) -> Self {
        Timestamp(us)
    }
    /// From milliseconds.
    pub fn from_millis(ms: i64) -> Self {
        Timestamp(ms.saturating_mul(1000))
    }
    /// Now.
    pub fn now() -> Self {
        Timestamp(crate::session::now_us())
    }
    /// Microseconds since the epoch.
    pub fn as_micros(self) -> i64 {
        self.0
    }
    /// Milliseconds since the epoch.
    pub fn as_millis(self) -> i64 {
        self.0.div_euclid(1000)
    }
    /// Seconds since the epoch.
    pub fn as_secs(self) -> i64 {
        self.0.div_euclid(1_000_000)
    }
    /// Whether unset (zero).
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    /// As a [`SystemTime`].
    pub fn to_system_time(self) -> SystemTime {
        if self.0 >= 0 {
            UNIX_EPOCH + Duration::from_micros(self.0 as u64)
        } else {
            UNIX_EPOCH - Duration::from_micros(self.0.unsigned_abs())
        }
    }
}

macro_rules! open_enum {
    ($(#[$m:meta])* pub enum $name:ident { $( $(#[$vm:meta])* $var:ident = $val:literal ),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(from = "i32", into = "i32")]
        pub enum $name {
            $( $(#[$vm])* $var, )*
            /// A value not known to this crate.
            Other(i32),
        }
        impl From<i32> for $name {
            fn from(v: i32) -> Self {
                match v { $( $val => $name::$var, )* o => $name::Other(o) }
            }
        }
        impl From<$name> for i32 {
            fn from(v: $name) -> i32 {
                match v { $( $name::$var => $val, )* $name::Other(o) => o }
            }
        }
        impl Default for $name {
            fn default() -> Self { $name::from(0) }
        }
        impl $name {
            /// The raw protocol value.
            pub fn value(self) -> i32 { self.into() }
        }
    };
}

macro_rules! newtype_enum {
    ($(#[$m:meta])* pub struct $name:ident; $( $(#[$cm:meta])* $c:ident = $v:literal ),* $(,)?) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub i32);
        impl $name {
            $( $(#[$cm])* pub const $c: $name = $name($v); )*
        }
        impl From<i32> for $name { fn from(v: i32) -> Self { $name(v) } }
        impl From<$name> for i32 { fn from(v: $name) -> i32 { v.0 } }
    };
}

/// A folder to list conversations from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Folder {
    /// Inbox (value 1).
    Inbox,
    /// Archive (value 2).
    Archive,
    /// Spam and blocked (value 5).
    SpamAndBlocked,
    /// Trash (value 8).
    Trash,
}

impl Folder {
    /// The raw protocol value.
    pub fn value(self) -> i32 {
        match self {
            Folder::Inbox => 1,
            Folder::Archive => 2,
            Folder::SpamAndBlocked => 5,
            Folder::Trash => 8,
        }
    }
}

open_enum! {
    /// Folder or state of a conversation.
    pub enum ConversationStatus {
        /// 0.
        Unknown = 0,
        /// In the inbox.
        Active = 1,
        /// Archived.
        Archived = 2,
        /// Deleted for good on the phone.
        Deleted = 3,
        /// Archived, kept archived when new messages arrive.
        KeepArchived = 4,
        /// In the spam folder.
        Spam = 5,
        /// In the blocked folder.
        Blocked = 6,
        /// Cross-country folder.
        CrossCountry = 7,
        /// In the trash.
        Trash = 8,
    }
}

impl ConversationStatus {
    /// The folder this status belongs to, if any.
    pub fn folder(self) -> Option<Folder> {
        match self {
            ConversationStatus::Active => Some(Folder::Inbox),
            ConversationStatus::Archived | ConversationStatus::KeepArchived => {
                Some(Folder::Archive)
            }
            ConversationStatus::Spam | ConversationStatus::Blocked => Some(Folder::SpamAndBlocked),
            ConversationStatus::Trash => Some(Folder::Trash),
            _ => None,
        }
    }
}

open_enum! {
    /// SMS/MMS or RCS.
    pub enum ConversationType {
        /// 0.
        Unknown = 0,
        /// SMS/MMS.
        Sms = 1,
        /// RCS.
        Rcs = 2,
    }
}

open_enum! {
    /// How a conversation sends.
    pub enum SendMode {
        /// Automatic.
        Auto = 0,
        /// SMS/MMS only.
        SmsOnly = 1,
        /// SMS/MMS (latched).
        SmsLatched = 2,
    }
}

open_enum! {
    /// Result of sending a message.
    pub enum SendResult {
        /// 0.
        Unknown = 0,
        /// The phone took the message.
        Accepted = 1,
        /// Failed, transient.
        FailedTransient = 2,
        /// Failed, transient (another code).
        FailedTransient2 = 3,
        /// Failed (possibly: Google Messages not the default SMS app).
        Failed = 4,
    }
}

impl SendResult {
    /// Whether a retry may succeed (values 2 and 3).
    pub fn is_transient(self) -> bool {
        matches!(
            self,
            SendResult::FailedTransient | SendResult::FailedTransient2
        )
    }
}

open_enum! {
    /// An alert pushed by the phone.
    pub enum Alert {
        /// 0.
        Unknown = 0,
        /// This browser session became inactive (another session took over).
        BrowserInactive = 1,
        /// This browser session became active.
        BrowserActive = 2,
        /// Phone on mobile data.
        MobileData = 3,
        /// Phone on Wi-Fi.
        Wifi = 4,
        /// Phone battery low.
        BatteryLow = 5,
        /// Phone battery no longer low.
        BatteryOk = 6,
        /// This browser session inactive: timeout.
        BrowserInactiveTimeout = 7,
        /// This browser session inactive: inactivity.
        BrowserInactiveInactivity = 8,
        /// RCS connected.
        RcsConnected = 9,
        /// Observer registered (unknown meaning).
        ObserverRegistered = 10,
        /// Phone database syncing.
        Syncing = 11,
        /// Phone database sync complete.
        SyncComplete = 12,
        /// Phone database sync started.
        SyncStarted = 13,
        /// Phone database partial sync complete.
        PartialSyncComplete = 14,
        /// Phone database partial sync started.
        PartialSyncStarted = 15,
        /// Contacts refresh started.
        ContactsRefreshStarted = 16,
        /// Contacts refresh complete.
        ContactsRefreshComplete = 17,
        /// Disconnected from satellite.
        SatelliteDisconnected = 18,
        /// Restoring messages from backup.
        Restoring = 19,
        /// Restore complete.
        RestoreComplete = 20,
        /// Restore started.
        RestoreStarted = 21,
        /// Push throttling.
        PushThrottling = 22,
        /// Push throttling started.
        PushThrottlingStarted = 23,
        /// Push throttling ended.
        PushThrottlingEnded = 24,
    }
}

impl Alert {
    /// Whether this alert says this browser session is no longer the active one
    /// (values 1, 7, 8).
    pub fn is_browser_inactive(self) -> bool {
        matches!(
            self,
            Alert::BrowserInactive
                | Alert::BrowserInactiveTimeout
                | Alert::BrowserInactiveInactivity
        )
    }
}

newtype_enum! {
    /// Status of a message (spec §7 "Message status"). Ranges: 1–99 our messages,
    /// 100–199 received, 200–299 system notices, 300 deleted.
    pub struct MessageStatus;
    /// Sent.
    SENT = 1,
    /// Delivered.
    DELIVERED = 2,
    /// Draft.
    DRAFT = 3,
    /// Waiting to be sent.
    WAITING = 4,
    /// Sending.
    SENDING = 5,
    /// Resending.
    RESENDING = 6,
    /// Waiting to retry.
    WAITING_RETRY = 7,
    /// Failed.
    FAILED = 8,
    /// Failed: emergency number.
    FAILED_EMERGENCY = 9,
    /// Sent after processing (pending).
    SENT_AFTER_PROCESSING = 10,
    /// Read by the recipient.
    READ = 11,
    /// Cancelled.
    CANCELLED = 12,
    /// Failed: too large.
    FAILED_TOO_LARGE = 13,
    /// Not delivered yet.
    NOT_DELIVERED_YET = 14,
    /// Revocation pending.
    REVOCATION_PENDING = 15,
    /// Scheduled.
    SCHEDULED = 16,
    /// Being validated.
    VALIDATING = 20,
    /// Deleted (our message).
    OUTGOING_DELETED = 23,
    /// Restricted (not sent).
    RESTRICTED = 26,
    /// Received (complete).
    RECEIVED = 100,
    /// Waiting for a manual download (MMS).
    WAITING_MANUAL_DOWNLOAD = 101,
    /// Download failed.
    DOWNLOAD_FAILED = 106,
    /// Expired or no longer available.
    EXPIRED = 107,
    /// Delivered (received side).
    RECEIVED_DELIVERED = 108,
    /// Displayed (read).
    RECEIVED_DISPLAYED = 109,
    /// Deleted (received message).
    RECEIVED_DELETED = 117,
    /// Deleted.
    DELETED = 300,
}

impl MessageStatus {
    /// Our message (1–99).
    pub fn is_outgoing(self) -> bool {
        (1..=99).contains(&self.0)
    }
    /// A received message (100–199).
    pub fn is_incoming(self) -> bool {
        (100..=199).contains(&self.0)
    }
    /// A system notice (200–299).
    pub fn is_system_notice(self) -> bool {
        (200..=299).contains(&self.0)
    }
    /// Deleted (300, or the "deleted" values of the outgoing and incoming ranges).
    pub fn is_deleted(self) -> bool {
        matches!(self.0, 300 | 23 | 117)
    }
    /// One of the "failed" statuses of our messages (shown as "Not sent").
    pub fn is_failed(self) -> bool {
        matches!(self.0, 8 | 9 | 13 | 17 | 18 | 19 | 21 | 22 | 24 | 25 | 27)
    }
    /// Our message, not sent (failed, cancelled or restricted).
    pub fn is_not_sent(self) -> bool {
        self.is_failed() || matches!(self.0, 12 | 26)
    }
    /// Our message still pending (waiting, sending, retrying, scheduled…).
    pub fn is_pending(self) -> bool {
        matches!(self.0, 4 | 5 | 6 | 7 | 10 | 16 | 20)
    }
    /// Our message sent (sent, delivered, read, not delivered yet).
    pub fn is_sent(self) -> bool {
        matches!(self.0, 1 | 2 | 11 | 14)
    }
    /// Our message read by the recipient.
    pub fn is_read(self) -> bool {
        self.0 == 11
    }
    /// A received MMS not yet downloaded by the phone (101–106).
    pub fn is_awaiting_download(self) -> bool {
        (101..=106).contains(&self.0)
    }
    /// A system notice that clients usually hide (protocol switches, creation notices,
    /// link previews, theme and identity changes).
    pub fn is_technical_notice(self) -> bool {
        matches!(self.0, 203..=207 | 210 | 213..=216 | 219 | 229 | 233..=238 | 245 | 259)
    }
}

newtype_enum! {
    /// Kind of a participant identifier (spec §7 `IdentifierKind`).
    pub struct IdentifierKind;
    /// Unknown.
    UNKNOWN = 0,
    /// Phone number, E.164.
    PHONE = 1,
    /// E-mail address.
    EMAIL = 2,
    /// Bot.
    BOT = 3,
}

newtype_enum! {
    /// Media format (spec §7 `MediaFormat`).
    pub struct MediaFormat;
    /// Unspecified.
    UNSPECIFIED = 0,
    /// Image, other.
    IMAGE_OTHER = 7,
    /// Video, other.
    VIDEO_OTHER = 13,
    /// Audio, other.
    AUDIO_OTHER = 23,
    /// `text/plain`.
    TEXT_PLAIN = 26,
    /// Application, other.
    APPLICATION_OTHER = 37,
}

const MIME_TABLE: &[(i32, &str)] = &[
    (1, "image/jpeg"),
    (2, "image/jpg"),
    (3, "image/png"),
    (4, "image/gif"),
    (5, "image/wbmp"),
    (6, "image/bmp"),
    (6, "image/x-ms-bmp"),
    (8, "video/mp4"),
    (9, "video/3gpp2"),
    (10, "video/3gpp"),
    (11, "video/webm"),
    (12, "video/x-matroska"),
    (14, "audio/aac"),
    (15, "audio/amr"),
    (16, "audio/mp3"),
    (17, "audio/mpeg"),
    (18, "audio/mpg"),
    (19, "audio/mp4"),
    (20, "audio/mp4-latm"),
    (21, "audio/3gpp"),
    (22, "audio/ogg"),
    (24, "text/vcard"),
    (25, "application/pdf"),
    (26, "text/plain"),
    (27, "text/html"),
    (28, "application/msword"),
    (
        29,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        30,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    (31, "application/vnd.ms-powerpoint"),
    (
        32,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (33, "application/vnd.ms-excel"),
    (34, "application/vnd.android.package-archive"),
    (35, "application/zip"),
    (36, "application/java-archive"),
    (38, "text/x-calendar"),
    (39, "text/x-calendar"),
    (40, "text/calendar"),
    (41, "text/x-calendar"),
    (42, "text/calendar"),
];

impl MediaFormat {
    /// The format value for a MIME type: its own value, else the "other" value of its
    /// top-level type (`text/*` → `text/plain`).
    pub fn from_mime(mime: &str) -> MediaFormat {
        let m = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if let Some((v, _)) = MIME_TABLE.iter().find(|(_, s)| *s == m) {
            return MediaFormat(*v);
        }
        match m.split('/').next().unwrap_or("") {
            "image" => MediaFormat::IMAGE_OTHER,
            "video" => MediaFormat::VIDEO_OTHER,
            "audio" => MediaFormat::AUDIO_OTHER,
            "text" => MediaFormat::TEXT_PLAIN,
            _ => MediaFormat::APPLICATION_OTHER,
        }
    }

    /// The MIME type of this format, when it names one.
    pub fn mime(self) -> Option<&'static str> {
        MIME_TABLE
            .iter()
            .find(|(v, _)| *v == self.0)
            .map(|(_, m)| *m)
    }

    /// Whether the format is an image (1–7).
    pub fn is_image(self) -> bool {
        (1..=7).contains(&self.0)
    }
    /// Whether the format is a video (8–13).
    pub fn is_video(self) -> bool {
        (8..=13).contains(&self.0)
    }
    /// Whether the format is audio (14–23).
    pub fn is_audio(self) -> bool {
        (14..=23).contains(&self.0)
    }
}

newtype_enum! {
    /// Kind of a reaction (spec §7 `ReactionKind`).
    pub struct ReactionKind;
    /// Unspecified.
    UNSPECIFIED = 0,
    /// 👍
    THUMBS_UP = 1,
    /// 😍
    LOVE = 2,
    /// 😂
    LAUGH = 3,
    /// 😮
    SURPRISED = 4,
    /// 😥
    SAD = 5,
    /// 😠
    ANGRY = 6,
    /// 👎
    THUMBS_DOWN = 7,
    /// Any other emoji (in the reaction's `emoji` field).
    CUSTOM = 8,
    /// 🤔
    THINKING = 9,
    /// 😢
    CRYING = 10,
    /// 😡
    MAD = 11,
    /// ❤️
    HEART = 12,
}

const REACTION_EMOJI: &[(i32, &str)] = &[
    (1, "👍"),
    (2, "😍"),
    (3, "😂"),
    (4, "😮"),
    (5, "😥"),
    (6, "😠"),
    (7, "👎"),
    (9, "🤔"),
    (10, "😢"),
    (11, "😡"),
    (12, "❤️"),
];

impl ReactionKind {
    /// The kind to send for an emoji: its named kind, else [`ReactionKind::CUSTOM`].
    pub fn for_emoji(emoji: &str) -> ReactionKind {
        let e = emoji.trim_end_matches('\u{fe0f}');
        REACTION_EMOJI
            .iter()
            .find(|(_, s)| s.trim_end_matches('\u{fe0f}') == e)
            .map_or(ReactionKind::CUSTOM, |(v, _)| ReactionKind(*v))
    }
    /// The emoji of a named kind.
    pub fn emoji(self) -> Option<&'static str> {
        REACTION_EMOJI
            .iter()
            .find(|(v, _)| *v == self.0)
            .map(|(_, e)| *e)
    }
}

newtype_enum! {
    /// Error code of a Google account pairing (spec §4 `PairingError`).
    pub struct PairingErrorCode;
    /// No error.
    NONE = 0,
    /// Wrong emoji chosen on the phone.
    WRONG_EMOJI = 5,
    /// Verification timed out.
    VERIFICATION_TIMED_OUT = 6,
    /// User cancelled on the phone.
    USER_CANCELLED = 7,
    /// User answered "this is not me".
    NOT_ME = 27,
}

/// Which SIM sends (`SimSelector`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SimSelector {
    /// Field 1 (meaning unknown, usually 2).
    pub field1: i32,
    /// SIM number.
    pub sim_number: i32,
}

impl From<wire::SimSelector> for SimSelector {
    fn from(s: wire::SimSelector) -> Self {
        SimSelector {
            field1: s.unknown1,
            sim_number: s.sim_number,
        }
    }
}

impl From<SimSelector> for wire::SimSelector {
    fn from(s: SimSelector) -> Self {
        wire::SimSelector {
            unknown1: s.field1,
            sim_number: s.sim_number,
        }
    }
}

/// A SIM card of the phone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimCard {
    /// Selector to send with this SIM.
    pub selector: Option<SimSelector>,
    /// RCS chats enabled on this SIM (absent: unknown).
    pub rcs_enabled: Option<bool>,
    /// Perhaps the default SIM.
    pub is_default: bool,
    /// Carrier name.
    pub carrier: String,
    /// Hex colour.
    pub colour: String,
    /// Our number, formatted.
    pub formatted_number: String,
    /// Our number, international.
    pub number: String,
    /// Our participant id for this SIM.
    pub participant_id: Option<String>,
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

impl From<wire::SimCard> for SimCard {
    fn from(s: wire::SimCard) -> Self {
        let data = s.data.unwrap_or_default();
        SimCard {
            selector: data.selector.map(Into::into),
            rcs_enabled: s.rcs.map(|r| r.enabled),
            is_default: data.is_default,
            carrier: data.carrier,
            colour: data.colour,
            formatted_number: data.formatted_number,
            number: data.number,
            participant_id: s.self_ref.and_then(|r| non_empty(r.id)),
        }
    }
}

/// A participant of a conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    /// Participant id, used as `sender_id` in messages.
    pub id: String,
    /// Phone number (E.164 for [`IdentifierKind::PHONE`]) or address.
    pub number: String,
    /// Kind of `number`.
    pub kind: IdentifierKind,
    /// First name.
    pub first_name: String,
    /// The contact's name, or the formatted number.
    pub full_name: String,
    /// Hex avatar colour.
    pub colour: String,
    /// This is us.
    pub is_me: bool,
    /// False for internal entries, to skip when listing members.
    pub visible: bool,
    /// Id in the address book (for avatars).
    pub contact_id: String,
    /// Formatted number.
    pub formatted_number: String,
    /// SIM, for our own entries.
    pub sim: Option<SimSelector>,
}

impl Participant {
    /// A name to display: full name, else first name, else formatted number, else number.
    pub fn display_name(&self) -> &str {
        [
            &self.full_name,
            &self.first_name,
            &self.formatted_number,
            &self.number,
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .map_or("", |s| s.as_str())
    }
}

impl From<wire::Participant> for Participant {
    fn from(p: wire::Participant) -> Self {
        let id = p.id.unwrap_or_default();
        Participant {
            id: id.participant_id,
            number: id.number,
            kind: IdentifierKind(id.kind),
            first_name: p.first_name,
            full_name: p.full_name,
            colour: p.colour,
            is_me: p.is_me,
            visible: p.visible,
            contact_id: p.contact_id,
            formatted_number: p.formatted_number,
            sim: p.sim.map(Into::into),
        }
    }
}

/// The latest message of a conversation, as shown in a list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    /// Text as shown in the list.
    pub text: String,
    /// We sent it.
    pub from_me: bool,
    /// Sender name.
    pub sender_name: String,
    /// Status of that message.
    pub status: MessageStatus,
}

/// A conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    /// Conversation id (a decimal number as a string, local to the phone).
    pub id: String,
    /// Display name: the contact's name, the number, or the group name.
    pub name: String,
    /// The latest message.
    pub preview: Option<Preview>,
    /// Timestamp of the latest message.
    pub last_activity: Timestamp,
    /// Unread.
    pub unread: bool,
    /// A group conversation.
    pub is_group: bool,
    /// Our participant id in this conversation (sender of our messages).
    pub our_participant_id: String,
    /// Folder or state.
    pub status: ConversationStatus,
    /// Cannot be answered.
    pub read_only: bool,
    /// Hex avatar colour.
    pub colour: String,
    /// Id of the latest message.
    pub latest_message_id: String,
    /// Send mode.
    pub send_mode: SendMode,
    /// Participants, including us.
    pub participants: Vec<Participant>,
    /// Participant ids other than us.
    pub other_participant_ids: Vec<String>,
    /// SMS/MMS or RCS.
    pub kind: ConversationType,
    /// Pinned.
    pub pinned: bool,
    /// Our participant id for the SIM used.
    pub sim_participant_id: Option<String>,
    /// The SIM this conversation uses.
    pub sim: Option<SimCard>,
    /// URL of an RCS group's picture.
    pub group_avatar_url: Option<String>,
}

impl Conversation {
    /// The folder the conversation is in.
    pub fn folder(&self) -> Option<Folder> {
        self.status.folder()
    }
    /// Deleted for good on the phone.
    pub fn is_deleted(&self) -> bool {
        self.status == ConversationStatus::Deleted
    }
    /// Participants to show as members (visible ones).
    pub fn visible_participants(&self) -> impl Iterator<Item = &Participant> {
        self.participants.iter().filter(|p| p.visible)
    }
    /// The other person: the first visible participant that is not us.
    pub fn other_participant(&self) -> Option<&Participant> {
        self.participants.iter().find(|p| p.visible && !p.is_me)
    }
    /// The other person's number, for one-to-one conversations (used to block, unblock
    /// and delete).
    pub fn other_number(&self) -> Option<&str> {
        if self.is_group {
            return None;
        }
        self.other_participant()
            .map(|p| p.number.as_str())
            .filter(|n| !n.is_empty())
    }
    /// The SIM selector to send with (`sim.data.selector`).
    pub fn sim_selector(&self) -> Option<SimSelector> {
        self.sim.as_ref().and_then(|s| s.selector)
    }
}

impl From<wire::Conversation> for Conversation {
    fn from(c: wire::Conversation) -> Self {
        Conversation {
            id: c.id,
            name: c.name,
            preview: c.preview.map(|p| Preview {
                text: p.text,
                from_me: p.from_me != 0,
                sender_name: p.sender_name,
                status: MessageStatus(p.status.map_or(0, |s| s.status)),
            }),
            last_activity: Timestamp(c.last_activity),
            unread: c.unread,
            is_group: c.group,
            our_participant_id: c.outgoing_id,
            status: c.status.into(),
            read_only: c.read_only,
            colour: c.colour,
            latest_message_id: c.latest_message_id,
            send_mode: c.send_mode.into(),
            participants: c.participants.into_iter().map(Into::into).collect(),
            other_participant_ids: c.others,
            kind: c.kind.into(),
            pinned: c.pinned,
            sim_participant_id: c.sim_participant.and_then(|r| non_empty(r.id)),
            sim: c.sim.map(Into::into),
            group_avatar_url: non_empty(c.group_avatar_url),
        }
    }
}

/// Width and height.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimensions {
    /// Width.
    pub width: i64,
    /// Height.
    pub height: i64,
}

/// What is available of a media part (spec §8 "Previews and full size").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAvailability {
    /// The full file: download `media_id` with `key`.
    Full,
    /// Only a preview uploaded: download `preview_id` with `preview_key`, or ask for the
    /// full file ([`Client::request_full_size_image`](crate::Client::request_full_size_image)).
    PreviewOnly,
    /// Only the small inline preview, in the clear.
    InlinePreviewOnly,
    /// Nothing yet: the phone is still downloading or uploading it.
    Pending,
}

/// A file attached to a message.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    /// Format.
    pub format: MediaFormat,
    /// Id of the full file on the upload service (empty if not available).
    pub media_id: String,
    /// File name.
    pub file_name: String,
    /// Plaintext size in bytes.
    pub size: i64,
    /// Dimensions.
    pub dimensions: Option<Dimensions>,
    /// Small preview image, in the clear.
    pub inline_preview: Vec<u8>,
    /// Id of a preview on the upload service.
    pub preview_id: String,
    /// Key of `media_id` (32 bytes).
    pub key: Vec<u8>,
    /// Key of `preview_id` (32 bytes).
    pub preview_key: Vec<u8>,
    /// MIME type (authoritative when present).
    pub mime: String,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("format", &self.format)
            .field("media_id", &self.media_id)
            .field("file_name", &self.file_name)
            .field("size", &self.size)
            .field("dimensions", &self.dimensions)
            .field("inline_preview", &self.inline_preview.len())
            .field("preview_id", &self.preview_id)
            .field("mime", &self.mime)
            .finish_non_exhaustive()
    }
}

impl Media {
    /// What can be fetched of this media now.
    pub fn availability(&self) -> MediaAvailability {
        if !self.media_id.is_empty() {
            MediaAvailability::Full
        } else if !self.preview_id.is_empty() {
            MediaAvailability::PreviewOnly
        } else if !self.inline_preview.is_empty() {
            MediaAvailability::InlinePreviewOnly
        } else {
            MediaAvailability::Pending
        }
    }
    /// The MIME type: `mime` when present, else the one of `format`.
    pub fn mime_type(&self) -> &str {
        if !self.mime.is_empty() {
            &self.mime
        } else {
            self.format.mime().unwrap_or("application/octet-stream")
        }
    }
}

impl From<wire::Media> for Media {
    fn from(m: wire::Media) -> Self {
        Media {
            format: MediaFormat(m.format),
            media_id: m.media_id,
            file_name: m.file_name,
            size: m.size,
            dimensions: m.dimensions.map(|d| Dimensions {
                width: d.width,
                height: d.height,
            }),
            inline_preview: m.inline_preview,
            preview_id: m.preview_id,
            key: m.key,
            preview_key: m.preview_key,
            mime: m.mime,
        }
    }
}

impl From<&Media> for wire::Media {
    fn from(m: &Media) -> Self {
        wire::Media {
            format: m.format.0,
            media_id: m.media_id.clone(),
            file_name: m.file_name.clone(),
            size: m.size,
            dimensions: m.dimensions.map(|d| wire::Dimensions {
                width: d.width,
                height: d.height,
            }),
            inline_preview: m.inline_preview.clone(),
            preview_id: m.preview_id.clone(),
            key: m.key.clone(),
            preview_key: m.preview_key.clone(),
            mime: m.mime.clone(),
        }
    }
}

/// Content of a message part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartContent {
    /// Text.
    Text(String),
    /// A file.
    Media(Media),
    /// Neither (unknown content).
    Empty,
}

/// One part of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagePart {
    /// Part id (used to ask for a full-size image).
    pub part_id: Option<String>,
    /// Content.
    pub content: PartContent,
}

impl From<wire::MessagePart> for MessagePart {
    fn from(p: wire::MessagePart) -> Self {
        let content = if let Some(m) = p.media {
            PartContent::Media(m.into())
        } else if let Some(t) = p.text {
            PartContent::Text(t.text)
        } else {
            PartContent::Empty
        };
        MessagePart {
            part_id: p.part_id,
            content,
        }
    }
}

/// Reactions with one emoji on a message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionGroup {
    /// The emoji (for named kinds, the emoji of the kind if the field is empty).
    pub emoji: String,
    /// Kind.
    pub kind: ReactionKind,
    /// Who reacted so.
    pub participant_ids: Vec<String>,
    /// Image reaction: its UUID.
    pub custom_id: Option<String>,
}

impl From<wire::ReactionGroup> for ReactionGroup {
    fn from(g: wire::ReactionGroup) -> Self {
        let r = g.reaction.unwrap_or_default();
        let kind = ReactionKind(r.kind);
        let emoji = if r.emoji.is_empty() {
            kind.emoji().unwrap_or("").to_string()
        } else {
            r.emoji
        };
        ReactionGroup {
            emoji,
            kind,
            participant_ids: g.participant_ids,
            custom_id: r.custom.and_then(|c| non_empty(c.uuid)),
        }
    }
}

/// A message. Messages are upserted: the phone sends the whole message again on every
/// change; key them by `id`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Message id.
    pub id: String,
    /// Conversation id.
    pub conversation_id: String,
    /// Participant id of the sender (ours: [`Conversation::our_participant_id`]).
    pub sender_id: String,
    /// Timestamp.
    pub timestamp: Timestamp,
    /// Status.
    pub status: MessageStatus,
    /// Error detail of the status.
    pub status_sub_code: i64,
    /// Error text of the status.
    pub status_error: String,
    /// Text to show for system notices.
    pub status_text: String,
    /// Content.
    pub parts: Vec<MessagePart>,
    /// 1 SMS, 2 MMS downloaded, 3 MMS not downloaded, 4 RCS (believed).
    pub transport: i64,
    /// For our messages sent from a web client: the `tmp_id` of the send call.
    pub tmp_id: Option<String>,
    /// MMS subject.
    pub subject: Option<String>,
    /// Reactions.
    pub reactions: Vec<ReactionGroup>,
    /// Id of the message replied to.
    pub reply_to: Option<String>,
    /// Sender details, sometimes present.
    pub sender: Option<Participant>,
}

impl Message {
    /// We sent it (status 1–99).
    pub fn is_ours(&self) -> bool {
        self.status.is_outgoing()
    }
    /// Received (status 100–199).
    pub fn is_incoming(&self) -> bool {
        self.status.is_incoming()
    }
    /// Our message failed ("Not sent").
    pub fn is_failed(&self) -> bool {
        self.status.is_failed()
    }
    /// Our message is still being sent.
    pub fn is_pending(&self) -> bool {
        self.status.is_pending()
    }
    /// A system notice (no sender).
    pub fn is_system_notice(&self) -> bool {
        self.status.is_system_notice()
    }
    /// Deleted.
    pub fn is_deleted(&self) -> bool {
        self.status.is_deleted()
    }
    /// A deletion by the system (status 300 with no sender).
    pub fn is_system_deletion(&self) -> bool {
        self.status == MessageStatus::DELETED && self.sender_id.is_empty()
    }
    /// The text parts joined with newlines.
    pub fn text(&self) -> String {
        let texts: Vec<&str> = self
            .parts
            .iter()
            .filter_map(|p| match &p.content {
                PartContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        texts.join("\n")
    }
    /// The media parts with their part ids.
    pub fn media(&self) -> impl Iterator<Item = (Option<&str>, &Media)> {
        self.parts.iter().filter_map(|p| match &p.content {
            PartContent::Media(m) => Some((p.part_id.as_deref(), m)),
            _ => None,
        })
    }
}

impl From<wire::Message> for Message {
    fn from(m: wire::Message) -> Self {
        let st = m.status.unwrap_or_default();
        Message {
            id: m.id,
            conversation_id: m.conversation_id,
            sender_id: m.sender_id,
            timestamp: Timestamp(m.timestamp),
            status: MessageStatus(st.status),
            status_sub_code: st.sub_code,
            status_error: st.error,
            status_text: st.status_text,
            parts: m.parts.into_iter().map(Into::into).collect(),
            transport: m.transport,
            tmp_id: non_empty(m.tmp_id),
            subject: m.subject,
            reactions: m.reactions.into_iter().map(Into::into).collect(),
            reply_to: m.reply_to.and_then(|r| non_empty(r.message_id)),
            sender: m.sender.map(Into::into),
        }
    }
}

/// An address book entry (one per number).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    /// Participant id.
    pub participant_id: String,
    /// Name.
    pub name: String,
    /// Number.
    pub number: String,
    /// Formatted number.
    pub formatted_number: Option<String>,
    /// Hex avatar colour.
    pub colour: String,
    /// Contact id (for thumbnails).
    pub contact_id: String,
}

impl From<wire::Contact> for Contact {
    fn from(c: wire::Contact) -> Self {
        let n = c.number.unwrap_or_default();
        Contact {
            participant_id: c.participant_id,
            name: if c.name.is_empty() {
                c.name_again
            } else {
                c.name
            },
            number: if n.number.is_empty() {
                n.number_again
            } else {
                n.number
            },
            formatted_number: n.formatted,
            colour: c.colour,
            contact_id: c.contact_id,
        }
    }
}

/// Someone typing (or stopping) in a conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typing {
    /// Conversation id.
    pub conversation_id: String,
    /// Number of who types.
    pub number: String,
    /// Started (true) or stopped (false).
    pub typing: bool,
}

impl From<wire::TypingInfo> for Typing {
    fn from(t: wire::TypingInfo) -> Self {
        Typing {
            conversation_id: t.conversation_id,
            number: t.who.map(|w| w.number).unwrap_or_default(),
            typing: t.state == 1,
        }
    }
}

/// RCS settings of the phone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcsSettings {
    /// RCS on.
    pub enabled: bool,
    /// Send read receipts.
    pub read_receipts: bool,
    /// Show typing indicators.
    pub typing_indicators: bool,
    /// Google Messages is the default SMS app (absent from some updates: keep the previous
    /// value).
    pub default_sms_app: Option<bool>,
}

/// Phone settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// SIM cards.
    pub sims: Vec<SimCard>,
    /// Web push enabled (absent ≠ false).
    pub web_push_enabled: Option<bool>,
    /// RCS settings.
    pub rcs: Option<RcsSettings>,
    /// Google Messages version on the phone.
    pub app_version: String,
}

impl From<wire::Settings> for Settings {
    fn from(s: wire::Settings) -> Self {
        Settings {
            sims: s.sims.into_iter().map(Into::into).collect(),
            web_push_enabled: s.misc.and_then(|m| m.web_push),
            rcs: s.rcs.map(|r| RcsSettings {
                enabled: r.enabled,
                read_receipts: r.read_receipts,
                typing_indicators: r.typing,
                default_sms_app: r.default_sms_app,
            }),
            app_version: s.app_version,
        }
    }
}

/// The Google account in use on the phone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Account (an e-mail address when meaningful).
    pub account: String,
}

impl From<wire::AccountInfo> for AccountInfo {
    fn from(a: wire::AccountInfo) -> Self {
        AccountInfo { account: a.account }
    }
}

/// Paging cursor: id and timestamp of the last item of the previous page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Id of the last item.
    pub last_id: String,
    /// Its timestamp (µs).
    pub timestamp: i64,
}

impl Cursor {
    pub(crate) fn from_wire(c: Option<wire::Cursor>) -> Option<Cursor> {
        c.filter(|c| !c.last_id.is_empty()).map(|c| Cursor {
            last_id: c.last_id,
            timestamp: c.timestamp,
        })
    }
    pub(crate) fn to_wire(&self) -> wire::Cursor {
        wire::Cursor {
            last_id: self.last_id.clone(),
            timestamp: self.timestamp,
        }
    }
}

/// A page of conversations.
#[derive(Debug, Clone, Default)]
pub struct ConversationPage {
    /// Conversations, most recent first.
    pub conversations: Vec<Conversation>,
    /// Cursor of the next page; `None` at the end.
    pub next: Option<Cursor>,
}

/// A page of messages.
#[derive(Debug, Clone, Default)]
pub struct MessagePage {
    /// Messages, sorted by timestamp (oldest first).
    pub messages: Vec<Message>,
    /// Approximate number of messages in the conversation.
    pub total: i64,
    /// Cursor to page further back; `None` at the start of the history.
    pub next: Option<Cursor>,
}

/// An avatar image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thumbnail {
    /// Contact or participant id.
    pub id: String,
    /// Image bytes (detect the format).
    pub image: Vec<u8>,
    /// Dimensions.
    pub dimensions: Option<Dimensions>,
}

/// Operations on a conversation (action 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationOp {
    /// Delete for good, with its messages (1).
    DeleteForever,
    /// Unblock (2).
    Unblock,
    /// Move back to the inbox (3).
    Unarchive,
    /// Archive (4).
    Archive,
    /// Block (7).
    Block,
    /// Block and report spam (8).
    BlockAndReport,
    /// Move to the trash (15).
    MoveToTrash,
}

impl ConversationOp {
    /// The raw protocol value.
    pub fn value(self) -> i32 {
        match self {
            ConversationOp::DeleteForever => 1,
            ConversationOp::Unblock => 2,
            ConversationOp::Unarchive => 3,
            ConversationOp::Archive => 4,
            ConversationOp::Block => 7,
            ConversationOp::BlockAndReport => 8,
            ConversationOp::MoveToTrash => 15,
        }
    }
    /// Whether the operation sends the other person's number.
    pub fn uses_number(self) -> bool {
        matches!(
            self,
            ConversationOp::DeleteForever
                | ConversationOp::Unblock
                | ConversationOp::Block
                | ConversationOp::BlockAndReport
        )
    }
    /// Whether the operation sends the block flag.
    pub fn is_block(self) -> bool {
        matches!(self, ConversationOp::Block | ConversationOp::BlockAndReport)
    }
}

/// What to do with a reaction (action 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReactionOp {
    /// Add (1).
    Add,
    /// Remove (2).
    Remove,
    /// Replace our previous reaction (3).
    Replace,
}

impl ReactionOp {
    /// The raw protocol value.
    pub fn value(self) -> i32 {
        match self {
            ReactionOp::Add => 1,
            ReactionOp::Remove => 2,
            ReactionOp::Replace => 3,
        }
    }
}

/// Outcome of [`Client::get_or_create_conversation`](crate::Client::get_or_create_conversation).
#[derive(Debug, Clone)]
pub enum GetOrCreateOutcome {
    /// Found or created.
    Conversation(Box<Conversation>),
    /// An RCS group would be created: call again with confirmation.
    NeedsRcsGroupConfirmation,
}

/// A message to send. Build it with [`OutgoingMessage::new`] from the conversation.
#[derive(Debug, Clone, Default)]
pub struct OutgoingMessage {
    /// Conversation id.
    pub conversation_id: String,
    /// Our participant id in the conversation ([`Conversation::our_participant_id`]).
    pub sender_id: String,
    /// SIM to send with ([`Conversation::sim_selector`]).
    pub sim: Option<SimSelector>,
    /// Text (a caption when there is media). Empty: none.
    pub text: String,
    /// Files, from [`Client::upload_media`](crate::Client::upload_media).
    pub media: Vec<Media>,
    /// Id of the message replied to.
    pub reply_to: Option<String>,
    /// Send as RCS even if the phone would fall back. Normally false.
    pub force_rcs: bool,
    /// Temporary id; a fresh UUID when `None`.
    pub tmp_id: Option<String>,
}

impl OutgoingMessage {
    /// A text message to a conversation.
    pub fn new(conversation: &Conversation, text: impl Into<String>) -> Self {
        OutgoingMessage {
            conversation_id: conversation.id.clone(),
            sender_id: conversation.our_participant_id.clone(),
            sim: conversation.sim_selector(),
            text: text.into(),
            ..Default::default()
        }
    }
    /// Adds a file.
    pub fn with_media(mut self, media: Media) -> Self {
        self.media.push(media);
        self
    }
    /// Sets the message replied to.
    pub fn reply_to(mut self, message_id: impl Into<String>) -> Self {
        self.reply_to = Some(message_id.into());
        self
    }
}

/// Result of sending a message.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    /// The temporary id; the message arrives later as an upsert with this `tmp_id`.
    pub tmp_id: String,
    /// Result reported by the phone.
    pub result: SendResult,
    /// Account reported with the reply.
    pub account: Option<AccountInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_keep_raw_values() {
        assert_eq!(ConversationStatus::from(99), ConversationStatus::Other(99));
        assert_eq!(i32::from(ConversationStatus::Other(99)), 99);
        assert_eq!(Alert::from(7), Alert::BrowserInactiveTimeout);
        assert!(Alert::from(8).is_browser_inactive());
        let j = serde_json::to_string(&Alert::Other(42)).unwrap();
        assert_eq!(j, "42");
        assert_eq!(
            serde_json::from_str::<Alert>("1").unwrap(),
            Alert::BrowserInactive
        );
    }

    #[test]
    fn message_status_helpers() {
        assert!(MessageStatus(8).is_failed() && MessageStatus(8).is_outgoing());
        assert!(MessageStatus(5).is_pending());
        assert!(MessageStatus(11).is_sent() && MessageStatus(11).is_read());
        assert!(MessageStatus(100).is_incoming());
        assert!(MessageStatus(215).is_system_notice() && MessageStatus(215).is_technical_notice());
        assert!(!MessageStatus(200).is_technical_notice());
        assert!(MessageStatus(300).is_deleted());
        assert!(MessageStatus(103).is_awaiting_download());
    }

    #[test]
    fn mime_formats() {
        assert_eq!(MediaFormat::from_mime("image/jpeg"), MediaFormat(1));
        assert_eq!(MediaFormat::from_mime("image/x-ms-bmp"), MediaFormat(6));
        assert_eq!(MediaFormat::from_mime("image/heic"), MediaFormat(7));
        assert_eq!(MediaFormat::from_mime("text/markdown"), MediaFormat(26));
        assert_eq!(MediaFormat::from_mime("application/x-foo"), MediaFormat(37));
        assert_eq!(
            MediaFormat::from_mime("audio/ogg; codecs=opus"),
            MediaFormat(22)
        );
        assert_eq!(MediaFormat::from_mime("text/calendar"), MediaFormat(40));
        assert_eq!(MediaFormat(41).mime(), Some("text/x-calendar"));
        assert_eq!(MediaFormat(42).mime(), Some("text/calendar"));
    }

    #[test]
    fn reaction_kinds() {
        assert_eq!(ReactionKind::for_emoji("👍"), ReactionKind::THUMBS_UP);
        assert_eq!(ReactionKind::for_emoji("❤"), ReactionKind::HEART);
        assert_eq!(ReactionKind::for_emoji("❤️"), ReactionKind::HEART);
        assert_eq!(ReactionKind::for_emoji("🦀"), ReactionKind::CUSTOM);
    }

    #[test]
    fn timestamps() {
        let t = Timestamp(1_700_000_000_123_456);
        assert_eq!(t.as_millis(), 1_700_000_000_123);
        assert_eq!(t.as_secs(), 1_700_000_000);
        assert_eq!(Timestamp::from_millis(5).as_micros(), 5000);
    }
}
