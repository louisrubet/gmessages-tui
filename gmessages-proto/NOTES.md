# Implementation notes: ambiguities of the specification and choices made

This crate implements `doc/protocol/` (README and chapters 1 to 9) and nothing else. Where
the specification is silent or ambiguous, the choice made is listed here.

## Encodings (§1)

1. **Empty repeated fields in pblite** are written `[]`, not `null` (the spec shows
   `destinations: []` for QR sessions; the general rule says unset is `null`). Decoders
   accept both.
2. **Present but empty nested messages**: a message type without fields (`{}`) is written
   `[]`; a message type with fields, present but with all fields unset, is written as a
   full-size array of `null`s (the sizing rule), not `[]`. Decoders accept both.
3. **64-bit integers** are encoded as JSON numbers (the spec's table); decoding accepts
   numbers, decimal strings and, for booleans, numbers.
4. Arrays are sized to the highest field number **this crate declares** for the message
   (the spec's "safe choice"); declared fields include all fields listed in the spec, so
   e.g. `PushOptions` is 102 slots long.
5. **Trailing JSON object** in a pblite array (a form some pblite encoders use for high field
   numbers) is accepted on decode as a map of field numbers. Not in the spec; harmless.
6. `bytes` / `b64(...)` fields: decoding also accepts URL-safe base64 and missing padding;
   encoding is standard base64 with padding.
7. Repeated scalars are encoded unpacked in binary; packed is accepted on decode.
8. A message's response body is decoded by `Content-Type`; an unknown type is sniffed
   (`[` → pblite). An empty body decodes as the default message.
9. The configuration (`/web/config`) is decoded as pblite first, whatever its content type.

## Transport, cookies, errors (§1, §3)

10. **Cookies** are sent to every host (RPC and config) when the session holds any; they are
    never sent to the upload service. `Set-Cookie` updates are applied only when the session
    already holds cookies (Google account sessions); domain and path attributes are ignored;
    `Max-Age<=0` deletes a cookie. Each change emits `Event::SessionChanged`.
11. `Authorization: SAPISIDHASH …` is omitted when there is no `SAPISID` cookie.
12. **Retries**: only HTTP 5xx answers of non-stream requests are retried (3 attempts, 1 s
    then 2 s), including `SendMessage` (a duplicate call is possible, as the policy implies).
    Network errors are not retried at that level.
13. **Fatal errors**: event stream opening answered HTTP 401/403, gRPC code 16, or code 5 with
    a `TachyonError` detail → `Event::LoggedOut(CredentialsRejected)`. Token refresh answered
    401/403/404 or code 16/5 → same. Other failures are transient.
14. **Token refresh** always goes to the `ACCOUNT` host (as the endpoint table says), for QR
    sessions too, with an empty `network` for them. It runs at connect, before each
    (re)opening of the stream, and on each liveness cycle (about every minute), when the
    expiry is less than one hour away.
15. The **client version** announced is a fixed date in `Config::client_version`
    (2026-09-02); the configuration's `client_version` is not followed.
16. `accept-encoding: gzip, deflate, br` for media requests is produced by reqwest's own
    decompression support rather than set by hand (setting it by hand would disable
    transparent decompression).

## Pairing (§4)

17. **Network of stream/ack requests during QR pairing**: empty (the "session kind" is QR),
    not `Bugle`; `Bugle` is used for `RegisterPhoneRelay` and `RefreshPhoneRelay` only.
18. QR pairing: a failed configuration fetch is not fatal (only the web device id is lost);
    in account pairing it is fatal (the web device id is required).
19. QR pairing gives up after 6 codes (`PairingError::QrExpired`), 30 s each.
20. The **2-second wait** after pairing is applied to account pairing too (the spec states it
    for QR pairing only).
21. Account pairing: a registration with no matching `RegistrationSeen` counts as seen at 0.
    Phones are ordered by `seen`, most recent first; `AccountPairingOptions::phone_index`
    picks one (0 by default).
22. The confirmed `emoji_version` and `key_version` are taken from the **init** reply
    (fields 6 and 7); the finish reply is only checked for errors.
23. A pairing reply fails when `error_kind ≠ 0` **or** `error ≠ 0`.
24. Error mapping: 5 → `WrongEmoji`; 7, 27 → `CancelledOnPhone`; 2, 3, 6, 17, 18 →
    `TimedOut`; 20, 21, 22, 26 → `Cancelled`; others → `Phone(code)`.
25. The cancel call (action 47) is sent on **any** failure after the init call, including
    errors reported by the phone, and on local cancellation (`CancelToken`).
26. Waiting for the finish reply has no fixed timeout in the spec: 10 minutes is used, then
    `TimedOut`. Waiting for the stream to open before the handshake: 60 s.
27. An emoji version other than 0 uses the version 1 list.
28. Unpairing a Google account session sends action 46 with no reply expected; a QR session
    uses `RevokeRelayPairing`. `unpair()` works without connecting.

## Relay (§5)

29. **Stream body framing**: the body is read as `[[` followed by items, each a complete JSON
    array, separated by commas, closed by `]]` (i.e. `[[ [item], [item] ]]`). Whitespace is
    tolerated; items are limited to 16 MiB.
30. **Reopen versus reconnect**: when the body ends, the stream is reopened with the same
    stream id and **no** new activation; a catch-up is sent if it was down more than 60 s. A
    new stream id (full reconnect, after 3 missed pings or `Client::reconnect`) triggers a new
    activation.
31. A body that ends without `]]` and without any item counts as a failure (backoff); one that
    delivered items is reopened at once. 60 s without data: reopened at once, and the stream
    counts as down since then for the catch-up rule.
32. **Backoff**: 10 s plus 0–5 s of jitter, plus 5 s per further consecutive failure, capped at
    5 minutes. `Event::Disconnected` is emitted on each failure and `Event::Connected` on
    recovery.
33. The **activation call** carries the new active session id both as request id and in
    `Call.session`.
34. Calls made before the first activation wait for it (up to 15 s) so they carry the active
    session id.
35. Every `Client` call counts as "a call a user is waiting on": after 5 s without reply it
    triggers an urgent ping (10 s timeout), subject to the 30 s minimum spacing of pings.
36. **Liveness**: ping timeout 60 s; on misses the next ping comes after 1, 2, 4… up to 64
    minutes. With that schedule the fourth miss (when "phone not responding" is reported)
    comes later than the "about 8 minutes" of the spec. A missed first ping after an
    activation reports it at once. Any reply or pushed update while pings are failing counts
    as the phone answering again (immediately, without waiting for the next ping).
37. The 3-hour "extra catch-up" check runs on the liveness cycle.
38. The default-SMS-app check (action 31) runs after activation and at most hourly; its
    result is emitted as `Event::DefaultSmsApp`.
39. **Backlog**: the counter is decremented by every route-19 item that is not a reply,
    whatever its action. Old items: messages are delivered with `old: true`; conversation
    updates, typing and alerts are dropped; settings and account changes are delivered;
    presence checks are not answered.
40. **Dedup** key: (id of the first conversation, else of the first message, else empty;
    SHA-256 of the decrypted update), last 8 kept.
41. Account sessions: an "intermediate" item (clear payload, no encrypted payload, action
    not 44/45) whose request id matches a pending call is ignored entirely (not counted as
    backlog).
42. The logged-out notice (action 16, no encrypted payload, clear `72 00`) is acknowledged,
    then `Event::LoggedOut(AccountLoggedOut)` is emitted and the client stops.
43. Acknowledgements: batched every 5 s, re-queued on failure, queue capped at 1 024 (oldest
    dropped); a last flush happens on shutdown.
44. The TTL of calls is the stored token TTL, except action 16 (absent) and pairing calls
    (300 s, including the cancel).

## Actions and data (§6, §7, §8)

45. **Send**: media parts come before the text part (the caption). Transient results (2, 3) are
    retried after 3, 8 and 20 s **with the same `tmp_id`**. The final result, accepted or not,
    is returned in `SendOutcome::result`; it is not turned into an error.
46. Calls answered with `success = false` (conversation update, delete, reaction) return
    `Error::PhoneRefused`; a missing reply returns `Error::Timeout`.
47. Mark read, full-size image and resend accept any reply body. Resend (25), prefetched
    contacts (52) and blocked contacts (53) are exposed but **unverified**; 53 is sent without
    a body.
48. Get-or-create: recipients use kind 2 and no formatted number. On status 3 the call
    returns `NeedsRcsGroupConfirmation` unless a group name was given, in which case it sends
    the confirmation itself.
49. Conversation operations send the other person's number only for delete, block, unblock
    and block-and-report, and only for non-group conversations (the first visible participant
    that is not us).
50. `Folder::SpamAndBlocked` is 5. A conversation's folder is derived from its status: 1 inbox;
    2, 4 archive; 5, 6 spam and blocked; 8 trash.
51. `MessageStatus::is_deleted` is true for 300 and also for 23 (our message deleted) and 117
    (received message deleted).
52. Reactions: when a named kind comes with an empty `emoji` field, the kind's emoji is filled
    in. Sending: the named kind if the emoji has one (❤ with or without U+FE0F), else 8.
53. `MediaFormat` from a MIME type: exact match first (`image/x-ms-bmp` → 6), else the
    "other" value of the top-level type, `text/*` → 26; parameters (`; codecs=…`) ignored.
54. Upload finalize body: treated as base64 when made only of standard-alphabet characters
    and `=` with a length multiple of 4.
55. Web push registration (refresh with `push` options) is not implemented; only the
    settings update (action 13) is exposed as `Client::set_web_push`.
56. Group avatars are fetched with the media header set (no api key, no `x-user-agent`, no
    cookies), capped at 5 MiB.
