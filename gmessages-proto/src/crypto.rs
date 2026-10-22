//! Cryptography of the protocol (spec §2).

use aes::Aes256;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use ctr::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use p256::pkcs8::EncodePublicKey;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};

type Aes256Ctr = ctr::Ctr128BE<Aes256>;
type HmacSha256 = Hmac<Sha256>;

/// Error from a cryptographic operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    /// The input is shorter than the minimum.
    #[error("ciphertext too short")]
    TooShort,
    /// The HMAC or the GCM tag does not match.
    #[error("authentication failed")]
    BadMac,
    /// The media header is malformed or announces too large chunks.
    #[error("invalid media header")]
    BadHeader,
    /// A key is malformed.
    #[error("invalid key: {0}")]
    BadKey(String),
}

/// Fills a fresh array with random bytes.
pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// The two symmetric keys shared with the phone (`K_aes`, `K_mac`).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct RequestKeys {
    /// AES-256 key.
    pub aes: [u8; 32],
    /// HMAC-SHA256 key.
    pub hmac: [u8; 32],
}

impl std::fmt::Debug for RequestKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RequestKeys(<redacted>)")
    }
}

impl RequestKeys {
    /// Generates random keys (QR pairing).
    pub fn generate() -> Self {
        RequestKeys {
            aes: random_bytes(),
            hmac: random_bytes(),
        }
    }

    /// Whether both keys are all zero (not set yet).
    pub fn is_empty(&self) -> bool {
        self.aes == [0; 32] && self.hmac == [0; 32]
    }

    /// Encrypts a payload: `AES-CTR(P) ‖ iv ‖ HMAC(C ‖ iv)`.
    pub fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        self.encrypt_with_iv(plain, random_bytes())
    }

    pub(crate) fn encrypt_with_iv(&self, plain: &[u8], iv: [u8; 16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(plain.len() + 48);
        out.extend_from_slice(plain);
        let mut c = Aes256Ctr::new(&self.aes.into(), &iv.into());
        c.apply_keystream(&mut out);
        out.extend_from_slice(&iv);
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.hmac).expect("any key size");
        mac.update(&out);
        out.extend_from_slice(&mac.finalize().into_bytes());
        out
    }

    /// Decrypts and authenticates a payload.
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if data.len() < 48 {
            return Err(CryptoError::TooShort);
        }
        let (body, tag) = data.split_at(data.len() - 32);
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.hmac).expect("any key size");
        mac.update(body);
        mac.verify_slice(tag).map_err(|_| CryptoError::BadMac)?;
        let (ct, iv) = body.split_at(body.len() - 16);
        let iv: [u8; 16] = iv.try_into().expect("16 bytes");
        let mut out = ct.to_vec();
        let mut c = Aes256Ctr::new(&self.aes.into(), &iv.into());
        c.apply_keystream(&mut out);
        Ok(out)
    }
}

/// Chunk size exponent used by writers (`2^15` = 32 768 bytes per encrypted chunk).
pub const MEDIA_CHUNK_EXP: u8 = 15;
/// Largest chunk size exponent accepted by readers.
pub const MEDIA_MAX_CHUNK_EXP: u8 = 22;
const GCM_OVERHEAD: usize = 12 + 16;

fn media_aad(last: bool, index: u32) -> [u8; 5] {
    let mut a = [0u8; 5];
    a[0] = last as u8;
    a[1..].copy_from_slice(&index.to_be_bytes());
    a
}

/// Encrypts a media file with the chunked AES-GCM format (chunk exponent 15).
pub fn encrypt_media(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
    encrypt_media_with_exp(key, plain, MEDIA_CHUNK_EXP)
}

/// Encrypts a media file with an explicit chunk size exponent `s` (`2^s > 28`).
pub fn encrypt_media_with_exp(key: &[u8; 32], plain: &[u8], s: u8) -> Vec<u8> {
    assert!(
        (5..=MEDIA_MAX_CHUNK_EXP).contains(&s),
        "chunk exponent out of range"
    );
    let piece = (1usize << s) - GCM_OVERHEAD;
    let cipher = Aes256Gcm::new(key.into());
    let n = plain.len().div_ceil(piece);
    let mut out = Vec::with_capacity(2 + plain.len() + n * GCM_OVERHEAD);
    out.push(0);
    out.push(s);
    for (i, chunk) in plain.chunks(piece).enumerate() {
        let nonce: [u8; 12] = random_bytes();
        let aad = media_aad(i + 1 == n, i as u32);
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: chunk,
                    aad: &aad,
                },
            )
            .expect("AES-GCM encryption cannot fail");
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
    }
    out
}

/// Decrypts a whole media file.
pub fn decrypt_media(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut d = MediaDecryptor::new(key);
    let mut out = d.update(data)?;
    out.extend(d.finish()?);
    Ok(out)
}

/// Incremental decryptor of the chunked AES-GCM media format.
///
/// Feed it the encrypted bytes as they arrive with [`update`](Self::update), which returns
/// the plaintext of every chunk known not to be the last; [`finish`](Self::finish) then
/// decrypts the last chunk. A full chunk is only released once at least one more byte is
/// buffered, so the last chunk is always recognised.
pub struct MediaDecryptor {
    cipher: Aes256Gcm,
    header: Vec<u8>,
    chunk: usize,
    index: u32,
    buf: Vec<u8>,
}

impl MediaDecryptor {
    /// Creates a decryptor for the given 32-byte key.
    pub fn new(key: &[u8; 32]) -> Self {
        MediaDecryptor {
            cipher: Aes256Gcm::new(key.into()),
            header: Vec::with_capacity(2),
            chunk: 0,
            index: 0,
            buf: Vec::new(),
        }
    }

    fn open(&mut self, data: &[u8], last: bool) -> Result<Vec<u8>, CryptoError> {
        if data.len() < GCM_OVERHEAD {
            return Err(CryptoError::TooShort);
        }
        let (nonce, ct) = data.split_at(12);
        let aad = media_aad(last, self.index);
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| CryptoError::BadMac)?;
        self.index += 1;
        Ok(pt)
    }

    /// Feeds encrypted bytes; returns the plaintext that can be released.
    pub fn update(&mut self, mut data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        while self.header.len() < 2 && !data.is_empty() {
            self.header.push(data[0]);
            data = &data[1..];
            if self.header.len() == 2 {
                let s = self.header[1];
                if self.header[0] != 0 || s > MEDIA_MAX_CHUNK_EXP || (1usize << s) <= GCM_OVERHEAD {
                    return Err(CryptoError::BadHeader);
                }
                self.chunk = 1 << s;
            }
        }
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        let mut start = 0;
        while self.chunk > 0 && self.buf.len() - start > self.chunk {
            let piece = self.buf[start..start + self.chunk].to_vec();
            out.extend(self.open(&piece, false)?);
            start += self.chunk;
        }
        self.buf.drain(..start);
        Ok(out)
    }

    /// Ends the stream: decrypts the last chunk.
    pub fn finish(mut self) -> Result<Vec<u8>, CryptoError> {
        if self.header.len() < 2 {
            return Err(CryptoError::TooShort);
        }
        if self.buf.is_empty() {
            return Ok(Vec::new());
        }
        let rest = std::mem::take(&mut self.buf);
        self.open(&rest, true)
    }
}

/// A JSON Web Key holding a P-256 private key (`kty: EC`, `crv: P-256`).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Jwk {
    /// Always `EC`.
    pub kty: String,
    /// Always `P-256`.
    pub crv: String,
    /// Private scalar, base64url without padding.
    pub d: String,
    /// Public x coordinate, base64url without padding.
    pub x: String,
    /// Public y coordinate, base64url without padding.
    pub y: String,
}

impl std::fmt::Debug for Jwk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jwk")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish_non_exhaustive()
    }
}

fn left_pad32(b: &[u8]) -> Result<[u8; 32], CryptoError> {
    if b.len() > 32 {
        return Err(CryptoError::BadKey("component longer than 32 bytes".into()));
    }
    let mut out = [0u8; 32];
    out[32 - b.len()..].copy_from_slice(b);
    Ok(out)
}

/// The client's ECDSA P-256 refresh key pair, used to sign token refreshes.
///
/// Serialized (serde) as a [`Jwk`].
#[derive(Clone)]
pub struct RefreshKey(SigningKey);

impl std::fmt::Debug for RefreshKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RefreshKey(<redacted>)")
    }
}

impl PartialEq for RefreshKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bytes() == other.0.to_bytes()
    }
}

impl RefreshKey {
    /// Generates a new key pair.
    pub fn generate() -> Self {
        RefreshKey(SigningKey::random(&mut OsRng))
    }

    /// Loads from the private scalar (big-endian, left-padded to 32 bytes if shorter).
    pub fn from_scalar_bytes(d: &[u8]) -> Result<Self, CryptoError> {
        let d = left_pad32(d)?;
        SigningKey::from_bytes(&d.into())
            .map(RefreshKey)
            .map_err(|e| CryptoError::BadKey(e.to_string()))
    }

    /// The private scalar, 32 bytes big-endian.
    pub fn scalar_bytes(&self) -> [u8; 32] {
        self.0.to_bytes().into()
    }

    /// Loads from a JWK (`d` is what matters; `x`/`y` are checked when present).
    pub fn from_jwk(jwk: &Jwk) -> Result<Self, CryptoError> {
        if jwk.kty != "EC" || jwk.crv != "P-256" {
            return Err(CryptoError::BadKey("not an EC P-256 JWK".into()));
        }
        let d = crate::codec::b64_decode_lenient(&jwk.d)
            .map_err(|e| CryptoError::BadKey(e.to_string()))?;
        let key = Self::from_scalar_bytes(&d)?;
        if !jwk.x.is_empty() {
            let x = crate::codec::b64_decode_lenient(&jwk.x)
                .map_err(|e| CryptoError::BadKey(e.to_string()))?;
            if left_pad32(&x)? != key.to_jwk_parts().1 {
                return Err(CryptoError::BadKey("JWK x does not match d".into()));
            }
        }
        Ok(key)
    }

    fn to_jwk_parts(&self) -> ([u8; 32], [u8; 32], [u8; 32]) {
        let p = self.0.verifying_key().to_encoded_point(false);
        let x: [u8; 32] = p
            .x()
            .expect("uncompressed")
            .as_slice()
            .try_into()
            .expect("32");
        let y: [u8; 32] = p
            .y()
            .expect("uncompressed")
            .as_slice()
            .try_into()
            .expect("32");
        (self.scalar_bytes(), x, y)
    }

    /// Exports as a JWK.
    pub fn to_jwk(&self) -> Jwk {
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let (d, x, y) = self.to_jwk_parts();
        Jwk {
            kty: "EC".into(),
            crv: "P-256".into(),
            d: e.encode(d),
            x: e.encode(x),
            y: e.encode(y),
        }
    }

    /// Public key as DER `SubjectPublicKeyInfo` (91 bytes).
    pub fn public_key_der(&self) -> Vec<u8> {
        self.0
            .verifying_key()
            .to_public_key_der()
            .expect("P-256 SPKI encoding")
            .into_vec()
    }

    /// ECDSA P-256 / SHA-256 signature of `msg`, DER-encoded.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let sig: Signature = self.0.sign(msg);
        sig.to_der().as_bytes().to_vec()
    }

    #[cfg(test)]
    pub(crate) fn verifying_key(&self) -> p256::ecdsa::VerifyingKey {
        *self.0.verifying_key()
    }
}

impl Serialize for RefreshKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_jwk().serialize(s)
    }
}

impl<'de> Deserialize<'de> for RefreshKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let jwk = Jwk::deserialize(d)?;
        RefreshKey::from_jwk(&jwk).map_err(serde::de::Error::custom)
    }
}

/// The ASCII string signed for a token refresh: `"<request_id>:<timestamp>"`.
pub fn refresh_signature_input(request_id: &str, timestamp: i64) -> String {
    format!("{request_id}:{timestamp}")
}

/// Value of the `Authorization` header for Google account sessions:
/// `SAPISIDHASH <t>_<sha1hex("<t> <SAPISID> https://messages.google.com")>`.
pub fn sapisid_hash(unix_secs: u64, sapisid: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{unix_secs} {sapisid} https://messages.google.com").as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("SAPISIDHASH {unix_secs}_{hex}")
}

/// HKDF-SHA256 with a 32-byte output: `HKDF(ikm, salt, info)`.
pub fn hkdf32(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = hkdf::Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out)
        .expect("32 bytes is a valid length");
    out
}

/// Java's `Arrays.hashCode(byte[])`: signed bytes, wrapping 32-bit arithmetic.
pub fn java_hash(bytes: &[u8]) -> i32 {
    bytes.iter().fold(1i32, |h, &b| {
        h.wrapping_mul(31).wrapping_add(b as i8 as i32)
    })
}

/// SHA-256 digest.
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trip_and_tamper() {
        let k = RequestKeys::generate();
        for len in [0usize, 1, 15, 16, 17, 1000] {
            let p: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let c = k.encrypt(&p);
            assert_eq!(c.len(), len + 48);
            assert_eq!(k.decrypt(&c).unwrap(), p);
            for i in [0, c.len() / 2, c.len() - 1] {
                let mut t = c.clone();
                t[i] ^= 1;
                assert_eq!(k.decrypt(&t), Err(CryptoError::BadMac));
            }
        }
        assert_eq!(k.decrypt(&[0; 47]), Err(CryptoError::TooShort));
        let other = RequestKeys::generate();
        assert_eq!(other.decrypt(&k.encrypt(b"x")), Err(CryptoError::BadMac));
    }

    #[test]
    fn payload_layout() {
        // C ‖ iv ‖ HMAC(C ‖ iv), with CTR-128-BE: the counter carries across bytes.
        let k = RequestKeys {
            aes: [7; 32],
            hmac: [9; 32],
        };
        let iv = [0xff; 16];
        let p = [0u8; 40];
        let c = k.encrypt_with_iv(&p, iv);
        assert_eq!(&c[40..56], &iv);
        // Keystream: block at counter iv, then iv+1 wrapping to zero.
        use aes::cipher::{BlockEncrypt, KeyInit as _};
        let aes = Aes256::new(&[7u8; 32].into());
        let mut b0 = aes::Block::from(iv);
        aes.encrypt_block(&mut b0);
        let mut b1 = aes::Block::from([0u8; 16]);
        aes.encrypt_block(&mut b1);
        assert_eq!(&c[..16], b0.as_slice());
        assert_eq!(&c[16..32], b1.as_slice());
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&[9; 32]).unwrap();
        mac.update(&c[..56]);
        assert_eq!(&c[56..], mac.finalize().into_bytes().as_slice());
    }

    #[test]
    fn media_format() {
        let key = [3u8; 32];
        // empty plaintext: only the header
        assert_eq!(encrypt_media(&key, &[]), vec![0, 15]);
        assert_eq!(decrypt_media(&key, &[0, 15]).unwrap(), Vec::<u8>::new());

        let piece: usize = 32768 - 28;
        for len in [1, piece - 1, piece, piece + 1, 2 * piece, 3 * piece + 5] {
            let p: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            let c = encrypt_media(&key, &p);
            let n = len.div_ceil(piece);
            assert_eq!(c.len(), 2 + len + 28 * n, "len {len}");
            assert_eq!(&c[..2], &[0, 15]);
            // every chunk but the last is exactly 2^15
            for i in 0..n {
                let start = 2 + i * 32768;
                let end = (start + 32768).min(c.len());
                let chunk = &c[start..end];
                if i + 1 < n {
                    assert_eq!(chunk.len(), 32768);
                }
                // AAD check: decrypt chunk manually
                let cipher = Aes256Gcm::new(&key.into());
                let aad = media_aad(i + 1 == n, i as u32);
                let pt = cipher
                    .decrypt(
                        Nonce::from_slice(&chunk[..12]),
                        Payload {
                            msg: &chunk[12..],
                            aad: &aad,
                        },
                    )
                    .unwrap();
                assert_eq!(pt, &p[i * piece..(i * piece + pt.len())]);
                // wrong last flag fails
                let bad = media_aad(i + 1 != n, i as u32);
                assert!(
                    cipher
                        .decrypt(
                            Nonce::from_slice(&chunk[..12]),
                            Payload {
                                msg: &chunk[12..],
                                aad: &bad
                            }
                        )
                        .is_err()
                );
            }
            assert_eq!(decrypt_media(&key, &c).unwrap(), p);
        }
    }

    #[test]
    fn media_aad_bytes() {
        assert_eq!(media_aad(true, 0x01020304), [1, 1, 2, 3, 4]);
        assert_eq!(media_aad(false, 0), [0, 0, 0, 0, 0]);
    }

    #[test]
    fn media_streaming_any_split() {
        let key = [5u8; 32];
        let p: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let c = encrypt_media_with_exp(&key, &p, 8); // 256-byte chunks
        for step in [1usize, 2, 3, 7, 100, 256, 257, 4096] {
            let mut d = MediaDecryptor::new(&key);
            let mut out = Vec::new();
            for piece in c.chunks(step) {
                out.extend(d.update(piece).unwrap());
            }
            out.extend(d.finish().unwrap());
            assert_eq!(out, p, "step {step}");
        }
        // exact multiple: last chunk is a full one
        let p2 = vec![1u8; (256 - 28) * 3];
        let c2 = encrypt_media_with_exp(&key, &p2, 8);
        assert_eq!(c2.len(), 2 + 256 * 3);
        assert_eq!(decrypt_media(&key, &c2).unwrap(), p2);
    }

    #[test]
    fn media_tamper_and_truncation() {
        let key = [5u8; 32];
        let p = vec![9u8; 600];
        let c = encrypt_media_with_exp(&key, &p, 8);
        let mut t = c.clone();
        t[100] ^= 1;
        assert!(decrypt_media(&key, &t).is_err());
        // dropping the last chunk: the previous one was not flagged last
        let n_full = 2 + 256 * 2;
        assert!(decrypt_media(&key, &c[..n_full]).is_err());
        // swapping chunks breaks the index in the AAD
        let mut sw = c[..2].to_vec();
        sw.extend_from_slice(&c[2 + 256..2 + 512]);
        sw.extend_from_slice(&c[2..2 + 256]);
        sw.extend_from_slice(&c[2 + 512..]);
        assert!(decrypt_media(&key, &sw).is_err());
        // header checks
        assert_eq!(
            decrypt_media(&key, &[0, 23, 1]),
            Err(CryptoError::BadHeader)
        );
        assert_eq!(decrypt_media(&key, &[1, 15]), Err(CryptoError::BadHeader));
        assert_eq!(decrypt_media(&key, &[0]), Err(CryptoError::TooShort));
        // wrong key
        assert!(decrypt_media(&[6; 32], &c).is_err());
    }

    #[test]
    fn sapisid_hash_format() {
        let h = sapisid_hash(1700000000, "abc/DEF");
        let mut s = Sha1::new();
        s.update(b"1700000000 abc/DEF https://messages.google.com");
        let hex: String = s.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(h, format!("SAPISIDHASH 1700000000_{hex}"));
        assert!(h.starts_with("SAPISIDHASH 1700000000_"));
        assert_eq!(h.len(), "SAPISIDHASH 1700000000_".len() + 40);
        assert!(
            h[23..]
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn refresh_key_jwk_der_sign() {
        use p256::ecdsa::signature::Verifier;
        let k = RefreshKey::generate();
        let der = k.public_key_der();
        assert_eq!(der.len(), 91);
        let jwk = k.to_jwk();
        let k2 = RefreshKey::from_jwk(&jwk).unwrap();
        assert_eq!(k, k2);
        let json = serde_json::to_string(&k).unwrap();
        let k3: RefreshKey = serde_json::from_str(&json).unwrap();
        assert_eq!(k, k3);
        let input = refresh_signature_input(
            "3f0b9a0e-0000-4000-8000-000000000001",
            1_700_000_000_123_000,
        );
        assert_eq!(
            input,
            "3f0b9a0e-0000-4000-8000-000000000001:1700000000123000"
        );
        let sig = k.sign(input.as_bytes());
        assert_eq!(sig[0], 0x30, "DER SEQUENCE");
        let parsed = Signature::from_der(&sig).unwrap();
        k.verifying_key().verify(input.as_bytes(), &parsed).unwrap();
        // left padding of a short d
        let mut d = k.scalar_bytes();
        d[0] = 0;
        if let Ok(short) = RefreshKey::from_scalar_bytes(&d[1..]) {
            assert_eq!(short.scalar_bytes(), d);
        }
    }

    #[test]
    fn java_hash_matches_java() {
        assert_eq!(java_hash(&[]), 1);
        assert_eq!(java_hash(&[1]), 32);
        // signed bytes: 0xff is -1
        assert_eq!(java_hash(&[0xff]), 31 - 1);
        assert_eq!(java_hash(&[0x80]), 31 - 128);
        // wrapping: Arrays.hashCode(new byte[]{1,2,3,4,5,6,7,8}) in Java
        let mut h: i32 = 1;
        for b in 1..=8i32 {
            h = h.wrapping_mul(31).wrapping_add(b);
        }
        assert_eq!(java_hash(&[1, 2, 3, 4, 5, 6, 7, 8]), h);
        // 32 bytes of 0xff overflow and stay consistent with i64-mod computation
        let v = [0xffu8; 32];
        let mut r: i64 = 1;
        for _ in 0..32 {
            r = (r * 31 - 1).rem_euclid(1 << 32);
        }
        assert_eq!(java_hash(&v), r as u32 as i32);
    }

    #[test]
    fn hkdf_rfc5869_case1() {
        // RFC 5869 test case 1, first 32 bytes of OKM
        let ikm = [0x0b; 22];
        let salt: Vec<u8> = (0..=0x0c).collect();
        let info: Vec<u8> = (0xf0..=0xf9).collect();
        let okm = hkdf32(&ikm, &salt, &info);
        let hex: String = okm.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        );
    }
}
