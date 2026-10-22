# gmessages-proto

A Rust client library for the **Google Messages for web** protocol: it drives an Android
phone running Google Messages through Google's relay, as messages.google.com does.

- pairing by QR code or through the Google account (emoji confirmation), unpairing;
- a persistent, `serde`-serializable session, with token refresh and cookie updates reported
  to the application;
- the event stream (long polling) with acknowledgements, activation, pings, reconnection
  and catch-up handled internally, delivered as typed events;
- calls to the phone: conversations (by folder, paging), messages, sending (text, replies,
  media), reactions, read marks, typing, contacts, thumbnails, conversation operations
  (archive, trash, delete, block…), media upload and download (with streaming decryption).

It was implemented **from a written specification** of the protocol
(`doc/protocol/` in the gmessages-tui repository), without reading the code of other
implementations. Ambiguities of the specification and the choices made are listed in
[NOTES.md](NOTES.md).

**Not affiliated with Google.** The protocol is undocumented and changes; Google's terms of
service apply to its use.

## Usage

```rust
use gmessages_proto::{CancelToken, Client, Config, Event, Folder, pair_with_qr};

async fn demo() -> gmessages_proto::Result<()> {
let config = Config::with_device_name("my-app");
let session = pair_with_qr(config.clone(), &CancelToken::new(), |url| {
    println!("render as a QR code: {url}");
})
.await?;
// persist `session.to_json()`
let (client, mut events) = Client::connect(config, session).await?;
let inbox = client.list_conversations(Folder::Inbox, 25, None).await?;
while let Some(ev) = events.recv().await {
    match ev {
        Event::MessageUpserted { message, old } => println!("{} {}", old, message.text()),
        Event::SessionChanged(s) => { /* persist s */ }
        Event::LoggedOut(_) => break, // pair again
        _ => {}
    }
}
Ok(())
}
```

See `examples/qr_pair_and_events.rs`.

## Credits

The specification this library implements was written by analysing the behaviour of:

- **libgm**, the Go library of [mautrix-gmessages](https://github.com/mautrix/gmessages)
  (`pkg/libgm`), by Tulir Asokan and the mautrix contributors (AGPL-3.0): the
  reverse-engineering work the protocol knowledge stands on. No code of it is used here.
- **mautrix-gmessages** itself, for how calls are used in practice.
- **gmessages-tui** (gmd), for behaviours observed against the current web client.
- **[UKEY2](https://github.com/google/ukey2)** by Google (Apache-2.0), the key exchange used
  by Google account pairing.
- **Google Messages for web** (messages.google.com).

## License

[AGPL-3.0-only](LICENSE). Copyright Louis Rubet.
