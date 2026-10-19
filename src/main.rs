//! gmessages-tui: a terminal client for Google Messages, paired with your Android phone
//! like Google Messages for web.
//!
//!   gmessages-tui [--reset-login] [--manual | --qr] | --uninstall | --version | --help
//!
//! Not affiliated with Google.

mod cli;
mod gmd;
mod key;
mod pair;
mod session;
#[cfg(any(feature = "wry-signin", test))]
mod signin;
mod store;
mod web;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::future::Future;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use chat_tui::backend::Result;
use chat_tui::data::{Attachment, ChatId, Discussion, Folder, Message, MessageId};
use chat_tui::{Backend, Capabilities, Downloaded, Draft, Event};
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use futures::channel::oneshot;

const NAME: &str = "gmessages-tui";

/// How long the interface waits to be connected before showing what is stored anyway.
const OFFLINE_AFTER: Duration = Duration::from_secs(10);

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
        Ok(cli::Command::Help) => {
            println!("{}\n\nUsage: {}", cli::HELP, cli::USAGE);
            return;
        }
        Ok(cli::Command::Uninstall) => {
            if let Err(e) = uninstall() {
                eprintln!("{NAME}: {e}");
                std::process::exit(1);
            }
            return;
        }
        Err(usage) => {
            eprintln!("usage: {usage}\n(--help for details)");
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

/// Removes what gmessages-tui left on this machine, after confirmation: the store key in the
/// keyring, the state (session, store, log, marks) and the cached gmd.
fn uninstall() -> std::result::Result<(), Box<dyn Error>> {
    let config = chat_tui::Config::new(NAME)?;
    let state = config.read_marks.parent().ok_or("no state directory")?;
    let cache = gmd::cache_dir()?;
    println!("This deletes, for good:");
    println!("  - the store key in the keyring (service {NAME})");
    println!("  - {} (session, messages, media)", state.display());
    println!("  - {} (gmd)", cache.display());
    print!("Continue? [y/N] ");
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Nothing removed.");
        return Ok(());
    }
    // The key first: without it, a store left behind cannot be read.
    key::forget(&state.join("messages.db"))?;
    for dir in [state, cache.as_path()] {
        match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("cannot remove {}: {e}", dir.display()).into());
            }
            _ => {}
        }
    }
    println!("Removed. Also remove gmessages-tui from your phone (Device pairing),");
    println!("and the program with `cargo uninstall gmessages-tui`.");
    Ok(())
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
    // The store first: its passphrase may be asked.
    let store_path = state.join("messages.db");
    let store = Rc::new(store::Store::open(
        &store_path,
        &key::store_key(&store_path)?,
    )?);
    let stored = store.conversations()?;
    let mut gmd = gmd::Gmd::start(&session, &state.join("gmd.log")).await?;
    // Paired already: connecting and fetching the conversations take a while.
    if session.exists() {
        pair::say("Syncing\u{2026}");
    }

    // Until connected (with conversations stored) or until the phone sends the conversations:
    // pairing, if needed, in the terminal or the pairing page.
    let mut pairing: Option<pair::Pairing> = None;
    // With conversations stored, the interface opens even if not connected by then (offline,
    // phone or Google not answering): what is stored can be read.
    let offline_after = tokio::time::Instant::now() + OFFLINE_AFTER;
    let ready = loop {
        let may_open = !stored.is_empty() && pairing.is_none();
        let pasted = async {
            match pairing.as_mut() {
                Some(pairing) => pairing.next().await,
                None => std::future::pending().await,
            }
        };
        let event = tokio::select! {
            _ = tokio::signal::ctrl_c(), if !manual => return Ok(()),
            _ = tokio::time::sleep_until(offline_after), if may_open => {
                break None;
            }
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
            // What is stored is shown at once, the rest follows.
            Some(gmd::Event::Connected) if !stored.is_empty() => {
                if let Some(pairing) = pairing.take() {
                    pairing.finish();
                }
                break None;
            }
            Some(gmd::Event::Ready {
                conversations,
                since,
            }) => {
                if let Some(pairing) = pairing.take() {
                    pairing.finish();
                }
                break Some((conversations, since));
            }
            Some(gmd::Event::Status { message }) => pair::say(&message),
            Some(gmd::Event::Fatal { message }) => return Err(message.into()),
            Some(
                gmd::Event::Hello { .. }
                | gmd::Event::Connected
                | gmd::Event::History { .. }
                | gmd::Event::Downloaded { .. }
                | gmd::Event::Done { .. }
                | gmd::Event::Gone { .. }
                | gmd::Event::Sent { .. }
                | gmd::Event::Message { .. }
                | gmd::Event::Conversation { .. }
                | gmd::Event::Typing { .. },
            ) => {}
            None => return Err("gmd stopped".into()),
        }
    };
    let mut discussions: Vec<Discussion> = stored.into_iter().map(stored_discussion).collect();
    let (lines, commands, control) = gmd.split();
    let mut sync = Sync {
        store: store.clone(),
        commands: commands.clone(),
        known: discussions.iter().map(|d| d.id.clone()).collect(),
        read: HashSet::new(),
        looked_up: HashSet::new(),
    };
    // What changed since last time (hidden, renamed, read on the phone): the first events.
    let mut changes = Vec::new();
    if let Some((conversations, since)) = ready {
        for event in sync.ready(conversations, since) {
            match event {
                Event::Chat(discussion) if !discussions.iter().any(|d| d.id == discussion.id) => {
                    discussions.push(discussion)
                }
                event => changes.push(event),
            }
        }
    }
    let backend = GMessages {
        commands,
        store,
        waiting: Waiting::default(),
        next_id: Cell::new(0),
        read: RefCell::default(),
    };
    let events = events(lines, sync, backend.waiting.clone(), changes);
    let result = chat_tui::run(backend, discussions, events, config).await;
    control.stop().await;
    Ok(result?)
}

/// What keeps the store and the phone in step.
struct Sync {
    store: Rc<store::Store>,
    commands: gmd::Commands,
    /// The conversations the interface has.
    known: HashSet<ChatId>,
    /// The conversations read on the phone: still read once their messages come.
    read: HashSet<ChatId>,
    /// The conversations asked about to the phone, missing from its lists.
    looked_up: HashSet<ChatId>,
}

impl Sync {
    /// The conversations listed by the phone (inbox, then hidden, the inbox complete from
    /// `since`): stored, the messages of those that changed fetched. Returns them as
    /// interface events: those not known yet, the others updated, those read on the phone.
    /// Those missing from the lists are asked about (in the trash, or deleted).
    fn ready(&mut self, conversations: Vec<gmd::Conversation>, since: i64) -> Vec<Event> {
        if let Err(e) = self.store.save_conversations(&conversations) {
            pair::say(&format!("Cannot store the conversations: {e}"));
        }
        let listed: Vec<&str> = conversations.iter().map(|c| c.id.as_str()).collect();
        let unlisted = self.store.unlisted(&listed, since).unwrap_or_default();
        if !unlisted.is_empty() {
            self.looked_up.extend(unlisted.iter().cloned());
            self.commands
                .send(serde_json::json!({"cmd": "lookup", "conversations": unlisted}));
        }
        let changed = self.store.changed().unwrap_or_default();
        if !changed.is_empty() {
            self.commands
                .send(serde_json::json!({"cmd": "fetch", "conversations": changed}));
        }
        let mut events = Vec::new();
        for conversation in conversations {
            events.extend(self.conversation(conversation));
        }
        events
    }

    /// A conversation looked up the phone no longer has: forgotten, and hidden.
    fn gone(&mut self, conversation: &str) -> Option<Event> {
        self.looked_up.remove(conversation);
        if let Err(e) = self.store.forget(conversation) {
            pair::say(&format!("Cannot forget the conversation: {e}"));
        }
        let gone = self
            .store
            .conversations_hidden(&[conversation.to_string()])
            .ok()?
            .pop()?;
        Some(Event::Chat(stored_discussion(gone)))
    }

    /// A message arriving live: stored, then shown, as new (notified) unless known already or
    /// old.
    fn live(&self, conversation: String, m: gmd::Message, old: bool) -> Event {
        let known = matches!(self.store.message(&m.id), Ok(Some(_)));
        let stored = self
            .store
            .save_messages(&conversation, std::slice::from_ref(&m))
            .and_then(|()| Ok(self.store.arrived(&conversation, &m.id)?));
        if let Err(e) = stored {
            return Event::Status(format!("Cannot store the message: {e}"));
        }
        let (message, quote) = message(m);
        if known || old {
            Event::History {
                chat: conversation,
                messages: vec![(message, quote)],
            }
        } else {
            Event::Message {
                chat: conversation,
                message,
                quote,
            }
        }
    }

    /// A conversation new or changed on the phone, as interface events: the discussion (new,
    /// or its title, folder and visibility for a known one), then whether it was read there.
    /// Deleted for good, its messages are forgotten.
    fn conversation(&mut self, conversation: gmd::Conversation) -> Vec<Event> {
        if conversation.deleted
            && let Err(e) = self.store.forget(&conversation.id)
        {
            pair::say(&format!("Cannot forget the conversation: {e}"));
        }
        let read = if conversation.unread {
            self.read.remove(&conversation.id);
            None
        } else {
            self.read.insert(conversation.id.clone());
            Some(Event::Read {
                chat: conversation.id.clone(),
            })
        };
        let messages = if self.known.insert(conversation.id.clone()) {
            self.store.messages(&conversation.id).unwrap_or_default()
        } else {
            Vec::new()
        };
        let discussion = if messages.is_empty() {
            discussion(conversation)
        } else {
            stored_discussion(store::Conversation {
                id: conversation.id,
                name: conversation.name,
                group: conversation.group,
                hidden: conversation.deleted,
                folder: conversation.folder,
                messages,
            })
        };
        std::iter::once(Event::Chat(discussion))
            .chain(read)
            .collect()
    }

    /// Messages of `conversation`: stored, then as the interface shows them.
    fn history(
        &self,
        conversation: &str,
        messages: Vec<gmd::Message>,
    ) -> Vec<(Message, Option<MessageId>)> {
        if let Err(e) = self
            .store
            .save_messages(conversation, &messages)
            .and_then(|()| Ok(self.store.fetched(conversation)?))
        {
            pair::say(&format!("Cannot store the messages: {e}"));
        }
        messages.into_iter().map(message).collect()
    }
}

/// The interface folder of a conversation filed in `folder` by the phone.
fn folder(folder: Option<&str>) -> Option<Folder> {
    match folder? {
        "archive" => Some(Folder::Archive),
        "spam" => Some(Folder::Spam),
        "trash" => Some(Folder::Trash),
        _ => None,
    }
}

/// The interface discussion of a stored conversation.
fn stored_discussion(conversation: store::Conversation) -> Discussion {
    let mut discussion = Discussion::new(conversation.id, conversation.name);
    discussion.is_group = conversation.group;
    discussion.hidden = conversation.hidden;
    discussion.folder = folder(conversation.folder.as_deref());
    let messages: Vec<_> = conversation.messages.into_iter().map(message).collect();
    let index: HashMap<MessageId, usize> = messages
        .iter()
        .enumerate()
        .map(|(i, (m, _))| (m.id.clone(), i))
        .collect();
    discussion.messages = messages
        .into_iter()
        .map(|(mut m, quote)| {
            m.reply_to = quote.and_then(|id| index.get(&id).copied());
            m
        })
        .collect();
    discussion
}

/// The interface discussion of a conversation, with its latest message until its history
/// comes.
fn discussion(conversation: gmd::Conversation) -> Discussion {
    let mut discussion = Discussion::new(conversation.id, conversation.name);
    discussion.is_group = conversation.group;
    discussion.hidden = conversation.deleted;
    discussion.folder = folder(conversation.folder.as_deref());
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
    message.remote_attachments = m
        .media
        .iter()
        .map(|medium| {
            medium["mime"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .to_string()
        })
        .collect();
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
    mut sync: Sync,
    waiting: Waiting,
    first: Vec<Event>,
) -> UnboundedReceiver<Event> {
    let (events, received) = unbounded();
    for event in first {
        let _ = events.unbounded_send(event);
    }
    tokio::task::spawn_local(async move {
        while let Ok(Some(event)) = gmd::next_event(&mut lines).await {
            let event = match event {
                gmd::Event::Ready {
                    conversations,
                    since,
                } => {
                    for event in sync.ready(conversations, since) {
                        if events.unbounded_send(event).is_err() {
                            return;
                        }
                    }
                    continue;
                }
                gmd::Event::Done { id, error } => {
                    if let Some(done) = waiting.actions.borrow_mut().remove(&id) {
                        let _ = done.send(error.map_or(Ok(()), Err));
                    }
                    continue;
                }
                gmd::Event::Downloaded {
                    id,
                    attachments,
                    error,
                } => {
                    if let Some(done) = waiting.downloads.borrow_mut().remove(&id) {
                        let _ = done.send(match error {
                            Some(error) => Err(error),
                            None => attachments.into_iter().map(attachment).collect(),
                        });
                    }
                    continue;
                }
                gmd::Event::Sent {
                    id,
                    conversation,
                    message,
                    error,
                } => {
                    let done = waiting.sends.borrow_mut().remove(&id);
                    match (done, message, error) {
                        (Some(done), Some(message), None) => {
                            let _ = done.send(Ok(message));
                            continue;
                        }
                        (Some(done), _, error) => {
                            let _ = done.send(Err(error.unwrap_or_else(|| "not sent".to_string())));
                            continue;
                        }
                        // Sent although given up (Esc): shown and stored all the same.
                        (None, Some(message), None) => Event::History {
                            messages: sync.history(&conversation, vec![message]),
                            chat: conversation,
                        },
                        (None, ..) => continue,
                    }
                }
                gmd::Event::Message {
                    conversation,
                    message,
                    old,
                } => sync.live(conversation, message, old),
                gmd::Event::Gone { conversation } => match sync.gone(&conversation) {
                    Some(event) => event,
                    None => continue,
                },
                gmd::Event::Conversation { conversation } => {
                    if let Err(e) = sync
                        .store
                        .save_conversations(std::slice::from_ref(&conversation))
                    {
                        pair::say(&format!("Cannot store the conversation: {e}"));
                    }
                    // Found in the trash: its messages, forgotten if it was taken as deleted.
                    if sync.looked_up.remove(&conversation.id)
                        && !conversation.deleted
                        && sync
                            .store
                            .changed()
                            .is_ok_and(|c| c.contains(&conversation.id))
                    {
                        sync.commands.send(
                            serde_json::json!({"cmd": "fetch", "conversations": [&conversation.id]}),
                        );
                    }
                    for event in sync.conversation(conversation) {
                        if events.unbounded_send(event).is_err() {
                            return;
                        }
                    }
                    continue;
                }
                gmd::Event::Typing {
                    conversation,
                    author,
                    name,
                    started,
                } => Event::Typing {
                    chat: conversation,
                    author,
                    name,
                    started,
                },
                gmd::Event::History {
                    conversation,
                    messages,
                } => {
                    let read = sync.read.contains(&conversation).then(|| Event::Read {
                        chat: conversation.clone(),
                    });
                    let history = Event::History {
                        messages: sync.history(&conversation, messages),
                        chat: conversation,
                    };
                    for event in std::iter::once(history).chain(read) {
                        if events.unbounded_send(event).is_err() {
                            return;
                        }
                    }
                    continue;
                }
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

/// Requests waiting for gmd's answer, by request id.
type Pending<T> = Rc<RefCell<HashMap<u64, oneshot::Sender<std::result::Result<T, String>>>>>;

/// What waits for gmd: downloads, messages being sent, conversations archived or deleted.
#[derive(Clone, Default)]
struct Waiting {
    downloads: Pending<Vec<Attachment>>,
    sends: Pending<gmd::Message>,
    actions: Pending<()>,
}

/// Waits for the answer to request `id` of `pending`, sent if `sent`.
async fn answer<T>(
    pending: &Pending<T>,
    id: u64,
    answer: oneshot::Receiver<std::result::Result<T, String>>,
    sent: bool,
    timeout: Duration,
) -> Result<T> {
    // Given up (dropped, e.g. Esc while sending) or answered: not waiting any more.
    struct Forget<'a, T>(&'a Pending<T>, u64);
    impl<T> Drop for Forget<'_, T> {
        fn drop(&mut self) {
            self.0.borrow_mut().remove(&self.1);
        }
    }
    let _forget = Forget(pending, id);
    let answer = if sent {
        tokio::time::timeout(timeout, answer).await
    } else {
        Ok(Err(oneshot::Canceled))
    };
    match answer {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(error))) => Err(error.into()),
        Ok(Err(_)) => Err("gmd has stopped".into()),
        Err(_) => Err("no answer from the phone in time".into()),
    }
}

/// How long a download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// How long sending may take (uploading attachments included).
const SEND_TIMEOUT: Duration = Duration::from_secs(120);
/// Archiving or deleting a conversation, given up after.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Google Messages, for the interface: shows the conversations and downloads their media
/// so far.
struct GMessages {
    commands: gmd::Commands,
    store: Rc<store::Store>,
    waiting: Waiting,
    next_id: Cell<u64>,
    /// The last message said read on the phone, per conversation.
    read: RefCell<HashMap<ChatId, MessageId>>,
}

impl GMessages {
    /// Asks gmd to `archive`, `restore` or `delete_chat` `chat`, until it is done.
    fn chat_action(&self, cmd: &str, chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        let id = self.request();
        let (done, done_rx) = oneshot::channel();
        self.waiting.actions.borrow_mut().insert(id, done);
        let sent = self
            .commands
            .send(serde_json::json!({"cmd": cmd, "id": id, "conversation": chat}));
        let actions = self.waiting.actions.clone();
        async move { answer(&actions, id, done_rx, sent, ACTION_TIMEOUT).await }
    }

    /// A new request id.
    fn request(&self) -> u64 {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        id
    }
}

/// The futures will own what they need (a handle to the client): `'static`, as the trait
/// wants.
#[allow(clippy::manual_async_fn)]
impl Backend for GMessages {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chat_actions: true,
            ..Capabilities::default()
        }
    }

    fn archive(&self, chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        self.chat_action("archive", chat)
    }

    /// To the trash of the phone, or for good (its messages then forgotten here, see
    /// [`Sync::conversation`]).
    fn delete_chat(&self, chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        self.chat_action("delete_chat", chat)
    }

    fn restore(&self, chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        self.chat_action("restore", chat)
    }

    /// Sent by gmd, then stored with its attachments: as the phone echoed it, with its id.
    fn send(&self, chat: &ChatId, draft: Draft) -> impl Future<Output = Result<Message>> + 'static {
        use base64::Engine;
        let id = self.request();
        let (done, sent) = oneshot::channel();
        self.waiting.sends.borrow_mut().insert(id, done);
        let attachments: Vec<_> = draft
            .attachments
            .iter()
            .map(|a| {
                serde_json::json!({
                    "content_type": a.content_type,
                    "name": a.file_name.clone().unwrap_or_default(),
                    "data": base64::engine::general_purpose::STANDARD.encode(&a.bytes),
                })
            })
            .collect();
        let command = serde_json::json!({
            "cmd": "send",
            "id": id,
            "conversation": chat,
            "text": draft.text,
            "reply": draft.reply_to.as_ref().map(|m| &m.id),
            "attachments": attachments,
        });
        let ok = self.commands.send(command);
        let (store, sends, chat) = (self.store.clone(), self.waiting.sends.clone(), chat.clone());
        async move {
            let sent = answer(&sends, id, sent, ok, SEND_TIMEOUT).await?;
            store.save_messages(&chat, std::slice::from_ref(&sent))?;
            if !draft.attachments.is_empty() {
                store.save_attachments(&sent.id, &draft.attachments)?;
            }
            let (mut message, _) = message(sent);
            message.remote_attachments.clear();
            message.attachments = draft.attachments;
            Ok(message)
        }
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

    /// Said to the phone, once per last message.
    fn read(&self, chat: &ChatId, last: &Message) {
        if last.id.is_empty() || self.read.borrow().get(chat) == Some(&last.id) {
            return;
        }
        self.read.borrow_mut().insert(chat.clone(), last.id.clone());
        self.commands.send(serde_json::json!({
            "cmd": "read",
            "conversation": chat,
            "up_to": last.id,
        }));
    }

    /// From the store if downloaded already; else by gmd, then stored.
    fn download(
        &self,
        _chat: &ChatId,
        message: &Message,
    ) -> impl Future<Output = Result<Downloaded>> + 'static {
        let (store, commands, downloads) = (
            self.store.clone(),
            self.commands.clone(),
            self.waiting.downloads.clone(),
        );
        let message_id = message.id.clone();
        let id = self.request();
        async move {
            if let Some(attachments) = store.attachments(&message_id)? {
                return Ok((attachments, Vec::new()));
            }
            let media = store
                .message(&message_id)?
                .map(|m| m.media)
                .unwrap_or_default();
            if media.is_empty() {
                return Err("no media known for this message yet".into());
            }
            let (done, answer_rx) = oneshot::channel();
            downloads.borrow_mut().insert(id, done);
            let sent = commands.send(serde_json::json!({
                "cmd": "download",
                "id": id,
                "message": message_id,
                "media": media,
            }));
            let attachments = answer(&downloads, id, answer_rx, sent, DOWNLOAD_TIMEOUT).await?;
            store.save_attachments(&message_id, &attachments)?;
            Ok((attachments, Vec::new()))
        }
    }
}
