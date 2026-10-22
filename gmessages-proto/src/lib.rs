//! A client library for the Google Messages for web protocol.
//!
//! It drives an Android phone running Google Messages through Google's relay, as the
//! Messages for web client does: pairing (QR code or Google account), the event stream,
//! calls to the phone (conversations, messages, contacts, media…).
//!
//! This crate was implemented from a written specification of the protocol. It is not
//! affiliated with, endorsed by, or supported by Google.
//!
//! # Overview
//!
//! 1. Pair once with [`pair_with_qr`] or [`pair_with_google_account`]; persist the
//!    returned [`Session`] (it is `serde`-serializable).
//! 2. [`Client::connect`] with the session; read [`Event`]s from the returned receiver.
//! 3. Call the phone with the [`Client`] methods.
//! 4. Persist the session again on [`Event::SessionChanged`]; pair again on
//!    [`Event::LoggedOut`].
//!
//! ```no_run
//! # async fn demo() -> gmessages_proto::Result<()> {
//! use gmessages_proto::{CancelToken, Client, Config, Event, Folder, pair_with_qr};
//!
//! let config = Config::with_device_name("my-app");
//! let session = pair_with_qr(config.clone(), &CancelToken::new(), |url| {
//!     println!("render this as a QR code: {url}");
//! })
//! .await?;
//! let (client, mut events) = Client::connect(config, session).await?;
//! let page = client.list_conversations(Folder::Inbox, 25, None).await?;
//! while let Some(ev) = events.recv().await {
//!     if let Event::MessageUpserted { message, old: false } = ev {
//!         println!("{}: {}", message.sender_id, message.text());
//!     }
//! }
//! # let _ = page; Ok(()) }
//! ```

#![warn(missing_docs)]

pub mod codec;
pub mod crypto;
pub mod emoji;
pub mod longpoll;
pub mod types;
pub mod ukey2;
pub mod wire;

mod client;
mod config;
mod conn;
mod error;
mod event;
mod media;
mod pairing;
mod relay;
mod session;

pub use client::{Client, EventReceiver};
pub use config::Config;
pub use error::{Error, PairingError, Result};
pub use event::{Event, LogoutReason};
pub use media::MediaDownload;
pub use pairing::{
    AccountPairingOptions, CancelToken, EmojiPrompt, POST_PAIRING_DELAY, QR_MAX_CODES,
    QR_REFRESH_INTERVAL, pair_with_google_account, pair_with_qr, qr_url, unpair,
};
pub use session::{Device, Session, SessionParts};
pub use types::*;
