//! Pairing, before the interface: the cookies of a browser signed in to the Google account
//! of the phone, pasted in the terminal or in the pairing page (see [`web`]).

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{execute, terminal};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::web;

/// The cookies libgm needs (mautrix-gmessages asks the same), the last one optional.
const COOKIES: [&str; 7] = [
    "SID",
    "HSID",
    "SSID",
    "APISID",
    "SAPISID",
    "OSID",
    "__Secure-1PSIDTS",
];
const OPTIONAL: usize = 1;

pub type Cookies = BTreeMap<String, String>;

pub const SIGN_IN: &str =
    "https://accounts.google.com/AccountChooser?continue=https://messages.google.com/web/config";

/// What the user gives, in the terminal or the browser.
pub enum Pasted {
    Cookies(Cookies),
    Quit,
}

/// A pairing under way: the cookies are pasted in the terminal or in the pairing page, served
/// on localhost, which also shows the emoji to tap.
pub struct Pairing {
    pasted: UnboundedReceiver<Pasted>,
    page: web::Page,
    stop: Arc<AtomicBool>,
    terminal: Option<std::thread::JoinHandle<()>>,
}

impl Pairing {
    /// Serves the page, tells where it is, and reads the terminal.
    pub async fn start() -> io::Result<Self> {
        let (paste, pasted) = unbounded();
        let page = web::Page::serve(paste.clone()).await?;
        say(&format!(
            "Pair with the Google account of your phone: open\n\n  {}\n\n\
             Or here: sign in to that account at {}, then in the developer tools (F12),\n\
             Network, reload, right-click the `config` request, Copy as cURL, and paste it here.\n\
             \n(Esc or Ctrl+C to quit; `gmessages-tui --qr` pairs by QR code instead.)",
            link(page.url()),
            link(SIGN_IN),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let terminal = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                if let Err(e) = read_terminal(&stop, &paste) {
                    say(&format!("Cannot read the terminal: {e}"));
                }
            })
        };
        Ok(Pairing {
            pasted,
            page,
            stop,
            terminal: Some(terminal),
        })
    }

    /// What is pasted next.
    pub async fn next(&mut self) -> Option<Pasted> {
        self.pasted.next().await
    }

    pub fn page(&self) -> &web::Page {
        &self.page
    }

    /// Paired and ready: the terminal goes back to the interface, the page says it is done.
    pub fn finish(mut self) {
        self.stop_terminal();
        self.page.done();
    }

    fn stop_terminal(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(terminal) = self.terminal.take() {
            let _ = terminal.join();
        }
    }
}

impl Drop for Pairing {
    fn drop(&mut self) {
        self.stop_terminal();
    }
}

/// Prints `text` as lines, also right while the terminal is raw (reading pastes).
pub fn say(text: &str) {
    let mut out = io::stdout().lock();
    let _ = write!(out, "{}\r\n", text.replace('\n', "\r\n"));
    let _ = out.flush();
}

/// `url`, clickable in terminals that know OSC 8 (the others show it as it is).
fn link(url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{url}\x1b]8;;\x1b\\")
}

/// Reads the pastes until `stop`, the terminal raw meanwhile: a bracketed paste, or keys up
/// to a pause after an Enter (terminals without bracketed paste).
fn read_terminal(stop: &AtomicBool, paste: &UnboundedSender<Pasted>) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let _ = execute!(io::stdout(), event::EnableBracketedPaste);
    let result = (|| {
        let mut text = String::new();
        let tick = Duration::from_millis(100);
        while !stop.load(Ordering::Relaxed) {
            if !event::poll(tick)? {
                continue;
            }
            let pasted = match event::read()? {
                Event::Paste(pasted) => pasted,
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char(c) => {
                        text.push(c);
                        continue;
                    }
                    // Pasted lines keep coming; a pause ends the paste.
                    KeyCode::Enter
                        if !text.trim().is_empty() && !event::poll(Duration::from_millis(300))? =>
                    {
                        std::mem::take(&mut text)
                    }
                    KeyCode::Enter => {
                        text.push('\n');
                        continue;
                    }
                    _ => continue,
                },
                _ => continue,
            };
            match parse(&pasted) {
                Ok(cookies) => {
                    let _ = paste.unbounded_send(Pasted::Cookies(cookies));
                }
                Err(e) => say(&format!("{e}. Paste again.")),
            }
        }
        if !stop.load(Ordering::Relaxed) {
            let _ = paste.unbounded_send(Pasted::Quit);
        }
        Ok(())
    })();
    let _ = execute!(io::stdout(), event::DisableBracketedPaste);
    terminal::disable_raw_mode()?;
    result
}

/// The cookies in `text`: a cURL command (Copy as cURL, from Chrome or Firefox), a `Cookie:`
/// header, or a JSON object.
pub fn parse(text: &str) -> Result<Cookies, String> {
    let text = text.trim();
    let all: Cookies = if text.starts_with('{') {
        serde_json::from_str(text).map_err(|e| format!("Not a JSON object of cookies: {e}"))?
    } else {
        pairs(curl_cookies(text).unwrap_or(text))
    };
    let missing: Vec<_> = COOKIES[..COOKIES.len() - OPTIONAL]
        .iter()
        .filter(|name| !all.contains_key(**name))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Missing cookies: {} (copy the request to messages.google.com, signed in)",
            missing.join(", ")
        ));
    }
    // Only these: the session keeps them, no more than it needs.
    Ok(all
        .into_iter()
        .filter(|(name, _)| COOKIES.contains(&name.as_str()))
        .collect())
}

/// The cookies of a cURL command: `-b '…'`, `--cookie '…'` or `-H 'Cookie: …'`.
fn curl_cookies(curl: &str) -> Option<&str> {
    for (i, _) in curl.match_indices(['\'', '"']) {
        let quote = &curl[i..i + 1];
        let before = curl[..i].trim_end();
        let rest = &curl[i + 1..];
        let value = &rest[..rest.find(quote)?];
        if before.ends_with(" -b") || before.ends_with(" --cookie") {
            return Some(value);
        }
        if (before.ends_with(" -H") || before.ends_with(" --header"))
            && let Some((name, cookies)) = value.split_once(':')
            && name.trim().eq_ignore_ascii_case("cookie")
        {
            return Some(cookies);
        }
    }
    None
}

/// `name=value; name=value`, with or without a `Cookie:` in front.
fn pairs(header: &str) -> Cookies {
    let header = header.trim();
    let header = match header.split_once(':') {
        Some((name, rest)) if name.trim().eq_ignore_ascii_case("cookie") => rest,
        _ => header,
    };
    header
        .split(';')
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOKIE: &str = "SID=s; HSID=h; SSID=ss; APISID=a; SAPISID=sa; OSID=o; NID=n";

    fn check(cookies: Cookies) {
        assert_eq!(cookies["SID"], "s");
        assert_eq!(cookies["OSID"], "o");
        assert!(!cookies.contains_key("NID"), "only the cookies libgm needs");
        assert_eq!(cookies.len(), 6);
    }

    #[test]
    fn chrome_curl() {
        let curl = format!(
            "curl 'https://messages.google.com/web/config' \\\n  -H 'accept: */*' \\\n  -b '{COOKIE}' \\\n  -H 'user-agent: x'"
        );
        check(parse(&curl).unwrap());
    }

    #[test]
    fn firefox_curl() {
        let curl = format!(
            "curl 'https://messages.google.com/web/config' -H 'Accept: */*' -H 'Cookie: {COOKIE}' -H 'TE: trailers'"
        );
        check(parse(&curl).unwrap());
    }

    #[test]
    fn header_and_json() {
        check(parse(&format!("Cookie: {COOKIE}")).unwrap());
        check(parse(COOKIE).unwrap());
        let json = r#"{"SID":"s","HSID":"h","SSID":"ss","APISID":"a","SAPISID":"sa","OSID":"o"}"#;
        check(parse(json).unwrap());
    }

    #[test]
    fn missing() {
        let e = parse("curl 'https://x' -b 'SID=s; HSID=h'").unwrap_err();
        assert!(e.contains("SSID, APISID, SAPISID, OSID"), "{e}");
    }
}
