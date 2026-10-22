//! The interface with fake chats, no phone: for the demo GIF of the README (doc/record.sh).
//!
//!   cargo run --example demo
//!
//! Sending only appends locally. Its state (pins, read marks, settings) goes to a temporary
//! directory.

use std::future::Future;
use std::io;

use chat_tui::backend::Result;
use chat_tui::data::{Attachment, ChatId, Discussion, Folder, Message};
use chat_tui::recipients::Contact;
use chat_tui::{Backend, Capabilities, Config, Downloaded, Draft};
use chrono::{Duration, Local, NaiveDateTime, NaiveTime};

/// Sends locally, and has nothing to download.
struct Demo;

// `async fn` would borrow `self` and the arguments: the trait wants `'static` futures.
#[allow(clippy::manual_async_fn)]
impl Backend for Demo {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chat_actions: true,
            new_chat: true,
            ..Capabilities::default()
        }
    }

    fn send(
        &self,
        _chat: &ChatId,
        draft: Draft,
    ) -> impl Future<Output = Result<Message>> + 'static {
        async move {
            let id = chrono::Utc::now().timestamp_millis();
            let mut message = Message::mine(id, draft.text, None, draft.forwarded);
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
        async { Ok(()) }
    }

    fn delete(
        &self,
        _chat: &ChatId,
        _message: &Message,
    ) -> impl Future<Output = Result<()>> + 'static {
        async { Ok(()) }
    }

    fn archive(&self, _chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        async { Ok(()) }
    }

    fn delete_chat(&self, _chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        async { Ok(()) }
    }

    fn restore(&self, _chat: &ChatId) -> impl Future<Output = Result<()>> + 'static {
        async { Ok(()) }
    }

    fn contacts(&self) -> impl Future<Output = Result<Vec<Contact>>> + 'static {
        async { Ok(contacts()) }
    }

    /// A chat named after its people, empty.
    fn start_chat(
        &self,
        numbers: Vec<String>,
    ) -> impl Future<Output = Result<Discussion>> + 'static {
        async move {
            let contacts = contacts();
            let names: Vec<&str> = numbers
                .iter()
                .map(|n| {
                    contacts
                        .iter()
                        .find(|c| &c.number == n)
                        .map_or(n.as_str(), |c| c.name.as_str())
                })
                .collect();
            let mut discussion = Discussion::new(numbers.join(","), names.join(", "));
            discussion.is_group = numbers.len() > 1;
            Ok(discussion)
        }
    }

    fn download(
        &self,
        _chat: &ChatId,
        _message: &Message,
    ) -> impl Future<Output = Result<Downloaded>> + 'static {
        async { Err("nothing to download in the demo".into()) }
    }
}

/// The address book of the phone.
fn contacts() -> Vec<Contact> {
    [
        ("Alice Martin", "+33612345678"),
        ("Bob Dupont", "+33698765432"),
        ("Mom", "+33711223344"),
        ("Sam Lee", "+14155550123"),
    ]
    .into_iter()
    .map(|(name, number)| Contact {
        name: name.to_string(),
        number: number.to_string(),
    })
    .collect()
}

fn main() -> io::Result<()> {
    let discussions = discussions();
    let state = std::env::temp_dir().join(format!("gmessages-tui-demo-{}", std::process::id()));
    std::fs::create_dir_all(&state)?;
    std::fs::write(state.join("pinned"), "lisbon\nfamily\n")?;
    // Read up to their last message but `unread` ones.
    let marks: String = discussions
        .iter()
        .map(|d| {
            let read = d
                .messages
                .len()
                .checked_sub(d.unread + 1)
                .map_or(0, |i| d.messages[i].sent_ms);
            format!("{} {read}\n", d.id)
        })
        .collect();
    std::fs::write(state.join("read"), marks)?;
    let config = Config {
        name: "gmessages-tui".to_string(),
        version: concat!("v", env!("CARGO_PKG_VERSION")).to_string(),
        settings: state.join("settings"),
        read_marks: state.join("read"),
        pins: state.join("pinned"),
        open: None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(tokio::task::LocalSet::new().run_until(chat_tui::run(
        Demo,
        discussions,
        futures::stream::pending(),
        config,
    )));
    let _ = std::fs::remove_dir_all(&state);
    result
}

/// `days` ago, at `h`:`m`.
fn at(days: i64, h: u32, m: u32) -> NaiveDateTime {
    (Local::now().date_naive() - Duration::days(days))
        .and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
}

fn from(name: &str, text: &str, time: NaiveDateTime) -> Message {
    Message {
        from_me: false,
        author: Some(name.to_string()),
        sender_name: name.to_string(),
        timestamp: time,
        ..Message::mine(0, text.to_string(), None, false)
    }
}

fn mine(text: &str, time: NaiveDateTime) -> Message {
    Message {
        timestamp: time,
        ..Message::mine(0, text.to_string(), None, false)
    }
}

/// A chat of `messages`, their ids and times set.
fn chat(
    id: &str,
    title: &str,
    group: bool,
    unread: usize,
    mut messages: Vec<Message>,
) -> Discussion {
    for (i, message) in messages.iter_mut().enumerate() {
        message.sent_ms = message.timestamp.and_utc().timestamp_millis() as u64;
        message.id = format!("{id}-{i}");
    }
    let mut discussion = Discussion::new(id.to_string(), title.to_string());
    discussion.is_group = group;
    discussion.messages = messages;
    discussion.unread = unread;
    discussion
}

fn discussions() -> Vec<Discussion> {
    let mut sunset = from(
        "Alice Martin",
        "Landed! The light here is unreal \u{1f305}",
        at(0, 9, 42),
    );
    sunset.attachments = vec![jpeg(include_bytes!("demo/sunset.jpg"))];
    sunset.set_reaction("me", true, Some("\u{2764}\u{fe0f}"));
    sunset.set_reaction("Bob Dupont", false, Some("\u{1f60d}"));
    let mut alfama = from("Bob Dupont", "Dinner tonight in Alfama?", at(0, 17, 5));
    alfama.attachments = vec![jpeg(include_bytes!("demo/alfama.jpg"))];
    let mut booked = from(
        "Alice Martin",
        "Perfect, I'll book for 8 \u{1f377}",
        at(0, 17, 9),
    );
    booked.reply_to = Some(2);
    let mut tram = from(
        "Bob Dupont",
        "Tram 28 after that, obviously \u{1f68b}",
        at(0, 18, 2),
    );
    tram.attachments = vec![jpeg(include_bytes!("demo/tram.jpg"))];
    let lisbon = chat(
        "lisbon",
        "Lisbon trip \u{1f30d}",
        true,
        2,
        vec![
            sunset,
            mine("Wow \u{1f60d} save me a pastel de nata!", at(0, 9, 45)),
            alfama,
            booked,
            tram,
        ],
    );

    let mut dessert = mine("We'll bring dessert \u{1f370}", at(1, 19, 40));
    dessert.set_reaction("Mom", false, Some("\u{2764}\u{fe0f}"));
    let family = chat(
        "family",
        "Family \u{1f3e1}",
        true,
        0,
        vec![
            from("Mom", "Sunday lunch at ours? \u{1f357}", at(1, 19, 2)),
            from("Dad", "I'm doing the barbecue", at(1, 19, 15)),
            from("Emma", "Count me in! \u{1f64b}", at(1, 19, 31)),
            dessert,
        ],
    );

    let mut route = from("Léa", "New route at the gym, 6b+ \u{1f62e}", at(0, 18, 10));
    route.set_reaction("Tom", false, Some("\u{1f525}"));
    let climbing = chat(
        "climbing",
        "Climbing crew \u{1f9d7}",
        true,
        3,
        vec![
            from("Tom", "Who's in on Thursday?", at(1, 12, 3)),
            mine("Me! Same place?", at(1, 12, 10)),
            route,
            from("Tom", "Let's try it on Thursday, 7pm", at(0, 18, 21)),
            from("Sam Lee", "I'll bring the chalk", at(0, 18, 30)),
        ],
    );

    let mom = chat(
        "mom",
        "Mom",
        false,
        0,
        vec![
            from("Mom", "Call me when you land \u{1f60a}", at(1, 20, 14)),
            mine("Landed, all good. Calling you tonight!", at(0, 9, 50)),
        ],
    );

    let parcel = chat(
        "chronopost",
        "Chronopost",
        false,
        1,
        vec![from(
            "Chronopost",
            "Your parcel XY123456789FR will be delivered today between 13:00 and 14:00.",
            at(0, 11, 31),
        )],
    );

    let code = chat(
        "38300",
        "38300",
        false,
        0,
        vec![from(
            "38300",
            "Your verification code is 482913. It expires in 10 minutes.",
            at(0, 8, 2),
        )],
    );

    let mut not_sent = mine("Running 10 min late, sorry!", at(1, 18, 55));
    not_sent.failed = true;
    let sam = chat(
        "sam",
        "Sam Lee",
        false,
        0,
        vec![
            from("Sam Lee", "Still on for a coffee tomorrow?", at(1, 17, 3)),
            mine("Yes! 7pm at the usual place?", at(1, 17, 10)),
            from("Sam Lee", "Perfect", at(1, 17, 11)),
            not_sent,
        ],
    );

    let dentist = chat(
        "dentist",
        "Dr. Moreau Dental",
        false,
        0,
        vec![from(
            "Dr. Moreau Dental",
            "Reminder: appointment on Monday at 9:30. Reply C to confirm.",
            at(3, 10, 0),
        )],
    );

    let mut landlord = chat(
        "landlord",
        "Old landlord",
        false,
        0,
        vec![from(
            "Old landlord",
            "Deposit sent back, all the best!",
            at(40, 14, 0),
        )],
    );
    landlord.folder = Some(Folder::Archive);

    let mut prize = chat(
        "prize",
        "+44 7700 900123",
        false,
        0,
        vec![from(
            "+44 7700 900123",
            "Congratulations! You won a free phone, claim it now: bit.ly/xxxx",
            at(2, 3, 14),
        )],
    );
    prize.folder = Some(Folder::Spam);

    vec![
        lisbon, family, climbing, parcel, mom, code, sam, dentist, landlord, prize,
    ]
}

/// A photo of `examples/demo` (credits in its README).
fn jpeg(bytes: &[u8]) -> Attachment {
    Attachment {
        content_type: "image/jpeg".to_string(),
        file_name: None,
        bytes: bytes.to_vec(),
        width: None,
        height: None,
    }
}
