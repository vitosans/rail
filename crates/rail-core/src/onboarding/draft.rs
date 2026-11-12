//! The account being set up, as editable text, and its validation. The rules mirror
//! `config.toml` validation, plus the duplicate checks against the existing config.

use std::collections::BTreeMap;
use std::fmt;

use super::preset::Preset;
use crate::config::model::TlsMode;
use crate::config::{AccountConfig, AccountKind, Config, MAX_ADDRESS_CHARS, default_account_name};

/// Tenants that name a group of directories rather than one organisation.
pub const SHARED_TENANTS: [&str; 3] = ["common", "organizations", "consumers"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Address,
    Name,
    Color,
    ClientId,
    ClientSecret,
    Tenant,
    /// The "I am an administrator of this tenant" checkbox.
    Admin,
    /// The path or https URL typed on the preset step.
    PresetSource,
    /// The organisation's tenant, asked before admin consent when the draft says `common`.
    ConsentTenant,
    ImapHost,
    ImapPort,
    ImapUser,
    Password,
    SmtpHost,
    SmtpPort,
    SmtpUser,
    SmtpPassword,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Self::Address => "Email address",
            Self::Name => "Account name",
            Self::Color => "Colour",
            Self::ClientId => "Client ID",
            Self::ClientSecret => "Client secret",
            Self::Tenant => "Tenant",
            Self::Admin => "I am an administrator of this tenant",
            Self::PresetSource => "Path or URL",
            Self::ConsentTenant => "Org tenant",
            Self::ImapHost => "IMAP host",
            Self::ImapPort => "IMAP port",
            Self::ImapUser => "IMAP username",
            Self::Password => "Password",
            Self::SmtpHost => "SMTP host",
            Self::SmtpPort => "SMTP port",
            Self::SmtpUser => "SMTP username",
            Self::SmtpPassword => "SMTP password",
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccountDraft {
    pub kind: Option<AccountKind>,
    pub address: String,
    pub name: String,
    /// True once the user typed a name; until then it follows the address.
    pub name_edited: bool,
    pub color: String,
    pub client_id: String,
    pub client_secret: String,
    pub tenant: String,
    pub admin: bool,
    /// The preset's label when a preset filled the app registration fields.
    pub preset: Option<String>,
    /// A Gmail preset without a secret still asks for one.
    pub preset_needs_secret: bool,
    pub help_url: Option<String>,
    pub imap_host: String,
    pub imap_port: String,
    pub imap_user: String,
    pub password: String,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_user: String,
    pub smtp_password: String,
}

/// Hides `client_secret` and passwords; shows only whether they are set.
impl fmt::Debug for AccountDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secret = if self.client_secret.is_empty() {
            "<empty>"
        } else {
            "<redacted>"
        };
        let password = if self.password.is_empty() {
            "<empty>"
        } else {
            "<redacted>"
        };
        let smtp_password = if self.smtp_password.is_empty() {
            "<empty>"
        } else {
            "<redacted>"
        };
        f.debug_struct("AccountDraft")
            .field("kind", &self.kind)
            .field("address", &self.address)
            .field("name", &self.name)
            .field("color", &self.color)
            .field("client_id", &self.client_id)
            .field("client_secret", &secret)
            .field("tenant", &self.tenant)
            .field("admin", &self.admin)
            .field("preset", &self.preset)
            .field("imap_host", &self.imap_host)
            .field("imap_port", &self.imap_port)
            .field("imap_user", &self.imap_user)
            .field("password", &password)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_user", &self.smtp_user)
            .field("smtp_password", &smtp_password)
            .finish_non_exhaustive()
    }
}

impl AccountDraft {
    pub fn for_kind(kind: AccountKind) -> Self {
        let (imap_port, smtp_port) = if kind == AccountKind::ImapSmtp {
            ("993".to_string(), "465".to_string())
        } else {
            (String::new(), String::new())
        };
        Self {
            kind: Some(kind),
            imap_port,
            smtp_port,
            ..Self::default()
        }
    }

    pub fn from_preset(preset: &Preset) -> Self {
        Self {
            kind: Some(preset.kind),
            client_id: preset.client_id.clone(),
            client_secret: preset.client_secret.clone().unwrap_or_default(),
            tenant: preset.tenant.clone().unwrap_or_default(),
            preset: Some(preset.label()),
            preset_needs_secret: preset.kind == AccountKind::Gmail
                && preset.client_secret.is_none(),
            help_url: preset.help_url.clone(),
            ..Self::default()
        }
    }

    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::Address => &self.address,
            Field::Name => &self.name,
            Field::Color => &self.color,
            Field::ClientId => &self.client_id,
            Field::ClientSecret => &self.client_secret,
            Field::Tenant => &self.tenant,
            Field::ImapHost => &self.imap_host,
            Field::ImapPort => &self.imap_port,
            Field::ImapUser => &self.imap_user,
            Field::Password => &self.password,
            Field::SmtpHost => &self.smtp_host,
            Field::SmtpPort => &self.smtp_port,
            Field::SmtpUser => &self.smtp_user,
            Field::SmtpPassword => &self.smtp_password,
            Field::Admin | Field::PresetSource | Field::ConsentTenant => "",
        }
    }

    /// Set a text field. Editing the address re-derives the name until the user types one.
    pub fn set(&mut self, field: Field, value: String, existing: &Config) {
        match field {
            Field::Address => {
                self.address = value;
                if !self.name_edited {
                    self.name = self.suggested_name(existing);
                }
            }
            Field::Name => {
                self.name_edited = true;
                self.name = value;
            }
            Field::Color => self.color = value,
            Field::ClientId => self.client_id = value,
            Field::ClientSecret => self.client_secret = value,
            Field::Tenant => self.tenant = value,
            Field::ImapHost => self.imap_host = value,
            Field::ImapPort => self.imap_port = value,
            Field::ImapUser => self.imap_user = value,
            Field::Password => self.password = value,
            Field::SmtpHost => self.smtp_host = value,
            Field::SmtpPort => self.smtp_port = value,
            Field::SmtpUser => self.smtp_user = value,
            Field::SmtpPassword => self.smtp_password = value,
            Field::Admin | Field::PresetSource | Field::ConsentTenant => {}
        }
    }

    fn suggested_name(&self, existing: &Config) -> String {
        let address = self.address.trim();
        if address.is_empty() {
            String::new()
        } else {
            default_account_name(address, existing)
        }
    }

    /// The fields the Details step shows, in order.
    pub fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Address, Field::Name, Field::Color];
        if self.kind == Some(AccountKind::ImapSmtp) {
            fields.extend([
                Field::ImapHost,
                Field::ImapPort,
                Field::Password,
                Field::SmtpHost,
                Field::SmtpPort,
            ]);
            return fields;
        }
        if self.preset.is_none() {
            fields.push(Field::ClientId);
        }
        if self.preset.is_none() || self.preset_needs_secret {
            fields.push(Field::ClientSecret);
        }
        if self.kind == Some(AccountKind::M365) {
            fields.extend([Field::Tenant, Field::Admin]);
        }
        fields
    }

    pub fn domain(&self) -> Option<&str> {
        self.address
            .trim()
            .rsplit_once('@')
            .map(|(_, domain)| domain)
            .filter(|domain| !domain.is_empty())
    }

    pub fn tenant_suggestions(&self) -> Vec<String> {
        let mut suggestions = vec!["common".to_string(), "organizations".to_string()];
        if let Some(domain) = self.domain() {
            suggestions.push(domain.to_ascii_lowercase());
        }
        suggestions
    }

    pub fn has_concrete_tenant(&self) -> bool {
        is_concrete_tenant(&self.tenant)
    }

    /// Per-field errors; empty means the draft can be submitted.
    pub fn validate(&self, existing: &Config) -> BTreeMap<Field, String> {
        let checks = [
            (Field::Address, self.address_error(existing)),
            (Field::Name, self.name_error(existing)),
            (Field::ClientId, self.client_id_error()),
            (Field::ClientSecret, self.secret_error()),
            (Field::Tenant, self.tenant_error()),
            (Field::ImapHost, self.imap_host_error()),
            (Field::ImapPort, self.imap_port_error()),
            (Field::Password, self.password_error()),
            (Field::SmtpHost, self.smtp_host_error()),
            (Field::SmtpPort, self.smtp_port_error()),
        ];
        checks
            .into_iter()
            .filter_map(|(field, error)| error.map(|e| (field, e)))
            .collect()
    }

    fn address_error(&self, existing: &Config) -> Option<String> {
        let address = self.address.trim();
        if address.is_empty() {
            return Some("enter the email address of the account".to_string());
        }
        if !is_address(address) {
            return Some(format!("{address:?} is not an email address"));
        }
        existing
            .accounts
            .iter()
            .any(|a| a.address.eq_ignore_ascii_case(address))
            .then(|| format!("{address} is already configured"))
    }

    fn name_error(&self, existing: &Config) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some("enter a short name for the account".to_string());
        }
        let valid = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Some("use only letters, digits, '-' or '_'".to_string());
        }
        existing
            .accounts
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(name))
            .then(|| format!("an account named {name:?} already exists"))
    }

    fn client_id_error(&self) -> Option<String> {
        if self.kind == Some(AccountKind::ImapSmtp) {
            return None;
        }
        let id = self.client_id.trim();
        if id.is_empty() {
            return Some("enter the client ID from the portal".to_string());
        }
        id.chars()
            .any(char::is_whitespace)
            .then(|| "the client ID has no spaces".to_string())
    }

    fn imap_host_error(&self) -> Option<String> {
        (self.kind == Some(AccountKind::ImapSmtp) && self.imap_host.trim().is_empty())
            .then(|| "enter the IMAP server hostname".to_string())
    }

    fn password_error(&self) -> Option<String> {
        (self.kind == Some(AccountKind::ImapSmtp) && self.password.trim().is_empty())
            .then(|| "enter the account password".to_string())
    }

    fn smtp_host_error(&self) -> Option<String> {
        (self.kind == Some(AccountKind::ImapSmtp) && self.smtp_host.trim().is_empty())
            .then(|| "enter the SMTP server hostname".to_string())
    }

    fn imap_port_error(&self) -> Option<String> {
        if self.kind != Some(AccountKind::ImapSmtp) || self.imap_port.trim().is_empty() {
            return None;
        }
        self.imap_port
            .trim()
            .parse::<u16>()
            .err()
            .map(|_| "port must be between 1 and 65535".to_string())
    }

    fn smtp_port_error(&self) -> Option<String> {
        if self.kind != Some(AccountKind::ImapSmtp) || self.smtp_port.trim().is_empty() {
            return None;
        }
        self.smtp_port
            .trim()
            .parse::<u16>()
            .err()
            .map(|_| "port must be between 1 and 65535".to_string())
    }

    fn secret_error(&self) -> Option<String> {
        (self.kind == Some(AccountKind::Gmail) && self.client_secret.trim().is_empty())
            .then(|| "Gmail desktop apps need the client secret".to_string())
    }

    fn tenant_error(&self) -> Option<String> {
        let tenant = self.tenant.trim();
        (self.kind == Some(AccountKind::M365) && !tenant.is_empty() && !is_tenant_shaped(tenant))
            .then(|| "use a tenant ID, a domain, common or organizations".to_string())
    }

    /// The account to log in and persist. `None` until a provider is chosen.
    pub fn to_account(&self) -> Option<AccountConfig> {
        let kind = self.kind?;
        let (imap_port, smtp_port, imap_tls, smtp_tls) = if kind == AccountKind::ImapSmtp {
            let i_port = if self.imap_port.trim().is_empty() {
                Some(993)
            } else {
                self.imap_port.trim().parse::<u16>().ok()
            };
            let s_port = if self.smtp_port.trim().is_empty() {
                Some(465)
            } else {
                self.smtp_port.trim().parse::<u16>().ok()
            };
            (
                i_port,
                s_port,
                Some(TlsMode::Required),
                Some(TlsMode::Required),
            )
        } else {
            (None, None, None, None)
        };
        Some(AccountConfig {
            name: self.name.trim().to_string(),
            kind,
            address: self.address.trim().to_ascii_lowercase(),
            color: non_empty(&self.color),
            label: None,
            client_id: self.client_id.trim().to_string(),
            client_secret: non_empty(&self.client_secret),
            tenant: match kind {
                AccountKind::M365 => non_empty(&self.tenant),
                _ => None,
            },
            signature: None,
            from_name: None,
            ai_send: None,
            history_days: None,
            body_cache_days: None,
            imap_host: non_empty(&self.imap_host),
            imap_port,
            imap_tls,
            imap_user: non_empty(&self.imap_user),
            smtp_host: non_empty(&self.smtp_host),
            smtp_port,
            smtp_tls,
            smtp_user: non_empty(&self.smtp_user),
            sent_folder: None,
        })
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// One `@` with something on each side and no whitespace anywhere.
pub fn is_address(text: &str) -> bool {
    match text.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && text.chars().count() <= MAX_ADDRESS_CHARS
                && !domain.contains('@')
                && !text.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

fn is_tenant_shaped(tenant: &str) -> bool {
    tenant
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// A tenant id or domain naming one organisation: not empty, not `common`/`organizations`/
/// `consumers`. Admin consent needs one.
pub fn is_concrete_tenant(tenant: &str) -> bool {
    let tenant = tenant.trim();
    !tenant.is_empty()
        && is_tenant_shaped(tenant)
        && !SHARED_TENANTS
            .iter()
            .any(|s| s.eq_ignore_ascii_case(tenant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{contoso_preset, existing_config};

    fn gmail(address: &str) -> AccountDraft {
        let mut draft = AccountDraft::for_kind(AccountKind::Gmail);
        let existing = existing_config();
        draft.set(Field::Address, address.into(), &existing);
        draft.set(Field::ClientId, "cid".into(), &existing);
        draft.set(Field::ClientSecret, "sec".into(), &existing);
        draft
    }

    #[test]
    fn name_follows_the_address_until_edited() {
        let existing = existing_config();
        let mut draft = gmail("work@corp.com");
        assert_eq!(
            draft.name, "work-2",
            "made unique against the existing \"work\""
        );
        draft.set(Field::Address, "dana@corp.com".into(), &existing);
        assert_eq!(draft.name, "dana");
        draft.set(Field::Name, "corp".into(), &existing);
        draft.set(Field::Address, "other@corp.com".into(), &existing);
        assert_eq!(draft.name, "corp");
    }

    #[test]
    fn validation_mirrors_the_config_rules() {
        let existing = existing_config();
        let cases: [(&str, Field, &str, &str); 8] = [
            (
                "empty address",
                Field::Address,
                "",
                "enter the email address",
            ),
            (
                "no @",
                Field::Address,
                "me.example.com",
                "not an email address",
            ),
            (
                "space",
                Field::Address,
                "me @example.com",
                "not an email address",
            ),
            (
                "duplicate address",
                Field::Address,
                "WORK@Example.com",
                "already configured",
            ),
            ("bad name", Field::Name, "has space", "letters, digits"),
            ("duplicate name", Field::Name, "Work", "already exists"),
            ("spaced client id", Field::ClientId, "a b", "no spaces"),
            ("empty client id", Field::ClientId, "", "client ID"),
        ];
        for (name, field, value, needle) in cases {
            let mut draft = gmail("me@corp.com");
            draft.set(field, value.into(), &existing);
            let errors = draft.validate(&existing);
            let error = errors
                .get(&field)
                .unwrap_or_else(|| panic!("{name}: {errors:?}"));
            assert!(error.contains(needle), "{name}: {error}");
        }
        assert!(gmail("me@corp.com").validate(&existing).is_empty());
    }

    #[test]
    fn gmail_requires_a_secret_and_microsoft_does_not() {
        let existing = existing_config();
        let mut draft = gmail("me@corp.com");
        draft.set(Field::ClientSecret, String::new(), &existing);
        assert!(draft.validate(&existing).contains_key(&Field::ClientSecret));
        draft.kind = Some(AccountKind::M365);
        assert!(draft.validate(&existing).is_empty());
    }

    #[test]
    fn tenant_must_be_a_tenant_id_or_domain() {
        let existing = existing_config();
        let mut draft = gmail("me@contoso.com");
        draft.kind = Some(AccountKind::M365);
        draft.set(Field::Tenant, "contoso com".into(), &existing);
        assert!(draft.validate(&existing).contains_key(&Field::Tenant));
        draft.set(Field::Tenant, "common".into(), &existing);
        assert!(draft.validate(&existing).is_empty());
        assert!(!draft.has_concrete_tenant());
        draft.set(Field::Tenant, "contoso.onmicrosoft.com".into(), &existing);
        assert!(draft.has_concrete_tenant());
        assert_eq!(
            draft.tenant_suggestions(),
            vec!["common", "organizations", "contoso.com"]
        );
    }

    #[test]
    fn fields_depend_on_kind_and_preset() {
        let gmail_fields = gmail("me@corp.com").fields();
        assert_eq!(
            gmail_fields,
            vec![
                Field::Address,
                Field::Name,
                Field::Color,
                Field::ClientId,
                Field::ClientSecret
            ]
        );
        let preset = AccountDraft::from_preset(&contoso_preset());
        assert_eq!(
            preset.fields(),
            vec![
                Field::Address,
                Field::Name,
                Field::Color,
                Field::Tenant,
                Field::Admin
            ]
        );
        let gmail_preset = Preset {
            kind: AccountKind::Gmail,
            tenant: None,
            ..contoso_preset()
        };
        assert!(
            AccountDraft::from_preset(&gmail_preset)
                .fields()
                .contains(&Field::ClientSecret)
        );
    }

    #[test]
    fn to_account_trims_lowercases_and_drops_empty_options() {
        let existing = existing_config();
        let mut draft = gmail("  Me@Corp.com ");
        draft.set(Field::Color, " ".into(), &existing);
        let account = draft.to_account().unwrap();
        assert_eq!(account.address, "me@corp.com");
        assert_eq!(account.name, "me");
        assert_eq!(account.color, None);
        assert!(account.client_secret.as_deref().is_some_and(|s| s == "sec"));
        assert_eq!(account.tenant, None);
        assert!(
            AccountDraft::default().to_account().is_none(),
            "no kind yet"
        );
    }

    #[test]
    fn debug_hides_the_client_secret() {
        let debug = format!("{:?}", gmail("me@corp.com"));
        assert!(!debug.contains("sec\""), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(is_address("a@b"));
        assert!(!is_address(&format!("{}@e.com", "x".repeat(250))));
        assert!(!is_address("a@b@c"));
        assert!(is_concrete_tenant("contoso.com"));
        assert!(!is_concrete_tenant("Organizations"));
    }

    #[test]
    fn imapsmtp_fields_and_validation() {
        let existing = existing_config();
        let mut draft = AccountDraft::for_kind(AccountKind::ImapSmtp);
        assert_eq!(
            draft.fields(),
            vec![
                Field::Address,
                Field::Name,
                Field::Color,
                Field::ImapHost,
                Field::ImapPort,
                Field::Password,
                Field::SmtpHost,
                Field::SmtpPort,
            ]
        );
        let errors = draft.validate(&existing);
        assert!(errors.contains_key(&Field::Address));
        assert!(errors.contains_key(&Field::Name));
        assert!(errors.contains_key(&Field::ImapHost));
        assert!(errors.contains_key(&Field::Password));
        assert!(errors.contains_key(&Field::SmtpHost));
        assert!(!errors.contains_key(&Field::ClientId));
        assert!(!errors.contains_key(&Field::ClientSecret));

        draft.set(Field::Address, "user@example.com".into(), &existing);
        draft.set(Field::ImapHost, "imap.example.com".into(), &existing);
        draft.set(Field::Password, "secret".into(), &existing);
        draft.set(Field::SmtpHost, "smtp.example.com".into(), &existing);
        assert!(draft.validate(&existing).is_empty());

        draft.set(Field::ImapPort, "not-a-port".into(), &existing);
        assert!(draft.validate(&existing).contains_key(&Field::ImapPort));
    }

    #[test]
    fn imapsmtp_to_account_and_debug_redaction() {
        let existing = existing_config();
        let mut draft = AccountDraft::for_kind(AccountKind::ImapSmtp);
        draft.set(Field::Address, "user@example.com".into(), &existing);
        draft.set(Field::ImapHost, "imap.example.com".into(), &existing);
        draft.set(Field::Password, "supersecret".into(), &existing);
        draft.set(Field::SmtpHost, "smtp.example.com".into(), &existing);
        draft.set(Field::SmtpPassword, "smtpsecret".into(), &existing);

        let debug = format!("{draft:?}");
        assert!(!debug.contains("supersecret"));
        assert!(!debug.contains("smtpsecret"));
        assert!(debug.contains("<redacted>"));

        let account = draft.to_account().unwrap();
        assert_eq!(account.kind, AccountKind::ImapSmtp);
        assert_eq!(account.address, "user@example.com");
        assert_eq!(account.imap_host.as_deref(), Some("imap.example.com"));
        assert_eq!(account.imap_port, Some(993));
        assert_eq!(account.imap_tls, Some(TlsMode::Required));
        assert_eq!(account.smtp_host.as_deref(), Some("smtp.example.com"));
        assert_eq!(account.smtp_port, Some(465));
        assert_eq!(account.smtp_tls, Some(TlsMode::Required));
    }
}
