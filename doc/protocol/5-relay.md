# 5. Relay

Everything exchanged with the phone goes through the relay:

- **calls**: the client posts a request for the phone (`SendMessage`); the relay answers at
  once that it accepted it, and the phone's **reply arrives later on the event stream**;
- **the event stream**: one long HTTP response (`ReceiveMessages`) delivering replies and
  pushed updates;
- **acknowledgements**: every item received on the stream is acknowledged (`AckMessages`).

Hosts: `ACCOUNT` for a Google account session, `API` for a QR session
([Transport](1-transport.md#endpoints)). All three use pblite.

## Calling the phone

`POST …Messaging/SendMessage`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Device` | `phone` | the phone device |
| 2 | `Routing` | `routing` | |
| 3 | `CallAuth` | `auth` | |
| 5 | `i64` | `ttl` | µs; see below |
| 9 | `[b64(str)]` | `destinations` | Google account session: `[destination id]` (the UUID as a lowercase string with dashes); QR session: `[]` |

`Routing`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `request_id` | the call's request id |
| 2 | `i32` | `route` | `19` (data) |
| 12 | `bytes` | `call` | binary protobuf of `Call` (below) |
| 23 | `{1: {}, 2: i32}` | `class` | field 1 an empty message (`[]`), field 2 the **message class** |

`CallAuth`: 1 `str` `request_id` (the same); 6 `bytes` relay token; 7 `ClientVersion`. (No
network field here.)

`Call` (binary):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `request_id` | the same request id |
| 2 | `enum Action` | `action` | what is asked ([6](6-actions.md)) |
| 3 | `bytes` | `clear` | payload in the clear (pairing only) |
| 5 | `bytes` | `encrypted` | payload: [encrypted](2-crypto.md#payload-encryption-aes-ctr--hmac) binary protobuf of the action's request |
| 6 | `str` | `session` | the current **active session id** (below); empty before the first activation |

The request id is a fresh random UUID per call unless stated (activation, pairing cancel).
Actions without a request body send neither `clear` nor `encrypted`.

`ttl`: the stored token TTL ([3](3-session.md#token-refresh)) by default; absent for action
16 (activation); 300 s for pairing calls.

Message class:

| Value | Used for |
|---|---|
| 2 | normal calls |
| 16 | the **first** "list conversations" call after the client starts |
| 20 | pairing init and pairing cancel |
| 3 | [unknown], not used |

HTTP reply: `{1: {2: str}, 2: opt str timestamp}`: only means the relay accepted the call.

### Waiting for the reply

The reply is the stream item whose data payload has `request_id` equal to the call's
([below](#data-payload)). [client policy] A working client waits up to **60 s**, then reports the
phone as not responding; for calls a user is waiting on, it pings the phone after 5 s to find
out sooner ([Liveness](#keeping-the-phone-attached)). The relay accepted the call, so the
phone may still carry it out later (a message may still be sent).

Some actions expect no reply: typing (12), settings update (13), activation (16), presence
acknowledgement (17), unpair (46), pairing cancel (47).

## The event stream

`POST …Messaging/ReceiveMessages`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | `request_id` = the **stream id**, `network` per session kind, `token`, `version` |
| 4 | `{2: {}}` | | always: `[null, []]` |

The stream id is a random UUID chosen when the client starts listening, reused each time the
stream is reopened, renewed on a full reconnect.

### Response body

The response is streamed. Its body, as received over time, is one JSON text:

```
[[ item , item , item , … ]]
```

- it starts with the two bytes `[[`;
- each `item` is a pblite `StreamItem` (a JSON array), items separated by `,`;
- it ends with `]]`, then the server closes the response.

Parse it incrementally: after `[[`, read one complete JSON array at a time (tracking bracket
depth, strings and escapes), skipping separators. Items arrive whole in practice, often one
per network read, but a parser should not rely on read boundaries. Items can be large (images
inline in messages): allow a few MiB.

`StreamItem` (one of):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `IncomingEnvelope` | `envelope` | something from the phone (or about pairing) |
| 3 | `{}` | `heartbeat` | keep-alive |
| 4 | `{1: opt i32}` | `backlog` | number of **backlog items** that follow ([below](#backlog)) |
| 5 | `{}` | `start` | the stream started |

### Incoming envelope

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `id` | the item's id: **acknowledge it** |
| 2 | `i32` | `route` | `19` data, `14` pairing, `7` account pairing |
| 3 | `u64` | | [unknown], execution start |
| 5 | `i32` | `class` | message class |
| 6 | `u64` | | [unknown], execution end |
| 7 | `u64` | | [unknown], duration in µs |
| 8 | `Device` | `phone` | |
| 9 | `Device` | `browser` | |
| 12 | `bytes` | `payload` | binary protobuf, by route (below) |
| 17 | `str` | | [unknown], a signature id |
| 21 | `str` | `timestamp` | |
| 23 | `{2: i32}` | | [unknown], seen on intermediate replies |

`payload` by route:

| Route | Payload |
|---|---|
| 19 | `DataPayload` ([below](#data-payload)) |
| 14 | `PairingEvent` ([4](4-pairing.md#5-paired)) |
| 7 | account pairing notice: `{1: i32 (9), 108: …}` with device registrations; informational, can be ignored |

### Data payload

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `request_id` | for a reply: the request id of the call answered |
| 3 | `i64` | `timestamp` | |
| 4 | `enum Action` | `action` | |
| 5 | `bytes` | `clear` | clear payload (pairing replies; logged-out notice) |
| 6, 7, 9 | `bool` | | [unknown] |
| 8 | `bytes` | `encrypted` | encrypted binary protobuf: the reply body of `action`, or a pushed update |
| 11 | `bytes` | `encrypted_extra` | encrypted; decrypts to `{2: AccountInfo}` ([7](7-data.md#account-info)) |

### Dispatching an item

For each envelope:

1. queue its `id` for acknowledgement (even if it cannot be decoded);
2. decode the payload by route; decrypt `encrypted` (or `encrypted_extra`);
3. if `request_id` matches a pending call, it is that call's **reply**: done. Exception, in
   Google account sessions: an item with `clear` but no `encrypted` is not a reply (the relay
   sends such intermediate items before real replies) unless its action is 44 or 45;
4. otherwise, route 19 with action 16 is a **pushed update** ([below](#pushed-updates));
   route 14 a pairing event; anything else is unexpected (log, ignore).

The phone sometimes sends the same update twice. [client policy] A working client keeps the
last 8 (thing id, SHA-256 of decrypted payload) pairs, thing id being the conversation or message
id, and drops an update whose pair is already there.

### Backlog

When the stream opens, the relay may first replay items already delivered to an earlier
stream. It announces them with a `backlog` item: the next *count* data items that are not
replies are backlog. Handle them, but as **old**:

- messages: apply (they may carry newer states), but do not notify;
- conversation updates, typing, alerts: ignore.

### Pushed updates

A data item with action 16 that is not a reply is a pushed update. Its decrypted body is an
`Update` ([7](7-data.md#updates)): conversations changed, messages new or changed, typing,
settings, alerts, a presence check, or an account change.

On a **presence check**, call the phone with action 17, no payload, no reply expected.

### Reconnecting

[client policy]:

- before (re)opening, refresh the token if needed ([3](3-session.md#token-refresh));
- when the body ends normally (`]]`), reopen at once;
- when nothing (not even a heartbeat) arrives for **60 s**, drop the connection and reopen;
- HTTP 401 or 403: fatal, pair again;
- other HTTP errors and network errors: retry after 10–15 s, then 5 s more per consecutive
  failure, back to the shortest after a success;
- if the stream was down for more than **1 minute**, send a catch-up once reopened: action 16
  with request id = the **current** active session id (not a new one), no payload, no TTL.

## Acknowledgements

Every envelope `id` received must be acknowledged; unacknowledged items may be delivered again
and may stall delivery. [client policy] Acknowledge in batches every **5 s**; on failure, queue
them again (a working client caps the queue at 1 024).

`POST …Messaging/AckMessages`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Credentials` | `credentials` | fresh `request_id`, `network`, `token`, `version` |
| 2 | `{}` | | always present, empty |
| 4 | `[Ack]` | `acks` | |

`Ack`: 1 `str` the envelope `id`; 2 `Device` the browser device.

Reply: as for calls, nothing useful.

## Activation

The phone pushes updates to one browser session at a time, the **active** one. After the stream
opens as a paired client:

1. [client policy] wait about 2 s (and up to 3 s more while backlog items are still expected),
   flush the pending acknowledgements, wait 1 s;
2. choose a new random UUID as the **active session id**;
3. call action **16** with request id = the active session id, no payload, no TTL, no reply
   expected;
4. from then on, every call carries this id in `Call.session`.

Activating takes the active role from any other browser session of the same pairing (the
other one receives a "browser inactive" alert, [7](7-data.md#alerts)), and the other way round:
an alert that this browser became inactive means another session took over.

[client policy] A working client then asks whether Google Messages is the phone's default SMS
app (action 31) at most once an hour.

## Keeping the phone attached

The phone dozes; the relay keeps calls until it wakes. To know whether it is reachable,
[client policy]:

- **ping** (action 22, body `{2: true}`) every minute, not more than once per 30 s; a reply
  within 60 s (10 s for an urgent ping) means alive. Any reply or pushed update from the phone
  counts as alive too;
- after 2 missed pings in a row, activate again (a **new** active session id);
- after 3, reconnect (new stream id, new activation);
- while failing, ping again after 1 min, then 2, 4… up to 64 min;
- report "phone not responding" after 4 missed pings in a row (about 8 minutes), or if the
  first ping after connecting is missed;
- when the phone answers again after 2 or more misses, send a catch-up (action 16 with the
  current active session id, [above](#reconnecting));
- if nothing at all came from the phone for about 3 hours, send an extra catch-up (never more
  often than every 5 minutes).

## Sequence summary

```
client                         relay                         phone
  | RegisterRefresh (if due) ---->|                              |
  | ReceiveMessages (stream) ---->|                              |
  |<------------- [[ start, backlog n, … (old items) …          |
  | AckMessages (every 5 s) ----->|                              |
  | SendMessage action 16 ------->|------------------------------>| activation
  |<------------------------------|<------ updates (action 16) ---|
  | SendMessage action 1 (list) ->|------------------------------>|
  |<------------- reply, request_id = call's -------------------|
  | SendMessage action 22 (ping) every minute …                  |
```
