//! Edit `config.toml` without destroying the user's comments and formatting.

use std::path::Path;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use super::load::{ConfigError, line_col, parse};
use super::model::{AccountConfig, Config, OnePasswordConfig, SecretsBackend};

fn document(text: &str, path: &Path) -> Result<DocumentMut, ConfigError> {
    text.parse().map_err(|e: toml_edit::TomlError| {
        let (line, col) = e.span().map_or((0, 0), |s| line_col(text, s.start));
        ConfigError::Parse {
            path: path.to_path_buf(),
            line,
            col,
            message: e.message().to_string(),
        }
    })
}

fn not_a_table(path: &Path, key: &str) -> ConfigError {
    ConfigError::Invalid {
        path: path.to_path_buf(),
        message: format!("`{key}` must be a table ([{key}])"),
    }
}

/// Return `text` with `[general] secrets` set to `backend` and, when given, the
/// `[onepassword]` table replaced. Comments elsewhere are kept. Validates the result.
pub fn set_secrets(
    text: &str,
    path: &Path,
    backend: SecretsBackend,
    onepassword: Option<&OnePasswordConfig>,
) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let general = doc
        .entry("general")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| not_a_table(path, "general"))?;
    general["secrets"] = value(backend.as_str());
    if let Some(op) = onepassword {
        write_onepassword(&mut doc, op);
    }
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

/// Update `[onepassword]` in place: an existing table keeps its comments and any key not named
/// here; a missing one is created. `op_path` is written only when present, unchanged otherwise.
fn write_onepassword(doc: &mut DocumentMut, op: &OnePasswordConfig) {
    if !doc.get("onepassword").is_some_and(Item::is_table) {
        doc["onepassword"] = Item::Table(Table::new());
    }
    let Some(table) = doc["onepassword"].as_table_mut() else {
        return;
    };
    set_string(table, "vault", &op.vault);
    match &op.account {
        Some(account) => set_string(table, "account", account),
        None => {
            table.remove("account");
        }
    }
    if let Some(op_path) = &op.op_path {
        set_string(table, "op_path", &op_path.to_string_lossy());
    }
}

/// Set `key` to a string, keeping the formatting and trailing comment of an existing value.
fn set_string(table: &mut Table, key: &str, text: &str) {
    set_value(table, key, toml_edit::Value::from(text));
}

/// Set `key` to `new`, keeping the formatting and trailing comment of an existing value.
fn set_value(table: &mut Table, key: &str, mut new: toml_edit::Value) {
    let decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(|v| v.decor().clone());
    if let Some(decor) = decor {
        *new.decor_mut() = decor;
    }
    table[key] = Item::Value(new);
}

/// Return `text` with `[general] key` set to `value`, or removed when `value` is `None`. The
/// table is created when absent; an existing value keeps its formatting and trailing comment;
/// comments and other keys stay. Validates the result.
pub fn set_general(
    text: &str,
    path: &Path,
    key: &str,
    value: Option<toml_edit::Value>,
) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let general = doc
        .entry("general")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| not_a_table(path, "general"))?;
    match value {
        Some(new) => set_value(general, key, new),
        None => {
            general.remove(key);
        }
    }
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

/// `set_general` for a text value.
pub fn set_general_text(
    text: &str,
    path: &Path,
    key: &str,
    value: Option<&str>,
) -> Result<String, ConfigError> {
    set_general(text, path, key, value.map(Into::into))
}

/// `set_general` for an integer value.
pub fn set_general_number(
    text: &str,
    path: &Path,
    key: &str,
    value: Option<i64>,
) -> Result<String, ConfigError> {
    set_general(text, path, key, value.map(Into::into))
}

/// `set_general` for a boolean value.
pub fn set_general_bool(
    text: &str,
    path: &Path,
    key: &str,
    value: Option<bool>,
) -> Result<String, ConfigError> {
    set_general(text, path, key, value.map(Into::into))
}

/// Return `text` with a new `[[accounts]]` table appended. Validates the result.
pub fn append_account(
    text: &str,
    path: &Path,
    account: &AccountConfig,
) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let accounts = doc
        .entry("accounts")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| ConfigError::Invalid {
            path: path.to_path_buf(),
            message: "`accounts` must be an array of tables ([[accounts]])".to_string(),
        })?;

    let mut table = Table::new();
    write_account_fields(&mut table, account);
    accounts.push(table);

    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

/// Return `text` with the `[[accounts]]` table named `account.name` updated. Validates the result.
pub fn update_account(
    text: &str,
    path: &Path,
    account: &AccountConfig,
) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let accounts = doc
        .get_mut("accounts")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| no_such_account(path, &account.name))?;
    let index = accounts
        .iter()
        .position(|t| {
            t.get("name")
                .and_then(Item::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(&account.name))
        })
        .ok_or_else(|| no_such_account(path, &account.name))?;
    let table = accounts
        .get_mut(index)
        .ok_or_else(|| no_such_account(path, &account.name))?;
    write_account_fields(table, account);
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

fn write_account_fields(table: &mut Table, account: &AccountConfig) {
    table["name"] = value(account.name.as_str());
    table["kind"] = value(account.kind.as_str());
    table["address"] = value(account.address.as_str());
    set_opt_str(table, "color", account.color.as_deref());
    set_opt_str(table, "label", account.label.as_deref());
    if account.kind != crate::config::AccountKind::ImapSmtp || !account.client_id.is_empty() {
        table["client_id"] = value(account.client_id.as_str());
    } else {
        table.remove("client_id");
    }
    set_opt_str(table, "client_secret", account.client_secret.as_deref());
    set_opt_str(table, "tenant", account.tenant.as_deref());
    if let Some(signature) = &account.signature {
        table["signature"] = value(signature.to_string_lossy().into_owned());
    } else {
        table.remove("signature");
    }
    set_opt_str(table, "from_name", account.from_name.as_deref());
    if let Some(ai_send) = account.ai_send {
        table["ai_send"] = value(ai_send.as_str());
    } else {
        table.remove("ai_send");
    }
    set_opt_i64(table, "history_days", account.history_days.map(i64::from));
    set_opt_i64(
        table,
        "body_cache_days",
        account.body_cache_days.map(i64::from),
    );
    write_imapsmtp_fields(table, account);
}

fn write_imapsmtp_fields(table: &mut Table, account: &AccountConfig) {
    set_opt_str(table, "imap_host", account.imap_host.as_deref());
    set_opt_i64(table, "imap_port", account.imap_port.map(i64::from));
    set_opt_str(table, "imap_tls", account.imap_tls.map(|t| t.as_str()));
    set_opt_str(table, "imap_user", account.imap_user.as_deref());
    set_opt_str(table, "smtp_host", account.smtp_host.as_deref());
    set_opt_i64(table, "smtp_port", account.smtp_port.map(i64::from));
    set_opt_str(table, "smtp_tls", account.smtp_tls.map(|t| t.as_str()));
    set_opt_str(table, "smtp_user", account.smtp_user.as_deref());
    set_opt_str(table, "sent_folder", account.sent_folder.as_deref());
}

fn set_opt_str(table: &mut Table, key: &str, val: Option<&str>) {
    match val {
        Some(s) => table[key] = value(s),
        None => {
            table.remove(key);
        }
    }
}

fn set_opt_i64(table: &mut Table, key: &str, val: Option<i64>) {
    match val {
        Some(n) => table[key] = value(n),
        None => {
            table.remove(key);
        }
    }
}

/// Return `text` without the `[[accounts]]` table named `name`. Other accounts, other tables
/// and comments stay. Validates the result.
pub fn remove_account(text: &str, path: &Path, name: &str) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let accounts = doc
        .get_mut("accounts")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| no_such_account(path, name))?;
    let index = accounts
        .iter()
        .position(|t| t.get("name").and_then(Item::as_str) == Some(name))
        .ok_or_else(|| no_such_account(path, name))?;
    accounts.remove(index);
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

fn no_such_account(path: &Path, name: &str) -> ConfigError {
    ConfigError::Invalid {
        path: path.to_path_buf(),
        message: format!("no [[accounts]] entry named {name:?}"),
    }
}

/// Return `text` with a saved search written. With `replaces`, the `[[saved_searches]]` table
/// of that name is edited **in place**, keeping its position in the file and the comments
/// around it, so the sidebar row does not move; otherwise a new table is appended. Names are
/// matched ignoring ASCII case, the rule the loader makes them unique by. A `None` query
/// leaves the edited table's `query` line exactly as the file has it. Validates the result.
///
/// # Errors
/// The text is not valid TOML, `saved_searches` is not an array of tables, `replaces` names no
/// entry, or the result would not load (a new table given no query among them).
pub fn put_saved_search(
    text: &str,
    path: &Path,
    name: &str,
    query: Option<&str>,
    replaces: Option<&str>,
) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let searches = doc
        .entry("saved_searches")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| not_an_array_of_tables(path, "saved_searches"))?;
    let index = if let Some(old) = replaces {
        saved_search_at(searches, old).ok_or_else(|| no_such_saved_search(path, old))?
    } else {
        searches.push(Table::new());
        searches.len() - 1
    };
    let table = searches
        .get_mut(index)
        .ok_or_else(|| not_an_array_of_tables(path, "saved_searches"))?;
    // Each line only when its value changed: `set_string` keeps the line's trailing comment,
    // but it cannot keep a literal string's `'…'` form, so writing a value back unchanged
    // would re-quote a line the caller asked nothing of. Not writing it is what keeps the
    // user's quoting (FR-20). Compared exactly, not the way the daemon keys a name, so a
    // case-only rename still writes the new case. A table just pushed has neither key, so
    // `None != Some(…)` writes both.
    if table.get("name").and_then(Item::as_str) != Some(name) {
        set_string(table, "name", name);
    }
    if let Some(query) = query.filter(|q| table.get("query").and_then(Item::as_str) != Some(*q)) {
        set_string(table, "query", query);
    }
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

/// Return `text` without the `[[saved_searches]]` table named `name` (ignoring ASCII case).
/// Other searches, other tables and comments stay. Validates the result.
///
/// # Errors
/// The text is not valid TOML, there is no such entry, or the result would not load.
pub fn remove_saved_search(text: &str, path: &Path, name: &str) -> Result<String, ConfigError> {
    let mut doc = document(text, path)?;
    let searches = doc
        .get_mut("saved_searches")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| no_such_saved_search(path, name))?;
    let index = saved_search_at(searches, name).ok_or_else(|| no_such_saved_search(path, name))?;
    searches.remove(index);
    let out = doc.to_string();
    parse(&out, path)?;
    Ok(out)
}

/// The position of the `[[saved_searches]]` table named `name`, ignoring ASCII case.
fn saved_search_at(searches: &ArrayOfTables, name: &str) -> Option<usize> {
    searches.iter().position(|t| {
        t.get("name")
            .and_then(Item::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

fn no_such_saved_search(path: &Path, name: &str) -> ConfigError {
    ConfigError::Invalid {
        path: path.to_path_buf(),
        message: format!("no [[saved_searches]] entry named {name:?}"),
    }
}

fn not_an_array_of_tables(path: &Path, key: &str) -> ConfigError {
    ConfigError::Invalid {
        path: path.to_path_buf(),
        message: format!("`{key}` must be an array of tables ([[{key}]])"),
    }
}

/// `dana.reyes+x@corp.com` -> `dana_reyes_x`, suffixed `-2`, `-3`... if taken.
pub fn default_account_name(address: &str, config: &Config) -> String {
    let local = address
        .split('@')
        .next()
        .unwrap_or(address)
        .to_ascii_lowercase();
    let base: String = local
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = if base.is_empty() {
        "account".to_string()
    } else {
        base
    };
    let taken = |name: &str| config.accounts.iter().any(|a| a.name == name);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken(candidate))
        .expect("unbounded range always yields a free name")
}
