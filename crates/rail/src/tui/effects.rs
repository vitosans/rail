//! Runs the wizard's `Action`s as daemon requests and reports each outcome back as a wizard
//! `Event`. The daemon runs the login and saves the account when it completes; the wizard's
//! state machine is unchanged, so its `Persist` step only collects what the daemon saved.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rail_core::auth::{TokenSet, random_token};
use rail_core::config::{AccountConfig, KeymapPreset, load_text_or_empty};
use rail_core::daemon::Connector;
use rail_core::error::ErrorClass;
use rail_core::onboarding::{Action, DoneReport, Event, Failure, Preset, PresetEntry};
use rail_core::ops::op::{AccountAddBegin, ConsentBegin, PendingWait, PresetFetch, PresetsList};
use rail_core::ops::{
    AccountAddRequest, BrowserEnv, ClientSecretInput, ConsentBeginRequest, ErrorCode, OpError,
    PendingId, PendingOutcome, PendingState, PresetFetchRequest, PresetSummary, PresetsListRequest,
    SavedAccount, WaitRequest, WaitResponse,
};
use rail_core::paths::Paths;
use tokio::sync::Notify;
use url::Url;

use crate::cmd::account::{CANCEL_TIMEOUT, error_text};
use crate::tui::app::{Effect, TaskResult, TaskSink};
use crate::tui::keys::Keymap;
use crate::tui::screens::mail::{Saved, write_general};
use crate::{browser, daemon_link};

mod in_flight;

pub use in_flight::Settled;
use in_flight::{Outstanding, begin};

/// Starts the browser on a URL. Injected so tests never start a real one.
pub type Opener = Arc<dyn Fn(&Url) -> io::Result<()> + Send + Sync>;

/// What the effects need from the outside world: how to open a portal page, and the daemon.
#[derive(Clone)]
pub struct EffectContext {
    /// Portal and help pages only; the daemon opens login pages.
    pub opener: Opener,
    pub daemon: Connector,
    /// What is in flight in the daemon, so a cancel or a quit can end it there.
    outstanding: Arc<Mutex<Outstanding>>,
    /// Told whenever a begin request is answered.
    answered: Arc<Notify>,
    /// What the daemon saved when the last login finished, for the wizard's `Persist` step.
    saved: Arc<Mutex<Option<SavedAccount>>>,
    /// Stand-ins for preset client secrets: an unguessable token per preset, mapped to the
    /// preset's source. The daemon never returns a preset's secret; the draft holds the token,
    /// and a login whose secret is exactly a token names that preset instead. Typed text cannot
    /// match: the tokens are random and never shown.
    preset_secrets: Arc<Mutex<HashMap<String, String>>>,
    /// The longest `settle` waits for the daemon.
    cancel_timeout: Duration,
}

impl EffectContext {
    /// The real context: the platform browser and the user's daemon.
    pub fn new(paths: Paths) -> Self {
        // The TUI opens a URL itself only when the user asks; it runs where the user is, so
        // the browser inherits this process's own display.
        let opener: Opener = Arc::new(|url: &Url| browser::launch(url, &BrowserEnv::default()));
        Self::with(opener, daemon_link::connector(&paths))
    }

    pub fn with(opener: Opener, daemon: Connector) -> Self {
        Self {
            opener,
            daemon,
            outstanding: Arc::default(),
            answered: Arc::default(),
            saved: Arc::default(),
            preset_secrets: Arc::default(),
            cancel_timeout: CANCEL_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_cancel_timeout(mut self, timeout: Duration) -> Self {
        self.cancel_timeout = timeout;
        self
    }

    /// The stand-in for the client secret of the preset at `source`.
    fn preset_secret(&self, source: &str) -> String {
        let token = format!("rail-preset-{}", random_token());
        lock(&self.preset_secrets).insert(token.clone(), source.to_string());
        token
    }

    /// The preset source `secret` stands for, if it is one of this process's stand-ins.
    fn preset_source(&self, secret: &str) -> Option<String> {
        lock(&self.preset_secrets).get(secret).cloned()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The shell effect that carries out `action`. Work that reports back becomes a spawned
/// job sending `TaskResult::Wizard` events.
pub fn to_effect(action: Action, ctx: &EffectContext) -> Effect {
    let ctx = ctx.clone();
    match action {
        Action::Quit => Effect::Quit,
        Action::CancelPending => Effect::CancelAll,
        Action::Copy(text) => Effect::Copy(text),
        Action::OpenUrl(url) => job(move |_| async move { open(&ctx.opener, &url) }),
        Action::LoadPresets => job(move |sink| load_presets(ctx, sink)),
        Action::ReadPreset(path) => job(move |sink| fetch(ctx, absolute(&path), sink)),
        Action::FetchPreset(url) => job(move |sink| fetch(ctx, url, sink)),
        Action::RunAdminConsent { client_id, tenant } => {
            job(move |sink| admin_consent(ctx, client_id, tenant, sink))
        }
        Action::RunLogin(account) => job(move |sink| login(ctx, account, sink)),
        Action::ValidateImapSmtp {
            account,
            password,
            smtp_password,
        } => job(move |sink| validate_imapsmtp(ctx, account, password, smtp_password, sink)),
        Action::Persist { .. } => job(move |sink| async move { persisted(&ctx, &sink) }),
        Action::PersistImapSmtp {
            account,
            password,
            smtp_password,
        } => job(move |sink| persist_imapsmtp(ctx, account, password, smtp_password, sink)),
        Action::SaveKeymap {
            preset,
            config_path,
        } => job(move |sink| async move {
            let result = tokio::task::spawn_blocking(move || save_keymap(&config_path, preset))
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
                .map_err(|message| Failure::new(message, ErrorClass::Permanent));
            send(&sink, Event::KeymapSaved(result));
        }),
    }
}

/// Write `keymap = "<preset>"` to the config at `path`, refusing a preset its `[keys]` clash
/// with: that config would stop Rail from starting.
fn save_keymap(path: &Path, preset: KeymapPreset) -> Result<(), String> {
    let (text, config) = load_text_or_empty(path).map_err(|e| e.to_string())?;
    Keymap::from_config(&config.keys, preset, &text, path).map_err(|e| e.to_string())?;
    write_general(
        path,
        "keymap",
        Saved::Text(Some(preset.as_str().to_string())),
    )
}

fn job<F, Fut>(run: F) -> Effect
where
    F: FnOnce(TaskSink) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Effect::Spawn(Box::new(move |sink| Box::pin(run(sink))))
}

fn send(sink: &TaskSink, event: Event) {
    // A closed channel means the event loop has ended and nobody is listening any more.
    let _ = sink.send(TaskResult::Wizard(event));
}

/// The URL is on screen whatever happens here, so a failed opener is logged, not fatal.
fn open(opener: &Opener, url: &Url) {
    if let Err(e) = opener(url) {
        tracing::warn!(error = %e, "could not start the browser opener");
    }
}

/// A path typed in the wizard, made absolute against this process's working directory: the
/// daemon runs in `/` and refuses relative paths. Left as typed if there is no working
/// directory, so the daemon's refusal explains it.
fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// An operation error as the wizard shows it: the message, any outside text escaped and
/// labelled on a line of its own, the hint, its class, and whether Microsoft wants an
/// administrator's consent.
fn failure(error: &OpError) -> Failure {
    Failure {
        message: error_text(error),
        class: error.class,
        consent_required: error.code == ErrorCode::ConsentRequired,
    }
}

/// A preset as the wizard holds it. A client secret the daemon kept is replaced by a stand-in
/// for the preset's source.
fn to_preset(ctx: &EffectContext, summary: PresetSummary, source: &str) -> Preset {
    Preset {
        name: summary.name.map(|n| n.into_inner()),
        kind: summary.kind,
        client_id: summary.client_id.into_inner(),
        tenant: summary.tenant.map(|t| t.into_inner()),
        client_secret: summary.has_client_secret.then(|| ctx.preset_secret(source)),
        help_url: summary.help_url.map(|u| u.into_inner()),
    }
}

async fn load_presets(ctx: EffectContext, sink: TaskSink) {
    let listed = ctx
        .daemon
        .call::<PresetsList>(&PresetsListRequest {})
        .await
        .map_err(|e| failure(&e));
    let entries = match listed {
        Ok(listed) => listed
            .presets
            .into_iter()
            .map(|listing| {
                let source = listing.source.into_inner();
                PresetEntry {
                    preset: match listing.preset {
                        Some(summary) => Ok(to_preset(&ctx, summary, &source)),
                        None => Err(listing.error.map(|e| error_text(&e)).unwrap_or_default()),
                    },
                    source: PathBuf::from(source),
                }
            })
            .collect(),
        Err(failure) => vec![PresetEntry {
            source: PathBuf::new(),
            preset: Err(failure.message),
        }],
    };
    send(&sink, Event::PresetsLoaded(entries));
}

/// A typed path or URL; the daemon refuses plain http before any request.
async fn fetch(ctx: EffectContext, source: String, sink: TaskSink) {
    let result = ctx
        .daemon
        .call::<PresetFetch>(&PresetFetchRequest { source })
        .await
        .map(|fetched| to_preset(&ctx, fetched.preset, &fetched.source))
        .map_err(|e| failure(&e));
    send(&sink, Event::PresetFetched(result));
}

/// The account the wizard built, as an add request: a preset's stand-in names the preset, a
/// client secret typed as `op://...` is a 1Password reference, anything else is the value.
fn add_request(ctx: &EffectContext, account: &AccountConfig) -> AccountAddRequest {
    let client_secret =
        account
            .client_secret
            .clone()
            .map(|secret| match ctx.preset_source(&secret) {
                Some(source) => ClientSecretInput::Preset(source),
                None => ClientSecretInput::from_flag(secret),
            });
    AccountAddRequest {
        kind: account.kind,
        address: account.address.clone(),
        client_id: Some(account.client_id.clone()),
        client_secret,
        tenant: account.tenant.clone(),
        name: Some(account.name.clone()),
        color: account.color.clone(),
        store_secret_in_1password: false,
        open_browser: true,
    }
}

/// `pending.wait` until the operation finishes.
async fn finished(ctx: &EffectContext, id: &PendingId) -> Result<WaitResponse, Failure> {
    let wait = WaitRequest {
        id: id.clone(),
        timeout_secs: None,
    };
    loop {
        let answer = ctx
            .daemon
            .call::<PendingWait>(&wait)
            .await
            .map_err(|e| failure(&e))?;
        if answer.state.is_finished() {
            return Ok(answer);
        }
    }
}

/// Begin the add in the daemon, show the URL, wait. The daemon saves the account as soon as
/// the login completes; the wizard then gets tokens that are only a placeholder, because the
/// real ones never leave the daemon.
async fn login(ctx: EffectContext, account: AccountConfig, sink: TaskSink) {
    let result = run_login(&ctx, &account, &sink).await;
    send(&sink, Event::LoginFinished(result));
}

async fn run_login(
    ctx: &EffectContext,
    account: &AccountConfig,
    sink: &TaskSink,
) -> Result<TokenSet, Failure> {
    let (begun, in_flight) = begin::<AccountAddBegin>(ctx, add_request(ctx, account)).await?;
    if let Ok(url) = Url::parse(&begun.url) {
        send(sink, Event::LoginUrl(url));
    }
    let answer = finished(ctx, &begun.id).await?;
    in_flight.disarm();
    match answer.state {
        PendingState::Done {
            outcome: PendingOutcome::Added { account },
        } => {
            *lock(&ctx.saved) = Some(account);
            Ok(saved_by_the_daemon())
        }
        PendingState::Failed { error } => Err(failure(&error)),
        other => Err(ended_without(&other, "the login ended without saving")),
    }
}

/// A finished operation that neither succeeded nor failed: it was cancelled, or the daemon
/// answered with an outcome of another kind.
fn ended_without(state: &PendingState, what: &str) -> Failure {
    let why = match state {
        PendingState::Cancelled => "it was cancelled",
        _ => "the daemon reported another kind of outcome",
    };
    Failure::new(format!("{what}: {why}"), ErrorClass::Permanent)
}

/// Stands for the tokens the daemon stored. The wizard only needs to know the login worked.
fn saved_by_the_daemon() -> TokenSet {
    TokenSet {
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: 0,
    }
}

/// The wizard's `Persist`: report what the daemon saved when the login finished.
fn persisted(ctx: &EffectContext, sink: &TaskSink) {
    let result = match lock(&ctx.saved).take() {
        Some(saved) => Ok(DoneReport {
            config_path: saved.config_path,
            token_backend: saved.token_backend,
            token_location: saved.token_location,
            store_path: saved.store_path,
        }),
        None => Err(Failure::new(
            "the daemon has no saved account to report; go back and log in again",
            ErrorClass::Permanent,
        )),
    };
    send(sink, Event::Persisted(result));
}

async fn admin_consent(ctx: EffectContext, client_id: String, tenant: String, sink: TaskSink) {
    let result = run_consent(&ctx, client_id, tenant, &sink).await;
    send(&sink, Event::AdminConsentFinished(result));
}

async fn run_consent(
    ctx: &EffectContext,
    client_id: String,
    tenant: String,
    sink: &TaskSink,
) -> Result<String, Failure> {
    let request = ConsentBeginRequest {
        client_id,
        tenant,
        open_browser: true,
    };
    let (begun, in_flight) = begin::<ConsentBegin>(ctx, request).await?;
    if let Ok(url) = Url::parse(&begun.url) {
        send(sink, Event::ConsentUrl(url));
    }
    let answer = finished(ctx, &begun.id).await?;
    in_flight.disarm();
    match answer.state {
        PendingState::Done {
            outcome: PendingOutcome::Consented { tenant },
        } => Ok(tenant.into_inner()),
        PendingState::Failed { error } => Err(failure(&error)),
        other => Err(ended_without(&other, "the consent ended without an answer")),
    }
}

async fn validate_imapsmtp(
    ctx: EffectContext,
    account: AccountConfig,
    password: rail_core::ops::Secret<String>,
    smtp_password: Option<rail_core::ops::Secret<String>>,
    sink: TaskSink,
) {
    let req = rail_core::ops::AccountValidateRequest {
        account: None,
        imap_host: account.imap_host,
        imap_port: account.imap_port,
        imap_tls: account.imap_tls,
        username: account.imap_user.or_else(|| Some(account.address.clone())),
        password: Some(password),
        smtp_host: account.smtp_host,
        smtp_port: account.smtp_port,
        smtp_tls: account.smtp_tls,
        smtp_username: account.smtp_user.or_else(|| Some(account.address.clone())),
        smtp_password,
    };
    let result = ctx
        .daemon
        .call::<rail_core::ops::op::AccountValidate>(&req)
        .await
        .map(|_| ())
        .map_err(|e| failure(&e));
    send(&sink, Event::ImapSmtpValidated(result));
}

async fn persist_imapsmtp(
    ctx: EffectContext,
    account: AccountConfig,
    password: rail_core::ops::Secret<String>,
    smtp_password: Option<rail_core::ops::Secret<String>>,
    sink: TaskSink,
) {
    let set_pw = rail_core::ops::AccountPasswordSetRequest {
        account: account.name.clone(),
        password,
        service: None,
    };
    if let Err(e) = ctx
        .daemon
        .call::<rail_core::ops::op::AccountPasswordSet>(&set_pw)
        .await
    {
        send(&sink, Event::Persisted(Err(failure(&e))));
        return;
    }
    if let Some(smtp_pw) = smtp_password {
        let set_smtp = rail_core::ops::AccountPasswordSetRequest {
            account: account.name.clone(),
            password: smtp_pw,
            service: Some("smtp".to_string()),
        };
        if let Err(e) = ctx
            .daemon
            .call::<rail_core::ops::op::AccountPasswordSet>(&set_smtp)
            .await
        {
            send(&sink, Event::Persisted(Err(failure(&e))));
            return;
        }
    }
    let save_res = tokio::task::spawn_blocking(move || {
        let paths =
            Paths::from_env().map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        let config_path = paths.config_file();
        let store_path = paths.db_file();
        let (text, _) = rail_core::config::load_text_or_empty(&config_path)
            .map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        let new_text = rail_core::config::append_account(&text, &config_path, &account)
            .map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        if let Some(dir) = config_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty() && !d.is_dir())
        {
            rail_core::paths::create_private_dir(dir)
                .map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        }
        rail_core::paths::replace_private_atomic(&config_path, new_text.as_bytes())
            .map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        rail_core::store::Store::open(&store_path)
            .and_then(|store| store.upsert_account(&account))
            .map_err(|e| Failure::new(e.to_string(), ErrorClass::Permanent))?;
        Ok(DoneReport {
            config_path,
            token_backend: "secrets".to_string(),
            token_location: "secret store".to_string(),
            store_path,
        })
    })
    .await
    .unwrap_or_else(|e| Err(Failure::new(e.to_string(), ErrorClass::Permanent)));

    send(&sink, Event::Persisted(save_res));
}

#[cfg(test)]
mod tests;
