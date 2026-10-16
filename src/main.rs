//! gmessages-tui: a terminal client for Google Messages, paired with your Android phone
//! like Google Messages for web.
//!
//!   gmessages-tui [--reset-login] [--manual | --qr] | --version
//!
//! Not affiliated with Google.

mod cli;
mod gmd;
mod pair;
mod session;
#[cfg(any(feature = "wry-signin", test))]
mod signin;
mod web;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use chat_tui::backend::Result;
use chat_tui::data::{Attachment, ChatId, Discussion, Message, MessageId};
use chat_tui::{Backend, Capabilities, Downloaded, Draft, Event};
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use futures::channel::oneshot;

const NAME: &str = "gmessages-tui";

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (qr, manual, reset_login) = match cli::parse(args.iter().map(String::as_str)) {
        Ok(cli::Command::Run {
            qr,
            manual,
            reset_login,
        }) => (qr, manual, reset_login),
        #[cfg(feature = "wry-signin")]
        Ok(cli::Command::Signin) => {
            signin::run_child();
            return;
        }
        Ok(cli::Command::Version) => {
            println!("v{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(usage) => {
            eprintln!("usage: {usage}");
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("cannot create the Tokio runtime");
    let local = tokio::task::LocalSet::new();
    if let Err(e) = runtime.block_on(local.run_until(run(qr, manual, reset_login))) {
        eprintln!("{NAME}: {e}");
        std::process::exit(1);
    }
}

/// `qr`: pair by QR code rather than with the Google account, if pairing.
async fn run(qr: bool, manual: bool, reset_login: bool) -> std::result::Result<(), Box<dyn Error>> {
    let config = chat_tui::Config::new(NAME)?;
    let state = config.read_marks.parent().ok_or("no state directory")?;
    create_private_dir(state)?;
    let session = state.join("session.json");
    if reset_login {
        session::reset(&session).map_err(|error| format!("cannot reset local login: {error}"))?;
        pair::say("Local login reset. Starting a new pairing.");
    }
    let mut gmd = gmd::Gmd::start(&session, &state.join("gmd.log")).await?;
    // Paired already: connecting and fetching the conversations take a while.
    if session.exists() {
        pair::say("Syncing\u{2026}");
    }

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
            _ = tokio::signal::ctrl_c(), if !manual => return Ok(()),
            event = gmd.next() => event?,
            Some(pasted) = pasted => {
                let cookies = match pasted {
                    pair::Pasted::Cookies(cookies) => cookies,
                    pair::Pasted::Quit => return Ok(()),
                    pair::Pasted::Failed(error) => return Err(error.into()),
                };
                pair::say("Asking the phone\u{2026}");
                if let Some(page) = pairing.as_ref().and_then(pair::Pairing::page) {
                    page.asking();
                }
                let command = serde_json::json!({"cmd": "pair_google", "cookies": cookies});
                gmd.send(command).await?;
                continue;
            }
        };
        let page = pairing.as_ref().and_then(pair::Pairing::page);
        match event {
            Some(gmd::Event::Login) if qr => {
                gmd.send(serde_json::json!({"cmd": "pair_qr"})).await?;
            }
            Some(gmd::Event::Login) => {
                let intro = "Pair with the Google account of your phone";
                drop(pairing.take());
                pairing = Some(pair::Pairing::start(intro, manual).await?);
            }
            Some(gmd::Event::Expired { message }) if pairing.is_none() || !manual => {
                drop(pairing.take());
                let intro = format!("Sign in again: {message}. Same Google account");
                pairing = Some(pair::Pairing::start(&intro, manual).await?);
            }
            Some(gmd::Event::Expired { message }) => {
                pair::say(&format!("Sign in again: {message}. Paste again."));
                page.inspect(|page| page.failed(&message));
            }
            Some(gmd::Event::PairFailed { message }) if qr => {
                return Err(format!("pairing failed: {message}").into());
            }
            Some(gmd::Event::PairFailed { message }) if !manual => {
                drop(pairing.take());
                pairing = Some(
                    pair::Pairing::start(
                        &format!("Pairing failed: {message}. Sign in again"),
                        manual,
                    )
                    .await?,
                );
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
                     (Device pairing):"
                );
                chat_tui::qr::print_qr(&url);
            }
            Some(gmd::Event::Paired) => {
                pair::say("Paired.\nSyncing\u{2026}");
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
            Some(
                gmd::Event::Hello { .. }
                | gmd::Event::History { .. }
                | gmd::Event::Downloaded { .. },
            ) => {}
            None => return Err("gmd stopped".into()),
        }
    };
    let discussions = conversations.into_iter().map(discussion).collect();

    let (lines, commands, control) = gmd.split();
    let backend = GMessages {
        commands,
        downloads: Downloads::default(),
        next_id: Cell::new(0),
    };
    let events = events(lines, backend.downloads.clone());
    let result = chat_tui::run(backend, discussions, events, config).await;
    control.stop().await;
    Ok(result?)
}

/// The interface discussion of a conversation, with its latest message until its history
/// comes.
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

/// The interface message of `m`, with the id of the one it quotes.
fn message(m: gmd::Message) -> (Message, Option<MessageId>) {
    let mut message = Message::mine(m.id, m.text, None, false);
    message.sent_ms = m.timestamp_ms.max(0) as u64;
    if let Some(time) = chrono::DateTime::from_timestamp_millis(m.timestamp_ms) {
        message.timestamp = time.with_timezone(&chrono::Local).naive_local();
    }
    message.from_me = m.from_me;
    if !m.from_me {
        message.author = Some(m.sender_id);
        message.sender_name = m.sender;
    }
    message.remote_attachments = m.media;
    for reaction in m.reactions {
        message.set_reaction(&reaction.sender_id, reaction.mine, Some(&reaction.emoji));
    }
    if m.deleted {
        message.mark_deleted();
    }
    (message, m.quote.filter(|id| !id.is_empty()))
}

/// The interface attachment of a downloaded medium.
fn attachment(a: gmd::Attachment) -> std::result::Result<Attachment, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&a.data)
        .map_err(|e| format!("bad medium from gmd: {e}"))?;
    Ok(Attachment {
        content_type: a.content_type,
        file_name: a.name.filter(|name| !name.is_empty()),
        bytes,
        width: a.width.filter(|&w| w > 0),
        height: a.height.filter(|&h| h > 0),
    })
}

/// What gmd reports while the interface runs, as interface events.
fn events(
    mut lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
    downloads: Downloads,
) -> UnboundedReceiver<Event> {
    let (events, received) = unbounded();
    tokio::task::spawn_local(async move {
        while let Ok(Some(event)) = gmd::next_event(&mut lines).await {
            let event = match event {
                gmd::Event::Downloaded {
                    id,
                    attachments,
                    error,
                } => {
                    if let Some(done) = downloads.borrow_mut().remove(&id) {
                        let _ = done.send(match error {
                            Some(error) => Err(error),
                            None => attachments.into_iter().map(attachment).collect(),
                        });
                    }
                    continue;
                }
                gmd::Event::History {
                    conversation,
                    messages,
                } => Event::History {
                    chat: conversation,
                    messages: messages.into_iter().map(message).collect(),
                },
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

/// Downloads waiting for gmd's answer, by request id.
type Downloads =
    Rc<RefCell<HashMap<u64, oneshot::Sender<std::result::Result<Vec<Attachment>, String>>>>>;

/// How long a download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Google Messages, for the interface: shows the conversations and downloads their media
/// so far.
struct GMessages {
    commands: gmd::Commands,
    downloads: Downloads,
    next_id: Cell<u64>,
}

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
        message: &Message,
    ) -> impl Future<Output = Result<Downloaded>> + 'static {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        let (done, answer) = oneshot::channel();
        self.downloads.borrow_mut().insert(id, done);
        let sent = self.commands.send(serde_json::json!({
            "cmd": "download",
            "id": id,
            "message": message.id,
        }));
        let downloads = self.downloads.clone();
        async move {
            if !sent {
                downloads.borrow_mut().remove(&id);
                return Err("gmd has stopped".into());
            }
            let answer = tokio::time::timeout(DOWNLOAD_TIMEOUT, answer).await;
            downloads.borrow_mut().remove(&id);
            match answer {
                Ok(Ok(Ok(attachments))) => Ok((attachments, Vec::new())),
                Ok(Ok(Err(error))) => Err(error.into()),
                Ok(Err(_)) => Err("gmd has stopped".into()),
                Err(_) => Err("the download took too long".into()),
            }
        }
    }
}
