//! Client configuration.

use std::time::Duration;

/// Settings of the client: identity shown to the phone and Google, and timeouts.
#[derive(Debug, Clone)]
pub struct Config {
    /// Name of this device as shown by the phone in its list of paired devices
    /// (`BrowserDetails.os`).
    pub device_name: String,
    /// `user-agent` header value (a current Chrome).
    pub user_agent: String,
    /// `sec-ch-ua` header value, consistent with `user_agent`.
    pub sec_ch_ua: String,
    /// `sec-ch-ua-mobile` header value.
    pub sec_ch_ua_mobile: String,
    /// `sec-ch-ua-platform` header value.
    pub sec_ch_ua_platform: String,
    /// `x-goog-api-key` header value (the public key of the web client).
    pub api_key: String,
    /// Build date of the web client imitated: (year, month, day).
    pub client_version: (i32, i32, i32),
    /// `BrowserDetails.browser` value (1: other).
    pub browser_kind: i32,
    /// `BrowserDetails.device` value (2: tablet).
    pub device_kind: i32,
    /// How long to wait for the phone's reply to a call.
    pub call_timeout: Duration,
    /// Size of the first page when the client lists conversations implicitly (unused by
    /// the library itself, available to applications).
    pub default_page_size: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            device_name: "gmessages-proto".into(),
            user_agent: "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36".into(),
            sec_ch_ua: r#""Google Chrome";v="152", "Chromium";v="152", "Not-A.Brand";v="24""#.into(),
            sec_ch_ua_mobile: "?1".into(),
            sec_ch_ua_platform: r#""Android""#.into(),
            api_key: "AIzaSyCA4RsOZUFrm9whhtGosPlJLmVPnfSHKz8".into(),
            client_version: (2026, 9, 2),
            browser_kind: 1,
            device_kind: 2,
            call_timeout: Duration::from_secs(60),
            default_page_size: 25,
        }
    }
}

impl Config {
    /// A default configuration with the given device name.
    pub fn with_device_name(name: impl Into<String>) -> Self {
        Config {
            device_name: name.into(),
            ..Default::default()
        }
    }
}
