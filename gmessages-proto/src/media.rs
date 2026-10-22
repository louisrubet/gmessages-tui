//! Media upload and download (spec §8).

use base64::Engine as _;
use reqwest::header::{HeaderName, HeaderValue};
use uuid::Uuid;

use crate::codec::{self, B64};
use crate::conn::{Conn, HeaderSet, UPLOAD};
use crate::crypto::{MediaDecryptor, encrypt_media, random_bytes};
use crate::error::{Error, Result};
use crate::types::{Media, MediaFormat};
use crate::wire;

const FORM: &str = "application/x-www-form-urlencoded;charset=UTF-8";

fn header(h: &mut reqwest::header::HeaderMap, k: &'static str, v: &str) -> Result<()> {
    let v =
        HeaderValue::from_str(v).map_err(|e| Error::InvalidInput(format!("header {k}: {e}")))?;
    h.insert(HeaderName::from_static(k), v);
    Ok(())
}

/// Decodes the finalize reply: base64 if the body is only base64 characters with a length
/// multiple of 4, else binary.
pub(crate) fn parse_upload_reply(body: &[u8]) -> Result<wire::UploadResponse> {
    let is_b64 = !body.is_empty()
        && body.len().is_multiple_of(4)
        && body
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
    let raw = if is_b64 {
        B64.decode(body).map_err(|e| Error::Decode(e.to_string()))?
    } else {
        body.to_vec()
    };
    Ok(codec::from_binary(&raw)?)
}

pub(crate) async fn upload(conn: &Conn, data: &[u8], file_name: &str, mime: &str) -> Result<Media> {
    let key: [u8; 32] = random_bytes();
    let enc = encrypt_media(&key, data);
    let len = enc.len().to_string();

    let start = wire::UploadStartRequest {
        one: 1,
        credentials: Some(conn.credentials(Uuid::new_v4().to_string(), conn.network())),
        phone: Some((&conn.session().phone).into()),
    };
    let mut h = conn.headers(HeaderSet::Media, Some(FORM));
    header(&mut h, "x-goog-upload-protocol", "resumable")?;
    header(&mut h, "x-goog-upload-command", "start")?;
    header(&mut h, "x-goog-upload-header-content-length", &len)?;
    header(&mut h, "x-goog-upload-header-content-type", mime)?;
    let rb = conn
        .http()
        .post(UPLOAD)
        .headers(h)
        .body(B64.encode(codec::to_binary(&start)));
    let resp = conn.send(rb, true).await?;
    let url = resp
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| Error::Decode("upload start reply without x-goog-upload-url".into()))?;
    let _ = resp.bytes().await;

    let mut h = conn.headers(HeaderSet::Media, Some(FORM));
    header(&mut h, "x-goog-upload-command", "upload, finalize")?;
    header(&mut h, "x-goog-upload-offset", "0")?;
    header(&mut h, "x-goog-upload-header-content-length", &len)?;
    header(&mut h, "x-goog-upload-header-content-type", mime)?;
    let rb = conn.http().post(&url).headers(h).body(enc);
    let resp = conn.send(rb, true).await?;
    let body = resp.bytes().await?;
    let reply = parse_upload_reply(&body)?;
    let media_id = reply
        .uploaded
        .map(|u| u.media_id)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            Error::Decode(format!("upload reply without media id: {}", reply.message))
        })?;
    Ok(Media {
        format: MediaFormat::from_mime(mime),
        media_id,
        file_name: file_name.to_string(),
        size: data.len() as i64,
        key: key.to_vec(),
        mime: mime.to_string(),
        ..Default::default()
    })
}

/// A media download in progress, decrypted as it arrives.
pub struct MediaDownload {
    resp: reqwest::Response,
    dec: Option<MediaDecryptor>,
}

impl MediaDownload {
    /// Size of the encrypted body, if announced.
    pub fn encrypted_len(&self) -> Option<u64> {
        self.resp.content_length()
    }

    /// The next piece of plaintext; `None` at the end.
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            let Some(dec) = self.dec.as_mut() else {
                return Ok(None);
            };
            match self.resp.chunk().await? {
                Some(b) => {
                    let out = dec.update(&b)?;
                    if !out.is_empty() {
                        return Ok(Some(out));
                    }
                }
                None => {
                    let last = self.dec.take().expect("present").finish()?;
                    return Ok(if last.is_empty() { None } else { Some(last) });
                }
            }
        }
    }

    /// Reads and decrypts everything.
    pub async fn bytes(mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(c) = self.next_chunk().await? {
            out.extend(c);
        }
        Ok(out)
    }
}

pub(crate) async fn download(conn: &Conn, media_id: &str, key: &[u8]) -> Result<MediaDownload> {
    let key: [u8; 32] = key.try_into().map_err(|_| {
        Error::InvalidInput(format!("media key must be 32 bytes, got {}", key.len()))
    })?;
    let meta = wire::DownloadMetadata {
        file: Some(wire::DownloadFile {
            media_id: media_id.to_string(),
            encrypted: true,
        }),
        credentials: Some(conn.credentials(Uuid::new_v4().to_string(), conn.network())),
    };
    let mut h = conn.headers(HeaderSet::Media, None);
    header(
        &mut h,
        "x-goog-download-metadata",
        &B64.encode(codec::to_binary(&meta)),
    )?;
    let resp = conn.send(conn.http().get(UPLOAD).headers(h), true).await?;
    Ok(MediaDownload {
        resp,
        dec: Some(MediaDecryptor::new(&key)),
    })
}

/// GETs a plain HTTPS URL with the browser headers, up to `cap` bytes.
pub(crate) async fn fetch_plain(conn: &Conn, url: &str, cap: usize) -> Result<Vec<u8>> {
    let mut resp = conn
        .send(
            conn.http()
                .get(url)
                .headers(conn.headers(HeaderSet::Media, None)),
            false,
        )
        .await?;
    let mut out = Vec::new();
    while let Some(c) = resp.chunk().await? {
        out.extend_from_slice(&c);
        if out.len() > cap {
            return Err(Error::InvalidInput(format!(
                "resource larger than {cap} bytes"
            )));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_reply_both_forms() {
        let r = wire::UploadResponse {
            uploaded: Some(wire::UploadedMedia {
                media_id: "MEDIA".into(),
                number: 5,
            }),
            message: "ok".into(),
        };
        let bin = codec::to_binary(&r);
        assert_eq!(parse_upload_reply(&bin).unwrap(), r);
        let b64 = B64.encode(&bin);
        assert_eq!(parse_upload_reply(b64.as_bytes()).unwrap(), r);
    }
}
