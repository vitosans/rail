//! `draft.push` and `draft.import`: a draft saved to Gmail's Drafts after the undo grace, and
//! a Gmail draft made into a local one that keeps its Gmail ids and owns its attachments as
//! files (`draft_files`). Front-end operations only: no MCP tool reaches them. Logs carry ids
//! and counts, never an address, a subject, a body or a file name.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::draft_files::{IMPORTS, fetch_into, import_dir, remove_dirs};
use super::drafts::{
    being_sent, checked_client_token, checked_id, draft_and_address, header_id, no_such_draft,
    not_queued, row_of_draft, saved_id, token_used, unexpected,
};
use crate::auth::unix_now;
use crate::config::AccountKind;
use crate::daemon::DaemonContext;
use crate::daemon::registry::Call;
use crate::error::ErrorClass;
use crate::ops::{
    Action, BodyFetchRequest, BodyOutcome, DraftId, DraftImportRequest, DraftImportResponse,
    DraftPushRequest, DraftPushResponse, ErrorCode, OpError, UNDO_GRACE, new_message_id,
};
use crate::provider::{Provider, ProviderError};
use crate::store::Store;
use crate::store::drafts::{Attachment, Draft, DraftKind, DraftState, DraftWrite, ReplyAttachment};
use crate::store::drafts_folder::ImportSource;
use crate::store::outbox::{NewAction, OutboxOutcome, OutboxWrite};
use crate::sync::Wants;
use crate::sync::batch::Batch;
use crate::sync::manager::{Reach, unobserved};

fn no_such_message() -> OpError {
    OpError::not_found("message: no such message")
}

/// Nothing a Gmail draft would hold: no recipient, subject, text or attachment.
fn is_empty(draft: &Draft) -> bool {
    draft.to.is_empty()
        && draft.cc.is_empty()
        && draft.bcc.is_empty()
        && draft.subject.is_empty()
        && draft.body.trim().is_empty()
        && draft.attachments.is_empty()
}

fn check_pushable(draft: &Draft) -> Result<(), OpError> {
    if draft.state == DraftState::Queued {
        return Err(being_sent());
    }
    if is_empty(draft) {
        return Err(OpError::invalid_input("draft: there is nothing to save"));
    }
    Ok(())
}

/// The draft's account, checked as a send checks it, and the draft pushable.
async fn push_account(call: &Call, id: DraftId) -> Result<(i64, String), OpError> {
    let db = call.ctx.paths.db_file();
    let (draft, address) = call
        .ctx
        .blocking(move || draft_and_address(&db, id))
        .await?;
    let target = call
        .ctx
        .sync
        .action_target(&call.ctx, &address, Wants::Compose)
        .await?;
    if target.account_id != draft.account_id {
        return Err(no_such_draft());
    }
    check_pushable(&draft)?;
    Ok((target.account_id, target.name))
}

pub(super) async fn push(
    call: Call,
    request: DraftPushRequest,
) -> Result<DraftPushResponse, OpError> {
    checked_client_token(&request.client_token)?;
    checked_id(request.draft)?;
    let id = request.draft;
    let (account_id, name) = push_account(&call, id).await?;
    let grace = i64::try_from(UNDO_GRACE.as_secs()).unwrap_or(10);
    let new = NewAction {
        account_id,
        action: Action::PushDraft { draft: id },
        threads: Vec::new(),
        not_before: unix_now() + grace,
        client_token: request.client_token,
    };
    let outcome = call
        .ctx
        .sync
        .writer(&call.ctx)
        .await?
        .outbox(OutboxWrite::Enqueue(new))
        .await?;
    let OutboxOutcome::Enqueued { rows, .. } = outcome else {
        return Err(unexpected());
    };
    let db = call.ctx.paths.db_file();
    let reader = db.clone();
    let Some(&row) = rows.first() else {
        return Err(call.ctx.blocking(move || not_queued(&reader, id)).await?);
    };
    call.ctx
        .blocking(move || row_of_draft(&db, row, id))
        .await?;
    call.ctx.sync.outbox_changed(&call.ctx).await;
    tracing::info!(account = %name, draft = id, row, "a draft push was queued");
    Ok(DraftPushResponse { row })
}

fn read_source(db: &Path, message: i64) -> Result<Option<ImportSource>, OpError> {
    Ok(Store::open_reader(db)?.import_source(message)?)
}

/// What an earlier call left for this import: the draft that already holds the Gmail
/// message `remote` (a replay), or a refusal when `token` made another draft.
fn earlier_import(
    db: &Path,
    account_id: i64,
    remote: &str,
    token: &str,
) -> Result<Option<DraftId>, OpError> {
    let store = Store::open_reader(db)?;
    if let Some(draft) = store.draft_by_remote_message(account_id, remote)? {
        return Ok(Some(draft));
    }
    if store.draft_token_owner(token)?.is_some() {
        return Err(token_used());
    }
    Ok(None)
}

/// The source of `message`: this account's, and a draft.
async fn import_source(
    ctx: &DaemonContext,
    message: i64,
    account_id: i64,
) -> Result<ImportSource, OpError> {
    let db = ctx.paths.db_file();
    let source = ctx
        .blocking(move || read_source(&db, message))
        .await?
        .ok_or_else(no_such_message)?;
    if source.account_id != account_id {
        return Err(OpError::invalid_input(
            "message: not a message of this account",
        ));
    }
    if !source.is_draft {
        return Err(OpError::invalid_input("message: not a draft"));
    }
    Ok(source)
}

/// The account's provider, as a body fetch reaches it.
async fn provider_of(ctx: &DaemonContext, account: &str) -> Result<Arc<dyn Provider>, OpError> {
    match ctx.sync.body_target(account).await? {
        Reach::Ready(target) => Ok(target.provider),
        Reach::Unavailable(error) => Err(error),
    }
}

/// `source` with its body: fetched first when the store has none. A fetch that fails fails
/// the import; a body Gmail no longer has, or Rail cannot read, imports as empty text.
async fn with_body(
    ctx: &DaemonContext,
    account: &str,
    source: ImportSource,
) -> Result<ImportSource, OpError> {
    if source.body_text.is_some() {
        return Ok(source);
    }
    let request = BodyFetchRequest {
        account: account.to_string(),
        message_ids: vec![source.remote_id.clone()],
    };
    let response = crate::sync::body_fetch::run(ctx, request).await?;
    if let Some(BodyOutcome::Failed(error)) = response.results.into_iter().next().map(|r| r.outcome)
    {
        return Err(error);
    }
    let (db, message) = (ctx.paths.db_file(), source.message);
    let fetched = ctx
        .blocking(move || read_source(&db, message))
        .await?
        .ok_or_else(no_such_message)?;
    if fetched.body_text.is_none() {
        tracing::warn!(
            message,
            body = "missing",
            "a draft was imported without its body"
        );
    }
    Ok(fetched)
}

/// Outlook edits keep the same message id, so its cached body and attachment listing may be
/// stale. Fetch both before a first import; any failure stops it. Use the fetched data directly
/// as well as updating the cache, so a concurrent store write cannot substitute an older body.
async fn refresh_outlook_body(
    ctx: &DaemonContext,
    provider: &dyn Provider,
    mut source: ImportSource,
) -> Result<ImportSource, OpError> {
    let body = unobserved(provider.body(&source.remote_id))
        .await
        .map_err(|error| match error {
            ProviderError::NotFound => OpError::not_found("Microsoft 365 no longer has this draft"),
            other => other.into(),
        })?;
    // Graph refused the list: a draft imported without its attachments would be sent without
    // them.
    if body.attachments_unknown {
        return Err(OpError::new(
            ErrorCode::Provider,
            ErrorClass::Permanent,
            "Rail could not list this draft's attachments in Microsoft 365",
        ));
    }
    source.body_text = Some(body.text.clone());
    source.attachments = body
        .attachments
        .iter()
        .map(|part| ReplyAttachment {
            part_id: part.part_id.clone(),
            filename: part.filename.clone(),
            mime_type: part.mime_type.clone(),
            size: part.size,
            plain_file: part.plain_file,
        })
        .collect();
    ctx.sync
        .writer(ctx)
        .await?
        .write(Batch {
            bodies: vec![(source.remote_id.clone(), body)],
            ..Batch::new(source.account_id)
        })
        .await?;
    Ok(source)
}

/// The local draft an import writes: the source's recipients, text and threading, the files
/// fetched for its attachments, and its own `Message-ID` when the MIME builder accepts it (`header_id` checks the inner part
/// with `valid_message_id_inner`), a fresh one otherwise.
fn import_write(source: ImportSource, files: Vec<Attachment>, address: &str) -> DraftWrite {
    let in_reply_to = source.in_reply_to.as_deref().and_then(header_id);
    let references: Vec<String> = source
        .references
        .iter()
        .filter_map(|id| header_id(id))
        .collect();
    let message_id = source
        .message_id
        .as_deref()
        .and_then(header_id)
        .unwrap_or_else(|| new_message_id(address));
    DraftWrite {
        id: None,
        account_id: source.account_id,
        kind: if in_reply_to.is_some() {
            DraftKind::Reply
        } else {
            DraftKind::New
        },
        thread_id: Some(source.thread_id),
        remote_thread_id: Some(source.remote_thread_id),
        in_reply_to,
        references: (!references.is_empty()).then(|| references.join(" ")),
        to: source.to.into_iter().map(Into::into).collect(),
        cc: source.cc.into_iter().map(Into::into).collect(),
        bcc: source.bcc.into_iter().map(Into::into).collect(),
        subject: source.subject,
        body: source.body_text.unwrap_or_default(),
        attachments: files,
        message_id,
        source_remote_id: None,
    }
}

/// The import's write, with the Gmail ids. The draft the token answers must hold the
/// imported message; `dir` is removed unless the stored draft names it.
struct ImportWrite {
    write: DraftWrite,
    remote_draft_id: String,
    remote_message_id: String,
    client_token: String,
    dir: PathBuf,
}

/// The Gmail ids an import names: the draft's and its message's.
struct RemoteIds {
    draft: String,
    message: String,
}

/// The stored draft is the one this import answers: it holds the Gmail message, or the Gmail
/// draft (Rail's copy of a draft edited elsewhere). `dir` goes unless the draft names it.
fn verified(db: &Path, id: DraftId, remote: &RemoteIds, dir: PathBuf) -> Result<DraftId, OpError> {
    let draft = Store::open_reader(db)?.draft(id)?;
    let owns = |d: &Draft| owned_dirs_contain(d, &dir);
    if !draft.as_ref().is_some_and(owns) {
        remove_dirs(std::slice::from_ref(&dir));
    }
    let holds = |d: &Draft| {
        d.remote_message_id.as_deref() == Some(remote.message.as_str())
            || d.remote_draft_id.as_deref() == Some(remote.draft.as_str())
    };
    match draft {
        Some(d) if holds(&d) => Ok(id),
        Some(_) => Err(token_used()),
        None => Err(no_such_draft()),
    }
}

fn owned_dirs_contain(draft: &Draft, dir: &Path) -> bool {
    draft.attachments.iter().any(|a| match a {
        Attachment::File { path, .. } => Path::new(path).parent() == Some(dir),
        Attachment::Part { .. } => false,
    })
}

async fn write_import(ctx: &DaemonContext, import: ImportWrite) -> Result<DraftId, OpError> {
    let ImportWrite {
        write,
        remote_draft_id,
        remote_message_id,
        client_token,
        dir,
    } = import;
    let written = async {
        let outcome = ctx
            .sync
            .writer(ctx)
            .await?
            .outbox(OutboxWrite::DraftImport {
                write: Box::new(write),
                remote_draft_id: remote_draft_id.clone(),
                remote_message_id: remote_message_id.clone(),
                client_token,
            })
            .await?;
        saved_id(outcome)
    }
    .await;
    let id = match written {
        Ok(id) => id,
        Err(e) => {
            remove_dirs(std::slice::from_ref(&dir));
            return Err(e);
        }
    };
    let db = ctx.paths.db_file();
    let remote = RemoteIds {
        draft: remote_draft_id,
        message: remote_message_id,
    };
    ctx.blocking(move || verified(&db, id, &remote, dir)).await
}

/// The Gmail draft that holds `source`, or the account's local draft already bound to it
/// (Rail's version wins over an edit made elsewhere: nothing is fetched for it).
enum Found {
    Held(DraftId),
    Gmail(Arc<dyn Provider>, String),
}

async fn find_gmail_draft(
    ctx: &DaemonContext,
    account: &str,
    account_id: i64,
    source: &ImportSource,
    provider_name: &str,
) -> Result<Found, OpError> {
    let provider = provider_of(ctx, account).await?;
    let remote_draft_id = provider
        .draft_find(&source.remote_id)
        .await?
        .ok_or_else(|| OpError::not_found(format!("{provider_name} no longer has this draft")))?;
    let (db, held) = (ctx.paths.db_file(), remote_draft_id.clone());
    let bound = ctx
        .blocking(move || -> Result<Option<DraftId>, OpError> {
            Ok(Store::open_reader(&db)?.draft_by_remote_draft(account_id, &held)?)
        })
        .await?;
    Ok(match bound {
        Some(draft) => Found::Held(draft),
        None => Found::Gmail(provider, remote_draft_id),
    })
}

/// The import's body and attachments: always refreshed for Outlook, fetched when uncached for
/// Gmail. Attachments become files under the token's directory.
async fn from_gmail(
    ctx: &DaemonContext,
    account: &str,
    provider: &dyn Provider,
    source: ImportSource,
    token: &str,
    provider_name: &str,
    kind: AccountKind,
) -> Result<(ImportSource, Vec<Attachment>, PathBuf), OpError> {
    let mut source = match kind {
        AccountKind::M365 => refresh_outlook_body(ctx, provider, source).await?,
        AccountKind::Gmail | AccountKind::ImapSmtp => with_body(ctx, account, source).await?,
    };
    // Only plain files: an Outlook draft's attached messages and links stay in Outlook, where
    // the reconcile leaves them, and its inline pictures go when the reconcile writes the text
    // body (every Gmail part is a plain file).
    let mut parts = std::mem::take(&mut source.attachments);
    parts.retain(|a| a.plain_file);
    let dir = import_dir(&ctx.paths, token)?;
    let remote = source.remote_id.clone();
    let files = fetch_into(ctx, provider, &remote, parts, dir.clone(), provider_name).await?;
    Ok((source, files, dir))
}

pub(super) async fn import(
    call: Call,
    request: DraftImportRequest,
) -> Result<DraftImportResponse, OpError> {
    checked_client_token(&request.client_token)?;
    if request.message <= 0 {
        return Err(no_such_message());
    }
    let (ctx, token) = (&call.ctx, request.client_token);
    let target = ctx
        .sync
        .action_target(ctx, &request.account, Wants::Compose)
        .await?;
    let provider_name = target.kind.provider_name();
    let source = import_source(ctx, request.message, target.account_id).await?;
    let _imports = IMPORTS.lock().await;
    let (db, remote, held) = (ctx.paths.db_file(), source.remote_id.clone(), token.clone());
    let account_id = target.account_id;
    if let Some(draft) = ctx
        .blocking(move || earlier_import(&db, account_id, &remote, &held))
        .await?
    {
        return Ok(DraftImportResponse { draft });
    }
    let (provider, remote_draft_id) =
        match find_gmail_draft(ctx, &request.account, account_id, &source, provider_name).await? {
            Found::Held(draft) => return Ok(DraftImportResponse { draft }),
            Found::Gmail(provider, id) => (provider, id),
        };
    let (source, files, dir) = from_gmail(
        ctx,
        &request.account,
        provider.as_ref(),
        source,
        &token,
        provider_name,
        target.kind,
    )
    .await?;
    let count = files.len();
    let import = ImportWrite {
        remote_message_id: source.remote_id.clone(),
        write: import_write(source, files, &target.address),
        remote_draft_id,
        client_token: token,
        dir,
    };
    let draft = write_import(ctx, import).await?;
    tracing::info!(account = %target.name, draft, message = request.message, files = count, "a provider draft was imported");
    Ok(DraftImportResponse { draft })
}
