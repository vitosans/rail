//! `account.add.begin`, `account.login.begin` and `consent.begin`. Each returns at once with
//! the URL; a detached task waits for the redirect, verifies the mailbox and saves. The task
//! belongs to the daemon, not to the connection, so a client that goes away never stops a save.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::accounts::{
    PreparedAdd, account_for_login, endpoints_for_login, login_secret, persist_login, persist_new,
    prepare_and_probe, probe_secrets,
};
use crate::auth::{
    AuthSession, TokenSet, begin_admin_consent, begin_auth_code, open_secret_store, unix_now,
};
use crate::config::{AccountConfig, AccountKind};
use crate::daemon::context::DaemonContext;
use crate::daemon::pending::{Refused, Slot};
use crate::daemon::registry::{Call, Caller};
use crate::error::ErrorClass;
use crate::ops::{
    AccountAddRequest, BeginResponse, BrowserEnv, ConsentBeginRequest, ErrorCode,
    LoginBeginRequest, OpError, PendingId, PendingKind, PendingOutcome, PendingState, names,
};

/// How long a begun login may wait for its redirect before it fails.
pub(crate) const LOGIN_LIFETIME: Duration = Duration::from_secs(900);

pub(super) async fn add_begin(
    call: Call,
    request: AccountAddRequest,
) -> Result<BeginResponse, OpError> {
    if request.kind == AccountKind::ImapSmtp {
        return Err(OpError::invalid_input(
            "IMAP/SMTP accounts do not use OAuth browser sign-in; add them using `rail account add --kind imapsmtp` or the setup wizard",
        ));
    }
    let ctx = call.ctx.clone();
    let slot = ensure_room(&ctx)?;
    let prepared = prepare_and_probe(&ctx.paths, &ctx.op_ids, &request, &ctx.http).await?;
    let endpoints = endpoints_for_login(&prepared.account, ctx.token_url());
    let session = begin_auth_code(&prepared.login_account(), &endpoints).await?;
    let url = session.url().clone();
    #[cfg(any(test, feature = "test-gates"))]
    ctx.begin_gate.pass(&url).await;
    let (id, cancel) = ctx
        .pending
        .begin(slot, PendingKind::AddAccount)
        .map_err(refusal)?;
    let task = finish_add(ctx.clone(), id.clone(), session, prepared, cancel);
    let browser = call.caller.browser().clone();
    spawn_pending(&ctx, call.caller, id.clone(), task);
    Ok(begun(
        &ctx,
        &browser,
        id,
        PendingKind::AddAccount,
        &url,
        request.open_browser,
    ))
}

pub(super) async fn login_begin(
    call: Call,
    request: LoginBeginRequest,
) -> Result<BeginResponse, OpError> {
    let ctx = call.ctx.clone();
    let slot = ensure_room(&ctx)?;
    let (paths, ids, who) = (
        ctx.paths.clone(),
        ctx.op_ids.clone(),
        request.account.clone(),
    );
    let (account, login) = ctx
        .blocking(move || {
            let (config, account) = account_for_login(&paths, &who)?;
            if account.kind == AccountKind::ImapSmtp {
                return Err(OpError::invalid_input(
                    "IMAP/SMTP accounts do not use OAuth browser login; update credentials with `rail secrets password <account>` or the setup wizard",
                ));
            }
            probe_secrets(
                open_secret_store(&config, &paths, &ids).as_ref(),
                &account.address,
            )?;
            let secret = login_secret(&account, &config, &paths)?;
            let login = AccountConfig {
                client_secret: secret.map(|s| s.expose().to_string()),
                ..account.clone()
            };
            Ok((account, login))
        })
        .await?;
    let endpoints = endpoints_for_login(&account, ctx.token_url());
    let session = begin_auth_code(&login, &endpoints).await?;
    let url = session.url().clone();
    let (id, cancel) = ctx
        .pending
        .begin(slot, PendingKind::Login)
        .map_err(refusal)?;
    let task = finish_login(ctx.clone(), id.clone(), session, account, cancel);
    let browser = call.caller.browser().clone();
    spawn_pending(&ctx, call.caller, id.clone(), task);
    Ok(begun(
        &ctx,
        &browser,
        id,
        PendingKind::Login,
        &url,
        request.open_browser,
    ))
}

pub(super) async fn consent_begin(
    call: Call,
    request: ConsentBeginRequest,
) -> Result<BeginResponse, OpError> {
    let ctx = call.ctx.clone();
    let slot = ensure_room(&ctx)?;
    let session = begin_admin_consent(&request.client_id, &request.tenant).await?;
    let url = session.url().clone();
    let (id, cancel) = ctx
        .pending
        .begin(slot, PendingKind::AdminConsent)
        .map_err(refusal)?;
    let task = async move {
        let waited = tokio::select! {
            result = tokio::time::timeout(LOGIN_LIFETIME, session.finish()) => result,
            _ = cancel => return None,
        };
        Some(match waited {
            Ok(Ok(tenant)) => done(PendingOutcome::Consented {
                tenant: tenant.into(),
            }),
            Ok(Err(e)) => failed(e.into()),
            Err(_) => failed(timed_out()),
        })
    };
    let browser = call.caller.browser().clone();
    spawn_pending(&ctx, call.caller, id.clone(), task);
    Ok(begun(
        &ctx,
        &browser,
        id,
        PendingKind::AdminConsent,
        &url,
        request.open_browser,
    ))
}

/// Try the browser if asked, and say what was begun. A failed opener is logged; the URL is
/// returned either way.
fn begun(
    ctx: &DaemonContext,
    browser: &BrowserEnv,
    id: PendingId,
    kind: PendingKind,
    url: &Url,
    open_browser: bool,
) -> BeginResponse {
    let opened = open_browser
        && (ctx.opener)(url, browser)
            .inspect_err(|e| tracing::warn!(error = %e, "could not start the browser"))
            .is_ok();
    BeginResponse {
        id,
        kind,
        url: url.to_string(),
        opened,
    }
}

/// Reserve a place for a new login before anything is bound: refused while `MAX_PENDING` are
/// waiting or being begun, and once the daemon is shutting down. The place is held until
/// `pending.begin` turns it into the entry, or given back if the begin fails first.
fn ensure_room(ctx: &DaemonContext) -> Result<Slot<'_>, OpError> {
    ctx.pending.try_reserve().map_err(refusal)
}

fn refusal(refused: Refused) -> OpError {
    let message = match refused {
        Refused::Full => "too many logins are in progress; finish or cancel one first",
        Refused::Stopping => "the daemon is shutting down",
    };
    OpError::new(ErrorCode::Daemon, ErrorClass::Transient, message)
}

/// Settles an entry as failed if its task ends without doing so (a panic, or an abort at
/// shutdown), so the daemon can idle out and `finish` does not wait for it.
struct Settle {
    ctx: Arc<DaemonContext>,
    id: PendingId,
}

impl Drop for Settle {
    fn drop(&mut self) {
        let error = OpError::internal("the login task ended unexpectedly");
        if self.ctx.pending.finish(&self.id, failed(error)).is_some() {
            tracing::warn!(id = %self.id, "a pending task ended without finishing its entry");
        }
    }
}

/// Run `task` detached and record its outcome; `None` means it was cancelled. The caller is
/// told with a `pending.finished` notification if it is still connected.
fn spawn_pending(
    ctx: &Arc<DaemonContext>,
    caller: Caller,
    id: PendingId,
    task: impl Future<Output = Option<PendingState>> + Send + 'static,
) {
    let table_ctx = ctx.clone();
    let ctx = ctx.clone();
    table_ctx.pending.spawn(async move {
        let _settle = Settle {
            ctx: ctx.clone(),
            id: id.clone(),
        };
        if let Some(state) = task.await
            && ctx.pending.finish(&id, state).is_none()
        {
            tracing::warn!(id = %id, "result discarded: the operation was cancelled");
        }
        match ctx.pending.get(&id) {
            Some(response) => {
                tracing::info!(id = %id, "pending operation finished");
                caller.notify(names::PENDING_FINISHED, &response);
            }
            None => tracing::warn!(id = %id, "a finished pending operation is gone"),
        }
    });
}

/// The redirect, code exchange and mailbox verification, unless cancelled (`None`) or timed out.
async fn wait_for_tokens(
    ctx: &DaemonContext,
    session: AuthSession,
    account: &AccountConfig,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Option<Result<TokenSet, OpError>> {
    let verified = async {
        let tokens = session
            .finish(&ctx.http, unix_now)
            .await
            .map_err(OpError::from)?;
        let api_root = match account.kind {
            AccountKind::Gmail => ctx.gmail_url(),
            AccountKind::M365 => ctx.graph_url(),
            AccountKind::ImapSmtp => {
                return Err(OpError::invalid_input(
                    "IMAP/SMTP accounts do not use OAuth login",
                ));
            }
        };
        crate::accounts::identity::verify_mailbox(&ctx.http, account, &tokens, api_root).await?;
        Ok(tokens)
    };
    tokio::select! {
        result = tokio::time::timeout(LOGIN_LIFETIME, verified) => {
            Some(match result {
                Ok(tokens) => tokens,
                Err(_) => Err(timed_out()),
            })
        }
        _ = cancel => None,
    }
}

async fn finish_add(
    ctx: Arc<DaemonContext>,
    id: PendingId,
    session: AuthSession,
    prepared: PreparedAdd,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Option<PendingState> {
    let tokens = match wait_for_tokens(&ctx, session, &prepared.account, cancel).await? {
        Ok(tokens) => tokens,
        Err(error) => return Some(failed(error)),
    };
    if !ctx.pending.start_saving(&id) {
        tracing::warn!(id = %id, "result discarded: the operation was cancelled");
        return None;
    }
    #[cfg(any(test, feature = "test-gates"))]
    ctx.save_gate.pass().await;
    let saved = ctx
        .write_locked(move |ctx| persist_new(&ctx.paths, &ctx.op_ids, &prepared, &tokens))
        .await;
    if saved.is_ok() {
        ctx.sync.reconcile(&ctx).await;
    }
    Some(match saved {
        Ok(account) => done(PendingOutcome::Added { account }),
        Err(error) => failed(error),
    })
}

async fn finish_login(
    ctx: Arc<DaemonContext>,
    id: PendingId,
    session: AuthSession,
    account: AccountConfig,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Option<PendingState> {
    let tokens = match wait_for_tokens(&ctx, session, &account, cancel).await? {
        Ok(tokens) => tokens,
        Err(error) => return Some(failed(error)),
    };
    if !ctx.pending.start_saving(&id) {
        tracing::warn!(id = %id, "result discarded: the operation was cancelled");
        return None;
    }
    #[cfg(any(test, feature = "test-gates"))]
    ctx.save_gate.pass().await;
    let saved = ctx
        .write_locked(move |ctx| {
            // Config may have changed while the user was in the browser: save against it.
            let (config, current) = account_for_login(&ctx.paths, &account.address)?;
            persist_login(&ctx.paths, &ctx.op_ids, &config, &current, &tokens)
        })
        .await;
    if let Ok(account) = &saved {
        ctx.sync.logged_in(&ctx, &account.address).await;
    }
    Some(match saved {
        Ok(account) => done(PendingOutcome::LoggedIn { account }),
        Err(error) => failed(error),
    })
}

fn done(outcome: PendingOutcome) -> PendingState {
    PendingState::Done { outcome }
}

fn failed(error: OpError) -> PendingState {
    PendingState::Failed { error }
}

fn timed_out() -> OpError {
    OpError::new(
        ErrorCode::Timeout,
        ErrorClass::Transient,
        "nobody finished the login in the browser within 15 minutes",
    )
    .with_hint("begin it again")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::BrowserEnv;
    use crate::test_support::daemon_context;

    /// A task that panics must still end its entry, or the daemon could never idle out.
    #[tokio::test]
    async fn a_panicking_task_fails_its_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, _urls) = daemon_context(&crate::paths::Paths::all_in(tmp.path()));
        let ctx = Arc::new(ctx);
        let (frames, _outgoing) = tokio::sync::mpsc::unbounded_channel();
        let slot = ctx.pending.try_reserve().unwrap();
        let (id, _cancel) = ctx.pending.begin(slot, PendingKind::Login).unwrap();
        spawn_pending(
            &ctx,
            Caller::new(frames, BrowserEnv::default()),
            id.clone(),
            async {
                panic!("boom");
            },
        );
        let answer = ctx.pending.wait(&id, Duration::from_secs(5)).await.unwrap();
        assert!(
            matches!(answer.state, PendingState::Failed { .. }),
            "{answer:?}"
        );
        assert_eq!(ctx.pending_count(), 0);
    }
}
