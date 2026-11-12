//! What the wizard screen draws, derived from the wizard's state. Pure data: the binary
//! decides layout and keys.

use std::fmt;

use super::draft::Field;
use super::events::{Failure, Remedy};
use super::instructions::{instructions, portal_url};
use super::preset::Preset;
use super::wizard::{Busy, Consent, DONE_CHOICES, PROVIDER_CHOICES, START_CHOICES, Step, Wizard};
use crate::config::{AccountKind, KeymapPreset};
use crate::error::ErrorClass;

/// The step list the screen shows on the left.
pub const STEP_LABELS: [&str; 7] = [
    "Start",
    "Provider",
    "Instructions",
    "Details",
    "Admin consent",
    "Login",
    "Done",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Done,
    Current,
    Upcoming,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub steps: Vec<(&'static str, StepState)>,
    pub title: String,
    pub body: Body,
    pub notice: Option<String>,
    pub failure: Option<FailureView>,
    /// What is in flight, for the spinner.
    pub busy: Option<String>,
    /// The login or consent URL, for manual copy.
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Choices(Vec<String>),
    Presets {
        choices: Vec<String>,
        problems: Vec<String>,
        source: FieldView,
    },
    Instructions {
        text: String,
        portal: String,
    },
    Fields(Vec<FieldView>),
    Message(String),
    Report {
        lines: Vec<(String, String)>,
        choices: Vec<String>,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct FieldView {
    pub field: Field,
    pub label: &'static str,
    pub value: String,
    /// Draw dots instead of the value.
    pub masked: bool,
    /// `Some(checked)` for a checkbox.
    pub checkbox: Option<bool>,
    pub error: Option<String>,
    pub hint: Option<String>,
}

/// Hides the value of a masked field.
impl fmt::Debug for FieldView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value: &str = if self.masked {
            "<redacted>"
        } else {
            &self.value
        };
        f.debug_struct("FieldView")
            .field("field", &self.field)
            .field("value", &value)
            .field("checkbox", &self.checkbox)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureView {
    /// The provider's description, prefixed with the error class.
    pub text: String,
    pub primary: Primary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Primary {
    Retry,
    /// Copy this admin-consent URL to send to an administrator.
    CopyUrl(String),
}

impl Wizard {
    pub fn view_model(&self) -> ViewModel {
        ViewModel {
            steps: self.step_states(),
            title: self.title(),
            body: self.body(),
            notice: self.notice.clone(),
            failure: self.failure.as_ref().map(|(f, r)| failure_view(f, r)),
            busy: self.busy.map(|b| self.busy_label(b)),
            url: self.url.as_ref().map(ToString::to_string),
        }
    }

    fn step_states(&self) -> Vec<(&'static str, StepState)> {
        let current = step_index(self.step);
        STEP_LABELS
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let state = if index == current {
                    StepState::Current
                } else if self.skips(index) {
                    StepState::Skipped
                } else if index < current {
                    StepState::Done
                } else {
                    StepState::Upcoming
                };
                (*label, state)
            })
            .collect()
    }

    fn skips(&self, index: usize) -> bool {
        let preset = self.draft.preset.is_some();
        let past_start = !matches!(self.step, Step::Start | Step::Preset);
        match index {
            1 => preset || (self.kind_hint.is_some() && past_start),
            2 => preset,
            4 => !(self.draft.kind == Some(AccountKind::M365) && self.draft.admin),
            _ => false,
        }
    }

    fn title(&self) -> String {
        let title = match self.step {
            Step::Start => "How do you want to set up your account?",
            Step::Preset => "Choose your organisation's preset",
            Step::Provider => "Which provider hosts your mail?",
            Step::Instructions => "Register Rail with your provider",
            Step::Details => match &self.draft.preset {
                Some(preset) => return format!("Account details ({preset})"),
                None => "Account details",
            },
            Step::ConsentTenant => "Which organisation are you approving Rail for?",
            Step::AdminConsent => "Grant admin consent",
            Step::Login => "Log in",
            Step::Keys => "Choose your keys",
            Step::Done => "All set",
        };
        title.to_string()
    }

    fn body(&self) -> Body {
        match self.step {
            Step::Start => Body::Choices(strings(&START_CHOICES)),
            Step::Provider => Body::Choices(strings(&PROVIDER_CHOICES)),
            Step::Preset => self.presets_body(),
            Step::Instructions => {
                let kind = self.draft.kind.unwrap_or(AccountKind::Gmail);
                Body::Instructions {
                    text: instructions(kind),
                    portal: portal_url(kind).to_string(),
                }
            }
            Step::Details => Body::Fields(
                self.draft
                    .fields()
                    .into_iter()
                    .map(|field| self.field_view(field))
                    .collect(),
            ),
            Step::ConsentTenant => Body::Fields(vec![self.consent_tenant_view()]),
            Step::AdminConsent => Body::Message(self.consent_message()),
            Step::Login => Body::Message(self.login_message()),
            Step::Keys => Body::Choices(KeymapPreset::ALL.iter().map(|p| p.label()).collect()),
            Step::Done => self.report_body(),
        }
    }

    fn presets_body(&self) -> Body {
        Body::Presets {
            choices: self.presets.iter().map(Preset::label).collect(),
            problems: self.preset_problems.clone(),
            source: FieldView {
                field: Field::PresetSource,
                label: Field::PresetSource.label(),
                value: self.preset_source.clone(),
                masked: false,
                checkbox: None,
                error: None,
                hint: Some("a preset file, or the https:// URL your organisation gave you".into()),
            },
        }
    }

    fn field_view(&self, field: Field) -> FieldView {
        let value = self.draft.value(field).to_string();
        let show_error = self.show_errors || !value.is_empty();
        FieldView {
            field,
            label: field.label(),
            masked: matches!(
                field,
                Field::ClientSecret | Field::Password | Field::SmtpPassword
            ),
            checkbox: (field == Field::Admin).then_some(self.draft.admin),
            error: show_error
                .then(|| self.errors.get(&field).cloned())
                .flatten(),
            hint: self.field_hint(field),
            value,
        }
    }

    fn field_hint(&self, field: Field) -> Option<String> {
        let microsoft = self.draft.kind == Some(AccountKind::M365);
        match field {
            Field::Name => Some("used in the UI and in config.toml".into()),
            Field::Color => Some("optional, for example blue".into()),
            Field::ClientSecret if microsoft => Some("optional for Microsoft".into()),
            Field::ClientSecret => Some("required for Gmail".into()),
            Field::ImapPort => Some("default 993".into()),
            Field::SmtpPort => Some("default 465".into()),
            Field::SmtpPassword => Some("optional if same as IMAP".into()),
            Field::Tenant => Some(format!(
                "empty means common; try {}",
                self.draft.tenant_suggestions().join(", ")
            )),
            _ => None,
        }
    }

    fn consent_tenant_view(&self) -> FieldView {
        let hint = match self.draft.domain() {
            Some(domain) => format!("your tenant ID or domain, for example {domain}"),
            None => "your tenant ID or domain".to_string(),
        };
        FieldView {
            field: Field::ConsentTenant,
            label: Field::ConsentTenant.label(),
            value: self.consent_tenant.clone(),
            masked: false,
            checkbox: None,
            error: self.errors.get(&Field::ConsentTenant).cloned(),
            hint: Some(hint),
        }
    }

    fn consent_message(&self) -> String {
        if let Some(Consent { tenant, .. }) = &self.consented {
            return format!("Consent granted for {tenant}. Press Enter to log in.");
        }
        if self.busy == Some(Busy::Consent) {
            return "Approve Rail in your browser, signed in as an administrator. Waiting for the consent page to finish.".to_string();
        }
        format!(
            "Rail needs an administrator to approve it once for everyone in {}. Press Enter to open Microsoft's admin consent page in your browser.",
            self.consent_tenant
        )
    }

    fn login_message(&self) -> String {
        match self.busy {
            Some(Busy::Persisting) => "Saving the account.".to_string(),
            Some(Busy::Login) if self.draft.kind == Some(AccountKind::ImapSmtp) => format!(
                "Testing IMAP and SMTP connections for {}.",
                self.draft.address.trim()
            ),
            Some(Busy::Login) => format!(
                "Log in as {} in your browser. Waiting for the browser to finish.",
                self.draft.address.trim()
            ),
            _ => {
                "The login did not finish. Retry it, or go back to change the details.".to_string()
            }
        }
    }

    fn report_body(&self) -> Body {
        let mut lines = Vec::new();
        if let Some(account) = self.existing.accounts.last() {
            let who = format!("{} <{}>", account.name, account.address);
            lines.push(("Account".to_string(), who));
        }
        if let Some(report) = &self.report {
            let tokens = format!("{} ({})", report.token_backend, report.token_location);
            lines.push((
                "Config".to_string(),
                report.config_path.display().to_string(),
            ));
            lines.push(("Tokens".to_string(), tokens));
            lines.push(("Store".to_string(), report.store_path.display().to_string()));
        }
        if let Some(name) = self.added.last() {
            lines.push(("Re-login".to_string(), format!("rail account login {name}")));
        }
        if let Some(keys) = self.keys {
            lines.push(("Keys".to_string(), keys.as_str().to_string()));
        }
        Body::Report {
            lines,
            choices: strings(&DONE_CHOICES),
        }
    }

    fn busy_label(&self, busy: Busy) -> String {
        let label = match busy {
            Busy::LoadingPresets => "Looking for presets",
            Busy::FetchingPreset => "Loading the preset",
            Busy::Consent => "Waiting for admin consent",
            Busy::Login if self.draft.kind == Some(AccountKind::ImapSmtp) => {
                "Testing IMAP and SMTP connections"
            }
            Busy::Login => "Waiting for the browser login",
            Busy::Persisting => "Saving",
            Busy::SavingKeys => "Saving the keys",
        };
        label.to_string()
    }
}

fn step_index(step: Step) -> usize {
    match step {
        Step::Start | Step::Preset => 0,
        Step::Provider => 1,
        Step::Instructions => 2,
        Step::Details => 3,
        Step::ConsentTenant | Step::AdminConsent => 4,
        Step::Login => 5,
        Step::Keys | Step::Done => 6,
    }
}

fn failure_view(failure: &Failure, remedy: &Remedy) -> FailureView {
    let class = match failure.class {
        ErrorClass::Transient => "Temporary problem",
        ErrorClass::Permanent => "Error",
    };
    let primary = match remedy {
        Remedy::Retry => Primary::Retry,
        Remedy::SendConsentUrl(url) => Primary::CopyUrl(url.to_string()),
    };
    FailureView {
        text: format!("{class}: {}", failure.message),
        primary,
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::events::{Failure, Input, Remedy};
    use crate::test_support::{done_report, edit, gmail_details, own_app, walk, wizard};

    fn details(w: &ViewModel) -> &[FieldView] {
        match &w.body {
            Body::Fields(fields) => fields,
            other => panic!("expected fields, got {other:?}"),
        }
    }

    #[test]
    fn steps_show_progress_and_skips() {
        let mut w = wizard();
        walk(&mut w, own_app(0));
        let states: Vec<StepState> = w.view_model().steps.iter().map(|(_, s)| *s).collect();
        assert_eq!(
            states,
            vec![
                StepState::Done,
                StepState::Done,
                StepState::Done,
                StepState::Current,
                StepState::Skipped,
                StepState::Upcoming,
                StepState::Upcoming,
            ]
        );
        assert_eq!(w.view_model().title, "Account details");
    }

    #[test]
    fn errors_show_once_a_field_has_text_or_next_was_pressed() {
        let mut w = wizard();
        walk(&mut w, own_app(0));
        let model = w.view_model();
        assert!(
            details(&model).iter().all(|f| f.error.is_none()),
            "untouched fields are quiet"
        );
        walk(&mut w, [edit(Field::Address, "nope")]);
        let model = w.view_model();
        assert!(
            details(&model)[0]
                .error
                .as_deref()
                .unwrap()
                .contains("not an email address")
        );
        walk(&mut w, [Input::Next]);
        let model = w.view_model();
        let secret = details(&model)
            .iter()
            .find(|f| f.field == Field::ClientSecret)
            .unwrap();
        assert!(secret.error.is_some(), "Next reveals every error");
    }

    #[test]
    fn the_secret_field_is_masked_and_its_debug_is_redacted() {
        let mut w = wizard();
        walk(&mut w, [own_app(0), gmail_details("me@gmail.com")].concat());
        let model = w.view_model();
        let secret = details(&model)
            .iter()
            .find(|f| f.field == Field::ClientSecret)
            .unwrap();
        assert!(secret.masked);
        assert!(secret.value == "sec", "the value is kept for editing");
        assert!(!format!("{secret:?}").contains("sec\""), "{secret:?}");
        assert!(!format!("{model:?}").contains("\"sec\""), "{model:?}");
    }

    #[test]
    fn failure_view_names_the_class_and_the_primary_action() {
        let mut w = wizard();
        w.failure = Some((
            Failure::new("token endpoint returned HTTP 503", ErrorClass::Transient),
            Remedy::Retry,
        ));
        let failure = w.view_model().failure.unwrap();
        assert!(
            failure.text.starts_with("Temporary problem: "),
            "{}",
            failure.text
        );
        assert_eq!(failure.primary, Primary::Retry);
        let url = url::Url::parse("https://login.microsoftonline.com/x").unwrap();
        w.failure = Some((
            Failure::new("denied", ErrorClass::Permanent),
            Remedy::SendConsentUrl(url.clone()),
        ));
        let failure = w.view_model().failure.unwrap();
        assert!(
            failure.text.starts_with("Error: denied"),
            "{}",
            failure.text
        );
        assert_eq!(failure.primary, Primary::CopyUrl(url.to_string()));
    }

    #[test]
    fn instructions_body_carries_the_text_and_the_portal() {
        let mut w = wizard();
        walk(&mut w, [Input::Choose(1), Input::Choose(0)]);
        match w.view_model().body {
            Body::Instructions { text, portal } => {
                assert!(text.contains("Desktop app"));
                assert!(portal.starts_with("https://console.cloud.google.com"));
            }
            other => panic!("expected instructions, got {other:?}"),
        }
    }

    #[test]
    fn done_body_reports_paths_and_the_login_command() {
        let mut w = wizard();
        walk(&mut w, [own_app(0), gmail_details("me@gmail.com")].concat());
        w.existing.accounts.push(w.draft.to_account().unwrap());
        w.added.push("me".into());
        w.report = Some(done_report());
        w.go(Step::Done);
        match w.view_model().body {
            Body::Report { lines, choices } => {
                let text: Vec<String> = lines.iter().map(|(k, v)| format!("{k}={v}")).collect();
                assert!(
                    text.contains(&"Account=me <me@gmail.com>".to_string()),
                    "{text:?}"
                );
                assert!(
                    text.iter()
                        .any(|l| l.starts_with("Config=/home/me/.config/rail/config.toml"))
                );
                assert!(text.iter().any(|l| l.starts_with("Tokens=file (")));
                assert!(text.iter().any(|l| l.starts_with("Store=")));
                assert!(
                    text.contains(&"Re-login=rail account login me".to_string()),
                    "{text:?}"
                );
                assert_eq!(choices, vec!["Add another account", "Finish"]);
            }
            other => panic!("expected a report, got {other:?}"),
        }
    }
}
