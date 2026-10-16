//! What the phone sent, kept between runs: the conversations, their messages and the media
//! downloaded, in a SQLite database encrypted with SQLCipher (key: see [`crate::key`]).
//!
//! The interface starts from it, then only what changed is fetched again.

use std::error::Error;
use std::path::Path;

use chat_tui::data::Attachment;
use rusqlite::{Connection, OptionalExtension, params};

use crate::gmd;

/// Version of the schema, in `PRAGMA user_version`.
const SCHEMA: i64 = 1;

/// Messages loaded per conversation at start: the most recent ones.
const LOADED: i64 = 1000;

pub struct Store {
    conn: Connection,
}

/// A conversation as stored.
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub group: bool,
    pub messages: Vec<gmd::Message>,
}

impl Store {
    /// Opens the store at `path`, encrypted with `key`, created if missing.
    pub fn open(path: &Path, key: &str) -> Result<Self, Box<dyn Error>> {
        restrict_new_files();
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "key", key)?;
        // Reading fails here with a wrong key.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|e| {
                format!(
                    "cannot open the store {} (wrong passphrase?): {e}",
                    path.display()
                )
            })?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE conversations (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    is_group INTEGER NOT NULL,
                    timestamp_ms INTEGER NOT NULL
                );
                CREATE TABLE messages (
                    id TEXT PRIMARY KEY,
                    conversation TEXT NOT NULL,
                    sent_ms INTEGER NOT NULL,
                    -- The message as gmd says it (JSON): media with their keys, reactions.
                    data TEXT NOT NULL
                );
                CREATE INDEX messages_by_conversation ON messages (conversation, sent_ms);
                CREATE TABLE attachments (
                    message TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    content_type TEXT NOT NULL,
                    file_name TEXT,
                    width INTEGER,
                    height INTEGER,
                    bytes BLOB NOT NULL,
                    PRIMARY KEY (message, position)
                );",
            )?;
        }
        self.conn.pragma_update(None, "user_version", SCHEMA)
    }

    /// The conversations, the most recent first, with their last messages in send order.
    pub fn conversations(&self) -> Result<Vec<Conversation>, Box<dyn Error>> {
        let mut conversations = self
            .conn
            .prepare("SELECT id, name, is_group FROM conversations ORDER BY timestamp_ms DESC")?
            .query_map([], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    group: row.get(2)?,
                    messages: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut messages = self.conn.prepare(
            "SELECT data FROM (SELECT data, sent_ms FROM messages WHERE conversation = ?1
             ORDER BY sent_ms DESC LIMIT ?2) ORDER BY sent_ms",
        )?;
        for conversation in &mut conversations {
            for data in messages.query_map(params![conversation.id, LOADED], |row| {
                row.get::<_, String>(0)
            })? {
                conversation.messages.push(serde_json::from_str(&data?)?);
            }
        }
        Ok(conversations)
    }

    /// Records the conversations as the phone lists them.
    pub fn save_conversations(&self, conversations: &[gmd::Conversation]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut save = tx.prepare(
                "INSERT INTO conversations (id, name, is_group, timestamp_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, is_group = ?3, timestamp_ms = ?4",
            )?;
            for c in conversations {
                save.execute(params![c.id, c.name, c.group, c.timestamp_ms])?;
            }
        }
        tx.commit()
    }

    /// Whether message `id` is stored.
    pub fn has_message(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn
            .query_row("SELECT 1 FROM messages WHERE id = ?1", [id], |_| Ok(()))
            .optional()
            .map(|found| found.is_some())
    }

    /// Records messages of `conversation`, replacing those stored already.
    pub fn save_messages(
        &self,
        conversation: &str,
        messages: &[gmd::Message],
    ) -> Result<(), Box<dyn Error>> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut save = tx.prepare(
                "INSERT INTO messages (id, conversation, sent_ms, data) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET conversation = ?2, sent_ms = ?3, data = ?4",
            )?;
            for m in messages {
                let data = serde_json::to_string(m)?;
                save.execute(params![m.id, conversation, m.timestamp_ms, data])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Message `id`, if stored.
    pub fn message(&self, id: &str) -> Result<Option<gmd::Message>, Box<dyn Error>> {
        let data: Option<String> = self
            .conn
            .query_row("SELECT data FROM messages WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(data.map(|data| serde_json::from_str(&data)).transpose()?)
    }

    /// The downloaded attachments of message `id`, `None` if not downloaded.
    pub fn attachments(&self, id: &str) -> rusqlite::Result<Option<Vec<Attachment>>> {
        let attachments = self
            .conn
            .prepare(
                "SELECT content_type, file_name, width, height, bytes FROM attachments
                 WHERE message = ?1 ORDER BY position",
            )?
            .query_map([id], |row| {
                Ok(Attachment {
                    content_type: row.get(0)?,
                    file_name: row.get(1)?,
                    width: row.get(2)?,
                    height: row.get(3)?,
                    bytes: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((!attachments.is_empty()).then_some(attachments))
    }

    /// Records the downloaded attachments of message `id`.
    pub fn save_attachments(&self, id: &str, attachments: &[Attachment]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM attachments WHERE message = ?1", [id])?;
        {
            let mut save = tx.prepare(
                "INSERT INTO attachments (message, position, content_type, file_name, width, height, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (position, a) in attachments.iter().enumerate() {
                save.execute(params![
                    id,
                    position as i64,
                    a.content_type,
                    a.file_name,
                    a.width,
                    a.height,
                    a.bytes
                ])?;
            }
        }
        tx.commit()
    }
}

/// New files (the store, its WAL…) readable by us only.
fn restrict_new_files() {
    #[cfg(unix)]
    // SAFETY: umask only sets the process file mode creation mask.
    unsafe {
        libc::umask(0o077);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "gmessages-store-{name}-{:x}.db",
            u64::from_ne_bytes(random)
        ))
    }

    fn message(id: &str, ms: i64) -> gmd::Message {
        serde_json::from_value(serde_json::json!({
            "id": id, "timestamp_ms": ms, "from_me": false, "sender_id": "2",
            "sender": "Ann", "text": format!("text of {id}"),
            "media": [{"mime": "image/jpeg", "media_id": "m", "key": "a2V5"}],
            "reactions": [{"emoji": "👍", "sender_id": "1", "mine": true}],
        }))
        .unwrap()
    }

    #[test]
    fn keeps_conversations_messages_and_attachments_encrypted() {
        let path = temp_path("roundtrip");
        {
            let store = Store::open(&path, "secret").unwrap();
            let conversations: Vec<gmd::Conversation> = serde_json::from_value(serde_json::json!([
                {"id": "old", "name": "Old", "group": false, "timestamp_ms": 1},
                {"id": "new", "name": "New", "group": true, "timestamp_ms": 2},
            ]))
            .unwrap();
            store.save_conversations(&conversations).unwrap();
            store
                .save_messages("new", &[message("b", 20), message("a", 10)])
                .unwrap();
            // Known messages replaced.
            let mut edited = message("a", 10);
            edited.text = "edited".to_string();
            store.save_messages("new", &[edited]).unwrap();
            let image = Attachment {
                content_type: "image/jpeg".to_string(),
                file_name: None,
                bytes: vec![1, 2, 3],
                width: Some(4),
                height: None,
            };
            store
                .save_attachments("a", std::slice::from_ref(&image))
                .unwrap();
            assert_eq!(store.attachments("a").unwrap(), Some(vec![image]));
            assert_eq!(store.attachments("b").unwrap(), None);
        }

        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"), "encrypted on disk");
        assert!(
            !bytes.windows(9).any(|w| w == b"text of b"),
            "no message in clear"
        );
        assert!(Store::open(&path, "wrong").is_err(), "the key is checked");

        let store = Store::open(&path, "secret").unwrap();
        let conversations = store.conversations().unwrap();
        let ids: Vec<_> = conversations.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["new", "old"], "most recent first");
        let messages = &conversations[0].messages;
        let texts: Vec<_> = messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["edited", "text of b"], "in send order");
        assert_eq!(messages[0].media[0]["mime"], "image/jpeg");
        assert!(messages[0].reactions[0].mine);
        assert!(store.has_message("b").unwrap());
        assert!(!store.has_message("c").unwrap());
        assert_eq!(store.message("b").unwrap().unwrap().sender, "Ann");

        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
