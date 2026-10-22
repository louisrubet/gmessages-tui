//! The persistent session (spec §3 "What a client keeps").

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{RefreshKey, RequestKeys};
use crate::error::{Error, Result};
use crate::wire;

/// Identity of a party (the phone or this browser) at the relay. Opaque: kept as received
/// and sent back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// `user_id` (field 1).
    pub user_id: i64,
    /// `source_id` (field 2).
    pub source_id: String,
    /// `network` (field 3).
    pub network: String,
}

impl From<wire::Device> for Device {
    fn from(d: wire::Device) -> Self {
        Device {
            user_id: d.user_id,
            source_id: d.source_id,
            network: d.network,
        }
    }
}

impl From<&Device> for wire::Device {
    fn from(d: &Device) -> Self {
        wire::Device {
            user_id: d.user_id,
            source_id: d.source_id.clone(),
            network: d.network.clone(),
        }
    }
}

mod b64 {
    use super::*;
    pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::codec::B64.encode(v))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        crate::codec::b64_decode_lenient(&s).map_err(serde::de::Error::custom)
    }
}

mod b64_32 {
    use super::*;
    pub fn serialize<S: serde::Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        super::b64::serialize(v, s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let v = super::b64::deserialize(d)?;
        v.try_into()
            .map_err(|_| serde::de::Error::custom("expected 32 bytes"))
    }
}

/// Everything needed to resume without pairing again.
///
/// Serializable with serde (keys and token as base64, the refresh key as a JWK). The
/// application should persist it after pairing and whenever an
/// [`Event::SessionChanged`](crate::Event::SessionChanged) arrives (token refreshed,
/// cookies updated).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// `K_aes`: AES-256 key of payloads exchanged with the phone.
    #[serde(with = "b64_32")]
    pub aes_key: [u8; 32],
    /// `K_mac`: HMAC-SHA256 key of payloads exchanged with the phone.
    #[serde(with = "b64_32")]
    pub hmac_key: [u8; 32],
    /// ECDSA P-256 key pair registered at pairing, signs token refreshes.
    pub refresh_key: RefreshKey,
    /// This client's identity.
    pub browser: Device,
    /// The phone's identity.
    pub phone: Device,
    /// Relay token.
    #[serde(with = "b64")]
    pub token: Vec<u8>,
    /// Token validity, µs.
    pub token_ttl_us: i64,
    /// Token expiry, µs since the Unix epoch.
    pub token_expiry_us: i64,
    /// Web device id from the configuration.
    #[serde(default)]
    pub web_device_id: Option<Uuid>,
    /// Destination id (the phone's registration). Present only for Google account
    /// sessions, which it identifies.
    #[serde(default)]
    pub destination_id: Option<Uuid>,
    /// Pairing attempt id (Google account sessions), needed to unpair.
    #[serde(default)]
    pub pairing_attempt_id: Option<Uuid>,
    /// Google cookies (Google account sessions), name → value.
    #[serde(default)]
    pub cookies: BTreeMap<String, String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("browser", &self.browser)
            .field("phone", &self.phone)
            .field("token_expiry_us", &self.token_expiry_us)
            .field("web_device_id", &self.web_device_id)
            .field("destination_id", &self.destination_id)
            .field("pairing_attempt_id", &self.pairing_attempt_id)
            .field("cookies", &self.cookies.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// Raw parts of a session, for importing a pairing made by another program.
///
/// Byte fields are raw bytes; ids are UUID strings (with or without dashes).
#[derive(Debug, Clone, Default)]
pub struct SessionParts {
    /// `K_aes`, 32 bytes.
    pub aes_key: Vec<u8>,
    /// `K_mac`, 32 bytes.
    pub hmac_key: Vec<u8>,
    /// Refresh key private scalar (32 bytes big-endian; shorter is left-padded). Use
    /// [`RefreshKey::from_jwk`] and [`Session::from_parts_with_key`] for a JWK.
    pub refresh_key_d: Vec<u8>,
    /// Browser device.
    pub browser: Device,
    /// Phone device.
    pub phone: Device,
    /// Relay token.
    pub token: Vec<u8>,
    /// Token TTL, µs (0: 24 hours assumed).
    pub token_ttl_us: i64,
    /// Token expiry, µs since the epoch (0: unknown, a refresh is forced at connect).
    pub token_expiry_us: i64,
    /// Web device id.
    pub web_device_id: Option<String>,
    /// Destination id (Google account sessions only).
    pub destination_id: Option<String>,
    /// Pairing attempt id (Google account sessions only).
    pub pairing_attempt_id: Option<String>,
    /// Cookies (Google account sessions only).
    pub cookies: BTreeMap<String, String>,
}

/// Current time in µs since the Unix epoch.
pub(crate) fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

pub(crate) const DEFAULT_TTL_US: i64 = 24 * 3600 * 1_000_000;

fn parse_uuid(s: &Option<String>, what: &str) -> Result<Option<Uuid>> {
    match s.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => Uuid::parse_str(s)
            .map(Some)
            .map_err(|e| Error::InvalidInput(format!("{what}: {e}"))),
    }
}

fn key32(v: &[u8], what: &str) -> Result<[u8; 32]> {
    v.try_into()
        .map_err(|_| Error::InvalidInput(format!("{what} must be 32 bytes, got {}", v.len())))
}

impl Session {
    /// Builds a session from raw parts.
    pub fn from_parts(parts: SessionParts) -> Result<Session> {
        let key = RefreshKey::from_scalar_bytes(&parts.refresh_key_d)?;
        Self::from_parts_with_key(parts, key)
    }

    /// Builds a session from raw parts, with an already loaded refresh key
    /// (`parts.refresh_key_d` is ignored).
    pub fn from_parts_with_key(parts: SessionParts, refresh_key: RefreshKey) -> Result<Session> {
        Ok(Session {
            aes_key: key32(&parts.aes_key, "aes_key")?,
            hmac_key: key32(&parts.hmac_key, "hmac_key")?,
            refresh_key,
            browser: parts.browser,
            phone: parts.phone,
            token: parts.token,
            token_ttl_us: parts.token_ttl_us,
            token_expiry_us: parts.token_expiry_us,
            web_device_id: parse_uuid(&parts.web_device_id, "web_device_id")?,
            destination_id: parse_uuid(&parts.destination_id, "destination_id")?,
            pairing_attempt_id: parse_uuid(&parts.pairing_attempt_id, "pairing_attempt_id")?,
            cookies: parts.cookies,
        })
    }

    /// A blank session around a refresh key (used while pairing).
    pub(crate) fn blank(refresh_key: RefreshKey) -> Session {
        Session {
            aes_key: [0; 32],
            hmac_key: [0; 32],
            refresh_key,
            browser: Device::default(),
            phone: Device::default(),
            token: Vec::new(),
            token_ttl_us: 0,
            token_expiry_us: 0,
            web_device_id: None,
            destination_id: None,
            pairing_attempt_id: None,
            cookies: BTreeMap::new(),
        }
    }

    /// Whether this is a Google account session (it has a destination id).
    pub fn is_google_account(&self) -> bool {
        self.destination_id.is_some()
    }

    /// The request keys.
    pub fn request_keys(&self) -> RequestKeys {
        RequestKeys {
            aes: self.aes_key,
            hmac: self.hmac_key,
        }
    }

    /// Whether the session holds what a connection needs (keys, token, devices).
    pub fn is_complete(&self) -> bool {
        !self.request_keys().is_empty()
            && !self.token.is_empty()
            && !self.phone.source_id.is_empty()
    }

    /// Time left before the token expires (zero if expired or unknown).
    pub fn token_time_left(&self) -> Duration {
        let left = self.token_expiry_us - now_us();
        Duration::from_micros(left.max(0) as u64)
    }

    /// Stores a new token: `expiry = now + ttl` (ttl 0 means 24 hours).
    pub(crate) fn set_token(&mut self, t: &wire::TokenData) {
        let ttl = if t.ttl > 0 { t.ttl } else { DEFAULT_TTL_US };
        self.token = t.token.clone();
        self.token_ttl_us = ttl;
        self.token_expiry_us = now_us() + ttl;
    }

    /// Serializes to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("session serializes")
    }

    /// Parses from JSON.
    pub fn from_json(s: &str) -> Result<Session> {
        serde_json::from_str(s).map_err(|e| Error::InvalidInput(format!("session JSON: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_parts() {
        let mut s = Session::blank(RefreshKey::generate());
        s.aes_key = [1; 32];
        s.hmac_key = [2; 32];
        s.token = vec![1, 2, 3];
        s.phone.source_id = "abc".into();
        s.destination_id = Some(Uuid::new_v4());
        s.cookies.insert("SAPISID".into(), "x".into());
        let j = s.to_json();
        let back = Session::from_json(&j).unwrap();
        assert_eq!(back, s);
        assert!(back.is_google_account());
        assert!(back.is_complete());

        let parts = SessionParts {
            aes_key: vec![1; 32],
            hmac_key: vec![2; 32],
            refresh_key_d: s.refresh_key.scalar_bytes().to_vec(),
            destination_id: Some(s.destination_id.unwrap().simple().to_string()),
            ..Default::default()
        };
        let p = Session::from_parts(parts).unwrap();
        assert_eq!(p.refresh_key, s.refresh_key);
        assert_eq!(p.destination_id, s.destination_id);
        assert!(
            Session::from_parts(SessionParts {
                aes_key: vec![1; 31],
                hmac_key: vec![2; 32],
                refresh_key_d: vec![1; 32],
                ..Default::default()
            })
            .is_err()
        );
    }
}
