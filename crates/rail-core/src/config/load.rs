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
    }
    if config.general.poll_interval < MIN_POLL_INTERVAL {
        return Err("general.poll_interval must be at least 5s".to_string());
    }
    validate_labels(config)?;
    super::saved::validate_saved_searches(config)?;
    super::link::validate_link_actions(config)?;
    validate_onepassword(config)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{AccountKind, Layout, SecretsBackend};
    use std::time::Duration;

    const FULL: &str = r#"
[general]
editor = "nvim"
poll_interval = "2m"
body_cache_days = 10
history_days = 365
layout = "split"
split_threshold = 120
secrets = "file"

[[accounts]]
name = "work"
kind = "gmail"
address = "me@example.com"
color = "blue"
client_id = "abc.apps.googleusercontent.com"
client_secret = "shh"
signature = "~/.config/rail/sig.txt"

[[accounts]]
name = "corp"
kind = "m365"
address = "me@corp.com"
client_id = "xyz"
tenant = "common"

[keys]
archive = "e"

"#;

    fn p(text: &str) -> Result<Config, ConfigError> {
        parse(text, Path::new("/etc/rail/config.toml"))
    }

    #[test]
    fn an_address_with_a_bidi_override_is_refused_with_its_line() {
        let text = "[[accounts]]\nname = \"ok\"\nkind = \"gmail\"\nclient_id = \"1\"\naddress = \"moc.elpmaxe@\u{202E}me\"\n";
        let err = p(text).unwrap_err().to_string();
        assert!(err.starts_with("/etc/rail/config.toml:5:"), "{err}");
        assert!(err.contains("address"), "{err}");
        assert!(!err.contains('\u{202E}'), "{err:?}");
    }

    #[test]
    fn parses_full_example() {
        let c = p(FULL).unwrap();
        assert_eq!(c.general.editor.as_deref(), Some("nvim"));
        assert_eq!(c.general.poll_interval, Duration::from_secs(120));
        assert_eq!(c.general.body_cache_days, 10);
        assert_eq!(c.general.history_days, 365);
        assert_eq!(c.general.layout, Layout::Split);
        assert_eq!(c.general.split_threshold, 120);
        assert_eq!(c.general.secrets, SecretsBackend::File);
        assert_eq!(c.accounts.len(), 2);
        assert_eq!(c.accounts[0].kind, AccountKind::Gmail);
        assert_eq!(c.accounts[0].client_secret.as_deref(), Some("shh"));
        assert_eq!(c.accounts[1].kind, AccountKind::M365);
        assert_eq!(c.accounts[1].tenant.as_deref(), Some("common"));
        assert_eq!(
            c.keys.get("archive").map(|k| k.get_ref().as_str()),
            Some("e")
        );
    }

    #[test]
    fn empty_config_uses_defaults() {
        let c = p("").unwrap();
        assert_eq!(c.general.poll_interval, Duration::from_secs(60));
        assert_eq!(c.general.body_cache_days, 30);
        assert_eq!(c.general.history_days, 0);
        assert_eq!(c.general.layout, Layout::Auto);
        assert_eq!(c.general.split_threshold, 140);
        assert_eq!(c.general.secrets, SecretsBackend::Keyring);
        assert!(c.accounts.is_empty());
    }

    #[test]
    fn bulk_confirm_defaults_to_fifty_and_zero_turns_the_question_off() {
        assert_eq!(crate::config::General::default().bulk_confirm, 50);
        assert_eq!(p("").unwrap().general.bulk_confirm, 50);
        assert_eq!(
            p("[general]\nbulk_confirm = 0\n")
                .unwrap()
                .general
                .bulk_confirm,
            0,
            "`0` is a value, not an omission"
        );
        assert_eq!(
            p("[general]\nbulk_confirm = 2000\n")
                .unwrap()
                .general
                .bulk_confirm,
            2000
        );
    }

    #[test]
    fn time_defaults_to_24h_and_takes_12h() {
        use crate::config::TimeFormat;
        assert_eq!(p("").unwrap().general.time, TimeFormat::H24);
        let twelve = p("[general]\ntime = \"12h\"\n").unwrap();
        assert_eq!(twelve.general.time, TimeFormat::H12);
        assert_eq!(TimeFormat::H12.as_str(), "12h");
        assert_eq!(
            p("[general]\ntime = \"24h\"\n").unwrap().general.time,
            TimeFormat::H24
        );
        let err = p("[general]\ntime = \"13h\"\n").unwrap_err().to_string();
        assert!(err.contains(":2:"), "{err}");
        assert!(err.contains("13h") && err.contains("24h"), "{err}");
    }

    #[test]
    fn thread_order_defaults_to_oldest_takes_newest_and_refuses_the_rest_at_its_line() {
        use crate::config::ThreadOrder;
        assert_eq!(p("").unwrap().general.thread_order, ThreadOrder::Oldest);
        let newest = p("[general]\nthread_order = \"newest\"\n").unwrap();
        assert_eq!(newest.general.thread_order, ThreadOrder::Newest);
        assert_eq!(
            p("[general]\nthread_order = \"oldest\"\n")
                .unwrap()
                .general
                .thread_order,
            ThreadOrder::Oldest
        );
        assert_eq!(ThreadOrder::Newest.as_str(), "newest");
        assert_eq!(ThreadOrder::Oldest.as_str(), "oldest");
        let err = p("[general]\nthread_order = \"sideways\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains(":2:"), "{err}");
        assert!(err.contains("sideways") && err.contains("oldest"), "{err}");
    }

    #[test]
    fn dim_read_accepts_the_three_modes_and_reports_invalid_values_at_their_line() {
        assert_eq!(p("").unwrap().general.dim_read, crate::config::DimRead::Dot);
        for mode in ["dot", "subject", "all"] {
            let text = format!("[general]\ndim_read = \"{mode}\"\n");
            let config = p(&text).unwrap();
            assert_eq!(config.general.dim_read.as_str(), mode);
        }
        for value in ["\"grey\"", "true", "42"] {
            let err = p(&format!("[general]\ndim_read = {value}\n"))
                .unwrap_err()
                .to_string();
            assert!(err.contains(":2:"), "{err}");
        }
    }

    #[test]
    fn field_keys_default_to_readline_and_report_invalid_values_at_their_line() {
        use crate::config::FieldKeys;
        assert_eq!(p("").unwrap().general.field_keys, FieldKeys::Readline);
        for mode in [FieldKeys::Readline, FieldKeys::Basic] {
            let text = format!("[general]\nfield_keys = \"{}\"\n", mode.as_str());
            assert_eq!(p(&text).unwrap().general.field_keys, mode);
        }
        for value in ["\"emacs\"", "true", "1"] {
            let err = p(&format!("[general]\nfield_keys = {value}\n"))
                .unwrap_err()
                .to_string();
            assert!(err.contains(":2:"), "{err}");
        }
    }

    #[test]
    fn icons_default_to_unicode_and_take_ascii() {
        assert_eq!(p("").unwrap().general.icons, crate::config::Icons::Unicode);
        let ascii = p("[general]\nicons = \"ascii\"\n").unwrap();
        assert_eq!(ascii.general.icons, crate::config::Icons::Ascii);
        let err = p("[general]\nicons = \"emoji\"\n").unwrap_err().to_string();
        assert!(err.contains(":2:"), "{err}");
    }

    #[test]
    fn hints_default_on_take_false_and_refuse_a_string_at_its_line() {
        assert!(p("").unwrap().general.hints);
        assert!(!p("[general]\nhints = false\n").unwrap().general.hints);
        // Like every typed key, the error points at the value's line and column.
        let err = p("[general]\nhints = \"no\"\n").unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::Parse {
                    line: 2,
                    col: 9,
                    ..
                }
            ),
            "{err}"
        );
        assert!(err.to_string().contains("expected a boolean"), "{err}");
    }

    #[test]
    fn clock_defaults_on_takes_false_and_refuses_a_string_at_its_line() {
        assert!(p("").unwrap().general.clock);
        assert!(!p("[general]\nclock = false\n").unwrap().general.clock);
        let err = p("[general]\nclock = \"no\"\n").unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::Parse {
                    line: 2,
                    col: 9,
                    ..
                }
            ),
            "{err}"
        );
        assert!(err.to_string().contains("expected a boolean"), "{err}");
    }

    #[test]
    fn unknown_key_reports_line_number() {
        let text = "[general]\neditor = \"vi\"\nbogus = 1\n";
        let err = p(text).unwrap_err();
        match err {
            ConfigError::Parse {
                line, ref message, ..
            } => {
                assert_eq!(line, 3, "{message}");
                assert!(message.contains("bogus"), "{message}");
            }
            ref other => panic!("expected Parse, got {other:?}"),
        }
        assert!(err.to_string().starts_with("/etc/rail/config.toml:3:"));
    }

    #[test]
    fn account_names_that_differ_only_in_case_are_duplicates() {
        let text = "[[accounts]]\nname = \"Work\"\nkind = \"gmail\"\naddress = \"x@example.com\"\nclient_id = \"1\"\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"y@example.com\"\nclient_id = \"2\"\n";
        let err = p(text).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
        assert!(err.to_string().contains("duplicate account name"), "{err}");
    }

    #[test]
    fn duplicate_account_name_is_invalid() {
        let text = r#"
[[accounts]]
name = "a"
kind = "gmail"
address = "x@example.com"
client_id = "1"
[[accounts]]
name = "a"
kind = "gmail"
address = "y@example.com"
client_id = "2"
"#;
        let err = p(text).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
        assert!(err.to_string().contains("duplicate account name"), "{err}");
    }

    #[test]
    fn bad_name_and_bad_address_are_invalid() {
        let bad_name = "[[accounts]]\nname = \"has space\"\nkind = \"gmail\"\naddress = \"x@e.com\"\nclient_id = \"1\"\n";
        assert!(
            p(bad_name)
                .unwrap_err()
                .to_string()
                .contains("account name")
        );
        let bad_addr = "[[accounts]]\nname = \"ok\"\nkind = \"gmail\"\naddress = \"nope\"\nclient_id = \"1\"\n";
        assert!(
            p(bad_addr)
                .unwrap_err()
                .to_string()
                .contains("not an email address")
        );
        let long = format!("{}@e.com", "x".repeat(250));
        let too_long = format!(
            "[[accounts]]\nname = \"ok\"\nkind = \"gmail\"\naddress = \"{long}\"\nclient_id = \"1\"\n"
        );
        let err = p(&too_long).unwrap_err().to_string();
        assert!(err.contains("longer than 254"), "{err}");
    }

    /// Final review M2: spaces and control characters in an address, colour or tenant are
    /// refused with the file path and the value's line.
    #[test]
    fn whitespace_and_control_characters_are_refused_with_path_and_line() {
        let account = |field: &str| {
            format!("[[accounts]]\nname = \"ok\"\nkind = \"m365\"\nclient_id = \"1\"\n{field}\n")
        };
        for (lines, field) in [
            ("address = \"a b@example.com\"", "address"),
            (
                "address = \"\\u001b]52;c;eA==\\u0007me@example.com\"",
                "address",
            ),
            (
                "address = \"x@e.com\"\ncolor = \"blue\\u001b[31m\"",
                "color",
            ),
            ("address = \"x@e.com\"\ntenant = \"con\\ttoso\"", "tenant"),
        ] {
            let text = account(lines);
            let line = text.lines().count();
            let err = p(&text).unwrap_err().to_string();
            assert!(
                err.starts_with(&format!("/etc/rail/config.toml:{line}:")),
                "{field}: {err}"
            );
            assert!(err.contains(field), "{err}");
            assert!(!err.contains('\u{1b}'), "{err}");
        }
    }

    #[test]
    fn poll_interval_floor_is_five_seconds() {
        let err = p("[general]\npoll_interval = \"1s\"\n").unwrap_err();
        assert!(err.to_string().contains("poll_interval"), "{err}");
    }

    #[test]
    fn load_reports_missing_file_with_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Missing { .. }));
        assert!(err.to_string().contains("rail account add"), "{err}");
        assert!(err.to_string().contains("rail setup"), "{err}");
    }

    #[test]
    fn parses_the_onepassword_table() {
        let text = "[general]\nsecrets = \"1password\"\n\n[onepassword]\nvault = \"Rail\"\naccount = \"my.1password.com\"\nop_path = \"/usr/bin/op\"\n";
        let c = p(text).unwrap();
        assert_eq!(c.general.secrets, SecretsBackend::OnePassword);
        let op = c.onepassword.unwrap();
        assert_eq!(op.vault, "Rail");
        assert_eq!(op.account.as_deref(), Some("my.1password.com"));
        assert_eq!(op.op_path, Some(PathBuf::from("/usr/bin/op")));
        assert_eq!(SecretsBackend::OnePassword.as_str(), "1password");
    }

    #[test]
    fn the_onepassword_backend_needs_a_vault() {
        let err = p("[general]\nsecrets = \"1password\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(err.to_string().contains("[onepassword]"), "{err}");
        let empty = p("[general]\nsecrets = \"1password\"\n[onepassword]\nvault = \" \"\n");
        assert!(empty.is_err());
        let slash = p("[onepassword]\nvault = \"a/b\"\n").unwrap_err();
        assert!(slash.to_string().contains("'/'"), "{slash}");
        let typo = p("[onepassword]\nvalt = \"Rail\"\n").unwrap_err();
        assert!(matches!(typo, ConfigError::Parse { line: 2, .. }), "{typo}");
    }

    #[test]
    fn load_text_or_empty_treats_a_missing_file_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (text, config) = load_text_or_empty(&path).unwrap();
        assert!(text.is_empty());
        assert_eq!(config, Config::default());
        std::fs::write(&path, "[general]\nsecrets = \"file\"\n").unwrap();
        let (text, config) = load_text_or_empty(&path).unwrap();
        assert!(text.contains("file"));
        assert_eq!(config.general.secrets, SecretsBackend::File);
    }

    #[test]
    fn line_col_counts_from_one() {
        assert_eq!(line_col("ab\ncd", 0), (1, 1));
        assert_eq!(line_col("ab\ncd", 3), (2, 1));
        assert_eq!(line_col("ab\ncd", 4), (2, 2));
    }

    #[test]
    fn every_error_is_permanent() {
        use crate::error::ErrorClass;
        let path = PathBuf::from("/x");
        let missing = ConfigError::Missing { path: path.clone() };
        let invalid = ConfigError::Invalid {
            path,
            message: "m".into(),
        };
        assert_eq!(missing.class(), ErrorClass::Permanent);
        assert_eq!(invalid.class(), ErrorClass::Permanent);
    }

    #[test]
    fn theme_and_accent_load_and_default_to_none() {
        let c = p("").unwrap();
        assert_eq!(c.general.theme, None);
        assert_eq!(c.general.accent, None);
        let c = p("[general]\ntheme = \"Dracula\"\naccent = 5\n").unwrap();
        assert_eq!(c.general.theme.as_deref(), Some("Dracula"));
        assert_eq!(c.general.accent, Some(crate::config::AccentSpec::Slot(5)));
        let c = p("[general]\naccent = \"#bd93f9\"\n").unwrap();
        assert_eq!(
            c.general.accent,
            Some(crate::config::AccentSpec::Color("#bd93f9".into()))
        );
    }

    #[test]
    fn a_bad_accent_or_empty_theme_is_refused() {
        let err = p("[general]\naccent = 16\n").unwrap_err().to_string();
        assert!(err.contains("accent must be 0 to 15"), "{err}");
        let err = p("[general]\naccent = -1\n").unwrap_err().to_string();
        assert!(err.contains("accent must be 0 to 15"), "{err}");
        let err = p("[general]\naccent = true\n").unwrap_err().to_string();
        assert!(err.contains("accent"), "{err}");
        let err = p("[general]\ntheme = \"\"\n").unwrap_err().to_string();
        assert!(err.contains("theme must not be empty"), "{err}");
        let err = p("[general]\ntheme = \"a\u{202E}b\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("the theme must not contain control characters, format characters or whitespace other than spaces"), "{err}");
        assert!(!err.contains('\u{202E}'), "{err:?}");
    }

    #[test]
    fn the_old_free_form_theme_table_is_an_unknown_key() {
        let err = p("[theme]\nunread = { bold = true }\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("theme"), "{err}");
    }

    #[test]
    fn a_theme_name_may_hold_spaces_but_not_control_or_format_characters() {
        for name in ["Catppuccin Mocha", "iTerm2 Solarized Dark"] {
            let c = p(&format!("[general]\ntheme = \"{name}\"\n")).unwrap();
            assert_eq!(c.general.theme.as_deref(), Some(name));
        }
        let err = p("[general]\ntheme = \"a\\tb\"\n").unwrap_err().to_string();
        assert!(err.contains("the theme must not contain control characters, format characters or whitespace other than spaces"), "{err}");
        let err = p("[general]\ntheme = \"a\u{202E}b\"\n")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("the theme must not contain control characters, format characters or whitespace other than spaces") && !err.contains('\u{202E}'),
            "{err:?}"
        );
    }

    #[test]
    fn an_empty_or_non_integer_accent_gets_the_accent_message() {
        for text in ["accent = \"\"", "accent = 5.0"] {
            let err = p(&format!("[general]\n{text}\n")).unwrap_err().to_string();
            assert!(
                err.contains("accent must be 0 to 15, a colour name or \"#rrggbb\""),
                "{err}"
            );
        }
    }

    #[test]
    fn a_label_loads_and_one_over_forty_characters_is_refused() {
        let account = |name: &str, line: &str| {
            format!(
                "[[accounts]]\nname = \"{name}\"\nkind = \"gmail\"\naddress = \"{name}@example.com\"\nclient_id = \"1\"\n{line}\n"
            )
        };
        let both = format!(
            "{}{}",
            account("a", "label = \"Personal\""),
            account("b", "")
        );
        let config = p(&both).unwrap();
        assert_eq!(config.accounts[0].label.as_deref(), Some("Personal"));
        assert_eq!(config.accounts[1].label, None);
        let fits = account("a", &format!("label = \"{}\"", "é".repeat(40)));
        assert!(p(&fits).is_ok(), "40 characters, not bytes");
        let long = format!(
            "{}{}",
            account("a", ""),
            account("b", &format!("label = \"{}\"", "x".repeat(41)))
        );
        let err = p(&long).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(
            err.to_string()
                .contains("accounts[1].label: at most 40 characters"),
            "{err}"
        );
    }

    /// AC-23 (FR-21): a hand-written `[[saved_searches]]` is checked like an account name, the
    /// error names the offending entry, and **no loader text ever quotes a query** — a
    /// `ConfigError::Invalid` message lands in `OpError.detail`, which the MCP `rail_status`
    /// tool hands to an agent.
    #[test]
    fn saved_searches_are_validated_like_account_names() {
        use crate::config::saved::{MAX_SAVED_SEARCHES, SavedSearch};

        let table = |name: &str, query: &str| {
            format!("[[saved_searches]]\nname = \"{name}\"\nquery = \"{query}\"\n")
        };

        assert!(
            p("").unwrap().saved_searches.is_empty(),
            "no table, no saved searches"
        );
        let one = p(&table("needs-reply", "from:dana is:unread")).unwrap();
        assert_eq!(
            one.saved_searches,
            [SavedSearch {
                name: "needs-reply".into(),
                query: "from:dana is:unread".into(),
            }]
        );

        let many: String = (0..=MAX_SAVED_SEARCHES)
            .map(|i| table(&format!("s{i}"), "is:unread"))
            .collect();
        let err = p(&many).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(
            err.to_string().contains("at most 20 saved searches"),
            "{err}"
        );

        // The query is reported by position and name, never by value: the marker proves it.
        let marker = "uniq-marker-zzz";
        let long = format!("from:{}", marker.repeat(1025 / marker.len() + 1));
        assert!(long.chars().count() > 1024);
        let err = p(&table("big", &long)).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("saved_searches[0]"), "{text}");
        assert!(text.contains("longer than 1024 characters"), "{text}");
        assert!(!text.contains(marker), "a query is never quoted: {text}");

        let err = p(&table("", "is:unread")).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(
            err.to_string()
                .contains("saved search name \"\" must be 1 to 40 letters, digits, '-' or '_'"),
            "{err}"
        );
        let err = p(&table("two words", "is:unread")).unwrap_err();
        assert!(
            err.to_string().contains("saved search name \"two words\""),
            "{err}"
        );
        let fits = table(&"x".repeat(40), "is:unread");
        assert!(p(&fits).is_ok(), "40 characters is the cap, not 39");
        let over = table(&"x".repeat(41), "is:unread");
        assert!(p(&over).is_err(), "41 is over it");

        let duplicate = format!(
            "{}{}",
            table("Dana", "from:dana"),
            table("dana", "is:unread")
        );
        let err = p(&duplicate).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(
            err.to_string().contains("duplicate saved search name"),
            "{err}"
        );

        // #110: an empty or blank query would list every thread of every account.
        let blanks = format!("{}{}", table("ok", "is:unread"), table("blank", " \\t "));
        let err = p(&blanks).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        assert!(
            err.to_string()
                .contains("saved_searches[1] (\"blank\"): the query is empty"),
            "{err}"
        );
        assert!(
            p(&table("none", "")).is_err(),
            "an empty query is refused too"
        );

        // `Config` derives `Debug`, so a derived one here would print every saved query.
        let shown = format!(
            "{:?}",
            SavedSearch {
                name: "x".into(),
                query: "from:dana@corp.example".into(),
            }
        );
        assert!(
            shown.contains('x') && shown.contains("query_chars"),
            "{shown}"
        );
        assert!(!shown.contains("dana"), "{shown}");
    }
}
