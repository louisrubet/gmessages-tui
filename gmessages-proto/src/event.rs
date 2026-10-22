//! Events delivered by a connected [`Client`](crate::Client).

use std::time::Duration;

use crate::session::Session;
use crate::types::{AccountInfo, Alert, Conversation, Message, Settings, Typing};

/// Why the session ended. After any of these, pair again.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogoutReason {
    /// This browser was unpaired (from the phone, or by [`Client::unpair`](crate::Client::unpair)).
    Unpaired,
    /// The Google account session was logged out.
    AccountLoggedOut,
    /// The servers rejected the credentials (token, cookies) or the pairing no longer
    /// exists.
    CredentialsRejected(String),
}

/// Something that happened on the connection or on the phone.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// The event stream is open (first connection, or recovery after
    /// [`Event::Disconnected`]).
    Connected,
    /// This client became the active browser session; pushed updates flow to it.
    Active,
    /// A conversation was created or changed (name, folder, unread, preview…).
    /// A status of [`ConversationStatus::Deleted`](crate::ConversationStatus::Deleted)
    /// means it was deleted for good.
    ConversationUpdated(Box<Conversation>),
    /// A message is new or changed. `old` is true for backlog replayed after a
    /// reconnection: apply it, but do not notify the user.
    MessageUpserted {
        /// The whole message.
        message: Box<Message>,
        /// Backlog item.
        old: bool,
    },
    /// Someone types or stopped typing.
    Typing(Typing),
    /// Phone settings (SIMs, RCS…).
    Settings(Box<Settings>),
    /// An alert from the phone. [`Alert::is_browser_inactive`] means another browser session
    /// took the active role: call [`Client::activate`](crate::Client::activate) to take it
    /// back, or stay passive.
    Alert(Alert),
    /// The Google account in use on the phone changed.
    AccountChanged(AccountInfo),
    /// The phone does not answer pings.
    PhoneNotResponding,
    /// The phone answers again after [`Event::PhoneNotResponding`].
    PhoneResponding,
    /// Whether Google Messages is the phone's default SMS app (checked at most hourly).
    DefaultSmsApp(bool),
    /// The session changed (token refreshed, cookies updated): persist this snapshot.
    SessionChanged(Box<Session>),
    /// The event stream failed; it is retried after `retry_in`.
    Disconnected {
        /// What failed.
        error: String,
        /// Delay before the next attempt.
        retry_in: Duration,
    },
    /// The session ended for good: pair again. The client stops after this event.
    LoggedOut(LogoutReason),
}
