# 3. Session state

## What a client keeps

To resume without pairing again, a client persists:

| Item | Type | Origin |
|---|---|---|
| request keys `K_aes`, `K_mac` | 2 × 32 bytes | pairing |
| refresh key pair | ECDSA P-256 | generated before pairing |
| **browser device** | `Device` | pairing: this client's identity |
| **phone device** | `Device` | pairing: the phone's identity |
| relay token | bytes | pairing, then each refresh |
| token expiry, token TTL | time, µs | with the token |
| web device id | UUID | configuration ([below](#configuration)) |
| destination id | UUID | account pairing only: the phone's registration. Its presence marks a **Google account session** |
| pairing attempt id | UUID | account pairing only, needed to unpair |
| cookies | name → value | account pairing only |

A Google account session is one with a destination id; it uses the `ACCOUNT` host for the
relay and sends cookies. A QR session has none.

## Device

Identifies a party (the phone or a browser) to the relay.

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `i64` | `user_id` | |
| 2 | `str` | `source_id` | |
| 3 | `str` | `network` | |

Devices are opaque: store them as received and send them back unchanged (one exception in
account pairing, [4](4-pairing.md#1-sign-in)).

## Credentials

Carried by most requests to the servers (not inside payloads to the phone).

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `request_id` | a fresh random UUID (lowercase, with dashes) per request, unless stated |
| 3 | `str` | `network` | see below |
| 6 | `bytes` | `token` | the relay token (absent before pairing) |
| 7 | `ClientVersion` | `version` | |

`network`:

| Context | Value |
|---|---|
| QR pairing requests | `Bugle` |
| Google account pairing requests | `GDitto` |
| other requests, QR session | absent (empty) |
| other requests, Google account session | `GDitto` |

## Client version

The version of the web client being imitated.

| # | Type | Name | Meaning |
|---|------|------|---------|
| 3 | `i32` | `year` | e.g. 2026 |
| 4 | `i32` | `month` | 1–12 |
| 5 | `i32` | `day` | 1–31 |
| 7 | `i32` | `minor_a` | always `4` |
| 9 | `i32` | `minor_b` | always `6` |

The date is the build date of the web client. Configuration reports the current one
(`client_version` below, `YYYYMMDD…`); a client may follow it or keep a known working date
(2026-09-02 at the time of writing) and update it with releases.

## Browser details

Describes the client at pairing; the phone lists it among paired devices.

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `user_agent` | the `user-agent` header value |
| 2 | `enum BrowserKind` | `browser` | |
| 3 | `str` | `os` | **shown by the phone as the device's name** (e.g. `gmessages-tui`) |
| 6 | `enum DeviceKind` | `device` | |

`BrowserKind`: 0 unknown, 1 other, 2 Chrome, 3 Firefox, 4 Safari, 5 Opera, 6 Internet Explorer,
7 Edge. A working client sends 1.

`DeviceKind`: 0 unknown, 1 web, 2 tablet, 3 installed web app. A working client sends 2.

## Configuration

`GET https://messages.google.com/web/config` (headers in [Transport](1-transport.md#headers);
cookies if any). Fetched before pairing, and before re-authenticating with new cookies. The
response is pblite:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `client_version` | starts with the build date `YYYYMMDD` |
| 2 | `str` | `server_version` | |
| 3 | `[Flag]` | `flags` | feature flags |
| 4 | `FlagList` | `more_flags` | |
| 5 | `WebDevice` | `device` | |
| 7 | `str` | `country` | country code |
| 8 | `[u64]` | | [unknown] |
| 9 | `i64` | `generated_at` | milliseconds |

`Flag`: 1 `str` key, 2 `i32` value, 4 `opt str`, 5 `opt str` [unknown].

`FlagList`: 1 `[TypedFlag]`. `TypedFlag`: 1 `str` key, 2 `{2: i32 | 3: str}` value.

`WebDevice`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `str` | `email` | the Google account, when cookies are sent |
| 3 | `str` | | [unknown] |
| 4 | `str` | `device_id` | a UUID: keep it as the **web device id** |

## Token refresh

The relay token expires. Refresh it when its expiry is less than one hour away, on connect
and before each (re)opening of the event stream, and when registering web push.

`POST ACCOUNT …Registration/RegisterRefresh`, pblite:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | fresh `request_id`, current `token`, `network` per session kind |
| 2 | `Device` | `browser` | the browser device |
| 3 | `i64` | `timestamp` | now, in **milliseconds × 1000** (microseconds at millisecond precision) |
| 4 | `bytes` | `signature` | see below |
| 13 | `RefreshOptions` | `options` | |
| 16 | `i32` | | always `2` |

`signature` = ECDSA-P256 with the refresh key over SHA-256 of the ASCII string
`"<request_id>:<timestamp>"` (the same `request_id` and `timestamp` as fields 1.1 and 3, the
timestamp in decimal), DER-encoded.

`RefreshOptions`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 9 | `{}` | | always present, empty |
| 23 | `PushOptions` | `push` | only to register web push |

`PushOptions`: 1 `i32` = `3`, 102 `WebPush`.

`WebPush`: 1 `str` = `messages_web`; 2 `str` push endpoint URL; 3 `str` P-256 DH public key,
base64url without padding; 4 `str` auth secret, base64url without padding.

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `TokenData` | `token` | |

`TokenData`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `bytes` | `token` | the new relay token |
| 2 | `i64` | `ttl` | validity in µs; if 0, assume 24 hours |

Store the token, `expiry = now + ttl`, and `ttl` itself: the TTL is also sent with every call
to the phone ([5](5-relay.md#calling-the-phone)).

A reply without a token is an error. HTTP 401/403/404, or error codes 16 or 5
([Transport › errors](1-transport.md#errors)), mean the session is gone: pair again. Other
failures are transient.

### Web push (optional)

A client that wants Web Push notifications while not connected registers its push
subscription by forcing a token refresh with `push` set, then sends the phone a settings update
enabling push ([6](6-actions.md#13-settings-update)). A terminal client does not need it.
