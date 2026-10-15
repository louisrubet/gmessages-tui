//! Pairing in the terminal, before the interface: the cookies of a browser signed in to the
//! Google account of the phone, pasted here.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{execute, terminal};

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

const SIGN_IN: &str =
    "https://accounts.google.com/AccountChooser?continue=https://messages.google.com/web/config";

/// Explains, then reads the pasted cookies until they are complete. `None` if cancelled.
pub fn ask_cookies() -> io::Result<Option<Cookies>> {
    println!(
        "Pair with the Google account of your phone:\n\
         \n  1. In a browser, preferably a private window, sign in to that account at\n     \
         {SIGN_IN}\n  \
         2. Open the developer tools (F12), Network tab, reload, right-click the `config` request:\n     \
         Copy > Copy as cURL.\n  \
         3. Paste it here. Then close the window without signing out: it would end the session.\n\
         \n(Esc or Ctrl+C to quit; `gmessages-tui --qr` pairs by QR code instead.)"
    );
    loop {
        let Some(text) = read_paste()? else {
            return Ok(None);
        };
        match parse(&text) {
            Ok(cookies) => return Ok(Some(cookies)),
            Err(e) => println!("{e}. Paste again."),
        }
    }
}

/// Reads what is pasted: a bracketed paste, or keys up to a pause after an Enter (terminals
/// without bracketed paste). `None` on Esc or Ctrl+C.
fn read_paste() -> io::Result<Option<String>> {
    terminal::enable_raw_mode()?;
    let _ = execute!(io::stdout(), event::EnableBracketedPaste);
    let result = (|| {
        let mut text = String::new();
        loop {
            match event::read()? {
                Event::Paste(paste) => return Ok(Some(paste)),
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Char(c) => text.push(c),
                    KeyCode::Enter if !text.trim().is_empty() => {
                        text.push('\n');
                        // Pasted lines keep coming; a pause ends the paste.
                        if !event::poll(Duration::from_millis(300))? {
                            return Ok(Some(text));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    })();
    let _ = execute!(io::stdout(), event::DisableBracketedPaste);
    terminal::disable_raw_mode()?;
    let _ = io::stdout().flush();
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
