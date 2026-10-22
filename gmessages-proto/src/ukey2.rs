//! UKEY2 handshake (P-256, SHA-512 commitments) and the Messages-specific derivation of
//! the request keys (spec §4 "Handshake").

use p256::elliptic_curve::sec1::{EncodedPoint, FromEncodedPoint, ToEncodedPoint};
use p256::{NistP256, PublicKey, SecretKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256, Sha512};

use crate::codec;
use crate::crypto::{RequestKeys, hkdf32, java_hash, random_bytes};
use crate::error::PairingError;
use crate::wire;

/// UKEY2 cipher P-256 with SHA-512 commitments.
pub const CIPHER_P256_SHA512: i32 = 100;
/// UKEY2 message types.
pub const MSG_CLIENT_INIT: i32 = 2;
/// Server init.
pub const MSG_SERVER_INIT: i32 = 3;
/// Client finish.
pub const MSG_CLIENT_FINISH: i32 = 4;
const NEXT_PROTOCOL: &str = "AES_256_CBC-HMAC_SHA256";

/// The fixed salt `C` of the request key derivation.
pub const KEY_DERIVATION_SALT: [u8; 32] = [
    130, 170, 85, 160, 211, 151, 248, 131, 70, 202, 28, 238, 141, 57, 9, 185, 95, 19, 250, 125,
    235, 29, 74, 179, 131, 118, 184, 37, 109, 168, 85, 16,
];

/// The client side of a UKEY2 handshake.
pub(crate) struct Ukey2Client {
    secret: SecretKey,
    /// Serialized `Ukey2Message` holding the `ClientInit`.
    pub init: Vec<u8>,
    /// Serialized `Ukey2Message` holding the `ClientFinished`.
    pub finish: Vec<u8>,
}

/// Keys derived from the handshake.
#[derive(Debug, Clone)]
pub(crate) struct HandshakeKeys {
    pub auth: [u8; 32],
    pub next: [u8; 32],
}

fn ec_point(pk: &PublicKey) -> wire::Ukey2EcPoint {
    let p = pk.to_encoded_point(false);
    let mut x = vec![0u8];
    x.extend_from_slice(p.x().expect("uncompressed"));
    let mut y = vec![0u8];
    y.extend_from_slice(p.y().expect("uncompressed"));
    wire::Ukey2EcPoint { x, y }
}

fn strip(v: &[u8]) -> Result<[u8; 32], PairingError> {
    let v = if v.len() == 33 && v[0] == 0 {
        &v[1..]
    } else {
        v
    };
    v.try_into()
        .map_err(|_| PairingError::Handshake(format!("bad coordinate length {}", v.len())))
}

impl Ukey2Client {
    /// Generates an ephemeral key and the `init` and `finish` messages.
    pub fn new() -> Self {
        Self::with_secret(SecretKey::random(&mut OsRng), random_bytes::<32>().to_vec())
    }

    pub(crate) fn with_secret(secret: SecretKey, random: Vec<u8>) -> Self {
        let finished = wire::Ukey2ClientFinished {
            public_key: Some(wire::Ukey2PublicKey {
                kind: 1,
                ec: Some(ec_point(&secret.public_key())),
            }),
        };
        let finish = codec::to_binary(&wire::Ukey2Message {
            kind: MSG_CLIENT_FINISH,
            data: codec::to_binary(&finished),
        });
        let commitment = Sha512::digest(&finish).to_vec();
        let client_init = wire::Ukey2ClientInit {
            version: 1,
            random,
            commitments: vec![wire::Ukey2Commitment {
                cipher: CIPHER_P256_SHA512,
                commitment,
            }],
            next_protocol: NEXT_PROTOCOL.into(),
        };
        let init = codec::to_binary(&wire::Ukey2Message {
            kind: MSG_CLIENT_INIT,
            data: codec::to_binary(&client_init),
        });
        Ukey2Client {
            secret,
            init,
            finish,
        }
    }

    /// Checks the server's `Ukey2Message` (server init) and derives `auth` and `next`.
    pub fn handle_server_init(&self, server: &[u8]) -> Result<HandshakeKeys, PairingError> {
        let bad = |m: &str| PairingError::Handshake(m.to_string());
        let msg: wire::Ukey2Message =
            codec::from_binary(server).map_err(|e| bad(&format!("server message: {e}")))?;
        if msg.kind != MSG_SERVER_INIT {
            return Err(bad(&format!("unexpected UKEY2 message type {}", msg.kind)));
        }
        let si: wire::Ukey2ServerInit =
            codec::from_binary(&msg.data).map_err(|e| bad(&format!("server init: {e}")))?;
        if si.version != 1 {
            return Err(bad("server init version is not 1"));
        }
        if si.cipher != CIPHER_P256_SHA512 {
            return Err(bad("server chose another cipher"));
        }
        if si.random.len() != 32 {
            return Err(bad("server random is not 32 bytes"));
        }
        let ec = si
            .public_key
            .and_then(|p| p.ec)
            .ok_or_else(|| bad("server public key missing"))?;
        let x = strip(&ec.x)?;
        let y = strip(&ec.y)?;
        let point = EncodedPoint::<NistP256>::from_affine_coordinates(&x.into(), &y.into(), false);
        let server_pk = Option::<PublicKey>::from(PublicKey::from_encoded_point(&point))
            .ok_or_else(|| bad("server public key not on the curve"))?;
        let shared =
            p256::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), server_pk.as_affine());
        let z = shared.raw_secret_bytes();
        Ok(derive_handshake(z.as_slice(), &self.init, server))
    }
}

/// `s = SHA-256(z)`, `ctx = init ‖ server`,
/// `auth = HKDF(s, "UKEY2 v1 auth", ctx)`, `next = HKDF(s, "UKEY2 v1 next", ctx)`.
pub(crate) fn derive_handshake(z: &[u8], init: &[u8], server: &[u8]) -> HandshakeKeys {
    let s = Sha256::digest(z);
    let mut ctx = init.to_vec();
    ctx.extend_from_slice(server);
    HandshakeKeys {
        auth: hkdf32(&s, b"UKEY2 v1 auth", &ctx),
        next: hkdf32(&s, b"UKEY2 v1 next", &ctx),
    }
}

/// The emoji index value: the first 4 bytes of `auth`, big-endian unsigned.
pub fn emoji_index(auth: &[u8; 32]) -> u32 {
    u32::from_be_bytes(auth[..4].try_into().expect("4 bytes"))
}

/// Derives the request keys from `next` for a confirmed key version (0 or 1).
pub fn derive_request_keys(next: &[u8; 32], key_version: i32) -> RequestKeys {
    let c = &KEY_DERIVATION_SALT;
    let kc = hkdf32(next, c, b"client");
    let ks = hkdf32(next, c, b"server");
    if key_version == 0 {
        return RequestKeys { aes: kc, hmac: ks };
    }
    let mut m = c.to_vec();
    if java_hash(&kc) < java_hash(&ks) {
        m.extend_from_slice(&kc);
        m.extend_from_slice(&ks);
    } else {
        m.extend_from_slice(&ks);
        m.extend_from_slice(&kc);
    }
    let d = Sha256::digest(&m);
    RequestKeys {
        aes: hkdf32(&d, b"Ditto salt 1", b"Ditto info 1"),
        hmac: hkdf32(&d, b"Ditto salt 2", b"Ditto info 2"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_init(pk: &PublicKey, pad: bool) -> Vec<u8> {
        let mut ec = ec_point(pk);
        if !pad {
            ec.x.remove(0);
            ec.y.remove(0);
        }
        let si = wire::Ukey2ServerInit {
            version: 1,
            random: vec![7; 32],
            cipher: CIPHER_P256_SHA512,
            public_key: Some(wire::Ukey2PublicKey {
                kind: 1,
                ec: Some(ec),
            }),
        };
        codec::to_binary(&wire::Ukey2Message {
            kind: MSG_SERVER_INIT,
            data: codec::to_binary(&si),
        })
    }

    #[test]
    fn handshake_messages_and_shared_secret() {
        let c = Ukey2Client::new();
        // init structure
        let m: wire::Ukey2Message = codec::from_binary(&c.init).unwrap();
        assert_eq!(m.kind, 2);
        let ci: wire::Ukey2ClientInit = codec::from_binary(&m.data).unwrap();
        assert_eq!(ci.version, 1);
        assert_eq!(ci.random.len(), 32);
        assert_eq!(ci.commitments[0].cipher, 100);
        assert_eq!(
            ci.commitments[0].commitment,
            Sha512::digest(&c.finish).to_vec()
        );
        assert_eq!(ci.next_protocol, "AES_256_CBC-HMAC_SHA256");
        // finish: coordinates prefixed with a zero byte
        let f: wire::Ukey2Message = codec::from_binary(&c.finish).unwrap();
        assert_eq!(f.kind, 4);
        let cf: wire::Ukey2ClientFinished = codec::from_binary(&f.data).unwrap();
        let ec = cf.public_key.unwrap().ec.unwrap();
        assert_eq!((ec.x.len(), ec.x[0], ec.y.len(), ec.y[0]), (33, 0, 33, 0));

        // a "server" side
        let server_secret = SecretKey::random(&mut OsRng);
        for pad in [true, false] {
            let server = server_init(&server_secret.public_key(), pad);
            let keys = c.handle_server_init(&server).unwrap();
            let shared = p256::ecdh::diffie_hellman(
                server_secret.to_nonzero_scalar(),
                c.secret.public_key().as_affine(),
            );
            let expect = derive_handshake(shared.raw_secret_bytes().as_slice(), &c.init, &server);
            assert_eq!(keys.auth, expect.auth);
            assert_eq!(keys.next, expect.next);
        }
        // wrong type
        let bad = codec::to_binary(&wire::Ukey2Message {
            kind: 1,
            data: vec![],
        });
        assert!(c.handle_server_init(&bad).is_err());
    }

    #[test]
    fn derivation_pieces() {
        let auth = {
            let mut a = [0u8; 32];
            a[..4].copy_from_slice(&[0xff, 0xff, 0xff, 0xfe]);
            a
        };
        // unsigned: 4294967294, not -2
        assert_eq!(emoji_index(&auth), 4_294_967_294);
        assert_eq!(
            crate::emoji::emoji_at(emoji_index(&auth), 0),
            crate::emoji::emoji_list(0)[(4_294_967_294u64 % 305) as usize]
        );
        let next = [42u8; 32];
        let v0 = derive_request_keys(&next, 0);
        assert_eq!(v0.aes, hkdf32(&next, &KEY_DERIVATION_SALT, b"client"));
        assert_eq!(v0.hmac, hkdf32(&next, &KEY_DERIVATION_SALT, b"server"));
        let v1 = derive_request_keys(&next, 1);
        let (kc, ks) = (v0.aes, v0.hmac);
        let mut m = KEY_DERIVATION_SALT.to_vec();
        if java_hash(&kc) < java_hash(&ks) {
            m.extend(kc);
            m.extend(ks);
        } else {
            m.extend(ks);
            m.extend(kc);
        }
        let d = Sha256::digest(&m);
        assert_eq!(v1.aes, hkdf32(&d, b"Ditto salt 1", b"Ditto info 1"));
        assert_eq!(v1.hmac, hkdf32(&d, b"Ditto salt 2", b"Ditto info 2"));
        assert_ne!(v1.aes, v0.aes);
    }
}
