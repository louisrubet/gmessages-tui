//! gmessages-tui: a terminal client for Google Messages, paired with your Android phone
//! like Google Messages for web.
//!
//!   gmessages-tui [--version]
//!
//! Not affiliated with Google.

use std::error::Error;
use std::future::Future;

use chat_tui::backend::Result;
use chat_tui::data::{ChatId, Message};
use chat_tui::{Backend, Capabilities, Downloaded, Draft};

const NAME: &str = "gmessages-tui";

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("v{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("cannot create the Tokio runtime");
    let local = tokio::task::LocalSet::new();
    if let Err(e) = runtime.block_on(local.run_until(run())) {
        eprintln!("{NAME}: {e}");
        std::process::exit(1);
    }
}

async fn run() -> std::result::Result<(), Box<dyn Error>> {
    let config = chat_tui::Config::new(NAME)?;
    chat_tui::run(GMessages, Vec::new(), futures::stream::pending(), config).await?;
    Ok(())
}

/// Google Messages, for the interface. Not connected yet: no chats, nothing works.
struct GMessages;

/// The futures will own what they need (a handle to the client): `'static`, as the trait
/// wants.
#[allow(clippy::manual_async_fn)]
impl Backend for GMessages {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn send(
        &self,
        _chat: &ChatId,
        _draft: Draft,
    ) -> impl Future<Output = Result<Message>> + 'static {
        async { Err("sending is not implemented yet".into()) }
    }

    fn react(
        &self,
        _chat: &ChatId,
        _message: &Message,
        _emoji: &str,
        _remove: bool,
    ) -> impl Future<Output = Result<()>> + 'static {
        async { Err("reactions are not implemented yet".into()) }
    }

    fn delete(
        &self,
        _chat: &ChatId,
        _message: &Message,
    ) -> impl Future<Output = Result<()>> + 'static {
        async { Err("deletion is not implemented yet".into()) }
    }

    fn download(
        &self,
        _chat: &ChatId,
        _message: &Message,
    ) -> impl Future<Output = Result<Downloaded>> + 'static {
        async { Err("downloads are not implemented yet".into()) }
    }
}
