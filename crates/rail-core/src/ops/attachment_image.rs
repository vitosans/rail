//! `attachment.image`: one inline picture of a message, the part its HTML names with
//! `cid:<content-id>`, written into Rail's image cache for the reader to draw. As
//! `attachment.open` it answers a **path**, never bytes; unlike it, the copy is kept and
//! answered again for the same image, so a thread read twice fetches nothing twice. Only
//! types the reader can decode, at most `MAX_IMAGE_BYTES`, and only bytes that carry their
//! type's signature. Terminal-only. Nothing here prints an id, a type or a path.

use std::fmt;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::attachments::checked_message_id;
use super::error::OpError;
use crate::provider::types::{MAX_CONTENT_ID_BYTES, content_id};

/// The largest picture Rail fetches for the reader, in bytes.
pub const MAX_IMAGE_BYTES: i64 = 5 * 1024 * 1024;

/// How long a cached picture is kept after it was written.
pub const IMAGE_STALE_AFTER: Duration = Duration::from_secs(7 * 86_400);

/// The most the image cache holds, in bytes: the oldest pictures go to make room for a new
/// one. Well over one open thread's worth (32 pictures of `MAX_IMAGE_BYTES`), so a thread's
/// own pictures are not evicted while the reader draws them.
pub const MAX_IMAGE_CACHE_BYTES: u64 = 512 * 1024 * 1024;

/// The types the reader draws and the extension each cached copy gets. Each is in `OPENABLE`
/// too, so `content_matches` checks its signature. SVG is absent on purpose: it is a program,
/// not a picture.
pub const IMAGE_TYPES: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

/// The extension a cached copy of `mime_type` gets; `None` for a type the reader does not draw.
#[must_use]
pub fn image_extension(mime_type: &str) -> Option<&'static str> {
    IMAGE_TYPES
        .iter()
        .find(|(mime, _)| mime.eq_ignore_ascii_case(mime_type.trim()))
        .map(|(_, extension)| *extension)
}

/// `attachment.image`: the inline part of `message_id` whose Content-ID is `content_id`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentImageRequest {
    /// An account's name or address.
    pub account: String,
    /// The provider's message id.
    pub message_id: String,
    /// The Content-ID a `cid:` address names, without angle brackets.
    pub content_id: String,
}

/// Where the cached copy is, and its type.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AttachmentImageResponse {
    /// A 0600 file in Rail's 0700 image cache, named after a hash of the request.
    pub path: String,
    /// One of `IMAGE_TYPES`.
    pub mime_type: String,
}

impl AttachmentImageRequest {
    /// Check the message id and the Content-ID.
    ///
    /// # Errors
    /// `InvalidInput`/`Permanent`, quoting neither id.
    pub fn checked(&self) -> Result<(), OpError> {
        checked_message_id(&self.message_id)?;
        if content_id(&self.content_id).as_deref() != Some(self.content_id.as_str()) {
            return Err(OpError::invalid_input(format!(
                "content_id: 1 to {MAX_CONTENT_ID_BYTES} bytes of printable ASCII, no spaces \
                 and no angle brackets"
            )));
        }
        Ok(())
    }
}

/// Nothing: a request names an account, a message and a part, all of which are mail.
impl fmt::Debug for AttachmentImageRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentImageRequest")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AttachmentImageResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentImageResponse")
            .finish_non_exhaustive()
    }
}
