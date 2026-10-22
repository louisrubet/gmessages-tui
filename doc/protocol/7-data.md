# 7. Data model

## Conversation

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `id` | conversation id (a decimal number as a string) |
| 2 | `str` | `name` | display name: the contact's name, the number, or the group name |
| 4 | `Preview` | `preview` | the latest message, as shown in a list |
| 5 | `i64` | `last_activity` | µs: timestamp of the latest message |
| 6 | `bool` | `unread` | |
| 10 | `bool` | `group` | a group conversation (believed) |
| 11 | `str` | `outgoing_id` | **our participant id** in this conversation: the sender of our messages |
| 12 | `enum ConversationStatus` | `status` | folder or state ([enumerations](#enumerations)) |
| 13 | `bool` | `read_only` | cannot be answered |
| 15 | `str` | `colour` | hex colour of the avatar |
| 17 | `str` | `latest_message_id` | |
| 18 | `enum SendMode` | `send_mode` | |
| 20 | `[Participant]` | `participants` | including us |
| 21 | `[str]` | `others` | participant ids other than us |
| 22 | `enum ConversationType` | `type` | SMS/MMS or RCS |
| 24 | `bool` | | [unknown] |
| 26 | `bool` | `pinned` | |
| 27 | `{1: str}` | `sim_participant` | our participant id for the SIM used |
| 28 | `i64` | | [unknown] timestamp, seen as `last_activity` + 1000 after marking unread |
| 29 | `bool` | | [unknown] |
| 31 | `GroupRef` | `group_ref` | |
| 32 | `SimCard` | `sim` | the SIM this conversation uses ([SIM cards](#sim-cards)) |
| 33 | `{1: MessageRef, 2: i64 ms}` | | the latest message, another reference |
| 34 | `str` | `group_avatar_url` | URL of an RCS group's picture ([8](8-media.md#avatars)) |
| 35 | `MessageRef` | | [unknown] |

`Preview`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `text` | text as shown in the list |
| 2 | `i64` | `from_me` | non-zero if we sent it |
| 4 | `str` | `sender_name` | |
| 5 | `{1: i64, 2: enum MessageStatus}` | `status` | |

`GroupRef`: 1 `str` conversation id; 2 `{1: str id, 2: str conference URI}` (RCS group);
3 `str` [unknown], perhaps a number.

`MessageRef`: 1 `str` message id; 2 `str` another id.

Conversation ids and participant ids are local to the phone (decimal numbers in strings).

## Participant

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Identifier` | `id` | |
| 2 | `str` | `first_name` | |
| 3 | `str` | `full_name` | the contact's name, or the formatted number |
| 5 | `str` | `colour` | hex avatar colour |
| 6 | `bool` | `is_me` | |
| 7 | `SimSelector` | `sim` | for our own entries |
| 8 | `bool` | `visible` | false for internal entries: skip them when listing members |
| 10 | `str` | `contact_id` | id in the address book (for avatars, [6](6-actions.md#29-37-thumbnails)) |
| 14 | `i64` | | [unknown] |
| 15 | `str` | `formatted_number` | |
| 19, 20 | `i64` | | [unknown] |

`Identifier`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `enum IdentifierKind` | `kind` | |
| 2 | `str` | `number` | the phone number (E.164 for kind 1) or address |
| 3 | `str` | `participant_id` | the participant id used in messages |

`IdentifierKind`: 0 unknown, 1 phone number (E.164), 2 email; values below were not observed
but belong to the same list: 3 bot, 4 unknown sender, 5 short code without country, 6 short code
with country, 7 emergency number, 8 local number with country, 9 local number without country,
10 national number, 11 satellite emergency, 12 assistant bot, 13 alphanumeric short code without
country, 14 alphanumeric short code with country, 15 number with country, 16 number with
international prefix, 17 number with `+`, 18 invalid (too short), 19 invalid (too long),
20 invalid (length), 21 invalid (country code), 22 unknown but dialable, 23 toll-free.

## Message

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `id` | message id |
| 3 | `{1: i64}` | `kind` | [unknown] |
| 4 | `MessageStatusInfo` | `status` | [below](#message-status) |
| 5 | `i64` | `timestamp` | µs |
| 7 | `str` | `conversation_id` | |
| 9 | `str` | `sender_id` | participant id of the sender (ours: `Conversation.outgoing_id`) |
| 10 | `[MessagePart]` | `parts` | text and media |
| 11 | `i64` | `transport` | 1 SMS, 2 MMS downloaded, 3 MMS not downloaded, 4 RCS (believed; [unverified]) |
| 12 | `str` | `tmp_id` | for our messages sent from a web client: the `tmp_id` of the send call |
| 14 | `opt str` | `subject` | MMS subject |
| 16 | `i64` | | [unknown] |
| 19 | `[ReactionGroup]` | `reactions` | [reactions](#reactions) |
| 21 | `ReplyRef` | `reply_to` | |
| 23 | `GroupRef` | | |
| 24 | `MessageRef` | | |
| 25 | `Participant` | `sender` | sometimes present |

`ReplyRef`: 1 `str` id of the message replied to; 2 `str` conversation id; 3 `{}`.

Messages are **upserted**: the phone sends the whole message again on every change (status,
reactions, media becoming available). Key them by `id`.

### Message part

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `opt str` | `part_id` | id of the part (used to ask for a full-size image) |
| 2 | `{1: str}` | `text` | one of: text content |
| 3 | `Media` | `media` | one of: a file ([8](8-media.md)) |

### Media

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `enum MediaFormat` | `format` | [enumerations](#enumerations) |
| 2 | `str` | `media_id` | id of the full file on the upload service |
| 4 | `str` | `file_name` | |
| 5 | `i64` | `size` | plaintext size in bytes |
| 6 | `Dimensions` | `dimensions` | |
| 7 | `bytes` | `inline_preview` | a small preview image, in the clear |
| 9 | `str` | `preview_id` | id of a preview on the upload service |
| 11 | `bytes` | `key` | 32-byte key of `media_id` |
| 12 | `bytes` | `preview_key` | 32-byte key of `preview_id` |
| 14 | `str` | `mime` | MIME type |

`Dimensions`: 1 `i64` width; 2 `i64` height.

### Message status

`MessageStatusInfo`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `enum MessageStatus` | `status` | |
| 3 | `i64` | `sub_code` | error detail |
| 4 | `str` | `error` | error text |
| 5 | `str` | `status_text` | text to show for system notices |
| 6 | `i64` | | [unknown] |

`MessageStatus` ranges:

| Range | Meaning |
|---|---|
| 1–99 | **our** messages: the value says how far sending went |
| 100–199 | **received** messages: the value says how far downloading went |
| 200–299 | **system notices** in the conversation (no sender): group created, renamed, someone joined… |
| 300 | deleted |

Our messages:

| Value | Meaning | |
|---|---|---|
| 1 | sent | sent |
| 2 | delivered | sent |
| 11 | read by the recipient | sent |
| 3 | draft | |
| 4 | waiting to be sent | pending |
| 5 | sending | pending |
| 6 | resending | pending |
| 7 | waiting to retry | pending |
| 10 | sent after processing | pending |
| 14 | not delivered yet | sent |
| 15 | revocation pending | |
| 16 | scheduled | pending |
| 20 | being validated | pending |
| 8 | failed | **failed** |
| 9 | failed: emergency number | **failed** |
| 12 | cancelled | not sent |
| 13 | failed: too large | **failed** |
| 17 | failed: recipient lost RCS | **failed** |
| 18 | failed, no retry and no fallback | **failed** |
| 19 | failed: recipient could not decrypt | **failed** |
| 21 | failed: recipient lost encryption | **failed** |
| 22 | failed: recipient could not decrypt, no more retries | **failed** |
| 23 | deleted | |
| 24 | failed: negative delivery report | **failed** |
| 25 | failed: emergency protocol determination | **failed** |
| 26 | restricted | not sent |
| 27 | failed to encrypt | **failed** |

The phone shows the "failed" ones as "Not sent".

Received messages:

| Value | Meaning |
|---|---|
| 100 | received (complete) |
| 101 | waiting for a manual download (MMS) |
| 102 | retrying a manual download |
| 103 | downloading (manual) |
| 104 | retrying an automatic download |
| 105 | downloading (automatic) |
| 106 | download failed |
| 107 | expired or no longer available |
| 108 | delivered |
| 109 | displayed (read) |
| 110 | download cancelled |
| 111 | download failed: too large |
| 112 | download failed: no mobile data on the SIM |
| 113 | could not decrypt |
| 114 | decryption aborted |
| 115 | waiting for an automatic download |
| 116 | unknown content type |
| 117 | deleted |
| 118 | download restricted |

System notices (200–279):

| Value | Notice |
|---|---|
| 200 | a participant joined |
| 201 | a participant left |
| 202 | we left |
| 203 | RCS group created |
| 204 | MMS group created |
| 205 | SMS broadcast created |
| 206 | one-to-one SMS conversation created |
| 207 | one-to-one RCS conversation created |
| 208 | switched to group MMS |
| 209 | switched to SMS broadcast |
| 210 | link previews shown |
| 211 | group renamed (locally) |
| 212 | verified SMS applicable |
| 213 | one-to-one end-to-end encrypted RCS created |
| 214 | switched to SMS/MMS |
| 215 | switched to RCS |
| 216 | switched to encrypted RCS |
| 217 | group renamed |
| 218 | group name cleared |
| 219 | switched to encrypted RCS (information) |
| 220 | we were removed from the group |
| 221 | a participant was removed from the group |
| 222 | an SMS participant upgraded |
| 223 | an RCS participant upgraded |
| 224 | an encrypted RCS participant upgraded |
| 225 | a participant joined the encrypted group |
| 226 | a participant joined the encrypted group (information) |
| 227 | a participant left the encrypted group |
| 228 | we left the encrypted group |
| 229 | encrypted group created |
| 230 | we were removed from the encrypted group |
| 231 | a participant was removed from the encrypted group |
| 232 | suggestion (shortcut) |
| 233 | group switched from RCS to encrypted |
| 234 | group switched from encrypted to RCS |
| 235 | switched from SMS/MMS to encrypted |
| 236 | switched from encrypted to SMS/MMS |
| 237 | switched from RCS to encrypted |
| 238 | switched from encrypted to RCS |
| 239 | satellite messaging information |
| 240 | business chat: SIM switched |
| 241 | business chat: privacy notice |
| 242 | assistant conversation notice |
| 243 | other participant reachable through Meet |
| 244 | missed video call |
| 245 | a participant changed the theme |
| 246–250 | end of a satellite emergency session (variants: success or failure, help still needed or not, unexpected end) |
| 251 | satellite messaging information |
| 252 | group switched from encrypted to MMS |
| 253 | assistant proactive message notice |
| 254 | possible scam warning |
| 255–258 | business chat: unsubscribe sent, subscribe sent, unsubscribe confirmed, subscribe confirmed |
| 259 | our identity changed |
| 260 | group picture changed |
| 261 | group picture removed |
| 262, 263 | business chat: unsubscribe error, subscribe error |
| 264 | emergency conversation created |
| 265 | sensitive content warning |
| 266, 267 | business chat: unsubscribe / subscribe confirmed (no link) |
| 268 | emergency questionnaire |
| 269, 270 | business chat: unsubscribe / subscribe sent (no link) |
| 271 | joined an RCS group by link |
| 272 | group invitation link enabled |
| 273 | group invitation link disabled |
| 274 | group invitation link reset (manually) |
| 275 | group invitation link reset (automatically) |
| 276 | MLS (encryption) notice |
| 277 | RCS group created by us |
| 278 | custom card |
| 279 | business chat conversation started by them |

`status_text` carries the text to show for a notice when there is one. Clients usually hide
the purely technical ones: protocol switches (214–216, 219, 233–238), creation notices (203–207,
213, 229), link previews (210), theme changes (245), identity changes (259).

Deleted (300): the message was deleted. With an empty `sender_id` it is a system deletion.
[client policy] While a full resynchronisation is in progress, a working client ignores
deletions, the phone emitting spurious ones then.

## Reactions

`ReactionGroup` (on a message, one per emoji):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `Reaction` | `reaction` | |
| 2 | `[str]` | `participant_ids` | who reacted so |

`Reaction`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `emoji` | the emoji itself |
| 2 | `enum ReactionKind` | `kind` | |
| 101 | `CustomEmoji` | `custom` | an image reaction |

`ReactionKind`: the phone has named kinds for some emoji; others are "custom" (8) with the emoji
in field 1. When sending, use the named kind if the emoji has one, else 8.

| Value | Emoji |
|---|---|
| 0 | unspecified |
| 1 | 👍 |
| 2 | 😍 |
| 3 | 😂 |
| 4 | 😮 |
| 5 | 😥 |
| 6 | 😠 |
| 7 | 👎 |
| 8 | custom: any other emoji |
| 9 | 🤔 |
| 10 | 😢 |
| 11 | 😡 |
| 12 | ❤️ (also matches ❤ without the variation selector) |
| 13 | [unknown] (an animated kind) |

`CustomEmoji`: 1 `str` UUID; 4 `opt i32`; 100 `{100: opt str, 101: {1: str mime, 2: str uri, 3: opt i32, 5: opt str}, 102: {1: {1: i32, 2: str mime, 3: str uri, 5: i32 width, 6: i32 height, 7: opt i32}}}`.

## Contact

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `participant_id` | |
| 2 | `str` | `name` | |
| 3 | `ContactNumber` | `number` | |
| 7 | `str` | `colour` | hex avatar colour |
| 8 | `str` | `name_again` | |
| 10 | `bool` | | [unknown] |
| 11 | `str` | `contact_id` | |

`ContactNumber`: 1 `i32` kind (2); 2 `str` number; 3 `str` number again; 4 `opt str` formatted.

A contact with several numbers appears once per number.

## Typing

`TypingInfo`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `str` | `conversation_id` | |
| 2 | `{1: i64, 2: str number}` | `who` | |
| 3 | `enum` | `state` | 0 stopped, 1 started |
| 4 | `GroupRef` | | |

## SIM cards

`SimSelector` (which SIM sends):

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `i32` | | [unknown], usually 2 |
| 2 | `i32` | `sim_number` | |

`SimCard`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 3 | `opt {1: bool}` | `rcs` | RCS chats enabled on this SIM |
| 5 | `SimData` | `data` | |
| 6 | `bool` | | [unknown] |
| 7 | `{1: str}` | `self` | our participant id for this SIM |
| 8, 9 | `str` | | RCS addresses |

`SimData`: 1 `SimSelector`; 2 `bool` (perhaps default); 3 `str` carrier name; 4 `str` hex colour;
5 `i64`; 6 `str` formatted own number; 7 `str` own number, international.

To send in a conversation, use `Conversation.sim.data.1` as the `SimSelector`.

## Settings

Pushed in updates ([below](#updates)), notably after activation.

| # | Type | Name | Meaning |
|---|------|------|---------|
| 2 | `[SimCard]` | `sims` | |
| 3 | `MiscSettings` | | |
| 4 | `RcsSettings` | `rcs` | |
| 5 | `str` | `app_version` | Google Messages version on the phone |
| 7 | `bool` | | [unknown] |
| 8 | message | | [unknown]: booleans in 1, 2, 6; `{1: bool}` in 3 and 5 |
| 9 | `bytes` | | [unknown] |
| 10 | message | | [unknown]: booleans in fields 1 to 34 |

`MiscSettings`: 3 `opt bool` web push enabled (absent ≠ false); 7, 12 `bool`; 15 `[bytes]`;
16 `str` JSON; 17 `str`; 23 `i64` timestamp. Only field 3 is understood.

`RcsSettings`:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `bool` | `enabled` | RCS on |
| 2 | `bool` | `read_receipts` | send read receipts |
| 3 | `bool` | `typing` | show typing indicators |
| 4 | `opt bool` | `default_sms_app` | absent from some updates: keep the previous value |
| 5 | `i32` | | usually 1 |
| 6 | `bytes` | | [unknown] |
| 7, 8 | `str` | | UUIDs, [unknown] |

## Account info

`AccountInfo`: 1 `str` account (an email address when meaningful); 2 `bool`. Sent with send
replies and as an update when the Google account in use on the phone changes.

## Updates

The decrypted body of a pushed update (action 16, [5](5-relay.md#pushed-updates)), one of:

| # | Type | Meaning |
|---|------|---------|
| 2 | `{2: [Conversation]}` | conversations created or changed (name, status, unread, pinned, preview…) |
| 3 | `{2: [Message]}` | messages new or changed |
| 4 | `{2: TypingInfo}` | typing |
| 5 | `Settings` | phone settings |
| 6 | `{2: enum Alert}` | an alert ([below](#alerts)) |
| 7 | `{}` | presence check: answer with action 17 |
| 15 | `AccountInfo` | account changed |

Fields 8 to 13 are known to exist (participants, conversation type, favourite stickers, recent
stickers, cloud storage information, attachment upload progress): [unverified] shapes.

A conversation whose `status` becomes 3 (deleted) was deleted for good on the phone. A
conversation moved to another folder arrives with its new `status`.

## Alerts

| Value | Meaning |
|---|---|
| 0 | unknown |
| 1 | this browser session became inactive |
| 2 | this browser session became active |
| 3 | phone on mobile data |
| 4 | phone on Wi-Fi |
| 5 | phone battery low |
| 6 | phone battery no longer low |
| 7 | this browser session inactive: timeout |
| 8 | this browser session inactive: inactivity |
| 9 | RCS connected |
| 10 | [unknown] (observer registered) |
| 11 | phone database syncing |
| 12 | phone database sync complete |
| 13 | phone database sync started |
| 14 | phone database partial sync complete |
| 15 | phone database partial sync started |
| 16 | contacts refresh started |
| 17 | contacts refresh complete |
| 18 | disconnected from satellite |
| 19 | restoring messages from backup |
| 20 | restore complete |
| 21 | restore started |
| 22 | push throttling |
| 23 | push throttling started |
| 24 | push throttling ended |
| 25–27 | the same as 22–24, in a test mode |

An inactive alert (1, 7, 8) means another session took the active role
([5](5-relay.md#activation)): activate again to take it back, or stay passive.

## Enumerations

`ConversationStatus`: 0 unknown, 1 active (inbox), 2 archived, 3 deleted, 4 archived (kept
archived when new messages arrive), 5 spam folder, 6 blocked folder, 7 cross-country folder,
8 trash.

`ConversationType`: 0 unknown, 1 SMS/MMS, 2 RCS.

`SendMode`: 0 automatic, 1 SMS/MMS only, 2 SMS/MMS (latched).

`MediaFormat`:

| Value | MIME | Value | MIME |
|---|---|---|---|
| 0 | unspecified | 22 | `audio/ogg` |
| 1 | `image/jpeg` | 23 | audio, other |
| 2 | `image/jpg` | 24 | `text/vcard` |
| 3 | `image/png` | 25 | `application/pdf` |
| 4 | `image/gif` | 26 | `text/plain` |
| 5 | `image/wbmp` | 27 | `text/html` |
| 6 | `image/bmp` (`image/x-ms-bmp`) | 28 | `application/msword` |
| 7 | image, other | 29 | `.docx` (`application/vnd.openxmlformats-officedocument.wordprocessingml.document`) |
| 8 | `video/mp4` | 30 | `.pptx` (`…presentationml.presentation`) |
| 9 | `video/3gpp2` | 31 | `application/vnd.ms-powerpoint` |
| 10 | `video/3gpp` | 32 | `.xlsx` (`…spreadsheetml.sheet`) |
| 11 | `video/webm` | 33 | `application/vnd.ms-excel` |
| 12 | `video/x-matroska` | 34 | `application/vnd.android.package-archive` |
| 13 | video, other | 35 | `application/zip` |
| 14 | `audio/aac` | 36 | `application/java-archive` |
| 15 | `audio/amr` | 37 | application, other |
| 16 | `audio/mp3` | 38 | vCalendar, `text/x-calendar` (`.vcs`) |
| 17 | `audio/mpeg` | 39 | vCalendar, another text form: treat as `text/x-calendar` |
| 18 | `audio/mpg` | 40 | iCalendar, `text/calendar` (`.ics`) |
| 19 | `audio/mp4` | 41 | vCalendar as an application type: treat as `text/x-calendar` |
| 20 | `audio/mp4-latm` | 42 | iCalendar as an application type: treat as `text/calendar` |
| 21 | `audio/3gpp` | 43 | another calendar format, [unknown] |

When sending, pick the value of the file's MIME type; else the "other" value of its top-level
type (7, 13, 23, 37; `text/*` → 26). The `mime` field of `Media` is authoritative when present.
