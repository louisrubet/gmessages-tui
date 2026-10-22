# 8. Media

Files are not carried in messages. They are encrypted with a random key
([chunked AES-GCM](2-crypto.md#media-encryption-chunked-aes-gcm)) and stored on Google's upload
service; a message part ([7](7-data.md#media)) carries the file's id and key.

Upload and download requests do **not** carry cookies, `x-goog-api-key` or `x-user-agent`: the
relay token inside their metadata authenticates them. They carry the other browser headers of
[Transport](1-transport.md#headers), plus `accept-encoding: gzip, deflate, br`.

## Upload

A resumable upload in two requests.

1. Encrypt the file: key `K` = 32 random bytes; `E` = the encrypted file.
2. **Start**: `POST https://instantmessaging-pa.googleapis.com/upload` with headers:

   | Header | Value |
   |---|---|
   | `x-goog-upload-protocol` | `resumable` |
   | `x-goog-upload-command` | `start` |
   | `x-goog-upload-header-content-length` | length of `E` |
   | `x-goog-upload-header-content-type` | the file's MIME type |
   | `content-type` | `application/x-www-form-urlencoded;charset=UTF-8` |

   and as body the standard base64 (with padding) of the binary protobuf:

   | # | Type | Name | Meaning |
   |---|------|------|---------|
   | 1 | `i64` | | always `1` |
   | 2 | `Credentials` | `credentials` | fresh `request_id`, `network` per session kind, `token`, `version` |
   | 3 | `Device` | `phone` | the phone device |

   Expect HTTP 200 and the headers `x-goog-upload-url` (where to send the file),
   `x-goog-upload-status`, `x-goog-upload-chunk-granularity`, `x-goog-upload-control-url`,
   `x-guploader-uploadid`.
3. **Send and finalize**: `POST <x-goog-upload-url>` with body `E` and headers:

   | Header | Value |
   |---|---|
   | `x-goog-upload-command` | `upload, finalize` |
   | `x-goog-upload-offset` | `0` |
   | `x-goog-upload-header-content-length` | length of `E` |
   | `x-goog-upload-header-content-type` | the file's MIME type |
   | `content-type` | `application/x-www-form-urlencoded;charset=UTF-8` |

   Expect HTTP 200. The body is a binary protobuf, usually base64-encoded: if the body consists
   only of base64 characters and its length is a multiple of 4, decode it first.

   | # | Type | Name | Meaning |
   |---|------|------|---------|
   | 1 | `{1: str, 2: i64}` | `uploaded` | field 1 the **media id**; field 2 a number |
   | 2 | `str` | | a message |

4. Send a message ([6](6-actions.md#3-send-a-message)) with a part whose `media` is:
   `format` (from the MIME type), `media_id`, `file_name`, `size` (plaintext size), `key` = `K`,
   `mime`.

The whole file goes in one request; the chunk granularity header can be ignored.

## Download

`GET https://instantmessaging-pa.googleapis.com/upload`, no body, with the header
`x-goog-download-metadata` set to standard base64 of the binary protobuf:

| # | Type | Name | Meaning |
|---|------|------|---------|
| 1 | `{1: str, 2: bool}` | `file` | field 1 the media id; field 2 `true` (encrypted) |
| 2 | `Credentials` | `credentials` | fresh `request_id`, `network`, `token`, `version` |

The response body is the encrypted file: decrypt it with the part's key, as a stream if large.

## Previews and full size

A media part of a received message may come in several states:

| `media_id` | `preview_id` | `inline_preview` | State |
|---|---|---|---|
| set | | | full file available: download `media_id` with `key` |
| empty | set | | only a preview uploaded: download `preview_id` with `preview_key`, or ask for the full file |
| empty | empty | set | only a small preview in the message, in the clear |
| empty | empty | empty | the phone is still downloading or uploading it: wait for the next update of the message |

To get the full image when only a preview is there, call action 36 ([6](6-actions.md#36-full-size-image))
with the message id and the part's `part_id`, then wait for the message update in which that
part has a `media_id` ([client policy] a working client waits some seconds, then falls back to
the preview).

For MMS not yet downloaded by the phone (message status 101–106), the file only comes once the
phone downloads it.

## Avatars

- **Contacts and participants**: thumbnails through actions 29 and 37 ([6](6-actions.md#29-37-thumbnails)).
- **RCS group pictures**: `Conversation.group_avatar_url` is a plain HTTPS URL: `GET` it with the
  browser headers, without `x-goog-api-key` and `x-user-agent`. [client policy] cap at 5 MiB.
