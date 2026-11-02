//! The onboarding wizard as a pure state machine. It never touches the filesystem, the
//! network or the terminal: `handle` (and `apply`, in `routing.rs`) return `Action`s for the
//! binary to run, and the outcomes come back as `Event`s.

use std::collections::BTreeMap;
use std::path::PathBuf;

use url::Url;

use super::draft::{AccountDraft, Field, is_concrete_tenant};
use super::events::{Action, DoneReport, Failure, Input, Remedy};
use super::instructions::portal_url;
use super::preset::Preset;
use crate::auth::TokenSet;
use crate::config::{AccountKind, Config, KeymapPreset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    /// The preset list and the path-or-URL field; shown under "Start" in the step list.
    Preset,
    Provider,
    Instructions,
    Details,
    /// The organisation's tenant, asked first when the draft says `common`.
    ConsentTenant,
    AdminConsent,
    Login,
    /// The first run's key preset, once its first account is saved; shown under "Done".
    Keys,
    Done,
}

/// The app registration and tenant an administrator approved Rail for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Consent {
    pub(super) client_id: String,
    pub(super) tenant: String,
}

/// The action in flight. Events that do not match it are stale and ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    LoadingPresets,
    FetchingPreset,
    Consent,
    Login,
    Persisting,
    SavingKeys,
}

pub const START_CHOICES: [&str; 2] = ["Use a preset from my organisation", "Set up my own app"];
pub const PROVIDER_CHOICES: [&str; 2] = ["Gmail / Google Workspace", "Microsoft 365 / Outlook"];
pub const DONE_CHOICES: [&str; 2] = ["Add another account", "Finish"];

#[derive(Debug)]
pub struct Wizard {
    pub(super) step: Step,
    pub(super) history: Vec<Step>,
    pub(super) draft: AccountDraft,
    pub(super) existing: Config,
    pub(super) kind_hint: Option<AccountKind>,
    pub(super) presets: Vec<Preset>,
    pub(super) preset_problems: Vec<String>,
    pub(super) preset_source: String,
    pub(super) consent_tenant: String,
    /// The tenant that granted admin consent in this session.
    pub(super) consented: Option<Consent>,
    pub(super) errors: BTreeMap<Field, String>,
    /// Set by Next on Details, so untouched fields show their errors too.
    pub(super) show_errors: bool,
    pub(super) busy: Option<Busy>,
    pub(super) failure: Option<(Failure, Remedy)>,
    pub(super) notice: Option<String>,
    /// The login or consent URL, shown for manual copy.
    pub(super) url: Option<Url>,
    /// The last consent and login URLs shown, kept across steps so a quit can print them.
    pub(super) last_consent_url: Option<Url>,
    pub(super) last_login_url: Option<Url>,
    /// Tokens from a finished login, kept until persist succeeds so a retry needs no new login.
    pub(super) tokens: Option<TokenSet>,
    pub(super) report: Option<DoneReport>,
    /// Names of the accounts saved in this session.
    pub(super) added: Vec<String>,
    /// No account was configured when the wizard started: the first one saved is followed by
    /// the Keys step.
    pub(super) first_run: bool,
    /// The preset chosen on the Keys step, once saved; `None` keeps Rail's own.
    pub(super) keys: Option<KeymapPreset>,
}

impl Wizard {
    pub fn new(existing: Option<Config>, kind_hint: Option<AccountKind>) -> Self {
        let first_run = existing.as_ref().is_none_or(|c| c.accounts.is_empty());
        Self {
            step: Step::Start,
            history: Vec::new(),
            draft: AccountDraft::default(),
            existing: existing.unwrap_or_default(),
            kind_hint,
            presets: Vec::new(),
            preset_problems: Vec::new(),
            preset_source: String::new(),
            consent_tenant: String::new(),
            consented: None,
            errors: BTreeMap::new(),
            show_errors: false,
            busy: None,
            failure: None,
            notice: None,
            url: None,
            last_consent_url: None,
            last_login_url: None,
            tokens: None,
            report: None,
            added: Vec::new(),
            first_run,
            keys: None,
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn draft(&self) -> &AccountDraft {
        &self.draft
    }

    pub fn errors(&self) -> &BTreeMap<Field, String> {
        &self.errors
    }

    pub fn busy(&self) -> Option<Busy> {
        self.busy
    }

    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref().map(|(failure, _)| failure)
    }

    pub fn remedy(&self) -> Option<&Remedy> {
        self.failure.as_ref().map(|(_, remedy)| remedy)
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn consent_tenant(&self) -> &str {
        &self.consent_tenant
    }

    pub fn existing(&self) -> &Config {
        &self.existing
    }

    /// The last consent and login URLs the wizard showed that are still unused. They carry
    /// no secret (client id, PKCE challenge, state), so the caller may print them.
    pub fn shown_urls(&self) -> (Option<Url>, Option<Url>) {
        (self.last_consent_url.clone(), self.last_login_url.clone())
    }

    pub fn added(&self) -> &[String] {
        &self.added
    }

    /// React to the user. Cancel always yields exactly `[Quit]`: nothing is written before
    /// the persist action, so quitting leaves nothing behind.
    pub fn handle(&mut self, input: Input) -> Vec<Action> {
        match input {
            Input::Cancel => vec![Action::Quit],
            Input::Back => self.back(),
            Input::Edit(field, value) => {
                if self.accepts_edits() {
                    self.edit(field, value);
                }
                Vec::new()
            }
            Input::ToggleAdmin => {
                if self.accepts_edits() {
                    self.toggle_admin();
                }
                Vec::new()
            }
            Input::Choose(index) => self.choose(index),
            Input::Next => self.next(),
            Input::Retry => self.retry(),
            Input::OpenPortal => self.open_portal(),
            Input::CopyConsentUrl => self.copy_consent_url(),
        }
    }

    pub(super) fn go(&mut self, step: Step) {
        self.history.push(self.step);
        self.step = step;
        self.clear_status();
    }

    pub(super) fn clear_status(&mut self) {
        self.failure = None;
        self.notice = None;
        self.url = None;
    }

    /// Back one step, cancelling whatever was in flight. Not from Done, and not while
    /// saving: a half-cancelled persist is the one thing that could leave a trace.
    fn back(&mut self) -> Vec<Action> {
        // The account is saved by the Keys step, and saving keys is as final as saving it.
        let saved = matches!(self.step, Step::Keys | Step::Done);
        if saved || matches!(self.busy, Some(Busy::Persisting | Busy::SavingKeys)) {
            return Vec::new();
        }
        let Some(previous) = self.history.pop() else {
            return Vec::new();
        };
        if self.step == Step::Login {
            self.tokens = None;
        }
        self.step = previous;
        self.clear_status();
        if self.busy.take().is_some() {
            vec![Action::CancelPending]
        } else {
            Vec::new()
        }
    }

    /// Fields change only on the steps that show them, and never while an action is in flight.
    fn accepts_edits(&self) -> bool {
        self.busy.is_none()
            && matches!(
                self.step,
                Step::Preset | Step::Details | Step::ConsentTenant
            )
    }

    /// Whether the current client id and tenant still need an administrator's approval.
    fn needs_consent(&self) -> bool {
        let tenant = if self.draft.has_concrete_tenant() {
            self.draft.tenant.trim()
        } else {
            self.consent_tenant.trim()
        };
        match &self.consented {
            Some(done) => {
                done.client_id != self.draft.client_id.trim()
                    || !done.tenant.eq_ignore_ascii_case(tenant)
            }
            None => true,
        }
    }

    fn edit(&mut self, field: Field, value: String) {
        match field {
            Field::PresetSource => self.preset_source = value,
            Field::ConsentTenant => self.consent_tenant = value,
            Field::Admin => {}
            _ => {
                self.draft.set(field, value, &self.existing);
                self.errors = self.draft.validate(&self.existing);
            }
        }
    }

    fn toggle_admin(&mut self) {
        if self.draft.kind == Some(AccountKind::M365) {
            self.draft.admin = !self.draft.admin;
            self.consented = None;
        }
    }

    fn choose(&mut self, index: usize) -> Vec<Action> {
        if self.busy.is_some() {
            return Vec::new();
        }
        match (self.step, index) {
            (Step::Start, 0) => {
                self.go(Step::Preset);
                self.busy = Some(Busy::LoadingPresets);
                vec![Action::LoadPresets]
            }
            (Step::Start, 1) => {
                match self.kind_hint {
                    Some(kind) => self.pick_provider(kind),
                    None => self.go(Step::Provider),
                }
                Vec::new()
            }
            (Step::Preset, i) => {
                if let Some(preset) = self.presets.get(i).cloned() {
                    self.use_preset(&preset);
                }
                Vec::new()
            }
            (Step::Provider, 0) => {
                self.pick_provider(AccountKind::Gmail);
                Vec::new()
            }
            (Step::Provider, 1) => {
                self.pick_provider(AccountKind::M365);
                Vec::new()
            }
            (Step::Done, 0) => {
                self.add_another();
                Vec::new()
            }
            (Step::Done, 1) => vec![Action::Quit],
            (Step::Keys, i) => self.choose_keys(i),
            _ => Vec::new(),
        }
    }

    /// Rail's own keys need nothing written; any other preset is saved before Done.
    fn choose_keys(&mut self, index: usize) -> Vec<Action> {
        match KeymapPreset::ALL.get(index) {
            Some(KeymapPreset::Rail) => {
                self.go(Step::Done);
                Vec::new()
            }
            Some(&preset) if self.busy.is_none() => {
                let Some(report) = &self.report else {
                    self.go(Step::Done);
                    return Vec::new();
                };
                let config_path = report.config_path.clone();
                self.keys = Some(preset);
                self.busy = Some(Busy::SavingKeys);
                vec![Action::SaveKeymap {
                    preset,
                    config_path,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn pick_provider(&mut self, kind: AccountKind) {
        if self.draft.kind != Some(kind) || self.draft.preset.is_some() {
            self.draft = AccountDraft::for_kind(kind);
        }
        self.go(Step::Instructions);
    }

    pub(super) fn use_preset(&mut self, preset: &Preset) {
        self.draft = AccountDraft::from_preset(preset);
        self.enter_details();
    }

    fn enter_details(&mut self) {
        self.errors = self.draft.validate(&self.existing);
        self.show_errors = false;
        self.go(Step::Details);
    }

    /// Back to Start for another account; the config now includes the ones just saved.
    fn add_another(&mut self) {
        let existing = std::mem::take(&mut self.existing);
        let added = std::mem::take(&mut self.added);
        *self = Self::new(Some(existing), None);
        self.added = added;
    }

    fn next(&mut self) -> Vec<Action> {
        if self.busy.is_some() {
            return Vec::new();
        }
        match self.step {
            Step::Preset => self.load_preset_source(),
            Step::Instructions => {
                self.enter_details();
                Vec::new()
            }
            Step::Details => self.submit_details(),
            Step::ConsentTenant => {
                self.submit_consent_tenant();
                Vec::new()
            }
            Step::AdminConsent => self.consent_or_continue(),
            Step::Start | Step::Provider | Step::Login | Step::Keys | Step::Done => Vec::new(),
        }
    }

    /// A typed path is read; anything with a scheme is fetched, and the fetch refuses
    /// everything but https.
    fn load_preset_source(&mut self) -> Vec<Action> {
        let source = self.preset_source.trim().to_string();
        if source.is_empty() {
            return Vec::new();
        }
        self.clear_status();
        self.busy = Some(Busy::FetchingPreset);
        if source.contains("://") {
            vec![Action::FetchPreset(source)]
        } else {
            vec![Action::ReadPreset(PathBuf::from(source))]
        }
    }

    fn submit_details(&mut self) -> Vec<Action> {
        self.errors = self.draft.validate(&self.existing);
        self.show_errors = true;
        if !self.errors.is_empty() {
            return Vec::new();
        }
        let wants_consent =
            self.draft.kind == Some(AccountKind::M365) && self.draft.admin && self.needs_consent();
        if wants_consent {
            self.enter_consent();
            return Vec::new();
        }
        self.go(Step::Login);
        self.start_login()
    }

    /// Admin consent needs one organisation's tenant. Ask for it when the draft has none.
    pub(super) fn enter_consent(&mut self) {
        if self.draft.has_concrete_tenant() {
            self.consent_tenant = self.draft.tenant.trim().to_string();
            self.go(Step::AdminConsent);
        } else {
            self.consent_tenant = self.draft.domain().unwrap_or_default().to_ascii_lowercase();
            self.go(Step::ConsentTenant);
        }
    }

    fn submit_consent_tenant(&mut self) {
        if is_concrete_tenant(&self.consent_tenant) {
            self.errors.remove(&Field::ConsentTenant);
            self.consent_tenant = self.consent_tenant.trim().to_string();
            self.go(Step::AdminConsent);
        } else {
            self.errors.insert(
                Field::ConsentTenant,
                "enter your organisation's tenant ID or domain, not common or organizations"
                    .to_string(),
            );
        }
    }

    fn consent_or_continue(&mut self) -> Vec<Action> {
        if !self.needs_consent() {
            self.go(Step::Login);
            return self.start_login();
        }
        self.start_consent()
    }

    pub(super) fn start_consent(&mut self) -> Vec<Action> {
        self.clear_status();
        self.busy = Some(Busy::Consent);
        vec![Action::RunAdminConsent {
            client_id: self.draft.client_id.trim().to_string(),
            tenant: self.consent_tenant.clone(),
        }]
    }

    pub(super) fn start_login(&mut self) -> Vec<Action> {
        let Some(account) = self.draft.to_account() else {
            return Vec::new();
        };
        self.clear_status();
        self.tokens = None;
        self.busy = Some(Busy::Login);
        vec![Action::RunLogin(account)]
    }

    pub(super) fn start_persist(&mut self, tokens: TokenSet) -> Vec<Action> {
        let Some(account) = self.draft.to_account() else {
            return Vec::new();
        };
        self.clear_status();
        self.busy = Some(Busy::Persisting);
        self.tokens = Some(tokens.clone());
        vec![Action::Persist { account, tokens }]
    }

    /// Retry the failed action. After a failed persist the kept tokens are reused, so the
    /// user fixes the environment and retries without logging in again.
    fn retry(&mut self) -> Vec<Action> {
        if self.busy.is_some() || self.failure.is_none() {
            return Vec::new();
        }
        match self.step {
            Step::Login => match self.tokens.clone() {
                Some(tokens) => self.start_persist(tokens),
                None => self.start_login(),
            },
            Step::AdminConsent => self.start_consent(),
            Step::Preset => self.load_preset_source(),
            _ => Vec::new(),
        }
    }

    fn open_portal(&self) -> Vec<Action> {
        let url = match (self.step, self.draft.kind) {
            (Step::Instructions, Some(kind)) => Some(portal_url(kind)),
            _ => self
                .draft
                .help_url
                .as_deref()
                .and_then(|help| Url::parse(help).ok()),
        };
        url.map(|u| vec![Action::OpenUrl(u)]).unwrap_or_default()
    }

    /// The consent URL a failure asks to send on, else the login URL on the Login step. The
    /// screen reports whether the copy worked once it has run.
    fn copy_consent_url(&self) -> Vec<Action> {
        let url = match (&self.failure, &self.url) {
            (Some((_, Remedy::SendConsentUrl(url))), _) => url,
            (_, Some(url)) if self.step == Step::Login => url,
            _ => return Vec::new(),
        };
        vec![Action::Copy(url.to_string())]
    }
}
