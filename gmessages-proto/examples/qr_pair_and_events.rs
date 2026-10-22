//! Pairs by QR code (or reuses a saved session), connects, and prints events.
//!
//! ```text
//! cargo run --example qr_pair_and_events -- session.json
//! ```
//!
//! The QR URL is printed: render it as a QR code (e.g. `qrencode -t ansiutf8 '<url>'`) and
//! scan it with Google Messages on the phone (Device pairing › QR code scanner).
//! Set `RUST_LOG=gmessages_proto=debug` for protocol logs.

use std::path::PathBuf;

use gmessages_proto::{CancelToken, Client, Config, Event, Folder, Session, pair_with_qr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "session.json".into()),
    );
    let config = Config::with_device_name("gmessages-proto example");

    let session = match std::fs::read_to_string(&path) {
        Ok(json) => Session::from_json(&json)?,
        Err(_) => {
            let cancel = CancelToken::new();
            let c2 = cancel.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                c2.cancel();
            });
            let session = pair_with_qr(config.clone(), &cancel, |url| {
                println!("\nScan this QR code (renewed every 30 s):\n{url}\n");
            })
            .await?;
            std::fs::write(&path, session.to_json())?;
            println!("paired; session saved to {}", path.display());
            session
        }
    };

    let (client, mut events) = Client::connect(config, session).await?;

    let c = client.clone();
    tokio::spawn(async move {
        match c.list_conversations(Folder::Inbox, 10, None).await {
            Ok(page) => {
                for conv in page.conversations {
                    let preview = conv.preview.map(|p| p.text).unwrap_or_default();
                    println!("[{}] {}: {}", conv.id, conv.name, preview);
                }
            }
            Err(e) => eprintln!("listing conversations failed: {e}"),
        }
    });

    while let Some(ev) = events.recv().await {
        match ev {
            Event::MessageUpserted { message, old } => println!(
                "message {} in {} ({:?}{}): {}",
                message.id,
                message.conversation_id,
                message.status,
                if old { ", old" } else { "" },
                message.text()
            ),
            Event::ConversationUpdated(c) => println!("conversation {} updated: {}", c.id, c.name),
            Event::SessionChanged(s) => std::fs::write(&path, s.to_json())?,
            Event::LoggedOut(reason) => {
                println!("logged out ({reason:?}): pair again");
                let _ = std::fs::remove_file(&path);
                break;
            }
            other => println!("{other:?}"),
        }
    }
    Ok(())
}
