//! `format=full` messages as Rail bodies. The text part is chosen as a mail reader would (the
//! HTML of a `multipart/alternative`, the first readable part of a `multipart/mixed`, never a
//! forwarded `message/rfc822`). HTML becomes text through `html::html_render` into text with
//! numbered link markers, which fetches nothing and runs in bounded time; plain text is
//! decoded from its charset, and its bare addresses get markers too.
//! The text keeps its lines and tabs and loses every other control and format character. A
//! body over `MAX_BODY_BYTES` is cut and ends with `TRUNCATED_MARKER`. The topmost
//! `Authentication-Results`, when it is Gmail's own (`GMAIL_AUTHSERV_ID`), says which From
//! domains Gmail vouched for (`auth_results`).

use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use super::metadata::{cut, one_line_capped};
use super::wire::{AttachmentBody, Header, Message, Part};
use super::{COST_ATTACHMENT, COST_GET, GmailProvider, checked_id, safe_id};
use crate::config::model::is_format_char;
use crate::provider::auth_results::vouched_domains;
use crate::provider::html::html_render;
use crate::provider::limits::{MAX_ATTACHMENTS, MAX_FILENAME_CHARS};
use crate::provider::links::{Rendered, plain_links};
use crate::provider::types::content_id;
use crate::provider::{AttachmentMeta, Body, ProviderError};

/// The most body text (and HTML) Rail keeps for one message.
pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
/// The end of a body Rail cut at `MAX_BODY_BYTES`.
pub const TRUNCATED_MARKER: &str = "\n\n[Rail kept only the first 10 MiB of this message.]";
pub(super) const MAX_PART_DEPTH: usize = 32;
const MAX_MIME_CHARS: usize = 255;
const MAX_ATTACHMENT_ID: usize = 1024;
/// A part id is dot-separated numbers (`0`, `1.2`); each one at most this long.
const MAX_PART_SEGMENT: usize = 16;
const MAX_PART_ID: usize = 64;
const MAX_CHARSET_CHARS: usize = 40;
/// The base64 characters that hold `MAX_BODY_BYTES` (and a little more, so a cut is seen).
const MAX_BASE64_CHARS: usize = MAX_BODY_BYTES.div_ceil(3) * 4;

const URL_SAFE: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);
const STANDARD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Html,
    Plain,
}

/// The text part a body is made from.
pub(crate) struct Chosen {
    pub kind: Kind,
    pub charset: Option<String>,
    pub data: Option<String>,
    pub attachment_id: Option<String>,
}

#[derive(Default)]
pub(crate) struct Walked {
    pub chosen: Option<Chosen>,
    pub attachments: Vec<AttachmentMeta>,
}

fn unreadable() -> ProviderError {
    ProviderError::permanent("Rail could not read this message's body")
}

impl GmailProvider {
    /// Message `id`'s body. A body Rail cannot read is a permanent error, logged by id only, so
    /// the caller skips that one message and goes on.
    pub(crate) async fn full_body(&self, id: &str) -> Result<Body, ProviderError> {
        let result = self.read_body(id).await;
        if let Err(error @ ProviderError::Permanent { .. }) = &result {
            tracing::warn!(
                id = %checked_id(id).unwrap_or("<unsafe id>"),
                reason = %error,
                "skipped a message body Rail could not read"
            );
        }
        result
    }

    async fn read_body(&self, id: &str) -> Result<Body, ProviderError> {
        let id = checked_id(id).ok_or_else(unreadable)?;
        let query = [("format", "full".to_string())];
        let message: Message = self
            .api
            .get_json(&format!("messages/{id}"), &query, COST_GET)
            .await?;
        if message.id.as_deref() != Some(id) {
            return Err(unreadable());
        }
        let payload = message.payload.ok_or_else(unreadable)?;
        let vouched = vouched(&payload.headers);
        let walked = walk(&payload);
        let Some(chosen) = walked.chosen else {
            return Ok(Body {
                attachments: walked.attachments,
                vouched_domains: vouched,
                ..Body::default()
            });
        };
        let data = match (chosen.data, chosen.attachment_id) {
            (Some(data), _) => data,
            (None, Some(attachment)) => self.attachment_data(id, &attachment).await?,
            (None, None) => String::new(),
        };
        let (kind, charset, attachments) = (chosen.kind, chosen.charset, walked.attachments);
        let mut body = tokio::task::spawn_blocking(move || {
            let bytes = decode_base64(&data)?;
            render(kind, &bytes, charset.as_deref(), attachments)
        })
        .await
        .map_err(|_| unreadable())??;
        body.vouched_domains = vouched;
        Ok(body)
    }

    /// A body part Gmail sent separately (`messages.attachments.get`, read only).
    pub(super) async fn attachment_data(
        &self,
        id: &str,
        attachment: &str,
    ) -> Result<String, ProviderError> {
        if !safe_id(attachment, MAX_ATTACHMENT_ID) {
            return Err(unreadable());
        }
        let path = format!("messages/{id}/attachments/{attachment}");
        let body: AttachmentBody = self.api.get_json(&path, &[], COST_ATTACHMENT).await?;
        Ok(body.data.unwrap_or_default())
    }
}

/// The server name Gmail's own `Authentication-Results` start with.
pub const GMAIL_AUTHSERV_ID: &str = "mx.google.com";

/// The domains the topmost `Authentication-Results` vouches for, when it is Gmail's.
fn vouched(headers: &[Header]) -> Vec<String> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Authentication-Results"))
        .map(|h| vouched_domains(&h.value, Some(GMAIL_AUTHSERV_ID)))
        .unwrap_or_default()
}

/// The part the body is made from, and every attachment.
pub(crate) fn walk(payload: &Part) -> Walked {
    let mut attachments = Vec::new();
    collect_attachments(payload, &mut attachments, 0);
    let chosen = choose(payload, 0).map(|(part, kind)| Chosen {
        kind,
        charset: charset(part),
        data: part.body.data.clone(),
        attachment_id: part.body.attachment_id.clone(),
    });
    Walked {
        chosen,
        attachments,
    }
}

/// The text part a reader shows for `part`: a text leaf itself; the HTML (else the first
/// readable) alternative of a `multipart/alternative`; the first readable part of any other
/// multipart. Attachments and forwarded messages are never the body.
fn choose(part: &Part, depth: usize) -> Option<(&Part, Kind)> {
    if depth > MAX_PART_DEPTH || !part.filename.is_empty() {
        return None;
    }
    let mime = part.mime_type.to_ascii_lowercase();
    match mime.as_str() {
        "text/html" => Some((part, Kind::Html)),
        "text/plain" => Some((part, Kind::Plain)),
        "multipart/alternative" => {
            let mut options = part.parts.iter().filter_map(|p| choose(p, depth + 1));
            let first = options.next()?;
            if first.1 == Kind::Html {
                return Some(first);
            }
            Some(options.find(|c| c.1 == Kind::Html).unwrap_or(first))
        }
        m if m.starts_with("multipart/") => part.parts.iter().find_map(|p| choose(p, depth + 1)),
        _ => None,
    }
}

/// Every part with a file name, outside forwarded messages, at most `MAX_ATTACHMENTS`.
fn collect_attachments(part: &Part, attachments: &mut Vec<AttachmentMeta>, depth: usize) {
    if depth > MAX_PART_DEPTH || attachments.len() >= MAX_ATTACHMENTS {
        return;
    }
    let mime = part.mime_type.to_ascii_lowercase();
    if !part.filename.is_empty() {
        if part_id_ok(&part.part_id) {
            attachments.push(AttachmentMeta {
                part_id: part.part_id.clone(),
                filename: one_line_capped(&part.filename, MAX_FILENAME_CHARS),
                mime_type: one_line_capped(&mime, MAX_MIME_CHARS),
                size: part.body.size.max(0),
                plain_file: true,
                content_id: part
                    .headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("content-id"))
                    .and_then(|h| content_id(&h.value)),
            });
        }
        return;
    }
    if mime == "message/rfc822" {
        return;
    }
    for child in &part.parts {
        collect_attachments(child, attachments, depth + 1);
    }
}

/// A part id Rail keeps: empty (the message itself) or dot-separated safe segments.
fn part_id_ok(id: &str) -> bool {
    id.is_empty()
        || (id.len() <= MAX_PART_ID && id.split('.').all(|s| safe_id(s, MAX_PART_SEGMENT)))
}

/// The `charset` parameter of the part's `Content-Type`, read as a parameter (`; charset=x`),
/// not found as a substring.
fn charset(part: &Part) -> Option<String> {
    let header = part
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))?;
    header.value.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        if !key.trim().eq_ignore_ascii_case("charset") {
            return None;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value)
            .trim();
        let valid = !value.is_empty()
            && value.len() <= MAX_CHARSET_CHARS
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
        valid.then(|| value.to_string())
    })
}

/// Gmail's base64 (URL-safe, or standard when it holds `+` or `/`), decoding only the prefix
/// that holds `MAX_BODY_BYTES`: the rest would be cut anyway.
fn decode_base64(data: &str) -> Result<Vec<u8>, ProviderError> {
    let prefix = data
        .get(..data.len().min(MAX_BASE64_CHARS))
        .ok_or_else(unreadable)?;
    let engine = if prefix.bytes().any(|b| b == b'+' || b == b'/') {
        &STANDARD
    } else {
        &URL_SAFE
    };
    engine.decode(prefix.as_bytes()).map_err(|_| unreadable())
}

/// Decode `data` from `charset`, cut to `MAX_BODY_BYTES`, render HTML to text, and clean the
/// text of control and format characters.
pub(crate) fn render(
    kind: Kind,
    data: &[u8],
    charset: Option<&str>,
    attachments: Vec<AttachmentMeta>,
) -> Result<Body, ProviderError> {
    let decoded = to_text(&data[..data.len().min(MAX_BODY_BYTES)], charset);
    let kept = cut(&decoded, MAX_BODY_BYTES);
    let rendered = match kind {
        Kind::Html => {
            let html = html_render(kept);
            Rendered {
                text: clean(&html.text),
                links: html.links,
            }
        }
        Kind::Plain => plain_links(&clean(kept)),
    };
    let mut text = cut(&rendered.text, MAX_BODY_BYTES).to_string();
    let truncated = data.len() > MAX_BODY_BYTES
        || decoded.len() > kept.len()
        || rendered.text.len() > text.len();
    if truncated {
        text.push_str(TRUNCATED_MARKER);
    }
    Ok(Body {
        text,
        html: (kind == Kind::Html).then(|| kept.to_string()),
        truncated,
        attachments,
        attachments_unknown: false,
        links: rendered.links,
        vouched_domains: Vec::new(),
    })
}

fn to_text(bytes: &[u8], charset: Option<&str>) -> String {
    let label = charset.unwrap_or("utf-8").to_ascii_lowercase();
    if matches!(label.as_str(), "utf-8" | "utf8" | "us-ascii" | "ascii") {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    match mail_parser::decoders::charsets::map::charset_decoder(label.as_bytes()) {
        Some(decode) => decode(bytes),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `text` with lines (CRLF and a lone CR become LF) and tabs kept, and every other control
/// character (ESC, NUL, C1...) and format character (bidi controls, zero-width characters,
/// the byte-order mark) removed.
pub(crate) fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\n' | '\t' => out.push(c),
            c if c.is_control() || is_format_char(c) => {}
            c => out.push(c),
        }
    }
    out
}
