//! How the wizard reacts to what the outside world reports.

use url::Url;

use super::events::{Action, DoneReport, Event, Failure, PresetEntry, Remedy};
use super::instructions::REDIRECT_URI;
use super::preset::Preset;
use super::wizard::{Busy, Consent, Step, Wizard};
use crate::auth::{AuthError, TokenSet, admin_consent_url};

/// `state` on a consent URL handed to someone else. Their browser's redirect lands nowhere,
/// so the value only has to be present.
const SHARED_CONSENT_STATE: &str = "rail";

/// The consent redirect carried neither `admin_consent` nor an error: the administrator
/// closed the page. "No authorization code" would mislead on this path.
fn consent_failure(failure: Failure) -> Failure {
    if failure.message == AuthError::MissingCode.to_string() {
        return Failure {
            message:
                "The administrator did not complete consent. Ask them to approve Rail, then retry."
                    .to_string(),
            ..failure
        };
    }
    failure
}

impl Wizard {
    /// React to an outcome. An event that does not match the action in flight is stale
    /// (the user went back) and changes nothing.
    pub fn apply(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::PresetsLoaded(entries) => self.presets_loaded(entries),
            Event::PresetFetched(result) => self.preset_fetched(result),
            Event::ConsentUrl(url) => self.show_url(Busy::Consent, url),
            Event::LoginUrl(url) => self.show_url(Busy::Login, url),
            Event::AdminConsentFinished(result) => self.consent_finished(result),
            Event::LoginFinished(result) => return self.login_finished(result),
            Event::ImapSmtpValidated(result) => return self.imapsmtp_validated(result),
            Event::Persisted(result) => self.persisted(result),
            Event::KeymapSaved(result) => self.keymap_saved(result),
        }
        Vec::new()
    }

    /// Clear the busy state if it is `busy`; false means the event is stale.
    fn finish(&mut self, busy: Busy) -> bool {
        if self.busy != Some(busy) {
            return false;
        }
        self.busy = None;
        true
    }

    fn show_url(&mut self, busy: Busy, url: Url) {
        if self.busy != Some(busy) {
            return;
        }
        match busy {
            Busy::Login => self.last_login_url = Some(url.clone()),
            _ => self.last_consent_url = Some(url.clone()),
        }
        self.url = Some(url);
    }

    fn presets_loaded(&mut self, entries: Vec<PresetEntry>) {
        if !self.finish(Busy::LoadingPresets) {
            return;
        }
        self.presets.clear();
        self.preset_problems.clear();
        for entry in entries {
            match entry.preset {
                Ok(preset) => self.presets.push(preset),
                Err(problem) => self.preset_problems.push(problem),
            }
        }
        if self.presets.is_empty() {
            self.notice = Some(
                "No preset files found. Type the path or https:// URL your organisation gave you."
                    .to_string(),
            );
        }
    }

    fn preset_fetched(&mut self, result: Result<Preset, Failure>) {
        if !self.finish(Busy::FetchingPreset) {
            return;
        }
        match result {
            Ok(preset) => self.use_preset(&preset),
            Err(failure) => self.failure = Some((failure, Remedy::Retry)),
        }
    }

    fn consent_finished(&mut self, result: Result<String, Failure>) {
        if !self.finish(Busy::Consent) {
            return;
        }
        match result {
            // Microsoft may omit the tenant or return its GUID; either way consent covers the
            // tenant the step asked about, which is what `needs_consent` compares against.
            Ok(_) => {
                self.last_consent_url = None;
                self.consented = Some(Consent {
                    client_id: self.draft.client_id.trim().to_string(),
                    tenant: self.consent_tenant.trim().to_string(),
                });
            }
            Err(failure) => self.failure = Some((consent_failure(failure), Remedy::Retry)),
        }
    }

    fn login_finished(&mut self, result: Result<TokenSet, Failure>) -> Vec<Action> {
        if !self.finish(Busy::Login) {
            return Vec::new();
        }
        match result {
            Ok(tokens) => {
                self.last_login_url = None;
                self.start_persist(tokens)
            }
            Err(failure) if failure.consent_required => {
                self.route_consent(failure);
                Vec::new()
            }
            Err(failure) => {
                self.failure = Some((failure, Remedy::Retry));
                Vec::new()
            }
        }
    }

    fn imapsmtp_validated(&mut self, result: Result<(), Failure>) -> Vec<Action> {
        if !self.finish(Busy::Login) {
            return Vec::new();
        }
        match result {
            Ok(()) => self.start_persist_imapsmtp(),
            Err(failure) => {
                self.failure = Some((failure, Remedy::Retry));
                Vec::new()
            }
        }
    }

    /// An administrator goes to the consent step; anyone else gets the URL to send to one.
    fn route_consent(&mut self, failure: Failure) {
        if self.draft.admin {
            self.consented = None;
            self.enter_consent();
            self.notice = Some(
                "Your organisation requires an administrator to approve Rail. As an administrator you can grant it now."
                    .to_string(),
            );
            return;
        }
        let url = self.shareable_consent_url();
        let message = format!(
            "An administrator must approve Rail for your organisation. Send them the URL below; once they approve, retry. ({})",
            failure.message
        );
        self.last_consent_url = Some(url.clone());
        self.failure = Some((Failure { message, ..failure }, Remedy::SendConsentUrl(url)));
    }

    /// The admin-consent URL for the draft's tenant, or its address's domain when the draft
    /// says `common`. It redirects to the registered loopback URI without a port.
    fn shareable_consent_url(&self) -> Url {
        let tenant = if self.draft.has_concrete_tenant() {
            self.draft.tenant.trim().to_string()
        } else {
            self.draft
                .domain()
                .map_or_else(|| "organizations".to_string(), str::to_ascii_lowercase)
        };
        admin_consent_url(
            self.draft.client_id.trim(),
            &tenant,
            REDIRECT_URI,
            SHARED_CONSENT_STATE,
        )
    }

    fn persisted(&mut self, result: Result<DoneReport, Failure>) {
        if !self.finish(Busy::Persisting) {
            return;
        }
        match result {
            Ok(report) => {
                if let Some(account) = self.draft.to_account() {
                    self.added.push(account.name.clone());
                    self.existing.accounts.push(account);
                }
                self.tokens = None;
                self.report = Some(report);
                let next = if std::mem::take(&mut self.first_run) {
                    Step::Keys
                } else {
                    Step::Done
                };
                self.go(next);
            }
            Err(failure) => self.failure = Some((failure, Remedy::Retry)),
        }
    }

    /// The account is saved whatever happens here, so a failed save still finishes, saying
    /// how to choose the keys later.
    fn keymap_saved(&mut self, result: Result<(), Failure>) {
        if !self.finish(Busy::SavingKeys) {
            return;
        }
        self.go(Step::Done);
        if let Err(failure) = result {
            self.keys = None;
            self.notice = Some(format!(
                "Keys not saved: {}. Choose them later with :keymap.",
                failure.message
            ));
        }
    }
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
