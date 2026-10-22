# 4. Pairing

Two ways to pair, both ending with the same state ([3](3-session.md#what-a-client-keeps)):

| | QR code | Google account |
|---|---|---|
| user action | scan a QR code with the phone | sign in to Google in a browser, then tap the emoji shown, on the phone |
| request keys | random, sent to the phone in the QR code | derived from a UKEY2 key exchange |
| relay host | `API` | `ACCOUNT`, with cookies |
| availability | being phased out by Google on some phones | current default of the web client |

Before either: generate the refresh key pair ([2](2-crypto.md#refresh-key-ecdsa-p-256)) and
fetch the configuration ([3](3-session.md#configuration)).

## QR code pairing

### 1. Register

`POST API …Pairing/RegisterPhoneRelay`, **binary**:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | fresh `request_id`, `network` = `Bugle`, `version`; no token |
| 3 | `BrowserDetails` | `browser` | [3](3-session.md#browser-details) |
| 4 | `KeyRegistration` | `keys` | |

`KeyRegistration`: 6 `PublicKeyEntry`. `PublicKeyEntry`: 1 `i64` = `2`; 2 `bytes` the refresh
public key as DER `SubjectPublicKeyInfo`.

(The message holding `credentials`, `browser` and `keys` has an alternative to field 4: field
5 `{1: Device}`, the browser device; [unverified], not needed.)

Reply (binary):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `{2: i64}` | | [unknown] |
| 2 | `Device` | `browser` | |
| 3 | `bytes` | `pairing_key` | goes into the QR code |
| 4 | `i64` | `valid_for` | |
| 5 | `TokenData` | `token` | a provisional relay token: store it ([3](3-session.md#token-refresh)) |
| 6 | `str` | `response_id` | |

### 2. Listen

Open the event stream ([5](5-relay.md#the-event-stream)) with the provisional token, without
pinging the phone (no phone is paired yet).

### 3. Show the QR code

```
https://support.google.com/messages/?p=web_computer#?c=<base64>
```

where `<base64>` is standard base64 (with padding) of the binary protobuf:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `bytes` | `pairing_key` | from the reply above |
| 2 | `bytes` | `aes_key` | `K_aes`, freshly generated (32 random bytes) |
| 3 | `bytes` | `hmac_key` | `K_mac`, freshly generated (32 random bytes) |

Render the whole URL as a QR code.

### 4. Renew the code

[client policy] The code is renewed every 30 seconds until scanned (a working client gives up
after 6 codes). `POST API …Pairing/RefreshPhoneRelay`, **binary**:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | fresh `request_id`, `network` = `Bugle`, current `token`, `version` |

Reply: 1 `{2: i64}` [unknown]; 2 `bytes` new `pairing_key`; 3 `i64` `valid_for`. Show a new
QR code with the new pairing key and the **same** request keys.

### 5. Paired

When the phone has scanned the code, the stream delivers an item on the pairing route
(route 14, [5](5-relay.md#incoming-envelope)) whose payload is a `PairingEvent`:

`PairingEvent` (one of):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 4 | `Paired` | `paired` | pairing done |
| 5 | `{1: Device}` | `revoked` | this browser was unpaired (also arrives later, when the user removes the device on the phone) |

`Paired`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Device` | `phone` | |
| 2 | `TokenData` | `token` | the definitive relay token |
| 3 | `Device` | `browser` | |

Store both devices and the token. Then wait about **2 seconds** before reconnecting (close the
stream, open it again as a paired client): reconnecting sooner, before the phone has saved
the pairing, gets the new pairing dropped.

## Google account pairing

Needs the cookies of a Google account signed in on `messages.google.com`, the same account as
on the phone (with "device pairing" allowed in Google Messages). The cookies needed:

| Cookie | Domain | |
|---|---|---|
| `OSID` | `messages.google.com` | required |
| `SID`, `HSID`, `SSID`, `APISID`, `SAPISID` | `.google.com` | required |
| `__Secure-1PSIDTS` | `.google.com` | optional, helps |

Keep every cookie received; send them all ([Transport › cookies](1-transport.md#google-account-sessions-cookies-and-authorization)).
Fetch the configuration with them first: it gives the web device id.

### 1. Sign in

`POST ACCOUNT …Registration/SignInGaia`, pblite:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | fresh `request_id`, `network` = `GDitto`, `version`; no token |
| 2 | `SignInBody` | `body` | |
| 4 | `str` | `network` | `GDitto` |

`SignInBody`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `WebDeviceRef` | `device` | |
| 36 | `b64({3: bytes})` | `key` | field 3: the refresh public key, DER `SubjectPublicKeyInfo` |

`WebDeviceRef`: 1 `i32` = `3`; 2 `str` = `messages-web-` followed by the 16 bytes of the web
device id in lowercase hex (32 characters, no dashes).

(Field 3 of the request, an `i32` set to 1 without `key`, is a lighter variant: [unverified],
not needed.)

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `{2: u64, 4: i64}` | | [unknown]; field 4 a timestamp |
| 2 | `b64(str)` | | [unknown], perhaps a browser UUID |
| 3 | `SignInDevices` | `devices` | |
| 4 | `TokenData` | `token` | the relay token: store it |

`SignInDevices`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `{1: Device}` | `self` | this browser's device |
| 2 | `[Registration]` | `registrations` | devices of the account |
| 3 | `[RegistrationSeen]` | `seen` | more data on them |

`Registration`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `b64(str)` | `id` | a UUID |
| 4 | `i32` | `role` | `1` for a phone (pairing destination); `6` seems to be a browser |
| 5 | `str` | `language` | language code |
| 7 | `u64` | | [unknown] |

`RegistrationSeen`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `b64(str)` | `id` | same UUID as a `Registration` |
| 3 | `i32` | | `1` phone, `6` browser, as `role` |
| 4 | `i32` | | [unknown] |
| 7 | `i64` | `seen` | µs; treated as last activity |
| 8 | `b64({1: i32, 2: u64, 3: bytes, 4: i32})` | | [unknown]; field 2 a timestamp for phones |

Then:

- **browser device** ← `devices.self.1`;
- **phone device** ← the same `Device` with `source_id` converted to **lowercase**;
- **destination**: among `registrations` with `role` = 1, the one whose matching
  `RegistrationSeen.seen` is the most recent. Several phones are possible (a working client
  lets the user try the next one if the chosen phone does not answer). None: the account
  has no phone usable for pairing. Store its UUID as the **destination id**: from now on the
  session is a Google account session.

### 2. Listen

Open the event stream ([5](5-relay.md#the-event-stream)), without pinging, and wait until it is
open: the handshake replies come on it.

### 3. Handshake

UKEY2 ([google/ukey2](https://github.com/google/ukey2)), cipher P-256 with SHA-512
commitments. The UKEY2 messages are binary protobuf, as defined by UKEY2:

- `Ukey2Message`: 1 `enum` type (2 client init, 3 server init, 4 client finish, 1 alert);
  2 `bytes` data.
- `ClientInit`: 1 `i32` version; 2 `bytes` random; 3 repeated `{1: enum cipher, 2: bytes commitment}`;
  4 `str` next protocol.
- `ServerInit`: 1 `i32` version; 2 `bytes` random; 3 `enum` cipher; 4 `PublicKey`.
- `ClientFinished`: 1 `PublicKey`.
- `PublicKey`: 1 `enum` type (1 = EC P-256); 2 `{1: bytes x, 2: bytes y}`.
- cipher P-256/SHA-512 = `100`.

Steps:

1. Generate an ephemeral ECDH P-256 key pair; a **pairing attempt id** (random UUID); note the
   start time.
2. `finish` ← `Ukey2Message{1: 4, 2: ClientFinished{1: PublicKey{1: 1, 2: {1: 0x00 ‖ X, 2: 0x00 ‖ Y}}}}`,
   `X`, `Y` the 32-byte big-endian coordinates of the ephemeral public key, each prefixed with a
   zero byte.
3. `init` ← `Ukey2Message{1: 2, 2: ClientInit{1: 1, 2: 32 random bytes, 3: [{1: 100, 2: SHA-512(finish)}], 4: "AES_256_CBC-HMAC_SHA256"}}`
   (`finish` and `init` are the serialized bytes; keep both).
4. Call the phone ([5](5-relay.md#calling-the-phone)) with action **44** (pairing init), payload
   **not encrypted**, message class **20**, TTL 300 s (300 000 000 µs), fresh request id,
   payload a `PairingRequest`:

   | # | Type | Name | Meaning |
   |---|------|------|---------|
   | 1 | `str` | `attempt_id` | the pairing attempt id |
   | 2 | `BrowserDetails` | `browser` | |
   | 3 | `i64` | `started` | start time, **milliseconds** |
   | 4 | `bytes` | `data` | `init` |
   | 5 | `i32` | `emoji_version` | proposed: `1` |
   | 6 | `i32` | `key_version` | proposed: `1` |
   | 8 | `str` | `statement` | finish only, see step 7 |

5. The reply (within 20 s; else cancel, below) has its payload in the clear: a `PairingReply`:

   | # | Type | Name | Meaning |
   |---|------|------|---------|
   | 1 | `i32` | `error_kind` | non-zero: failed |
   | 2 | `enum PairingError` | `error` | |
   | 3 | `i32` | | `1` for init [unknown] |
   | 4 | `str` | `session` | a UUID |
   | 5 | `bytes` | `data` | `server`: a `Ukey2Message` holding the `ServerInit` |
   | 6 | `i32` | `emoji_version` | confirmed: 0 or 1 |
   | 7 | `i32` | `key_version` | confirmed: 0 or 1 |
   | 8 | `bytes` | | [unknown] |

   Check: message type 3; `ServerInit` version 1, cipher 100, random of 32 bytes. The server's
   `x` and `y` may be 33 bytes with a leading zero byte: strip it; then both must be 32 bytes.
6. Derive:
   - `z` ← the ECDH shared secret (the 32-byte x-coordinate of the shared point) between the
     ephemeral private key and the server's public key;
   - `s` ← SHA-256(`z`);
   - `ctx` ← `init ‖ server` (the two serialized `Ukey2Message`s);
   - `auth` ← HKDF(`s`, `"UKEY2 v1 auth"`, `ctx`); `next` ← HKDF(`s`, `"UKEY2 v1 next"`, `ctx`)
     (salts as ASCII);
   - `n` ← the first 4 bytes of `auth` as a big-endian **unsigned** 32-bit integer;
   - the emoji to show ← `list[n mod len(list)]`, `list` being the emoji list of the confirmed
     `emoji_version` ([appendix](9-emoji.md)).

   Show the emoji; the user taps the same one on the phone.
7. Call the phone with action **45** (pairing finish), payload not encrypted, message class
   **2**, TTL 300 s, a `PairingRequest` with fields 1–3 as before, `data` = `finish`, fields 5
   and 6 absent, and field 8 set to exactly:

   ```
   This is an undocumented API. Use or access of undocumented Google APIs without express authorization is prohibited per the Google API Terms of Service (https://developers.google.com/terms).
   ```

   The reply, again a `PairingReply`, comes once the user has answered on the phone (no fixed
   timeout). `error_kind` ≠ 0 means failure, see `PairingError` below.
8. Derive the request keys. Let `C` be these 32 bytes (decimal):

   ```
   130 170  85 160 211 151 248 131  70 202  28 238 141  57   9 185
    95  19 250 125 235  29  74 179 131 118 184  37 109 168  85  16
   ```

   - `kc` ← HKDF(`next`, `C`, `"client"`); `ks` ← HKDF(`next`, `C`, `"server"`) (`C` as salt).
   - Confirmed `key_version` 0: `K_aes` ← `kc`, `K_mac` ← `ks`.
   - Confirmed `key_version` 1:
     - `h(b)`: start at 1; for each byte, `h ← 31·h + b` with `b` taken as **signed** (−128…127),
       wrapping 32-bit signed arithmetic (Java's `Arrays.hashCode(byte[])`);
     - `m` ← `C ‖ kc ‖ ks` if `h(kc) < h(ks)` (signed comparison), else `C ‖ ks ‖ kc`;
     - `d` ← SHA-256(`m`);
     - `K_aes` ← HKDF(`d`, `"Ditto salt 1"`, `"Ditto info 1"`);
       `K_mac` ← HKDF(`d`, `"Ditto salt 2"`, `"Ditto info 2"`).
9. Store the request keys and the pairing attempt id; reconnect (close the stream, open it
   again as a paired client).

### Cancelling

On failure or user cancel after the init was sent, call the phone with action **47**, request
id = the **pairing attempt id**, no payload, not encrypted, message class 20, TTL 300 s; no
reply is expected.

### `PairingError`

| Value | Meaning |
|---|---|
| 0 | none |
| 1 | UKEY2 alert |
| 2 | request too old |
| 3 | request not received quickly enough |
| 4 | invalid user |
| 5 | **wrong emoji chosen** on the phone |
| 6 | verification timed out |
| 7 | **user cancelled** on the phone |
| 8 | other UKEY2 error |
| 9 | emoji download failed on the phone |
| 10 | not the latest attempt |
| 11 | phone verification service missing |
| 12 | phone verification screens missing |
| 13 | wrong UKEY2 message type |
| 14 | completed with an unknown result |
| 15 | phone not waiting for a confirmation |
| 16 | UKEY2 message missing |
| 17 | client init timed out |
| 18 | client finish timed out |
| 19 | unknown |
| 20 | cancelled from the web (cancel button) |
| 21 | cancelled from the web (back button) |
| 22 | cancelled from the web (page left) |
| 23 | new request while waiting for verification |
| 24 | UKEY2 handshake error |
| 25 | emoji version mismatch |
| 26 | cancelled on the web during verification |
| 27 | **user answered "this is not me"** on the phone |
| 28 | key derivation version mismatch |
| 29 | backup key not found |
| 30 | cannot encrypt backup key |
| 31 | cannot decrypt backup key |
| 32 | client attestation missing |
| 33 | client attestation mismatch |
| 34 | reply from an unexpected phone registration |
| 35 | client attestation version mismatch |

Values 2, 3 and 6 are best reported as "timed out", 7 and 27 as "cancelled".

## Unpairing

- **Google account session**: call the phone with action **46**, encrypted payload
  `{1: str attempt id}` (the stored pairing attempt id); no reply is expected.
- **QR session**: `POST API …Pairing/RevokeRelayPairing`, binary:
  1 `Credentials` (fresh `request_id`, `token`, `version`; no network), 2 `Device` browser.
  Reply: an empty message (field 1 `{2: i64}`, [unknown]).

Unpairing from the phone (removing the device there) is announced on the stream as a
`PairingEvent.revoked` (QR sessions), and fails later requests with the "entity not found" or
"invalid credentials" errors.

### Logged out (Google account session)

A data item for action 16 ([5](5-relay.md#pushed-updates)) with **no** encrypted payload and a
clear payload of exactly the two bytes `0x72 0x00` means the account session was logged out:
acknowledge it and pair again.
