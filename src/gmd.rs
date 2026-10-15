//! gmd, the Go side speaking to Google Messages, embedded in this executable: written to
//! the cache directory on first use, then run as a child process speaking JSON lines.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const GMD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gmd"));
const GMD_SHA256: &str = env!("GMD_SHA256");
/// Version of the JSON lines protocol, as gmd says in its `hello`.
const PROTOCOL: u32 = 2;

/// What gmd reports.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Hello {
        protocol: u32,
    },
    /// No session: tell how to pair (`pair_google` or `pair_qr`).
    Login,
    /// Scan this from the phone to pair.
    Qr {
        url: String,
    },
    /// Tap this emoji on the phone to pair.
    Emoji {
        emoji: String,
    },
    /// The Google session has ended: new cookies of the account sign in again.
    Expired {
        message: String,
    },
    /// Pairing failed: try again.
    PairFailed {
        message: String,
    },
    Paired,
    Ready {
        conversations: Vec<Conversation>,
    },
    Status {
        message: String,
    },
    /// gmd stops.
    Fatal {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub group: bool,
    pub timestamp_ms: i64,
    pub latest: Option<Latest>,
}

/// The last message of a conversation, as the phone summarizes it.
#[derive(Debug, Deserialize)]
pub struct Latest {
    pub id: String,
    pub text: String,
    pub from_me: bool,
    pub sender: String,
}

/// gmd running.
pub struct Gmd {
    child: Child,
    stdin: ChildStdin,
    events: Lines<BufReader<ChildStdout>>,
}

impl Gmd {
    /// Starts gmd, its session in `session` and its log in `log`, and checks it speaks our
    /// protocol.
    pub async fn start(session: &Path, log: &Path) -> io::Result<Self> {
        let mut child = Command::new(install()?)
            .arg("--session")
            .arg(session)
            .arg("--log")
            .arg(log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let events = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        let mut gmd = Gmd {
            child,
            stdin,
            events,
        };
        match gmd.next().await? {
            Some(Event::Hello { protocol }) if protocol == PROTOCOL => Ok(gmd),
            other => Err(io::Error::other(format!(
                "gmd does not speak protocol {PROTOCOL}: {other:?}"
            ))),
        }
    }

    /// Sends a command.
    pub async fn send(&mut self, command: serde_json::Value) -> io::Result<()> {
        let mut line = command.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await
    }

    /// The next event, `None` once gmd has stopped.
    pub async fn next(&mut self) -> io::Result<Option<Event>> {
        next_event(&mut self.events).await
    }

    /// Splits it: the events to read, the rest to stop it.
    pub fn split(self) -> (Lines<BufReader<ChildStdout>>, Control) {
        let control = Control {
            child: self.child,
            stdin: self.stdin,
        };
        (self.events, control)
    }
}

/// The next event of `events`, `None` at the end. Lines that are not events are skipped.
pub async fn next_event(events: &mut Lines<BufReader<ChildStdout>>) -> io::Result<Option<Event>> {
    while let Some(line) = events.next_line().await? {
        if let Ok(event) = serde_json::from_str(&line) {
            return Ok(Some(event));
        }
    }
    Ok(None)
}

/// What stops gmd.
pub struct Control {
    child: Child,
    stdin: ChildStdin,
}

impl Control {
    /// Asks gmd to quit, and kills it if it does not within a second.
    pub async fn stop(mut self) {
        let _ = self.stdin.write_all(b"{\"cmd\":\"quit\"}\n").await;
        let _ = self.stdin.flush().await;
        if tokio::time::timeout(std::time::Duration::from_secs(1), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

/// Writes the embedded gmd to the cache directory (once per version), and returns its path.
fn install() -> io::Result<PathBuf> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let path = cache_dir()?.join(format!("gmd-{}{ext}", &GMD_SHA256[..12]));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == GMD.len() as u64) {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().expect("in the cache directory"))?;
    // Written aside then renamed: another instance never runs a half-written file.
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, GMD)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// `gmessages-tui` in the user's cache directory: `~/.cache` (or `$XDG_CACHE_HOME`),
/// `~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows.
fn cache_dir() -> io::Result<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|home| home.join(".cache")))
    };
    base.map(|dir| dir.join("gmessages-tui"))
        .ok_or_else(|| io::Error::other("no cache directory (HOME is not set)"))
}
