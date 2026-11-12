use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::model::{Config, SecretsBackend};
use crate::error::ErrorClass;

const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// The longest email address Rail takes (RFC 5321's limit for a forward path, less its
/// brackets). Confirmations show addresses whole, so the length is limited here.
pub const MAX_ADDRESS_CHARS: usize = 254;

// `PathBuf` has no `Display`, so paths are formatted through `.display()`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no config file at {}; run `rail setup` (or `rail account add`) to create one", .path.display())]
    Missing { path: PathBuf },
    #[error("cannot read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{}:{line}:{col}: {message}", .path.display())]
    Parse {
        path: PathBuf,
        line: usize,
        col: usize,
        message: String,
    },
    #[error("{}: {message}", .path.display())]
    Invalid { path: PathBuf, message: String },
}

impl ConfigError {
    /// A bad or missing config never fixes itself by retrying.
    pub fn class(&self) -> ErrorClass {
        ErrorClass::Permanent
    }
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConfigError::Missing {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    parse(&text, path)
}

/// The config text and its parsed value; a missing file counts as empty. For edits that
/// create the file, such as adding the first account.
pub fn load_text_or_empty(path: &Path) -> Result<(String, Config), ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let config = parse(&text, path)?;
            Ok((text, config))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok((String::new(), Config::default()))
        }
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Parse and validate config text. `path` is only used in error messages.
pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
    check_no_passwords(text, path)?;
    let config: Config = toml::from_str(text).map_err(|e| {
        let (line, col) = e.span().map_or((0, 0), |s| line_col(text, s.start));
        ConfigError::Parse {
            path: path.to_path_buf(),
            line,
            col,
            message: e.message().to_string(),
        }
    })?;
    validate(&config).map_err(|message| ConfigError::Invalid {
        path: path.to_path_buf(),
        message,
    })?;
    Ok(config)
}

fn check_no_passwords(text: &str, path: &Path) -> Result<(), ConfigError> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Ok(());
    };
    check_table_no_passwords(doc.as_table(), text, path)
}

fn check_table_no_passwords(
    table: &toml_edit::Table,
    text: &str,
    path: &Path,
) -> Result<(), ConfigError> {
    for (key, item) in table.iter() {
        if is_password_key(key) {
            let (line, col) = find_key_line_col(text, key);
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                line,
                col,
                message: "passwords must not be stored in config.toml; run `rail secrets set` to store credentials securely".to_string(),
            });
        }
        match item {
            toml_edit::Item::Table(sub) => check_table_no_passwords(sub, text, path)?,
            toml_edit::Item::ArrayOfTables(arr) => {
                for sub in arr.iter() {
                    check_table_no_passwords(sub, text, path)?;
                }
            }
            toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)) => {
                for (sub_k, _) in inline.iter() {
                    if is_password_key(sub_k) {
                        let (line, col) = find_key_line_col(text, sub_k);
                        return Err(ConfigError::Parse {
                            path: path.to_path_buf(),
                            line,
                            col,
                            message: "passwords must not be stored in config.toml; run `rail secrets set` to store credentials securely".to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn find_key_line_col(text: &str, key: &str) -> (usize, usize) {
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let code = line.split('#').next().unwrap_or(line);
        if let Some((k, _)) = code.split_once('=') {
            let k_trimmed = k.trim();
            if is_password_key(k_trimmed) && k_trimmed.eq_ignore_ascii_case(key) {
                let col = line.find(k_trimmed).unwrap_or(0) + 1;
                return (line_idx + 1, col);
            }
        }
    }
    (1, 1)
}

fn is_password_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "password"
        || lower == "imap_password"
        || lower == "smtp_password"
        || lower.ends_with("_password")
}

/// 1-based line and column of a byte offset.
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn validate(config: &Config) -> Result<(), String> {
    if let Some(theme) = &config.general.theme {
        if theme.is_empty() {
            return Err("general.theme must not be empty".to_string());
        }
        super::model::plain_theme(theme)?;
    }
    validate_accounts(config)?;
    if config.general.poll_interval < MIN_POLL_INTERVAL {
        return Err("general.poll_interval must be at least 5s".to_string());
    }
    validate_labels(config)?;
    super::saved::validate_saved_searches(config)?;
    super::link::validate_link_actions(config)?;
    validate_onepassword(config)
}

fn validate_accounts(config: &Config) -> Result<(), String> {
    let mut names = HashSet::new();
    let mut addresses = HashSet::new();
    for account in &config.accounts {
        let name_ok = !account.name.is_empty()
            && account
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !name_ok {
            return Err(format!(
                "account name {:?} must be non-empty and use only letters, digits, '-' or '_'",
                account.name
            ));
        }
        if !names.insert(account.name.to_ascii_lowercase()) {
            return Err(format!(
                "duplicate account name {:?} (names are compared ignoring case)",
                account.name
            ));
        }
        if !account.address.contains('@') {
            return Err(format!(
                "account {:?}: {:?} is not an email address",
                account.name, account.address
            ));
        }
        if account.address.chars().count() > MAX_ADDRESS_CHARS {
            return Err(format!(
                "account {:?}: the address is longer than {MAX_ADDRESS_CHARS} characters",
                account.name
            ));
        }
        if !addresses.insert(account.address.to_ascii_lowercase()) {
            return Err(format!("duplicate account address {:?}", account.address));
        }
        if account.kind == super::model::AccountKind::ImapSmtp {
            if account.imap_host.as_deref().unwrap_or("").trim().is_empty() {
                return Err(format!(
                    "account {:?}: missing required imap_host",
                    account.name
                ));
            }
            if account.smtp_host.as_deref().unwrap_or("").trim().is_empty() {
                return Err(format!(
                    "account {:?}: missing required smtp_host",
                    account.name
                ));
            }
        } else if account.client_id.trim().is_empty() {
            return Err(format!(
                "account {:?}: client_id must not be empty",
                account.name
            ));
        }
    }
    Ok(())
}

/// Each account's `label` is at most `MAX_LABEL_CHARS` characters; the error names the account
/// by its position (0-based, as TOML paths do).
fn validate_labels(config: &Config) -> Result<(), String> {
    let max = super::model::MAX_LABEL_CHARS;
    for (index, account) in config.accounts.iter().enumerate() {
        if account
            .label
            .as_ref()
            .is_some_and(|l| l.chars().count() > max)
        {
            return Err(format!("accounts[{index}].label: at most {max} characters"));
        }
    }
    Ok(())
}

fn validate_onepassword(config: &Config) -> Result<(), String> {
    let vault = config.onepassword.as_ref().map(|o| o.vault.trim());
    if config.general.secrets == SecretsBackend::OnePassword && vault.is_none_or(str::is_empty) {
        return Err(
            "secrets = \"1password\" needs an [onepassword] table with vault = \"<vault name>\""
                .to_string(),
        );
    }
    if vault.is_some_and(|v| v.contains('/')) {
        return Err("onepassword.vault must not contain '/'".to_string());
    }
    Ok(())
}
