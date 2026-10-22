# Google Messages for web: protocol specification

This specification describes the protocol between a "Messages for web" client and Google's
servers, through which the client drives an Android phone running Google Messages. It is
written to serve as the basis of an independent implementation (a Rust library).

It describes the protocol only: hosts, requests, field numbers and types, enumeration
values, cryptography, and the behaviour observed. All names, groupings and prose here are
this document's own. Field and value **numbers** are what matters on the wire; names are
descriptive and may be changed freely by an implementation.

## Contents

1. [Transport](1-transport.md): hosts, endpoints, headers, encodings, errors
2. [Cryptography](2-crypto.md): request encryption, media encryption, signatures, key derivation
3. [Session state](3-session.md): what a client keeps, credentials, configuration, token refresh
4. [Pairing](4-pairing.md): QR code pairing, Google account pairing, unpairing
5. [Relay](5-relay.md): calling the phone, the event stream, acknowledgements, keeping the phone attached
6. [Actions](6-actions.md): every call to the phone, request and reply bodies
7. [Data model](7-data.md): conversations, messages, participants, contacts, settings, events, enumerations
8. [Media](8-media.md): upload, download, previews
9. [Appendix: pairing emoji](9-emoji.md)

## How the system works, in short

The phone is the only holder of the messages. The client never talks to the phone directly:
both talk to a Google relay ("Tachyon" / "Ditto"). The client:

1. **pairs** once, either by showing a QR code the phone scans, or through the Google account
   signed in on both (the user confirms an emoji on the phone). Pairing gives it a relay
   token, the identities of the phone and of itself, and two symmetric keys shared with the
   phone;
2. **opens a long-lived HTTP stream** (long polling) on which the relay delivers everything
   the phone sends: replies to calls and pushed updates (new messages, conversation changes,
   typing, settings);
3. **calls the phone** by posting a request to the relay, encrypted with the shared keys; the
   reply comes back later on the stream, matched by request id;
4. **acknowledges** every item received on the stream, refreshes its token before it
   expires, and pings the phone regularly to know whether it is reachable.

Media are not carried in messages: they are encrypted with a per-file key and stored on a
Google upload service, the message carrying only an id and the key.

## Conventions

### Field tables

Messages are protobuf (proto3 semantics: a zero value is the same as absent). Each message is
given as a table:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `example_id` | what the field holds |

Types:

| Notation | Meaning |
|---|---|
| `str` | UTF-8 string |
| `bytes` | byte string |
| `i32`, `i64`, `u64` | varint integers |
| `fx64` | fixed64 |
| `bool` | boolean |
| `enum E` | varint holding a value of enumeration `E` |
| `M` | nested message of type `M` |
| `[T]` | repeated field of `T` |
| `b64(M)` / `b64(str)` | in the pblite encoding only: the field is not a nested array but a base64 string of the binary protobuf of `M` (or of the UTF-8 bytes of the string). In binary protobuf it is an ordinary field. See [Transport › pblite](1-transport.md#pblite-json-encoding) |
| `opt T` | the field needs explicit presence: absent and zero mean different things |

A message given inline as `{1: x, 2: y}` lists field numbers and their values.

### Certainty

Unless marked otherwise, a statement describes what a working client does today. Marks:

- **[unverified]**: the shape is known (field numbers, codes) but no working client is known
  to exercise it; treat as a starting point.
- **[client policy]**: a choice of a working client (a timing, a retry count), not a
  requirement of the servers.
- **[unknown]**: present on the wire, meaning not understood.

### Units

Unless stated, timestamps are **microseconds** since the Unix epoch, and durations in token
data are microseconds.

## Scope and limits

- The protocol is undocumented and changes: Google updates the web client often. The client
  version announced in every request (see [Session › Client version](3-session.md#client-version))
  should track the current web client.
- The servers expect the browser-like headers listed in [Transport](1-transport.md).
- In Google account pairing, the client must send a fixed statement acknowledging that the API
  is undocumented (see [Pairing](4-pairing.md#3-handshake)). The servers require it; it also
  means Google's terms of service apply to its use.

## References and credits

This specification was written by analysing the behaviour of:

- **libgm**, the Go library of [mautrix-gmessages](https://github.com/mautrix/gmessages)
  (`pkg/libgm`), by Tulir Asokan and the mautrix contributors, AGPL-3.0. libgm is the
  reverse-engineering work this document stands on; credit for discovering the protocol
  belongs to its authors.
- **mautrix-gmessages** itself (`pkg/connector`), for how calls are used in practice.
- **gmessages-tui** (gmd), for a few behaviours observed against the current web client
  (folders, archive and trash actions, device name).
- **[UKEY2](https://github.com/google/ukey2)** by Google (Apache-2.0), the key exchange used by
  Google account pairing, whose messages are public.
- **Google Messages for web** (messages.google.com), the client whose behaviour all of the above
  reproduce.

No source code, protobuf schema or comment of libgm is reproduced here.
