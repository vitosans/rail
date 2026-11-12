//! Graph's JSON as Rail reads it. A field Graph may leave out is optional.

use serde::Deserialize;
use serde_json::Value;

/// One page of a collection, and where the next page is.
#[derive(Deserialize)]
pub(crate) struct Page<T> {
    #[serde(default = "Vec::new")]
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

/// A `mailFolder` (or a `mailSearchFolder`, told apart by its type).
#[derive(Deserialize)]
pub(crate) struct FolderWire {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "childFolderCount")]
    pub child_folder_count: Option<i64>,
    #[serde(rename = "@odata.type")]
    pub odata_type: Option<String>,
}

/// An answer of which Rail reads only the id.
#[derive(Deserialize)]
pub(crate) struct IdOnly {
    pub id: String,
}

/// A `message`, from a delta page or a single get. Every field may be missing from an update.
#[derive(Deserialize, Default)]
pub(crate) struct MessageWire {
    pub id: Option<String>,
    #[serde(rename = "conversationId")]
    pub conversation_id: Option<String>,
    #[serde(rename = "internetMessageId")]
    pub internet_message_id: Option<String>,
    pub subject: Option<String>,
    pub from: Option<Recipient>,
    pub sender: Option<Recipient>,
    #[serde(rename = "toRecipients")]
    pub to: Option<Vec<Recipient>>,
    #[serde(rename = "ccRecipients")]
    pub cc: Option<Vec<Recipient>>,
    #[serde(rename = "bccRecipients")]
    pub bcc: Option<Vec<Recipient>>,
    #[serde(rename = "replyTo")]
    pub reply_to: Option<Vec<Recipient>>,
    #[serde(rename = "receivedDateTime")]
    pub received: Option<String>,
    #[serde(rename = "sentDateTime")]
    pub sent: Option<String>,
    #[serde(rename = "isRead")]
    pub is_read: Option<bool>,
    #[serde(rename = "isDraft")]
    pub is_draft: Option<bool>,
    pub flag: Option<Flag>,
    #[serde(rename = "bodyPreview")]
    pub body_preview: Option<String>,
    #[serde(rename = "parentFolderId")]
    pub parent_folder_id: Option<String>,
    /// Set on a delta item gone from the folder: deleted, or moved out.
    #[serde(rename = "@removed")]
    pub removed: Option<Value>,
}

#[derive(Deserialize)]
pub(crate) struct Recipient {
    #[serde(rename = "emailAddress")]
    pub email_address: Option<EmailAddress>,
}

#[derive(Deserialize)]
pub(crate) struct EmailAddress {
    pub name: Option<String>,
    pub address: Option<String>,
}

/// Outlook's follow-up flag: Rail's star.
#[derive(Deserialize)]
pub(crate) struct Flag {
    #[serde(rename = "flagStatus")]
    pub flag_status: Option<String>,
}

/// The answer to a `$batch` post.
#[derive(Deserialize)]
pub(crate) struct BatchAnswers {
    #[serde(default = "Vec::new")]
    pub responses: Vec<BatchItem>,
}

#[derive(Deserialize)]
pub(crate) struct BatchItem {
    pub id: String,
    pub status: u16,
    /// The item's own headers (a throttled one's `Retry-After`); values are strings.
    #[serde(default)]
    pub headers: serde_json::Map<String, Value>,
    #[serde(default)]
    pub body: Option<Value>,
}

/// A message's body, as `$select=` `BODY_FIELDS` answers it.
#[derive(Deserialize)]
pub(crate) struct BodyWire {
    pub id: Option<String>,
    pub body: Option<ItemBody>,
    #[serde(rename = "hasAttachments")]
    pub has_attachments: Option<bool>,
    #[serde(rename = "internetMessageHeaders")]
    pub internet_message_headers: Option<Vec<HeaderWire>>,
    pub from: Option<Recipient>,
}

/// One of `internetMessageHeaders`, in the order the message carries them.
#[derive(Deserialize)]
pub(crate) struct HeaderWire {
    pub name: String,
    pub value: String,
}

#[derive(Deserialize, Default)]
pub(crate) struct ItemBody {
    #[serde(rename = "contentType")]
    pub content_type: Option<String>,
    pub content: Option<String>,
}

/// One attachment of the listing (file, item or reference).
#[derive(Deserialize)]
pub(crate) struct AttachmentWire {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "contentType")]
    pub content_type: Option<String>,
    pub size: Option<i64>,
    #[serde(rename = "isInline", default)]
    pub is_inline: bool,
    #[serde(rename = "@odata.type")]
    pub odata_type: Option<String>,
    #[serde(rename = "contentId", default)]
    pub content_id: Option<String>,
}
