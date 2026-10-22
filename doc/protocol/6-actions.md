# 6. Actions

Each call to the phone names an action ([5](5-relay.md#calling-the-phone)). Request and reply
bodies are binary protobuf, encrypted with the request keys. The data types (`Conversation`,
`Message`, …) are in [7](7-data.md).

## Action codes

| Code | Action | Here |
|---|---|---|
| 0 | unspecified | |
| 1 | list conversations | [↓](#1-list-conversations) |
| 2 | list messages | [↓](#2-list-messages) |
| 3 | send a message | [↓](#3-send-a-message) |
| 4 | message updates | [unknown] |
| 6 | list contacts | [↓](#6-list-contacts) |
| 7 | conversation updates | [unknown] |
| 9 | get or create a conversation | [↓](#9-get-or-create-a-conversation) |
| 10 | mark read | [↓](#10-mark-read) |
| 11 | presence check (pushed) | [5](5-relay.md#pushed-updates) |
| 12 | typing | [↓](#12-typing) |
| 13 | settings update | [↓](#13-settings-update) |
| 14 | alert (pushed) | [7](7-data.md#alerts) |
| 15 | update a conversation | [↓](#15-update-a-conversation) |
| 16 | updates: activation, catch-up, and pushed updates | [5](5-relay.md#activation) |
| 17 | acknowledge a presence check | [5](5-relay.md#pushed-updates) |
| 18 | list sticker sets | [unknown] |
| 19 | leave an RCS group | [unknown] |
| 20 | add a participant to an RCS group | [unknown] |
| 21 | conversation type | [↓](#21-conversation-type) |
| 22 | ping | [↓](#22-ping) |
| 23 | delete a message | [↓](#23-delete-a-message) |
| 24 | install a sticker set | [unknown] |
| 25 | resend a message | [↓](#25-resend-a-message) |
| 26 | RCS group status of a contact | [unknown] |
| 27 | download a message (MMS) | [unknown] |
| 28 | top contacts | [↓](#28-top-contacts) |
| 29 | contact thumbnails | [↓](#29-37-thumbnails) |
| 30 | change a participant's colour | [unknown] |
| 31 | is Google Messages the default SMS app | [↓](#31-default-sms-app) |
| 32 | sticker user context | [unknown] |
| 33 | favourite sticker packs | [unknown] |
| 34 | recent stickers | [unknown] |
| 35 | update recent stickers | [unknown] |
| 36 | full-size image | [↓](#36-full-size-image) |
| 37 | participant thumbnails | [↓](#29-37-thumbnails) |
| 38 | reaction | [↓](#38-reaction) |
| 39 | send a reply | [unknown]; replies go through action 3 |
| 40 | blob for an attachment | [unknown] |
| 41 | devices available for account pairing | [unknown] |
| 42 | create an account pairing | [unknown] |
| 43 | get a conversation | [↓](#43-get-a-conversation) |
| 44 | account pairing: init | [4](4-pairing.md#3-handshake) |
| 45 | account pairing: finish | [4](4-pairing.md#3-handshake) |
| 46 | account pairing: unpair | [4](4-pairing.md#unpairing) |
| 47 | account pairing: cancel | [4](4-pairing.md#cancelling) |
| 48 | prewarm | [unknown] |
| 49 | search group names | [unknown] |
| 50 | link an RCS identity | [unknown] |
| 51 | unlink an RCS identity | [unknown] |
| 52 | prefetch contacts | [↓](#52-53-prefetched-and-blocked-contacts) |
| 53 | blocked contacts | [↓](#52-53-prefetched-and-blocked-contacts) |
| 54 | remove a participant from an RCS group | [unknown] |
| 55 | update a group icon | [unknown] |
| 56 | destinations to conversations | [unknown] |

## 1. List conversations

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `i64` | `count` | page size |
| 4 | `enum Folder` | `folder` | |
| 5 | `Cursor` | `cursor` | absent for the first page |

`Folder`: 1 inbox, 2 archive, 5 spam and blocked, 8 trash (8 observed with the current web
client).

`Cursor`: 1 `str` id of the last item of the previous page; 2 `i64` its timestamp (µs).

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `[Conversation]` | `conversations` | most recent first |
| 3 | `bytes` | | [unknown] |
| 5 | `Cursor` | `next` | cursor for the next page |

Paging: send `next` as the next request's cursor. The last page is shorter than `count`, or has
no `next` (or an empty id). The phone may ignore the cursor and send the same page again:
stop when a page brings no conversation not seen yet.

The **first** list call after the client starts uses message class 16 ([5](5-relay.md#calling-the-phone)).

## 2. List messages

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `str` | `conversation_id` | |
| 3 | `i64` | `count` | |
| 5 | `Cursor` | `cursor` | absent: the latest messages; else older than the cursor |

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `[Message]` | `messages` | sort them by `timestamp` |
| 3 | `bytes` | | [unknown] |
| 4 | `i64` | `total` | approximate number of messages in the conversation |
| 5 | `Cursor` | `next` | to page further back; absent at the start of the history |

## 3. Send a message

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `str` | `conversation_id` | |
| 3 | `Draft` | `draft` | |
| 4 | `SimSelector` | `sim` | the SIM of the conversation ([7](7-data.md#sim-cards)) |
| 5 | `str` | `tmp_id` | client-chosen id of the message |
| 6 | `bool` | `force_rcs` | send as RCS even if the phone would fall back; normally false |
| 8 | `{1: str}` | `reply_to` | id of the message replied to |

`Draft`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `tmp_id` | the same |
| 6 | `{1: {1: str}}` | | text, an alternative form; [unverified], use field 10 |
| 7 | `str` | `conversation_id` | the same |
| 9 | `str` | `sender_id` | our participant id in this conversation: `Conversation.outgoing_id` ([7](7-data.md#conversation)) |
| 10 | `[MessagePart]` | `parts` | the content ([7](7-data.md#message-part)) |
| 12 | `str` | `tmp_id_again` | the same `tmp_id` |

`tmp_id`: a fresh random UUID (as the Android app does). The same value in all three places.

Content: a text is one part with `text`. A file is one part with `media` (from an upload,
[8](8-media.md#upload)), optionally followed by a text part (a caption).

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `AccountInfo` | `account` | [7](7-data.md#account-info) |
| 3 | `enum SendResult` | `result` | |

`SendResult`: 0 unknown; **1 accepted**; 2 and 3 failed, transient ([client policy] retry after
3 s, 8 s, 20 s); 4 failed (possibly: Google Messages not the default SMS app).

"Accepted" only means the phone took the message. The message itself arrives later as a pushed
message update carrying the same `tmp_id` (`Message` field 12), then again on every status change
(sending, sent, delivered, read, or failed, [7](7-data.md#message-status)).

## 6. List contacts

Request: `{5: 1, 6: 350, 7: 50}` (constant; [unknown] meaning, perhaps page sizes).

Reply: 2 `[Contact]`: the phone's address book, one entry per number ([7](7-data.md#contact)).
Observed to return the whole book (hundreds of entries) at once.

## 9. Get or create a conversation

Finds the conversation with these recipients, or creates it.

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `[Recipient]` | `recipients` | one per number; several make a group |
| 3 | `opt str` | `group_name` | RCS group name |
| 4 | `opt bool` | `create_group` | confirm the creation of an RCS group |

`Recipient`: 1 `i32` kind (`2`; perhaps `7` for numbers typed by the user, [unverified]);
2 `str` number; 3 `str` the number again; 4 `opt str` formatted number.

Reply:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `Conversation` | `conversation` | |
| 3 | `enum` | `status` | 1 found or created; **3: an RCS group would be created, confirm** |

On status 3, send the same request again with `group_name` set (`""` if none) and
`create_group` = true.

Several recipients without RCS give an MMS group or an SMS broadcast conversation, as the phone's
settings decide.

## 10. Mark read

Request: 2 `str` conversation id; 3 `str` id of the last message read. A reply comes (its body is
not needed). The phone marks the conversation read, and pushes the conversation update.

## 12. Typing

Request: 2 `{1: str conversation id, 3: bool typing}`; 3 `SimSelector`. No reply. Clients send
`typing` = true while the user types; there is no explicit stop.

## 13. Settings update

Request: 3 `{3: bool}`: enable or disable web push. No reply.

## 15. Update a conversation

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `ConversationChange` | `change` | [unverified], see below |
| 2 | `enum ConversationOp` | `op` | |
| 3 | `str` | `conversation_id` | |
| 5 | `opt {2: bool}` | `block_flag` | `{2: true}` when blocking |
| 6 | `ConversationRef` | `target` | |

`ConversationRef`: 1 `str` conversation id; 3 `opt str` phone number of the other person.

`ConversationOp`:

| Value | Operation | `target` |
|---|---|---|
| 1 | delete for good (with its messages) | id and, for a one-to-one conversation, the other person's number |
| 2 | unblock | id, number |
| 3 | move back to the inbox (unarchive) | id |
| 4 | archive | id |
| 7 | block (with `block_flag`) | id, number |
| 8 | block and report spam (with `block_flag`) | id, number |
| 15 | move to the trash | id; refused if already in the trash |

The number of the other person: the participant that is visible and not us, its identifier's
number ([7](7-data.md#participant)).

`ConversationChange` (field 1): 1 `str` conversation id, then one of 12 `enum ConversationStatus`
or 7 `i32` mute (1 mute, 0 unmute). Observed: a status sent this way is answered "success" and
ignored; use the operations above.

Reply: 1 `bool` success. The resulting change is also pushed as a conversation update.

## 16. Updates

No request body. See [activation and catch-up](5-relay.md#activation). A body `{2: i64}`, perhaps
"updates since a timestamp", exists: [unverified].

## 21. Conversation type

Request: 2 `str` conversation id. Reply: 2 `str` conversation id; 3 `i32` type; 5 `bool`;
6 `i32` ([unknown] beyond the type, see `ConversationType` in [7](7-data.md#enumerations)).

## 22. Ping

Request: `{2: true}`. Reply: empty. See [Keeping the phone attached](5-relay.md#keeping-the-phone-attached).

## 23. Delete a message

Request: 2 `str` message id. Reply: 2 `bool` success. Deletes it on the phone; a message update
with status "deleted" follows.

## 25. Resend a message

[unverified] Request: 2 `str` message id (a message that failed). Known from the web client's
action list; not exercised by any client known.

## 28. Top contacts

Request: 1 `i32` count (8). Reply: 1 `[Contact]`.

## 29, 37. Thumbnails

Avatars of contacts (29, by contact id) or of participants (37, by participant id).

Request: 1 `[str]` ids. Reply: 1 `[{1: str id, 2: Thumbnail}]`.

`Thumbnail`: 3 `bytes` image (detect its format); 4 `i32` [unknown]; 5 `Dimensions`;
2 `{13: fx64}` [unknown], perhaps a hash.

## 31. Default SMS app

No request body. Reply: 1 `bool`: whether Google Messages is the phone's default SMS app.

## 36. Full-size image

The phone sometimes sends only a preview of an image ([8](8-media.md#previews-and-full-size)).

Request: 1 `str` message id; 2 `str` part id (`MessagePart.part_id`). Reply: empty. The phone
then uploads the full image and pushes a message update in which that part's `media_id` is set.

## 38. Reaction

Request:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `message_id` | |
| 2 | `Reaction` | `reaction` | [7](7-data.md#reactions) |
| 3 | `enum` | `op` | 1 add, 2 remove, 3 replace our previous reaction |
| 4 | `SimSelector` | `sim` | |

Reply: 1 `bool` success.

## 43. Get a conversation

Request: 1 `str` conversation id. Reply: 1 `Conversation` (status 3 if it was deleted).

## 52, 53. Prefetched and blocked contacts

Replies have the same shape as [list contacts](#6-list-contacts) (2 `[Contact]`). Requests:
[unverified]; for 52, `{2: 350}`.

## Other bodies known

[unverified] Shapes seen in the web client's definitions, action not established:
`{2: str conversation id}` (opening a conversation), `{2: 1}` (preparing to open one).
