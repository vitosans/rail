//! `attachment.image`: one inline picture fetched into `images_dir()`, a 0700 directory of
//! 0600 files named by a hash of the account, message and Content-ID, so the same picture is
//! answered from disk the next time. When the store lists no such part, the message's body is
//! fetched again once (`inline_refresh`) and the part looked up again. Refused before any
//! picture fetch when the store still lists no such part, its type is not one the reader draws
//! or its listed size is over the cap; refused after it when the bytes are over the cap or lack
//! their type's signature, and nothing is written then. Each write is a temporary file renamed
//! into place, after a sweep of pictures older than `IMAGE_STALE_AFTER` and a trim of the oldest
//! ones that keeps the cache under `MAX_IMAGE_CACHE_BYTES`; a copy answered from disk has its age
//! started again first. Logs carry error kinds only.

use std::path::Path;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::attachments::{create_new_private, sweep_token, write_synced};
use super::inline_refresh::refresh_once;
use crate::auth::random_token;
use crate::daemon::DaemonContext;
use crate::daemon::registry::Call;
use crate::error::ErrorClass;
use crate::ops::{
    AttachmentImageRequest, AttachmentImageResponse, CONTENT_MISMATCH, ErrorCode,
    IMAGE_STALE_AFTER, MAX_IMAGE_BYTES, MAX_IMAGE_CACHE_BYTES, OpError, content_matches,
    image_extension,
};
use crate::paths::{Paths, create_private_dir};
use crate::provider::http::{RetryPolicy, with_retry_policy};
use crate::store::Store;
use crate::store::view::AttachmentView;
use crate::sync::manager::Reach;

pub(super) async fn image(
    call: Call,
    request: AttachmentImageRequest,
) -> Result<AttachmentImageResponse, OpError> {
    request.checked()?;
    let target = match call.ctx.sync.body_target(&request.account).await? {
        Reach::Ready(target) => target,
        Reach::Unavailable(error) => return Err(error),
    };
    // The writer opens (and migrates) the store; the handle is not kept.
    call.ctx.sync.writer(&call.ctx).await?;
    let part = match inline_part(&call.ctx, target.account_id, &request).await? {
        Some(part) => part,
        None => {
            refresh_once(&call.ctx, &target, &request.message_id).await?;
            inline_part(&call.ctx, target.account_id, &request)
                .await?
                .ok_or_else(no_such_part)?
        }
    };
    let extension = image_extension(&part.mime_type).ok_or_else(not_drawn)?;
    if part.size > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    let path = call.ctx.paths.images_dir().join(format!(
        "{}.{extension}",
        cache_key(target.account_id, &request)
    ));
    let response = |path: &Path| AttachmentImageResponse {
        path: path.display().to_string(),
        mime_type: part.mime_type.to_ascii_lowercase(),
    };
    let hit = path.clone();
    if call.ctx.blocking(move || Ok(cached(&hit))).await? {
        return Ok(response(&path));
    }
    // As an open's fetch: one short retry, because a person is waiting.
    let bytes = with_retry_policy(
        RetryPolicy::interactive(),
        target
            .provider
            .attachment(&request.message_id, &part.part_id),
    )
    .await?;
    if i64::try_from(bytes.len()).unwrap_or(i64::MAX) > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    if !content_matches(&part.mime_type, &bytes) {
        tracing::warn!("an inline picture's bytes did not match its type; not kept");
        return Err(OpError::new(
            ErrorCode::Unsupported,
            ErrorClass::Permanent,
            CONTENT_MISMATCH,
        ));
    }
    let (paths, target_path) = (call.ctx.paths.clone(), path.clone());
    call.ctx
        .blocking(move || write_cached(&paths, &target_path, &bytes))
        .await?;
    Ok(response(&path))
}

async fn inline_part(
    ctx: &DaemonContext,
    account_id: i64,
    request: &AttachmentImageRequest,
) -> Result<Option<AttachmentView>, OpError> {
    let db = ctx.paths.db_file();
    let (message, content_id) = (request.message_id.clone(), request.content_id.clone());
    ctx.blocking(move || {
        Ok(Store::open_reader(&db)?.inline_part(account_id, &message, &content_id)?)
    })
    .await
}

/// Never the Content-ID: it is the sender's text, and the front end already knows it.
fn no_such_part() -> OpError {
    OpError::not_found("this message has no such inline picture")
}

/// The hex SHA-256 of the account, message and Content-ID: a file name that says nothing of
/// the mail and is the same for the same picture.
fn cache_key(account_id: i64, request: &AttachmentImageRequest) -> String {
    let mut hash = Sha256::new();
    for part in [
        account_id.to_string().as_str(),
        &request.message_id,
        &request.content_id,
    ] {
        hash.update(part.len().to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Sweep and trim, then write `bytes` to a temporary 0600 file in `images_dir()` and rename it
/// to `path`, so a reader never sees half a picture.
pub(super) fn write_cached(paths: &Paths, path: &Path, bytes: &[u8]) -> Result<(), OpError> {
    let dir = paths.images_dir();
    create_private_dir(&dir).map_err(|error| io_error("creating the image cache", &error))?;
    if let Err(error) = sweep(&dir, SystemTime::now()) {
        tracing::warn!(kind = ?error.kind(), "the cached pictures were not swept");
    }
    let incoming = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if let Err(error) = trim(&dir, MAX_IMAGE_CACHE_BYTES, incoming) {
        tracing::warn!(kind = ?error.kind(), "the image cache was not trimmed");
    }
    let temporary = dir.join(format!(".{}.part", random_token()));
    let written = create_new_private(&temporary)
        .and_then(|mut file| write_synced(&mut file, bytes))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(io_error("writing a cached picture", &error));
    }
    Ok(())
}

/// Whether `path` is a cached picture, its modified time set to now when it is: the sweep
/// before another picture's write then leaves it for the reader the answer names, and a picture
/// in use is not fetched again each week. One that cannot be touched is fetched and written anew.
pub(super) fn cached(path: &Path) -> bool {
    let touched = std::fs::File::open(path).and_then(|file| {
        if !file.metadata()?.is_file() {
            return Ok(false);
        }
        file.set_modified(SystemTime::now()).map(|()| true)
    });
    match touched {
        Ok(hit) => hit,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "a cached picture's age was not refreshed");
            false
        }
    }
}

/// Remove every regular file directly in `dir` last changed before `now - IMAGE_STALE_AFTER`;
/// how many went. As the attachments sweep: a symlink or a directory is never followed or
/// removed, and an entry it cannot read or remove does not stop the rest, the first such
/// failure coming back at the end.
fn sweep(dir: &Path, now: SystemTime) -> std::io::Result<usize> {
    match now.checked_sub(IMAGE_STALE_AFTER) {
        Some(cutoff) => sweep_token(dir, cutoff),
        None => Ok(0),
    }
}

/// Remove the oldest regular files directly in `dir` until they and `incoming` more bytes fit in
/// `cap`; how many went. A symlink or a directory is never counted or removed, nor another
/// write's temporary `.part` file. An entry it cannot read or remove does not stop the rest,
/// the first such failure coming back at the end.
pub(super) fn trim(dir: &Path, cap: u64, incoming: u64) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut failed = None;
    let mut files = Vec::new();
    for entry in entries {
        match entry.and_then(|entry| cached_file(&entry)) {
            Ok(Some(file)) => files.push(file),
            Ok(None) => {}
            Err(error) => keep_first(&mut failed, error),
        }
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    let mut total = files.iter().map(|(_, len, _)| *len).sum::<u64>();
    let mut removed = 0;
    for (path, len, _) in files {
        if total.saturating_add(incoming) <= cap {
            break;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(error) => keep_first(&mut failed, error),
        }
        // A file another trim removed first is room made all the same.
        total = total.saturating_sub(len);
    }
    failed.map_or(Ok(removed), Err)
}

/// A cached picture's path, size and last change, or `None` for an entry that is not one.
fn cached_file(
    entry: &std::fs::DirEntry,
) -> std::io::Result<Option<(std::path::PathBuf, u64, SystemTime)>> {
    let in_flight = entry.file_name().to_string_lossy().ends_with(".part");
    if in_flight || !entry.file_type()?.is_file() {
        return Ok(None);
    }
    let metadata = entry.metadata()?;
    Ok(Some((entry.path(), metadata.len(), metadata.modified()?)))
}

/// Keep `error` as what the trim reports, unless the entry has gone (there is nothing left to
/// do about it) or a failure is already kept.
fn keep_first(failed: &mut Option<std::io::Error>, error: std::io::Error) {
    if error.kind() != std::io::ErrorKind::NotFound && failed.is_none() {
        *failed = Some(error);
    }
}

fn not_drawn() -> OpError {
    OpError::new(
        ErrorCode::Unsupported,
        ErrorClass::Permanent,
        "Rail does not draw this picture's type",
    )
}

fn too_large() -> OpError {
    OpError::new(
        ErrorCode::Unsupported,
        ErrorClass::Permanent,
        "this picture is larger than Rail draws",
    )
}

fn io_error(what: &str, error: &std::io::Error) -> OpError {
    OpError::io(ErrorCode::Internal, what, error)
}
