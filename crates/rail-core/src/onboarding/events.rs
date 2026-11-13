//! The messages between the wizard and the outside world: what the user does (`Input`),
//! what the wizard asks for (`Action`), and how an outcome is described (`Failure`).

use std::fmt;
use std::path::PathBuf;

use url::Url;

use super::draft::Field;
use super::preset::Preset;
use crate::auth::{AuthError, TokenSet};
use crate::config::{AccountConfig, KeymapPreset};
use crate::error::ErrorClass;
use crate::ops::Secret;

/// What the user does.
#[derive(Clone, PartialEq, Eq)]
pub enum Input {
    /// Pick option `n` of the current step's list.
    Choose(usize),
    /// Replace a field's text.
    Edit(Field, String),
    ToggleAdmin,
    Next,
    Back,
    Retry,
    /// Open the provider's portal, or the preset's help link.
    OpenPortal,
    CopyConsentUrl,
    Cancel,
}

/// Hides the text typed into the client secret field.
impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Choose(index) => f.debug_tuple("Choose").field(index).finish(),
            Self::Edit(
                field @ (Field::ClientSecret | Field::Password | Field::SmtpPassword),
                _,
            ) => f
                .debug_tuple("Edit")
                .field(field)
                .field(&"<redacted>")
                .finish(),
            Self::Edit(field, value) => f.debug_tuple("Edit").field(field).field(value).finish(),
            Self::ToggleAdmin => f.write_str("ToggleAdmin"),
            Self::Next => f.write_str("Next"),
            Self::Back => f.write_str("Back"),
            Self::Retry => f.write_str("Retry"),
            Self::OpenPortal => f.write_str("OpenPortal"),
            Self::CopyConsentUrl => f.write_str("CopyConsentUrl"),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}

/// What the wizard asks the outside world to do. `Debug` is safe: `AccountConfig` and
/// `TokenSet` redact their secrets.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Read `*.toml` presets from the known directories.
    LoadPresets,
    ReadPreset(PathBuf),
    FetchPreset(String),
    OpenUrl(Url),
    Copy(String),
    RunAdminConsent {
        client_id: String,
        tenant: String,
    },
    /// Check the account can be added, then run the browser login.
    RunLogin(AccountConfig),
    /// Validate IMAP and SMTP connectivity and credentials.
    ValidateImapSmtp {
        account: AccountConfig,
        password: Secret<String>,
        smtp_password: Option<Secret<String>>,
    },
    /// Tokens, then the store row, then config.toml.
    Persist {
        account: AccountConfig,
        tokens: TokenSet,
    },
    /// Save an ImapSmtp account: passwords into secret store, store row, config.toml.
    PersistImapSmtp {
        account: AccountConfig,
        password: Secret<String>,
        smtp_password: Option<Secret<String>>,
    },
    /// Write `keymap` under `[general]` of the config the account was saved to, once checked
    /// against that config's `[keys]`.
    SaveKeymap {
        preset: KeymapPreset,
        config_path: PathBuf,
    },
    /// Abort whatever is in flight; the user went back.
    CancelPending,
    Quit,
}

/// A failed action, as the wizard shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub class: ErrorClass,
    /// The provider says an administrator has to approve Rail first.
    pub consent_required: bool,
}

impl Failure {
    pub fn new(message: impl Into<String>, class: ErrorClass) -> Self {
        Self {
            message: message.into(),
            class,
            consent_required: false,
        }
    }

    pub fn from_auth(error: &AuthError) -> Self {
        Self {
            message: error.to_string(),
            class: error.class(),
            consent_required: error.consent_required(),
        }
    }
}

/// The one primary way out of a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    Retry,
    /// Hand this admin-consent URL to an administrator.
    SendConsentUrl(Url),
}

/// What the Done step reports. Locations only, never token values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneReport {
    pub config_path: PathBuf,
    pub token_backend: String,
    pub token_location: String,
    pub store_path: PathBuf,
}

/// One discovered preset file and whether it parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetEntry {
    pub source: PathBuf,
    pub preset: Result<Preset, String>,
}

/// What the outside world reports back. Each outcome carries a `Result`; the wizard decides
/// what the user sees. `Debug` is safe: `TokenSet` redacts its secrets.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PresetsLoaded(Vec<PresetEntry>),
    PresetFetched(Result<Preset, Failure>),
    /// The admin-consent URL, as soon as the browser is asked to open it.
    ConsentUrl(Url),
    /// The tenant that consented, which may be empty.
    AdminConsentFinished(Result<String, Failure>),
    /// The login URL, as soon as the browser is asked to open it.
    LoginUrl(Url),
    LoginFinished(Result<TokenSet, Failure>),
    ImapSmtpValidated(Result<(), Failure>),
    Persisted(Result<DoneReport, Failure>),
    KeymapSaved(Result<(), Failure>),
}
