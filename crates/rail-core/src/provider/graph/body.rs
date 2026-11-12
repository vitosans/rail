//! One message's body: `GET me/messages/{id}?$select=` `BODY_FIELDS` with
//! `Prefer: outlook.body-content-type="html"`, rendered as Gmail's are (`gmail::body::render`:
//! HTML through `provider::html`, text as it is, both cut at `MAX_BODY_BYTES`). With
//! `hasAttachments`, the attachment list comes with it, inline ones included, as Gmail lists
//! every named part; each says whether it is a plain file (`reconcile::plain_file`), the only
//! kind a forward carries and an import fetches, and its Content-ID. Graph says
//! `hasAttachments` is false when every attachment is inline: the list is then read only when
//! the HTML names a `cid:` image (in any case). A list Graph refuses leaves the body readable
//! and says its attachments are unknown, so the ones already stored are kept. The topmost
//! `Authentication-Results` of `internetMessageHeaders`, the one Exchange Online adds, says
//! which From domains it vouched for (`auth_results`). Mail a colleague sent inside the tenant
//! has none that pass, since Exchange signs with DKIM only on the way out, so Exchange's own
//! stamp that the sender signed in to the organisation (`INTERNAL_STAMPS`) vouches for the From
//! domain instead, and only when it is the account's own domain: an internal sender may still
//! write any From line. Exchange strips `X-MS-Exchange-Organization-*` from mail that arrives from
//! outside, and each stamp must appear exactly once, so a sender cannot add its own.

use super::reconcile::plain_file;
use super::wire::{AttachmentWire, BodyWire, HeaderWire, Page, Recipient};
use super::{COST_REQUEST, GraphProvider, graph_id};
use crate::provider::auth_results::{MAX_VOUCHED, address_domain, vouched_domains};
use crate::provider::gmail::body::{Kind, render};
use crate::provider::gmail::metadata::one_line_capped;
use crate::provider::limits::{MAX_ATTACHMENTS, MAX_FILENAME_CHARS};
use crate::provider::types::content_id;
use crate::provider::{AttachmentMeta, Body, ProviderError};

/// What a body fetch selects.
pub const BODY_FIELDS: &str = "id,body,hasAttachments,internetMessageHeaders,from";

/// The headers Exchange Online stamps on mail whose sender signed in to the organisation, each
/// of which must say `Internal`, once.
const INTERNAL_STAMPS: [&str; 2] = [
    "X-MS-Exchange-Organization-AuthAs",
    "X-MS-Exchange-CrossTenant-AuthAs",
];

/// The preference that asks for the HTML body.
pub const HTML_BODY: &str = "outlook.body-content-type=\"html\"";
const MAX_MIME_CHARS: usize = 255;

fn unreadable() -> ProviderError {
    ProviderError::permanent("Rail could not read this message's body")
}

fn meta(wire: AttachmentWire) -> AttachmentMeta {
    let mime = wire
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream")
        .to_ascii_lowercase();
    AttachmentMeta {
        filename: one_line_capped(
            wire.name.as_deref().unwrap_or("attachment"),
            MAX_FILENAME_CHARS,
        ),
        mime_type: one_line_capped(&mime, MAX_MIME_CHARS),
        size: wire.size.unwrap_or(0).max(0),
        plain_file: plain_file(wire.odata_type.as_deref(), wire.is_inline),
        content_id: wire.content_id.as_deref().and_then(content_id),
        part_id: wire.id,
    }
}

/// The domains the topmost `Authentication-Results` vouches for, and `own_domain` when Exchange
/// stamped the sender as signed in to the organisation and the From line is at it. Exchange Online's header names
/// `mx.microsoft.com`, but other Microsoft clouds may name another server, so none is asked
/// for: the topmost header is the receiving server's either way.
fn vouched(
    headers: &[HeaderWire],
    from: Option<&Recipient>,
    own_domain: Option<&str>,
) -> Vec<String> {
    let mut domains = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Authentication-Results"))
        .map(|h| vouched_domains(&h.value, None))
        .unwrap_or_default();
    if let Some(domain) = internal_sender_domain(headers, from)
        && own_domain == Some(domain.as_str())
        && !domains.contains(&domain)
        && domains.len() < MAX_VOUCHED
    {
        domains.push(domain);
    }
    domains
}

/// The From domain of mail Exchange stamped as sent by someone signed in to the organisation:
/// every one of `INTERNAL_STAMPS` there exactly once and saying `Internal`.
fn internal_sender_domain(headers: &[HeaderWire], from: Option<&Recipient>) -> Option<String> {
    let stamped = INTERNAL_STAMPS.iter().all(|stamp| {
        let mut values = headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(stamp));
        let once = values.next().filter(|_| values.next().is_none());
        once.is_some_and(|h| h.value.trim().eq_ignore_ascii_case("Internal"))
    });
    if !stamped {
        return None;
    }
    let address = from?.email_address.as_ref()?.address.as_deref()?;
    address_domain(address)
}

/// Whether HTML names a `cid:` image, in any case (`CID:` is the same scheme).
fn names_cid(html: &str) -> bool {
    html.as_bytes()
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"cid:"))
}

impl GraphProvider {
    pub(crate) async fn read_body(&self, id: &str) -> Result<Body, ProviderError> {
        if !graph_id(id) {
            return Err(unreadable());
        }
        let query = [("$select", BODY_FIELDS.to_string())];
        let path = format!("me/messages/{id}");
        let wire: BodyWire = self
            .api
            .get_json_with(&path, &query, COST_REQUEST, &[HTML_BODY])
            .await?;
        if wire.id.as_deref() != Some(id) {
            return Err(unreadable());
        }
        let vouched = vouched(
            wire.internet_message_headers.as_deref().unwrap_or_default(),
            wire.from.as_ref(),
            self.own_domain.as_deref(),
        );
        let body = wire.body.unwrap_or_default();
        let kind = match body.content_type.as_deref() {
            Some(text) if text.eq_ignore_ascii_case("text") => Kind::Plain,
            _ => Kind::Html,
        };
        let content = body.content.unwrap_or_default();
        // Graph says `hasAttachments` is false when every attachment is inline, so HTML that
        // names a `cid:` image is listed anyway.
        let wanted =
            wire.has_attachments == Some(true) || (kind == Kind::Html && names_cid(&content));
        let listed = if wanted {
            self.listed_if_allowed(id).await?
        } else {
            Some(Vec::new())
        };
        let unknown = listed.is_none();
        let attachments = listed.unwrap_or_default();
        let mut body = tokio::task::spawn_blocking(move || {
            render(kind, content.as_bytes(), None, attachments)
        })
        .await
        .map_err(|_| unreadable())??;
        body.attachments_unknown = unknown;
        body.vouched_domains = vouched;
        Ok(body)
    }

    /// The attachment list, or `None` when Graph refuses it. The body is readable without it:
    /// a refused listing must not cost it, nor the attachments already stored. Anything a
    /// retry may fix (or a login) still fails the fetch, to be tried again.
    async fn listed_if_allowed(
        &self,
        id: &str,
    ) -> Result<Option<Vec<AttachmentMeta>>, ProviderError> {
        match self.attachment_list(id).await {
            Err(error @ ProviderError::Permanent { .. }) => {
                tracing::warn!(class = ?error.class(), detail = ?error.detail(), "the attachment list was refused; the body is kept without it");
                Ok(None)
            }
            listed => listed.map(Some),
        }
    }

    async fn attachment_list(&self, id: &str) -> Result<Vec<AttachmentMeta>, ProviderError> {
        let query = [
            // `contentId` is a file attachment's own property: Graph refuses it on the base
            // attachment type (HTTP 400), so it is asked for through a type cast.
            (
                "$select",
                "id,name,contentType,size,isInline,microsoft.graph.fileAttachment/contentId"
                    .to_string(),
            ),
            ("$top", MAX_ATTACHMENTS.to_string()),
        ];
        let path = format!("me/messages/{id}/attachments");
        let page: Page<AttachmentWire> = self.api.get_json(&path, &query, COST_REQUEST).await?;
        Ok(page
            .value
            .into_iter()
            .filter(|a| graph_id(&a.id))
            .take(MAX_ATTACHMENTS)
            .map(meta)
            .collect())
    }
}
