# 2. Cryptography

| Use | Algorithm | Keys |
|---|---|---|
| payloads exchanged with the phone | AES-256-CTR, then HMAC-SHA256 (encrypt-then-MAC) | the two **request keys**, shared with the phone at pairing |
| media | AES-256-GCM in chunks | a random key per file, sent inside the message |
| token refresh | ECDSA P-256 / SHA-256 | the client's **refresh key** pair, registered at pairing |
| Google account pairing | ECDH P-256 (UKEY2), HKDF-SHA256 | ephemeral |

## Request keys

Two 32-byte keys:

- `K_aes`: AES-256 key;
- `K_mac`: HMAC-SHA256 key.

How they come to exist depends on the pairing: QR pairing generates them at random on the
client and hands them to the phone inside the QR code; Google account pairing derives them
from a key exchange ([4](4-pairing.md)).

## Payload encryption (AES-CTR + HMAC)

Used for every encrypted payload in both directions ([5](5-relay.md)).

**Encrypt** plaintext `P`:

1. `iv` ← 16 random bytes;
2. `C` ← AES-256-CTR(`K_aes`, `iv`, `P`), the counter being the whole 16-byte block taken as a
   128-bit big-endian integer starting at `iv`, incremented per block (the usual "CTR-128-BE");
3. `T` ← HMAC-SHA256(`K_mac`, `C ‖ iv`);
4. output `C ‖ iv ‖ T` (`T` is 32 bytes).

**Decrypt** input `X`:

1. reject if `len(X) < 48`;
2. `T` ← last 32 bytes; `B` ← the rest; check `T == HMAC-SHA256(K_mac, B)` in constant time;
3. `iv` ← last 16 bytes of `B`; `C` ← the rest of `B`;
4. `P` ← AES-256-CTR(`K_aes`, `iv`, `C`).

## Media encryption (chunked AES-GCM)

A fresh 32-byte key `K` per file. The ciphertext is a 2-byte header followed by chunks:

```
0x00 ‖ s ‖ chunk_0 ‖ chunk_1 ‖ … ‖ chunk_n
```

- `s`: the chunk size as a power of two: every encrypted chunk is `2^s` bytes, except the
  last which may be shorter. Writers use `s = 15` (32 768 bytes). Readers should refuse
  `2^s > 2^22`.
- `chunk_i` = `nonce_i ‖ ciphertext_i ‖ tag_i`: a 12-byte random nonce, then AES-256-GCM of
  the plaintext piece with a 16-byte tag. Each chunk adds 28 bytes, so with `s = 15` a full
  chunk carries 32 740 bytes of plaintext.
- Additional authenticated data of chunk *i*: 5 bytes, `last ‖ BE32(i)`, where `last` is `0x01`
  for the final chunk and `0x00` otherwise, and `BE32(i)` the chunk index (from 0) as a
  big-endian 32-bit integer.

The plaintext is split into pieces of `2^s − 28` bytes; the last piece holds the rest. An
empty plaintext gives just the header.

**Decrypting a stream**: read the 2-byte header, then read `2^s` bytes at a time; the chunk
after which the stream ends (a short read) is the last one. One way to know it is the last:
read one byte ahead.

## Refresh key (ECDSA P-256)

A P-256 key pair generated once, before pairing, and kept for the lifetime of the session.

- **Registered** at pairing: its public key, DER-encoded as an X.509 `SubjectPublicKeyInfo`
  (91 bytes for P-256).
- **Signing** a token refresh ([3](3-session.md#token-refresh)): ECDSA over the SHA-256 digest
  of an ASCII string, the signature DER-encoded (ASN.1 `SEQUENCE { r, s }`).

A JWK (`kty: EC`, `crv: P-256`, base64url `d`, `x`, `y`) is a convenient way to store it;
left-pad `d`, `x`, `y` to 32 bytes when loading.

## HKDF

HKDF-SHA256 (RFC 5869), output length 32 bytes, written `HKDF(ikm, salt, info)`.

## Google account pairing key exchange

UKEY2 with NIST P-256 and SHA-512 commitments, then a Messages-specific derivation of the
request keys. Described with the flow in [Pairing › account](4-pairing.md#google-account-pairing).

## Implementation notes (Rust)

| Need | Crates |
|---|---|
| AES-256-CTR (128-bit big-endian counter) | `aes` + `ctr` (`Ctr128BE<Aes256>`) |
| HMAC-SHA256, SHA-1, SHA-256, SHA-512 | `hmac`, `sha1`, `sha2` |
| AES-256-GCM | `aes-gcm` |
| ECDSA, ECDH, SPKI DER | `p256` (features `ecdsa`, `ecdh`, `pkcs8`) |
| HKDF | `hkdf` |
| randomness | `rand` / `getrandom` |
