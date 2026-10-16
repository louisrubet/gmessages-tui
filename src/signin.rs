//! Private sign-in subprocess protocol; never print credentials or page URLs.
use crate::pair::{self, Cookies};

#[derive(Debug, PartialEq)]
enum Page {
    Messages,
    Refused,
    Other,
}

fn classify(address: &str) -> Page {
    let Ok(url) = url::Url::parse(address) else {
        return Page::Other;
    };
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Page::Other;
    }
    match (url.host_str(), url.path()) {
        (
            Some("messages.google.com"),
            "/web/config" | "/web/conversations" | "/web/conversations/",
        ) => Page::Messages,
        (Some("accounts.google.com"), path)
            if path.split('/').any(|segment| segment == "rejected") =>
        {
            Page::Refused
        }
        _ => Page::Other,
    }
}

// Wry 0.57's macOS cookies_for_url compares domains for equality, excluding
// Google's parent-domain cookies. Limit its all-cookies fallback to this target.
#[cfg(any(target_os = "macos", test))]
fn messages_cookie_scope(domain: Option<&str>, path: Option<&str>) -> bool {
    if !matches!(
        domain.map(|domain| domain.trim_start_matches('.')),
        Some("google.com" | "messages.google.com")
    ) {
        return false;
    }
    let target = "/web/config";
    let path = path.unwrap_or("/");
    path == target
        || (!path.is_empty()
            && target.starts_with(path)
            && (path.ends_with('/') || target.as_bytes().get(path.len()) == Some(&b'/')))
}

fn credentials(line: &str) -> Result<Cookies, String> {
    // Do not include serde errors: they can quote secret input.
    let all: Cookies =
        serde_json::from_str(line).map_err(|_| "Invalid native sign-in response".to_string())?;
    pair::parse(&serde_json::to_string(&all).expect("string map"))
}

#[cfg(feature = "wry-signin")]
mod native;
#[cfg(feature = "wry-signin")]
pub use native::{Native, run_child};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_trusted_pages_only() {
        assert_eq!(
            classify("https://messages.google.com/web/config?token=secret"),
            Page::Messages
        );
        assert_eq!(
            classify("https://messages.google.com/web/conversations"),
            Page::Messages
        );
        for url in [
            "https://messages.google.com/web/config-evil",
            "https://messages.google.com.evil/web/config",
            "https://messages.google.com@evil.test/web/config",
            "http://messages.google.com/web/config",
            "https://messages.google.com:444/web/config",
            "https://evil.test/rejected",
        ] {
            assert_eq!(classify(url), Page::Other, "{url}");
        }
        assert_eq!(
            classify("https://accounts.google.com/signin/v2/rejected?secret=x"),
            Page::Refused
        );
    }

    #[test]
    fn messages_cookie_scope_accepts_parent_domain_and_matching_paths_only() {
        for domain in ["google.com", ".google.com", "messages.google.com"] {
            for path in ["/", "/web", "/web/", "/web/config"] {
                assert!(messages_cookie_scope(Some(domain), Some(path)));
            }
        }
        for domain in [
            "evilgoogle.com",
            "accounts.google.com",
            "com",
            "google.com.evil",
        ] {
            assert!(!messages_cookie_scope(Some(domain), Some("/")));
        }
        for path in ["/we", "/web/config/", "/web/config-evil", "/other"] {
            assert!(!messages_cookie_scope(Some("google.com"), Some(path)));
        }
        assert!(!messages_cookie_scope(None, Some("/")));
    }

    #[test]
    fn protocol_validates_filters_and_never_echoes_secrets() {
        let line = r#"{"SID":"s","HSID":"h","SSID":"ss","APISID":"a","SAPISID":"sa","OSID":"o","SOCS":"unneeded","__Secure-1PSIDTS":"optional"}"#;
        let cookies = credentials(line).unwrap();
        assert_eq!(cookies.len(), 7);
        assert!(!cookies.contains_key("SOCS"));
        assert!(credentials(&line.replace("\"o\"", "\"\"")).is_err());
        for invalid in ["secret-not-json", r#"{"SID":123,"secret":"sensitive"}"#] {
            let error = credentials(invalid).unwrap_err();
            assert!(!error.contains("secret"));
            assert!(!error.contains("sensitive"));
        }
    }
}
