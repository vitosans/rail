//! IMAP message listing, envelope parsing, and body/attachment reading.

use std::collections::HashMap;

use async_imap::Session;
use async_imap::types::{Fetch, Flag};
use futures_util::StreamExt;
use mail_parser::{Address, HeaderValue, Message, MessageParser, MimeHeaders};

use super::client::{ImapStream, classify_imap_error};
use super::identity::{ImapMessageId, compute_thread_id};
use crate::provider::auth_results::vouched_domains;
use crate::provider::gmail::body::{MAX_BODY_BYTES, TRUNCATED_MARKER};
use crate::provider::html::html_render;
use crate::provider::limits::{MAX_ATTACHMENTS, MAX_FILENAME_CHARS};
use crate::provider::links::plain_links;
use crate::provider::types::content_id;
use crate::provider::{
    AttachmentMeta, Body, Envelope, EnvelopePage, IdPage, Mailbox, PageToken, ProviderError,
    RemoteId, Window,
};

pub const MAX_SUBJECT_CHARS: usize = 998;
pub const MAX_ADDRESSES: usize = 100;
pub const MAX_SNIPPET_CHARS: usize = 300;
const MAX_NAME_CHARS: usize = 200;
const MAX_ADDRESS_CHARS: usize = 254;
const MAX_ID_CHARS: usize = 998;
const MAX_REFERENCES: usize = 50;

/// Formats a Unix timestamp (seconds) into IMAP date format `DD-Mon-YYYY` (e.g. `01-Jan-2024`).
pub fn unix_to_imap_date(secs: i64) -> String {
    let days = (secs.div_euclid(86400)) + 719468;
    let era = (if days >= 0 { days } else { days - 146096 }) / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_str = months.get((m as usize).saturating_sub(1)).unwrap_or(&"Jan");
    format!("{d:02}-{month_str}-{y:04}")
}

/// Builds an IMAP search query string for the given receive-time window.
pub fn build_search_query(window: &Window) -> String {
    let mut criteria = Vec::new();
    if let Some(after) = window.after_ms {
        criteria.push(format!(
            "SINCE {}",
            unix_to_imap_date(after.div_euclid(1000))
        ));
    }
    if let Some(before) = window.before_ms {
        criteria.push(format!(
            "BEFORE {}",
            unix_to_imap_date(before.div_euclid(1000))
        ));
    }
    if criteria.is_empty() {
        "ALL".to_string()
    } else {
        criteria.join(" ")
    }
}

/// Parses an RFC 5322 header block into an Envelope.
#[allow(clippy::too_many_arguments)]
pub fn parse_envelope(
    mailbox: &str,
    uidvalidity: u32,
    uid: u32,
    internal_date_ms: i64,
    size: i64,
    is_read: bool,
    is_starred: bool,
    is_draft: bool,
    raw_header: &[u8],
) -> Option<Envelope> {
    let remote_id = ImapMessageId::new(mailbox, uidvalidity, uid).to_remote_id();
    let parsed = MessageParser::default().parse(raw_header);
    let msg = parsed.as_ref();

    let msg_id = msg.and_then(|p| p.message_id()).map(capped_id);
    let in_reply_to = msg.and_then(|p| texts(p.in_reply_to()).into_iter().next());
    let references = msg.map(|p| texts(p.references())).unwrap_or_default();
    let thread_id = compute_thread_id(
        msg_id.as_deref().unwrap_or(remote_id.as_str()),
        in_reply_to.as_deref(),
        &references,
    );

    let subject = msg
        .and_then(|p| p.subject())
        .map(|s| one_line_capped(s, MAX_SUBJECT_CHARS))
        .unwrap_or_default();
    let date = msg
        .and_then(|p| p.date())
        .map(|d| d.to_timestamp())
        .unwrap_or_else(|| internal_date_ms.div_euclid(1000));

    Some(Envelope {
        remote_id,
        thread_id,
        message_id: msg_id,
        in_reply_to,
        references,
        subject: subject.clone(),
        from: msg.map(|p| mailboxes(p.from())).unwrap_or_default(),
        to: msg.map(|p| mailboxes(p.to())).unwrap_or_default(),
        cc: msg.map(|p| mailboxes(p.cc())).unwrap_or_default(),
        reply_to: msg.map(|p| mailboxes(p.reply_to())).unwrap_or_default(),
        bcc: msg.map(|p| mailboxes(p.bcc())).unwrap_or_default(),
        date,
        internal_date: internal_date_ms,
        snippet: subject.chars().take(MAX_SNIPPET_CHARS).collect(),
        folders: vec![mailbox.to_string()],
        is_read,
        is_starred,
        is_draft,
        size: size.max(0),
    })
}

/// Parses an RFC 822 / MIME raw message into Rail's Body structure.
pub fn parse_body(raw_bytes: &[u8]) -> Result<Body, ProviderError> {
    let parsed = MessageParser::default()
        .parse(raw_bytes)
        .ok_or_else(|| ProviderError::permanent("failed to parse message structure"))?;

    let (text, html, links) = render_body(&parsed);
    let truncated = text.len() > MAX_BODY_BYTES;
    let text = if truncated {
        format!("{}{}", cut(&text, MAX_BODY_BYTES), TRUNCATED_MARKER)
    } else {
        text
    };

    Ok(Body {
        text,
        html,
        truncated,
        attachments: attachments_of(&parsed),
        attachments_unknown: false,
        links,
        vouched_domains: vouched_domains_of(&parsed),
    })
}

/// The readable text, the HTML source and its links, preferring HTML when the message has it.
fn render_body(parsed: &Message<'_>) -> (String, Option<String>, Vec<String>) {
    let has_html = parsed
        .parts
        .iter()
        .any(|p| p.is_content_type("text", "html"));
    if has_html {
        if let Some(html) = parsed.body_html(0) {
            let rendered = html_render(&html);
            return (rendered.text, Some(html.to_string()), rendered.links);
        }
    } else if let Some(plain) = parsed.body_text(0) {
        let rendered = plain_links(&plain);
        return (rendered.text, None, rendered.links);
    }
    (String::new(), None, Vec::new())
}

fn attachments_of(parsed: &Message<'_>) -> Vec<AttachmentMeta> {
    let mut attachments = Vec::new();
    for (idx, att) in parsed.attachments().take(MAX_ATTACHMENTS).enumerate() {
        let filename = att
            .attachment_name()
            .map(|n| one_line_capped(n, MAX_FILENAME_CHARS))
            .unwrap_or_else(|| format!("attachment_{idx}"));
        let mime_type = att
            .content_type()
            .map(|ct| {
                let sub = ct.c_subtype.as_deref().unwrap_or("octet-stream");
                format!("{}/{}", ct.c_type, sub)
            })
            .unwrap_or_else(|| "application/octet-stream".to_string());
        attachments.push(AttachmentMeta {
            part_id: idx.to_string(),
            filename,
            mime_type,
            size: att.contents().len() as i64,
            plain_file: true,
            content_id: att.content_id().and_then(content_id),
        });
    }
    attachments
}

fn vouched_domains_of(parsed: &Message<'_>) -> Vec<String> {
    parsed
        .header("Authentication-Results")
        .and_then(|h| h.as_text())
        .map(|val| vouched_domains(val, None))
        .unwrap_or_default()
}

/// Extracts raw bytes of a message attachment by its part index or filename.
pub fn extract_attachment(raw_bytes: &[u8], part: &str) -> Result<Vec<u8>, ProviderError> {
    let parsed = MessageParser::default()
        .parse(raw_bytes)
        .ok_or_else(|| ProviderError::permanent("failed to parse message attachments"))?;

    if let Ok(idx) = part.parse::<usize>()
        && let Some(att) = parsed.attachments().nth(idx)
    {
        return Ok(att.contents().to_vec());
    }

    for att in parsed.attachments() {
        if att.attachment_name() == Some(part) || att.content_id() == Some(part) {
            return Ok(att.contents().to_vec());
        }
    }

    Err(ProviderError::NotFound)
}

/// Lists message IDs in a mailbox within a receive-time window, returning a paginated `IdPage`.
pub async fn list_messages(
    session: &mut Session<ImapStream>,
    mailbox: &str,
    uidvalidity: u32,
    window: &Window,
    page: Option<&PageToken>,
    page_size: usize,
) -> Result<IdPage, ProviderError> {
    let examine_status = session
        .examine(mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    let current_uidvalidity = examine_status.uid_validity.unwrap_or(uidvalidity);

    let query = build_search_query(window);
    let uids_set = session
        .uid_search(&query)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut uids: Vec<u32> = uids_set.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));

    let offset = match page {
        Some(token) => token
            .0
            .parse::<usize>()
            .map_err(|_| ProviderError::PageTokenRejected)?,
        None => 0,
    };

    if offset >= uids.len() {
        return Ok(IdPage {
            ids: Vec::new(),
            next: None,
        });
    }

    let end = (offset + page_size).min(uids.len());
    let page_uids = &uids[offset..end];
    let ids: Vec<RemoteId> = page_uids
        .iter()
        .map(|uid| ImapMessageId::new(mailbox, current_uidvalidity, *uid).to_remote_id())
        .collect();

    let next = if end < uids.len() {
        Some(PageToken(end.to_string()))
    } else {
        None
    };

    Ok(IdPage { ids, next })
}

/// Fetches envelopes for remote message IDs grouped by mailbox.
pub async fn fetch_envelopes(
    session: &mut Session<ImapStream>,
    ids: &[RemoteId],
) -> Result<EnvelopePage, ProviderError> {
    let mut by_mailbox: HashMap<(String, u32), Vec<u32>> = HashMap::new();
    let mut skipped = Vec::new();

    for id in ids {
        match ImapMessageId::parse(id.as_str()) {
            Some(parsed) => {
                by_mailbox
                    .entry((parsed.mailbox, parsed.uidvalidity))
                    .or_default()
                    .push(parsed.uid);
            }
            None => skipped.push(id.clone()),
        }
    }

    let mut envelopes = Vec::new();
    for ((mailbox, uidvalidity), mut uids) in by_mailbox {
        uids.sort_unstable();
        uids.dedup();
        envelopes.extend(fetch_mailbox_envelopes(session, &mailbox, uidvalidity, &uids).await);
    }

    Ok(EnvelopePage {
        envelopes,
        skipped,
        next: None,
    })
}

/// Fetches and parses the envelopes of `uids` in one mailbox. A mailbox that cannot be
/// examined or fetched is logged and skipped, so one bad folder does not lose the rest.
async fn fetch_mailbox_envelopes(
    session: &mut Session<ImapStream>,
    mailbox: &str,
    uidvalidity: u32,
    uids: &[u32],
) -> Vec<Envelope> {
    if let Err(e) = session.examine(mailbox).await {
        tracing::warn!(mailbox = %mailbox, error = %e, "failed to examine mailbox for envelopes");
        return Vec::new();
    }

    let uids_str = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut stream = match session
        .uid_fetch(
            &uids_str,
            "(UID INTERNALDATE RFC822.SIZE FLAGS BODY.PEEK[HEADER])",
        )
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(mailbox = %mailbox, error = %e, "failed to fetch envelopes");
            return Vec::new();
        }
    };

    let mut envelopes = Vec::new();
    while let Some(fetch_res) = stream.next().await {
        let fetch = match fetch_res {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "error reading fetch envelope record");
                continue;
            }
        };
        if let Some(env) = envelope_from_fetch(mailbox, uidvalidity, &fetch) {
            envelopes.push(env);
        }
    }
    envelopes
}

/// Builds an `Envelope` from one IMAP `FETCH` record.
fn envelope_from_fetch(mailbox: &str, uidvalidity: u32, fetch: &Fetch) -> Option<Envelope> {
    let uid = fetch.uid?;
    let internal_date_ms = fetch
        .internal_date()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0);
    let size = fetch.size.unwrap_or(0) as i64;
    let mut is_read = false;
    let mut is_starred = false;
    let mut is_draft = false;
    for flag in fetch.flags() {
        match flag {
            Flag::Seen => is_read = true,
            Flag::Flagged => is_starred = true,
            Flag::Draft => is_draft = true,
            _ => {}
        }
    }
    parse_envelope(
        mailbox,
        uidvalidity,
        uid,
        internal_date_ms,
        size,
        is_read,
        is_starred,
        is_draft,
        fetch.header().unwrap_or_default(),
    )
}

/// Fetches the full message body for a given remote ID.
pub async fn fetch_body(
    session: &mut Session<ImapStream>,
    id: &RemoteId,
) -> Result<Body, ProviderError> {
    let parsed_id = ImapMessageId::parse(id.as_str())
        .ok_or_else(|| ProviderError::permanent("invalid IMAP message ID"))?;

    session
        .examine(&parsed_id.mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut stream = session
        .uid_fetch(parsed_id.uid.to_string(), "(BODY.PEEK[])")
        .await
        .map_err(|e| classify_imap_error(&e))?;

    while let Some(fetch_res) = stream.next().await {
        let fetch = fetch_res.map_err(|e| classify_imap_error(&e))?;
        if fetch.uid == Some(parsed_id.uid) {
            let body_bytes = fetch.body().unwrap_or_default();
            return parse_body(body_bytes);
        }
    }

    Err(ProviderError::NotFound)
}

/// Fetches raw attachment bytes for a message ID and part identifier.
pub async fn fetch_attachment(
    session: &mut Session<ImapStream>,
    id: &RemoteId,
    part: &str,
) -> Result<Vec<u8>, ProviderError> {
    let parsed_id = ImapMessageId::parse(id.as_str())
        .ok_or_else(|| ProviderError::permanent("invalid IMAP message ID"))?;

    session
        .examine(&parsed_id.mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut stream = session
        .uid_fetch(parsed_id.uid.to_string(), "(BODY.PEEK[])")
        .await
        .map_err(|e| classify_imap_error(&e))?;

    while let Some(fetch_res) = stream.next().await {
        let fetch = fetch_res.map_err(|e| classify_imap_error(&e))?;
        if fetch.uid == Some(parsed_id.uid) {
            let body_bytes = fetch.body().unwrap_or_default();
            return extract_attachment(body_bytes, part);
        }
    }

    Err(ProviderError::NotFound)
}

/// Performs an IMAP search on the given mailbox matching text query terms.
pub async fn search_messages(
    session: &mut Session<ImapStream>,
    mailbox: &str,
    uidvalidity: u32,
    query: &str,
    limit: usize,
) -> Result<Vec<RemoteId>, ProviderError> {
    session
        .examine(mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let sanitized = query.replace(['"', '\r', '\n'], "");
    let criteria = format!("TEXT \"{sanitized}\"");
    let uids_set = session
        .uid_search(&criteria)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut uids: Vec<u32> = uids_set.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));

    let ids = uids
        .into_iter()
        .take(limit)
        .map(|uid| ImapMessageId::new(mailbox, uidvalidity, uid).to_remote_id())
        .collect();

    Ok(ids)
}

fn mailboxes(address: Option<&Address<'_>>) -> Vec<Mailbox> {
    let Some(address) = address else {
        return Vec::new();
    };
    address
        .iter()
        .filter_map(|a| {
            let addr = a.address()?;
            Some(Mailbox {
                name: a.name().map(|n| one_line_capped(n, MAX_NAME_CHARS)),
                address: one_line_capped(addr, MAX_ADDRESS_CHARS),
            })
        })
        .take(MAX_ADDRESSES)
        .collect()
}

fn texts(value: &HeaderValue<'_>) -> Vec<String> {
    let all: Vec<String> = match (value.as_text_list(), value.as_text()) {
        (Some(list), _) => list.iter().map(|s| s.to_string()).collect(),
        (None, Some(one)) => vec![one.to_string()],
        (None, None) => Vec::new(),
    };
    all.into_iter()
        .take(MAX_REFERENCES)
        .map(|id| capped_id(&id))
        .collect()
}

fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn one_line_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().map(|c| if c.is_control() { ' ' } else { c })
}

fn one_line_capped(text: &str, max: usize) -> String {
    one_line_chars(text).take(max).collect()
}

fn capped_id(id: &str) -> String {
    id.chars().take(MAX_ID_CHARS).collect()
}
