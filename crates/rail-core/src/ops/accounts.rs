//! Adding, logging in to and removing accounts, and Microsoft admin consent.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::error::OpError;
use super::pending::{PendingId, PendingKind};
use super::untrusted::{Secret, Untrusted};
use super::yes;
use crate::config::AccountKind;
use crate::config::model::TlsMode;

/// A 1Password secret reference starts with this.
pub const OP_REFERENCE_PREFIX: &str = "op://";

/// Where the client secret comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientSecretInput {
    /// The value itself. Only the flag commands and the TUI send this; no MCP tool takes it.
    Value(Secret),
    /// The `client_secret_*.json` Google lets you download; it also carries the client id.
    JsonFile(PathBuf),
    /// An `op://vault/item/field` reference, resolved with `op read` when a login needs it.
    Reference(String),
    /// The preset file path or `https://` URL whose client secret (and client id) to use.
    Preset(String),
}

impl ClientSecretInput {
    /// What `--client-secret` means: an `op://` reference, or the value itself.
    pub fn from_flag(value: String) -> Self {
        if value.starts_with(OP_REFERENCE_PREFIX) {
            Self::Reference(value)
        } else {
            Self::Value(Secret::new(value))
        }
    }
}

/// `account.add.begin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountAddRequest {
    pub kind: AccountKind,
    pub address: String,
    /// Optional when the client secret input carries one (a Google JSON file or a preset).
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<ClientSecretInput>,
    /// Microsoft only: a tenant id or domain, `common` or `organizations`.
    #[serde(default)]
    pub tenant: Option<String>,
    /// Short name for the UI and config; derived from the address when absent.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// Put the client secret in 1Password and write an `op://` reference into config instead.
    #[serde(default)]
    pub store_secret_in_1password: bool,
    /// Try to open the login page in the browser. The URL is returned either way.
    #[serde(default = "yes")]
    pub open_browser: bool,
}

/// The arguments of the `rail_account_add` MCP tool: an account to add, without any way to
/// pass a raw client secret. At most one of `client_secret_file`, `client_secret_ref` and
/// `preset` may be given.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAccountAdd {
    /// `gmail` or `m365`.
    pub kind: AccountKind,
    /// The mailbox address.
    pub address: String,
    /// The OAuth client id. Not needed with a Google client secret file or a preset.
    #[serde(default)]
    pub client_id: Option<String>,
    /// Path to the `client_secret_*.json` downloaded from Google Cloud Console.
    #[serde(default)]
    pub client_secret_file: Option<PathBuf>,
    /// An `op://vault/item/field` reference to the client secret in 1Password.
    #[serde(default)]
    pub client_secret_ref: Option<String>,
    /// A preset file path or `https://` URL from the user's organisation.
    #[serde(default)]
    pub preset: Option<String>,
    /// Microsoft only: the tenant id or domain. Single-tenant apps need it.
    #[serde(default)]
    pub tenant: Option<String>,
    /// Short name; derived from the address when absent.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// Store the client secret in 1Password and keep only a reference in config. Refused for
    /// an `op://` reference, which is already in 1Password.
    #[serde(default)]
    pub store_secret_in_1password: bool,
    /// Set when an agent sent a `client_secret` field. Its value is dropped unread, never kept
    /// or shown, and the call is refused. Not part of the schema.
    #[serde(
        default,
        rename = "client_secret",
        deserialize_with = "pasted",
        skip_serializing
    )]
    #[schemars(skip)]
    pub pasted_client_secret: bool,
    /// Set when an agent sent a `password` field. Dropped unread, never kept or echoed.
    #[serde(
        default,
        rename = "password",
        deserialize_with = "pasted",
        skip_serializing
    )]
    #[schemars(skip)]
    pub pasted_password: bool,
    /// Set when an agent sent an `smtp_password` field. Dropped unread, never kept or echoed.
    #[serde(
        default,
        rename = "smtp_password",
        deserialize_with = "pasted",
        skip_serializing
    )]
    #[schemars(skip)]
    pub pasted_smtp_password: bool,
}

/// Every field but the `op://` reference, which an agent may have filled with a pasted secret:
/// a log line or an error built from this can never carry it.
impl std::fmt::Debug for AgentAccountAdd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reference = self.client_secret_ref.as_ref().map(|_| "<redacted>");
        f.debug_struct("AgentAccountAdd")
            .field("kind", &self.kind)
            .field("address", &self.address)
            .field("client_id", &self.client_id)
            .field("client_secret_file", &self.client_secret_file)
            .field("client_secret_ref", &reference)
            .field("preset", &self.preset)
            .field("tenant", &self.tenant)
            .field("name", &self.name)
            .field("color", &self.color)
            .field("store_secret_in_1password", &self.store_secret_in_1password)
            .field("pasted_client_secret", &self.pasted_client_secret)
            .field("pasted_password", &self.pasted_password)
            .field("pasted_smtp_password", &self.pasted_smtp_password)
            .finish()
    }
}

/// The refusal of a `client_secret` sent by an agent. Fixed text: the value is never echoed.
pub const NO_SECRETS_FROM_AGENTS: &str = "Rail does not take client secrets from agents; pass an op:// reference, a preset, or an absolute path to the client JSON file";

/// The refusal of a password sent by an agent. Fixed text: the value is never echoed.
pub const NO_PASSWORDS_FROM_AGENTS: &str = "Rail does not take passwords from agents; configure IMAP/SMTP accounts using the setup wizard or `rail account add --kind imapsmtp` in a terminal";

/// Any value at all: skipped without being read into memory as text.
fn pasted<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(deserializer)?;
    Ok(true)
}

impl AgentAccountAdd {
    /// The daemon request this tool call stands for. The browser is always tried.
    pub fn into_request(self) -> Result<AccountAddRequest, OpError> {
        if self.pasted_client_secret {
            return Err(OpError::invalid_input(NO_SECRETS_FROM_AGENTS));
        }
        if self.pasted_password || self.pasted_smtp_password {
            return Err(OpError::invalid_input(NO_PASSWORDS_FROM_AGENTS));
        }
        if self.kind == AccountKind::ImapSmtp {
            return Err(OpError::invalid_input(
                "IMAP/SMTP accounts do not use OAuth browser login; configure IMAP/SMTP accounts using the setup wizard or `rail account add --kind imapsmtp` in a terminal",
            ));
        }
        let sources = [
            self.client_secret_file.map(ClientSecretInput::JsonFile),
            self.client_secret_ref.map(ClientSecretInput::Reference),
            self.preset.map(ClientSecretInput::Preset),
        ];
        let mut given = sources.into_iter().flatten();
        let client_secret = given.next();
        if given.next().is_some() {
            return Err(OpError::invalid_input(
                "give at most one of client_secret_file, client_secret_ref and preset",
            ));
        }
        Ok(AccountAddRequest {
            kind: self.kind,
            address: self.address,
            client_id: self.client_id,
            client_secret,
            tenant: self.tenant,
            name: self.name,
            color: self.color,
            store_secret_in_1password: self.store_secret_in_1password,
            open_browser: true,
        })
    }
}

/// `account.login.begin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginBeginRequest {
    /// The account's name or address, matched without regard to case.
    pub account: String,
    /// Try to open the login page in the browser. The URL is returned either way.
    #[serde(default = "yes")]
    pub open_browser: bool,
}

/// `consent.begin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsentBeginRequest {
    /// The app registration's Application (client) ID.
    pub client_id: String,
    /// The organisation's tenant id or domain; never `common` or `organizations`.
    pub tenant: String,
    /// Try to open the consent page in the browser. The URL is returned either way.
    #[serde(default = "yes")]
    pub open_browser: bool,
}

/// What every `*.begin` returns at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BeginResponse {
    /// Pass this to `pending.wait` (`rail_wait`) or `pending.cancel` (`rail_cancel`).
    pub id: PendingId,
    pub kind: PendingKind,
    /// The page the user opens. It carries no secret and only works while the daemon runs.
    pub url: String,
    /// Whether the browser was asked to open it.
    pub opened: bool,
}

/// Where an added or re-authenticated account was saved. Locations only, never token values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SavedAccount {
    pub name: String,
    pub kind: AccountKind,
    pub address: String,
    pub config_path: PathBuf,
    pub token_backend: String,
    pub token_location: String,
    pub store_path: PathBuf,
}

/// `account.remove`. Only a trusted front end (the terminal command after the user said yes,
/// the MCP server after the user confirmed through elicitation) sets `confirm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRemoveRequest {
    /// The account's name or address.
    pub account: String,
    /// Must be true, or nothing is removed.
    #[serde(default)]
    pub confirm: bool,
    /// The `plan` digest from the daemon's `NotConfirmed` refusal, which carried the text the
    /// person confirmed. A confirmed request without it, or with a stale one, is refused.
    #[serde(default)]
    pub plan: Option<String>,
}

/// The arguments of the `rail_account_remove` MCP tool. There is no way for the agent to
/// confirm on the user's behalf: the user confirms through the client's own dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAccountRemove {
    /// The account's name or address.
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRemoveResponse {
    pub name: String,
    pub kind: AccountKind,
    pub address: String,
    pub tokens_removed: bool,
    /// Why the tokens could not be deleted (the token store's text); the config entry is gone
    /// either way.
    pub token_error: Option<Untrusted<String>>,
    /// Always true: Rail never deletes mail. The account's rows stay in the store.
    pub mail_kept: bool,
}

/// `account.validate`: test IMAP and SMTP connectivity and credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountValidateRequest {
    /// An existing account name or address; when given, missing fields default from it.
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub imap_host: Option<String>,
    #[serde(default)]
    pub imap_port: Option<u16>,
    #[serde(default)]
    pub imap_tls: Option<TlsMode>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<Secret<String>>,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    #[serde(default)]
    pub smtp_tls: Option<TlsMode>,
    #[serde(default)]
    pub smtp_username: Option<String>,
    #[serde(default)]
    pub smtp_password: Option<Secret<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountValidateResponse {
    pub imap_ok: bool,
    pub smtp_ok: bool,
    pub imap_capabilities: Vec<String>,
    pub special_use_mailboxes: Vec<String>,
    /// Things the user should read even though the account works (for example, TLS is off).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `account.password.set`: store an account or service password in the secret store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordSetRequest {
    pub account: String,
    pub password: Secret<String>,
    /// Optional service name: "imap" or "smtp". Unset stores the account-level password.
    #[serde(default)]
    pub service: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordSetResponse {
    pub account: String,
    pub service: Option<String>,
    pub backend: String,
}

/// `account.password.check`: check whether a password is stored without revealing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordCheckRequest {
    pub account: String,
    #[serde(default)]
    pub service: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordCheckResponse {
    pub account: String,
    pub service: Option<String>,
    pub has_password: bool,
    pub backend: String,
}

/// `account.password.delete`: remove a stored password from the secret store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordDeleteRequest {
    pub account: String,
    #[serde(default)]
    pub service: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountPasswordDeleteResponse {
    pub account: String,
    pub service: Option<String>,
    pub deleted: bool,
}
