//! gmessages-tui: a terminal client for Google Messages, paired with your Android phone
//! like Google Messages for web.
//!
//!   gmessages-tui [--version]
//!
//! Not affiliated with Google.

mod gmd;

use std::error::Error;
use std::future::Future;
use std::path::Path;

use chat_tui::backend::Result;
use chat_tui::data::{ChatId, Discussion, Message};
use chat_tui::{Backend, Capabilities, Downloaded, Draft, Event};
use futures::channel::mpsc::{UnboundedReceiver, unbounded};

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
    let state = config.read_marks.parent().ok_or("no state directory")?;
    create_private_dir(state)?;
    let mut gmd = gmd::Gmd::start(&state.join("session.json"), &state.join("gmd.log")).await?;

    // Until the phone sends the conversations: pairing, if needed, in the terminal.
    let conversations = loop {
        match gmd.next().await? {
            Some(gmd::Event::Qr { url }) => {
                println!("Scan this QR code from Google Messages on your phone (Device pairing):");
                chat_tui::qr::print_qr(&url);
            }
            Some(gmd::Event::Paired) => println!("Paired, loading the conversations\u{2026}"),
            Some(gmd::Event::Ready { conversations }) => break conversations,
            Some(gmd::Event::Status { message }) => eprintln!("{message}"),
            Some(gmd::Event::Fatal { message }) => return Err(message.into()),
            Some(gmd::Event::Hello { .. }) => {}
            None => return Err("gmd stopped".into()),
        }
    };
    let discussions = conversations.into_iter().map(discussion).collect();

    let (lines, control) = gmd.split();
    let result = chat_tui::run(GMessages, discussions, events(lines), config).await;
    control.stop().await;
    Ok(result?)
}

/// The interface discussion of a conversation, with its latest message for now.
fn discussion(conversation: gmd::Conversation) -> Discussion {
    let mut discussion = Discussion::new(conversation.id, conversation.name);
    discussion.is_group = conversation.group;
    if let Some(latest) = conversation.latest {
        let sent_ms = conversation.timestamp_ms.max(0) as u64;
        let mut message = Message::mine(latest.id, latest.text, None, false);
        message.sent_ms = sent_ms;
        if let Some(time) = chrono::DateTime::from_timestamp_millis(sent_ms as i64) {
            message.timestamp = time.with_timezone(&chrono::Local).naive_local();
        }
        message.from_me = latest.from_me;
        if !latest.from_me {
            message.author = Some(latest.sender.clone());
            message.sender_name = latest.sender;
        }
        discussion.messages.push(message);
    }
    discussion
}

/// What gmd reports while the interface runs, as interface events.
fn events(
    mut lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
) -> UnboundedReceiver<Event> {
    let (events, received) = unbounded();
    tokio::task::spawn_local(async move {
        while let Ok(Some(event)) = gmd::next_event(&mut lines).await {
            let event = match event {
                gmd::Event::Status { message } => Event::Status(message),
                gmd::Event::Fatal { message } => Event::Status(format!("Disconnected: {message}")),
                _ => continue,
            };
            if events.unbounded_send(event).is_err() {
                break;
            }
        }
    });
    received
}

/// Creates `dir`, readable by the user only: the session in it gives access to the messages.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Google Messages, for the interface. Only lists the conversations so far.
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
