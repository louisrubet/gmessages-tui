//! gmessages-tui: a terminal client for Google Messages, paired with your Android phone
//! like Google Messages for web.
//!
//!   gmessages-tui [--qr | --version]
//!
//! Not affiliated with Google.

mod gmd;
mod pair;
mod web;

use std::error::Error;
use std::future::Future;
use std::path::Path;

use chat_tui::backend::Result;
use chat_tui::data::{ChatId, Discussion, Message};
use chat_tui::{Backend, Capabilities, Downloaded, Draft, Event};
use futures::channel::mpsc::{UnboundedReceiver, unbounded};

const NAME: &str = "gmessages-tui";

fn main() {
    let qr = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--qr") => true,
        Some("--version") => {
            println!("v{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some(_) => {
            eprintln!("usage: {NAME} [--qr | --version]");
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("cannot create the Tokio runtime");
    let local = tokio::task::LocalSet::new();
    if let Err(e) = runtime.block_on(local.run_until(run(qr))) {
        eprintln!("{NAME}: {e}");
        std::process::exit(1);
    }
}

/// `qr`: pair by QR code rather than with the Google account, if pairing.
async fn run(qr: bool) -> std::result::Result<(), Box<dyn Error>> {
    let config = chat_tui::Config::new(NAME)?;
    let state = config.read_marks.parent().ok_or("no state directory")?;
    create_private_dir(state)?;
    let mut gmd = gmd::Gmd::start(&state.join("session.json"), &state.join("gmd.log")).await?;

    // Until the phone sends the conversations: pairing, if needed, in the terminal or the
    // pairing page.
    let mut pairing: Option<pair::Pairing> = None;
    let conversations = loop {
        let pasted = async {
            match pairing.as_mut() {
                Some(pairing) => pairing.next().await,
                None => std::future::pending().await,
            }
        };
        let event = tokio::select! {
            event = gmd.next() => event?,
            Some(pasted) = pasted => {
                let pair::Pasted::Cookies(cookies) = pasted else {
                    return Ok(());
                };
                pair::say("Asking the phone\u{2026}");
                if let Some(pairing) = &pairing {
                    pairing.page().asking();
                }
                let command = serde_json::json!({"cmd": "pair_google", "cookies": cookies});
                gmd.send(command).await?;
                continue;
            }
        };
        let page = pairing.as_ref().map(pair::Pairing::page);
        match event {
            Some(gmd::Event::Login) if qr => {
                gmd.send(serde_json::json!({"cmd": "pair_qr"})).await?;
            }
            Some(gmd::Event::Login) => {
                let intro = "Pair with the Google account of your phone";
                pairing = Some(pair::Pairing::start(intro).await?);
            }
            Some(gmd::Event::Expired { message }) if pairing.is_none() => {
                let intro = format!("Sign in again: {message}. Same Google account");
                pairing = Some(pair::Pairing::start(&intro).await?);
            }
            Some(gmd::Event::Expired { message }) => {
                pair::say(&format!("Sign in again: {message}. Paste again."));
                page.inspect(|page| page.failed(&message));
            }
            Some(gmd::Event::PairFailed { message }) if qr => {
                return Err(format!("pairing failed: {message}").into());
            }
            Some(gmd::Event::PairFailed { message }) => {
                pair::say(&format!(
                    "Pairing failed: {}. Paste again.",
                    message.trim_end_matches('.')
                ));
                page.inspect(|page| page.failed(&message));
            }
            Some(gmd::Event::Emoji { emoji }) => {
                pair::say(&format!(
                    "\nOn your phone, Google Messages asks to confirm: tap   {emoji}\n"
                ));
                page.inspect(|page| page.emoji(&emoji));
            }
            Some(gmd::Event::Qr { url }) => {
                println!(
                    "Scan this QR code from Google Messages on your phone \
                     (Device pairing > QR code scanner):"
                );
                chat_tui::qr::print_qr(&url);
            }
            Some(gmd::Event::Paired) => {
                pair::say("Paired, loading the conversations\u{2026}");
                page.inspect(|page| page.paired());
            }
            Some(gmd::Event::Ready { conversations }) => {
                if let Some(pairing) = pairing.take() {
                    pairing.finish();
                }
                break conversations;
            }
            Some(gmd::Event::Status { message }) => pair::say(&message),
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
                gmd::Event::Expired { message } => Event::Status(format!(
                    "Disconnected: {message}. Restart {NAME} to sign in again"
                )),
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
