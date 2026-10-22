# 1. Transport

## Hosts

| Alias | Base URL | Used for |
|---|---|---|
| `API` | `https://instantmessaging-pa.googleapis.com` | pairing service, relay for QR sessions, media |
| `ACCOUNT` | `https://instantmessaging-pa.clients6.google.com` | registration service, relay for Google account sessions |
| `WEB` | `https://messages.google.com` | client configuration |

RPC calls are HTTP `POST` to:

```
<host>/$rpc/google.internal.communications.instantmessaging.v1.<Service>/<Method>
```

## Endpoints

| Service | Method | Host | Body encoding | Purpose |
|---|---|---|---|---|
| `Pairing` | `RegisterPhoneRelay` | `API` | binary | start QR pairing ([4](4-pairing.md#qr-code-pairing)) |
| `Pairing` | `RefreshPhoneRelay` | `API` | binary | new QR code for the same pairing attempt |
| `Pairing` | `GetWebEncryptionKey` | `API` | binary | [unknown] purpose, not needed |
| `Pairing` | `RevokeRelayPairing` | `API` | binary | unpair a QR session |
| `Registration` | `SignInGaia` | `ACCOUNT` | pblite | start Google account pairing |
| `Registration` | `RegisterRefresh` | `ACCOUNT` | pblite | refresh the relay token |
| `Messaging` | `ReceiveMessages` | `API` or `ACCOUNT` ¹ | pblite | the event stream (long polling) |
| `Messaging` | `SendMessage` | `API` or `ACCOUNT` ¹ | pblite | call the phone |
| `Messaging` | `AckMessages` | `API` or `ACCOUNT` ¹ | pblite | acknowledge stream items |

¹ `ACCOUNT` when the session holds Google cookies (account pairing), `API` otherwise.

Other URLs:

| URL | Method | Purpose |
|---|---|---|
| `https://messages.google.com/web/config` | `GET` | client configuration ([3](3-session.md#configuration)) |
| `https://instantmessaging-pa.googleapis.com/upload` | `POST` / `GET` | media upload and download ([8](8-media.md)) |
| `https://messages.google.com/web/authentication`, `/web/timesource` | | exist in the web client; not needed |

## Headers

Every RPC request carries headers mimicking Chrome running the web client:

| Header | Value |
|---|---|
| `content-type` | `application/x-protobuf` or `application/json+protobuf` (see encodings) |
| `accept` | `*/*` |
| `x-goog-api-key` | `AIzaSyCA4RsOZUFrm9whhtGosPlJLmVPnfSHKz8` (the public key of the web client) |
| `x-user-agent` | `grpc-web-javascript/0.1` |
| `user-agent` | a current Chrome user agent, e.g. `Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36` |
| `sec-ch-ua` | matching brands, e.g. `"Google Chrome";v="152", "Chromium";v="152", "Not-A.Brand";v="24"` |
| `sec-ch-ua-mobile` | `?1` |
| `sec-ch-ua-platform` | `"Android"` |
| `origin` | `https://messages.google.com` |
| `referer` | `https://messages.google.com/` |
| `sec-fetch-site` | `cross-site` |
| `sec-fetch-mode` | `cors` |
| `sec-fetch-dest` | `empty` |
| `accept-language` | `en-US,en;q=0.9` |

The user agent and `sec-ch-ua` should stay consistent with each other and reasonably recent.

### Google account sessions: cookies and `Authorization`

When the session holds Google cookies, every RPC request also carries:

- `Cookie`: all the cookies held (see [Pairing › account](4-pairing.md#google-account-pairing)
  for which ones);
- `Authorization: SAPISIDHASH <t>_<h>`, where `<t>` is the current Unix time in seconds and
  `<h>` the lowercase hex SHA-1 of the ASCII string `"<t> <SAPISID> https://messages.google.com"`
  (`<SAPISID>` being the value of the `SAPISID` cookie).

Cookies set by responses (`Set-Cookie`) replace the held ones and must be persisted.

The configuration request (`GET /web/config`) uses the same headers, except: no
`x-user-agent`, no `origin`, and `sec-fetch-site: same-origin`.

## Encodings

Two encodings of protobuf messages are used, chosen per endpoint (table above):

- **binary**: standard protobuf wire format, `content-type: application/x-protobuf`;
- **pblite**: a JSON form, `content-type: application/json+protobuf`.

Responses are decoded by their `Content-Type`: `application/x-protobuf` is binary;
`application/json+protobuf` and `text/plain` are pblite.

Several fields of type `bytes` carry another message in **binary** protobuf whatever the
outer encoding (the relay payloads, for instance): this is stated per field.

### pblite (JSON encoding)

A message is a JSON array in which the value of field number *n* sits at index *n − 1*.

| Field type | JSON value |
|---|---|
| unset (or proto3 zero value) | `null` |
| integers, enums | number. Decoders must also accept a decimal string (64-bit values may come as strings) |
| `bool` | `true` / `false`. Decoders must also accept a number (non-zero is true) |
| `str` | string |
| `bytes` | string, standard base64 with padding |
| nested message | nested array, encoded the same way; an empty message is `[]` |
| repeated | array of the encoded elements |
| `b64(M)` | string: standard base64 of the **binary** protobuf of `M` |
| `b64(str)` | string: standard base64 of the UTF-8 bytes of the string |

Encoding: the array has as many slots as the highest field number of the message type
(trailing `null`s included); slots of unset fields are `null`. A working client sizes arrays
to the highest field number it knows; sizing them to the highest field number listed in this
specification is the safe choice.

Decoding: a missing index (array shorter than the field number) or `null` means unset; values
at unknown indices are ignored.

Example: a message with field 1 = `"a"`, field 3 = a message `{2: 5}`, highest field 4:

```json
["a", null, [null, 5], null]
```

## Errors

A non-2xx response carries an error object (binary or pblite, by `Content-Type`):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `i64` | `code` | gRPC status code |
| 2 | `str` | `message` | description |
| 3 | `[ErrorDetail]` | `details` | |

`ErrorDetail`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `type_url` | e.g. `type.googleapis.com/google.internal.communications.instantmessaging.v1.TachyonError` |

Errors known:

| HTTP / code | Message | Meaning |
|---|---|---|
| code 16 | `Request had invalid authentication credentials. …` | token or cookies no longer valid: fatal, pair again |
| code 5, detail `TachyonError` | `Requested entity was not found.` | the pairing no longer exists (unpaired from the phone): fatal |
| code 7 | `The caller does not have permission` | |
| HTTP 401, 403 | | fatal for the event stream and the token refresh |
| HTTP 404 | | fatal for the token refresh |
| HTTP 5xx | | transient |

[client policy] Non-stream requests answered 5xx are retried, up to 3 attempts in all, waiting
1 s then 2 s.
