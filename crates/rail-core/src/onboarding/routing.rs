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
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth::AuthError;
    use crate::config::{AccountKind, KeymapPreset};
    use crate::error::ErrorClass;
    use crate::onboarding::draft::Field;
    use crate::onboarding::events::Input;
    use crate::onboarding::view::{Body, Primary};
    use crate::test_support::{
        contoso_preset, done_report, edit, gmail_details, m365_details, own_app, tokens, walk,
        wizard,
    };

    fn consent_failure() -> Failure {
        Failure {
            message: "authorization denied by the provider: AADSTS65001: The user or administrator has not consented".into(),
            class: ErrorClass::Permanent,
            consent_required: true,
        }
    }

    /// A Microsoft account on the bring-your-own path, at Details with everything filled in.
    fn microsoft(admin: bool, tenant: &str) -> Wizard {
        let mut w = wizard();
        let mut inputs = [own_app(1), m365_details("me@contoso.com", tenant)].concat();
        if admin {
            inputs.push(Input::ToggleAdmin);
        }
        walk(&mut w, inputs);
        w
    }

    /// A Gmail account waiting for its login.
    fn gmail_logging_in() -> Wizard {
        let mut w = wizard();
        walk(
            &mut w,
            [own_app(0), gmail_details("me@gmail.com"), vec![Input::Next]].concat(),
        );
        assert_eq!(w.busy(), Some(Busy::Login));
        w
    }

    #[test]
    fn consent_required_routes_an_administrator_to_the_consent_step() {
        let mut w = microsoft(true, "contoso.onmicrosoft.com");
        walk(&mut w, [Input::Next]);
        assert_eq!(w.step(), Step::AdminConsent);
        walk(&mut w, [Input::Next]);
        assert!(
            w.apply(Event::AdminConsentFinished(Ok("tid".into())))
                .is_empty()
        );
        let login = walk(&mut w, [Input::Next]);
        assert!(
            matches!(login.as_slice(), [Action::RunLogin(_)]),
            "{login:?}"
        );
        assert_eq!(w.step(), Step::Login);

        let actions = w.apply(Event::LoginFinished(Err(consent_failure())));

        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(w.step(), Step::AdminConsent);
        assert!(
            w.notice().unwrap().contains("administrator"),
            "{:?}",
            w.notice()
        );
        assert!(w.failure().is_none());
        assert_eq!(
            walk(&mut w, [Input::Next]),
            vec![Action::RunAdminConsent {
                client_id: "cid".into(),
                tenant: "contoso.onmicrosoft.com".into()
            }],
            "consent runs again instead of looping on the login"
        );
    }

    #[test]
    fn consent_required_shows_a_non_administrator_the_consent_url() {
        let mut w = microsoft(false, "");
        walk(&mut w, [Input::Next]);
        assert_eq!(w.step(), Step::Login);

        let actions = w.apply(Event::LoginFinished(Err(consent_failure())));

        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(
            w.step(),
            Step::Login,
            "a non-administrator is never sent to consent"
        );
        let failure = w.failure().unwrap();
        assert!(
            failure.message.contains("administrator must approve"),
            "{}",
            failure.message
        );
        let Some(Remedy::SendConsentUrl(url)) = w.remedy().cloned() else {
            panic!("expected a consent URL, got {:?}", w.remedy());
        };
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
        assert_eq!(url.path(), "/contoso.com/v2.0/adminconsent");
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "client_id" && v == "cid")
        );
        assert_eq!(
            w.view_model().failure.unwrap().primary,
            Primary::CopyUrl(url.to_string())
        );
        assert_eq!(
            walk(&mut w, [Input::CopyConsentUrl]),
            vec![Action::Copy(url.to_string())]
        );
        assert!(matches!(
            walk(&mut w, [Input::Retry]).as_slice(),
            [Action::RunLogin(_)]
        ));
    }

    #[test]
    fn shown_urls_outlive_the_step_and_are_forgotten_on_success() {
        let mut w = gmail_logging_in();
        assert_eq!(w.shown_urls(), (None, None));
        let login = url::Url::parse("https://accounts.example/auth").unwrap();
        w.apply(Event::LoginUrl(login.clone()));
        assert_eq!(w.shown_urls(), (None, Some(login.clone())));
        walk(&mut w, [Input::Back]);
        assert_eq!(w.view_model().url, None, "the step no longer shows it");
        assert_eq!(w.shown_urls(), (None, Some(login)), "the last one is kept");

        let mut w = gmail_logging_in();
        w.apply(Event::LoginUrl(
            url::Url::parse("https://accounts.example/auth").unwrap(),
        ));
        w.apply(Event::LoginFinished(Ok(tokens())));
        assert_eq!(w.shown_urls(), (None, None), "a finished login needs none");
    }

    #[test]
    fn the_consent_url_for_an_administrator_counts_as_shown() {
        let mut w = microsoft(false, "");
        walk(&mut w, [Input::Next]);
        w.apply(Event::LoginFinished(Err(consent_failure())));
        let Some(Remedy::SendConsentUrl(url)) = w.remedy().cloned() else {
            panic!("expected a consent URL");
        };
        assert_eq!(w.shown_urls(), (Some(url), None));
    }

    #[test]
    fn copy_on_the_login_step_copies_the_login_url() {
        let mut w = gmail_logging_in();
        assert!(
            walk(&mut w, [Input::CopyConsentUrl]).is_empty(),
            "no URL yet"
        );
        let login = url::Url::parse("https://accounts.example/auth").unwrap();
        w.apply(Event::LoginUrl(login.clone()));
        assert_eq!(
            walk(&mut w, [Input::CopyConsentUrl]),
            vec![Action::Copy(login.to_string())]
        );
    }

    #[test]
    fn a_successful_login_persists_and_then_reports_done() {
        let mut w = gmail_logging_in();
        let actions = w.apply(Event::LoginUrl(
            url::Url::parse("https://accounts.example/auth").unwrap(),
        ));
        assert!(actions.is_empty());
        assert_eq!(
            w.view_model().url.as_deref(),
            Some("https://accounts.example/auth")
        );

        let actions = w.apply(Event::LoginFinished(Ok(tokens())));
        let [
            Action::Persist {
                account,
                tokens: saved,
            },
        ] = actions.as_slice()
        else {
            panic!("expected a persist, got {actions:?}");
        };
        assert_eq!(account.address, "me@gmail.com");
        assert_eq!(*saved, tokens());
        assert_eq!(w.busy(), Some(Busy::Persisting));
        assert!(
            walk(&mut w, [Input::Back]).is_empty(),
            "no going back while saving"
        );

        assert!(w.apply(Event::Persisted(Ok(done_report()))).is_empty());
        assert_eq!(w.step(), Step::Done);
        assert_eq!(w.added(), ["me".to_string()]);
        assert!(
            w.existing()
                .accounts
                .iter()
                .any(|a| a.address == "me@gmail.com")
        );
        assert!(walk(&mut w, [Input::Back]).is_empty());
        assert_eq!(w.step(), Step::Done);
    }

    /// A first run (no account in the config yet), its Gmail account saved.
    fn first_run_saved() -> Wizard {
        let mut w = Wizard::new(None, None);
        walk(
            &mut w,
            [own_app(0), gmail_details("me@gmail.com"), vec![Input::Next]].concat(),
        );
        w.apply(Event::LoginFinished(Ok(tokens())));
        assert!(w.apply(Event::Persisted(Ok(done_report()))).is_empty());
        w
    }

    #[test]
    fn a_first_run_asks_for_keys_after_the_account_is_saved() {
        let mut w = first_run_saved();
        assert_eq!(w.step(), Step::Keys);
        let model = w.view_model();
        assert_eq!(model.title, "Choose your keys");
        let Body::Choices(choices) = &model.body else {
            panic!("choices, got {:?}", model.body)
        };
        assert_eq!(choices.len(), KeymapPreset::ALL.len());
        assert!(choices[0].starts_with("rail — "), "{choices:?}");
        assert!(choices[1].starts_with("vim — "), "{choices:?}");
        assert!(
            walk(&mut w, [Input::Back]).is_empty(),
            "the account is saved"
        );
        assert_eq!(w.step(), Step::Keys);

        let actions = walk(&mut w, [Input::Choose(1)]);
        assert_eq!(
            actions,
            vec![Action::SaveKeymap {
                preset: KeymapPreset::Vim,
                config_path: done_report().config_path,
            }]
        );
        assert_eq!(w.busy(), Some(Busy::SavingKeys));
        assert!(
            walk(&mut w, [Input::Back]).is_empty(),
            "no going back while saving"
        );
        assert!(w.apply(Event::KeymapSaved(Ok(()))).is_empty());
        assert_eq!(w.step(), Step::Done);
        let Body::Report { lines, .. } = w.view_model().body else {
            panic!("the report")
        };
        assert!(
            lines.contains(&("Keys".to_string(), "vim".to_string())),
            "{lines:?}"
        );
    }

    #[test]
    fn rails_own_keys_need_nothing_saved() {
        let mut w = first_run_saved();
        assert!(walk(&mut w, [Input::Choose(0)]).is_empty());
        assert_eq!(w.step(), Step::Done);
        let Body::Report { lines, .. } = w.view_model().body else {
            panic!("the report")
        };
        assert!(lines.iter().all(|(name, _)| name != "Keys"), "{lines:?}");
    }

    /// The account is saved either way, so a failed keys save finishes too and says how to
    /// choose them later.
    #[test]
    fn a_failed_keys_save_still_finishes_and_says_so() {
        let mut w = first_run_saved();
        walk(&mut w, [Input::Choose(2)]);
        let failure = Failure::new("config.toml is read-only", ErrorClass::Permanent);
        w.apply(Event::KeymapSaved(Err(failure)));
        assert_eq!(w.step(), Step::Done);
        let notice = w.view_model().notice.unwrap_or_default();
        assert!(notice.contains("config.toml is read-only"), "{notice}");
        assert!(notice.contains(":keymap"), "{notice}");
    }

    /// Only the first run asks: a config that has an account, or a second account added in
    /// the same session, goes straight to Done.
    #[test]
    fn only_the_first_account_of_a_first_run_asks_for_keys() {
        let mut w = gmail_logging_in();
        w.apply(Event::LoginFinished(Ok(tokens())));
        w.apply(Event::Persisted(Ok(done_report())));
        assert_eq!(w.step(), Step::Done);

        let mut w = first_run_saved();
        walk(&mut w, [Input::Choose(0)]);
        walk(&mut w, [Input::Choose(0)]);
        assert_eq!(w.step(), Step::Start, "add another");
        walk(
            &mut w,
            [
                own_app(0),
                gmail_details("two@gmail.com"),
                vec![Input::Next],
            ]
            .concat(),
        );
        w.apply(Event::LoginFinished(Ok(tokens())));
        w.apply(Event::Persisted(Ok(done_report())));
        assert_eq!(w.step(), Step::Done);
    }

    #[test]
    fn add_another_returns_to_start_knowing_the_new_account() {
        let mut w = gmail_logging_in();
        w.apply(Event::LoginFinished(Ok(tokens())));
        w.apply(Event::Persisted(Ok(done_report())));
        walk(&mut w, [Input::Choose(0)]);
        assert_eq!(w.step(), Step::Start);
        assert_eq!(
            w.added(),
            ["me".to_string()],
            "the session's accounts are kept"
        );
        walk(
            &mut w,
            [own_app(0), gmail_details("ME@gmail.com"), vec![Input::Next]].concat(),
        );
        assert_eq!(w.step(), Step::Details);
        assert!(w.errors()[&Field::Address].contains("already configured"));
    }

    #[test]
    fn finish_quits() {
        let mut w = gmail_logging_in();
        w.apply(Event::LoginFinished(Ok(tokens())));
        w.apply(Event::Persisted(Ok(done_report())));
        assert_eq!(walk(&mut w, [Input::Choose(1)]), vec![Action::Quit]);
    }

    #[test]
    fn a_transient_login_failure_offers_retry_first() {
        let mut w = gmail_logging_in();
        let failure = Failure::new(
            "network error talking to the token endpoint: timed out",
            ErrorClass::Transient,
        );
        assert!(w.apply(Event::LoginFinished(Err(failure))).is_empty());
        let view = w.view_model().failure.unwrap();
        assert!(
            view.text.starts_with("Temporary problem: network error"),
            "{}",
            view.text
        );
        assert_eq!(view.primary, Primary::Retry);
        let retry = walk(&mut w, [Input::Retry]);
        assert!(
            matches!(retry.as_slice(), [Action::RunLogin(_)]),
            "{retry:?}"
        );
        assert!(w.failure().is_none());
    }

    #[test]
    fn a_persist_failure_keeps_the_draft_and_retries_the_persist() {
        let mut w = gmail_logging_in();
        w.apply(Event::LoginFinished(Ok(tokens())));
        let message = "storing tokens: keyring unavailable: no secret service. Set `secrets = \"file\"` under [general] in config.toml to keep tokens in a private file instead";
        assert!(
            w.apply(Event::Persisted(Err(Failure::new(
                message,
                ErrorClass::Permanent
            ))))
            .is_empty()
        );
        assert_eq!(w.step(), Step::Login);
        assert!(
            w.view_model()
                .failure
                .unwrap()
                .text
                .contains("secrets = \"file\"")
        );
        assert_eq!(w.draft().address, "me@gmail.com");
        let retry = walk(&mut w, [Input::Retry]);
        assert!(
            matches!(retry.as_slice(), [Action::Persist { tokens: t, .. }] if *t == tokens()),
            "the kept tokens are persisted again, with no new login: {retry:?}"
        );
    }

    #[test]
    fn stale_events_are_ignored() {
        let mut w = gmail_logging_in();
        walk(&mut w, [Input::Back]);
        assert!(w.apply(Event::LoginFinished(Ok(tokens()))).is_empty());
        assert_eq!(w.step(), Step::Details);
        assert!(w.apply(Event::Persisted(Ok(done_report()))).is_empty());
        assert!(
            w.apply(Event::PresetFetched(Ok(contoso_preset())))
                .is_empty()
        );
        assert!(
            w.apply(Event::AdminConsentFinished(Ok("t".into())))
                .is_empty()
        );
        assert_eq!(w.step(), Step::Details);
        assert!(w.added().is_empty());
    }

    #[test]
    fn loaded_presets_are_listed_with_their_problems() {
        let mut w = wizard();
        walk(&mut w, [Input::Choose(0)]);
        let entries = vec![
            PresetEntry {
                source: PathBuf::from("/etc/rail/presets/bad.toml"),
                preset: Err("/etc/rail/presets/bad.toml:1:8: unknown variant `imap`".into()),
            },
            PresetEntry {
                source: PathBuf::from("/etc/rail/presets/contoso.toml"),
                preset: Ok(contoso_preset()),
            },
        ];
        assert!(w.apply(Event::PresetsLoaded(entries)).is_empty());
        assert_eq!(w.busy(), None);
        match w.view_model().body {
            crate::onboarding::view::Body::Presets {
                choices, problems, ..
            } => {
                assert_eq!(choices, vec!["Contoso Microsoft 365"]);
                assert!(problems[0].contains("bad.toml:1:8"), "{problems:?}");
            }
            other => panic!("expected presets, got {other:?}"),
        }
        walk(&mut w, [Input::Choose(0)]);
        assert_eq!(w.step(), Step::Details);
        assert_eq!(w.draft().kind, Some(AccountKind::M365));
    }

    #[test]
    fn no_presets_found_says_where_to_type_one() {
        let mut w = wizard();
        walk(&mut w, [Input::Choose(0)]);
        w.apply(Event::PresetsLoaded(Vec::new()));
        assert!(
            w.notice().unwrap().contains("https:// URL"),
            "{:?}",
            w.notice()
        );
    }

    #[test]
    fn a_fetched_preset_fills_the_draft_and_a_refused_one_can_be_retried() {
        let mut w = wizard();
        walk(&mut w, [Input::Choose(0)]);
        w.apply(Event::PresetsLoaded(Vec::new()));
        let fetch = walk(
            &mut w,
            [
                edit(Field::PresetSource, "http://intranet/rail.toml"),
                Input::Next,
            ],
        );
        assert_eq!(
            fetch,
            vec![Action::FetchPreset("http://intranet/rail.toml".into())]
        );
        let refused = Failure::new(
            "refusing to fetch the preset from http://intranet/rail.toml: it must be an https:// URL",
            ErrorClass::Permanent,
        );
        assert!(w.apply(Event::PresetFetched(Err(refused))).is_empty());
        assert_eq!(w.step(), Step::Preset);
        assert!(w.failure().unwrap().message.contains("https://"));
        walk(
            &mut w,
            [edit(Field::PresetSource, "https://intranet/rail.toml")],
        );
        assert_eq!(
            walk(&mut w, [Input::Retry]),
            vec![Action::FetchPreset("https://intranet/rail.toml".into())]
        );
        assert!(
            w.apply(Event::PresetFetched(Ok(contoso_preset())))
                .is_empty()
        );
        assert_eq!(w.step(), Step::Details);
        assert_eq!(w.draft().client_id, "contoso-app");
    }

    #[test]
    fn a_failed_consent_can_be_retried_and_a_granted_one_moves_on() {
        let mut w = microsoft(true, "contoso.onmicrosoft.com");
        walk(&mut w, [Input::Next, Input::Next]);
        let url = url::Url::parse("https://login.microsoftonline.com/consent").unwrap();
        w.apply(Event::ConsentUrl(url));
        assert!(w.view_model().url.is_some());
        let denied = Failure::new(
            "authorization denied by the provider: AADSTS65004",
            ErrorClass::Permanent,
        );
        assert!(w.apply(Event::AdminConsentFinished(Err(denied))).is_empty());
        assert!(w.failure().is_some());
        assert!(matches!(
            walk(&mut w, [Input::Retry]).as_slice(),
            [Action::RunAdminConsent { .. }]
        ));
        w.apply(Event::AdminConsentFinished(Ok(String::new())));
        match w.view_model().body {
            crate::onboarding::view::Body::Message(text) => {
                assert!(
                    text.contains("Consent granted for contoso.onmicrosoft.com"),
                    "{text}"
                );
            }
            other => panic!("expected a message, got {other:?}"),
        }
    }

    #[test]
    fn a_consent_redirect_without_a_verdict_reads_as_incomplete_consent() {
        let mut w = microsoft(true, "contoso.onmicrosoft.com");
        walk(&mut w, [Input::Next, Input::Next]);
        let missing = Failure::from_auth(&AuthError::MissingCode);

        assert!(
            w.apply(Event::AdminConsentFinished(Err(missing)))
                .is_empty()
        );

        let failure = w.failure().unwrap();
        assert!(
            failure.message.contains("did not complete consent"),
            "{}",
            failure.message
        );
        assert!(
            !failure.message.contains("authorization code"),
            "{}",
            failure.message
        );
        assert_eq!(w.remedy(), Some(&Remedy::Retry));
        assert_eq!(w.step(), Step::AdminConsent);
        assert_eq!(w.busy(), None);
    }

    #[test]
    fn consent_granted_records_the_tenant_the_step_used_whatever_microsoft_returned() {
        for returned in ["", "3f2a-guid"] {
            let mut w = microsoft(true, "contoso.onmicrosoft.com");
            walk(&mut w, [Input::Next, Input::Next]);
            assert!(
                w.apply(Event::AdminConsentFinished(Ok(returned.into())))
                    .is_empty()
            );
            assert_eq!(w.busy(), None);
            let login = walk(&mut w, [Input::Next]);
            assert!(
                matches!(login.as_slice(), [Action::RunLogin(_)]),
                "consent counts as granted, so Next logs in: {login:?}"
            );
            assert_eq!(w.step(), Step::Login);
        }
    }
}
