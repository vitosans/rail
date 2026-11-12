//! Adding an account: everything that can be checked offline is checked before the browser
//! opens, and the save writes tokens, then the store row, then config.toml last, so a failure
//! never leaves an account in config that a retry would refuse as a duplicate.

use std::path::{Path, PathBuf};

use super::client_secret::{ResolvedSecret, resolve_client_secret};
use super::login::saved_account;
use super::{blocking, context};
use crate::auth::{
    ItemIdCache, OpCli, SecretStore, TokenSet, open_secret_store, store_client_secret,
};
use crate::config::{self, AccountConfig, AccountKind, Config, load_text_or_empty};
use crate::error::ErrorClass;
use crate::ops::{
    AccountAddRequest, ClientSecretInput, ErrorCode, OP_REFERENCE_PREFIX, OpError, SavedAccount,
    Secret, write_config_text,
};
use crate::paths::Paths;
use crate::store::Store;

/// A validated new account, ready for its login. No `Debug`: it holds the client secret.
pub struct PreparedAdd {
    /// The account as config.toml will hold it: `client_secret` is the value or a reference.
    pub account: AccountConfig,
    /// The client secret the OAuth client uses for the login.
    pub login_secret: Option<Secret>,
    /// Put the client secret in 1Password at save time and write the reference instead.
    pub store_in_1password: bool,
    pub config_path: PathBuf,
}

impl PreparedAdd {
    /// The account with the resolved client secret, for the login only. Never written.
    pub fn login_account(&self) -> AccountConfig {
        AccountConfig {
            client_secret: self.login_secret.as_ref().map(|s| s.expose().to_string()),
            ..self.account.clone()
        }
    }
}

/// Check `request` against the current config: no duplicate address or name, a client id from
/// the request or the resolved input, and a config that can take the account.
pub fn prepare_add(
    paths: &Paths,
    request: &AccountAddRequest,
    resolved: ResolvedSecret,
) -> Result<PreparedAdd, OpError> {
    let config_path = paths.config_file();
    let (text, config) = load_text_or_empty(&config_path)?;
    let account = build_account(request, &resolved, &config)?;
    check_shown_fields(&account)?;
    appended(&text, &config_path, &config, &account)?;
    if request.store_secret_in_1password {
        check_store_in_1password(request, &config, &resolved)?;
    }
    Ok(PreparedAdd {
        account,
        login_secret: resolved.login,
        store_in_1password: request.store_secret_in_1password,
        config_path,
    })
}

/// `store_secret_in_1password` needs a vault and a client secret, and refuses a secret that is
/// already an `op://` reference: copying it would put the value of an item the request named
/// (any item the user can read) into Rail's vault.
fn check_store_in_1password(
    request: &AccountAddRequest,
    config: &Config,
    resolved: &ResolvedSecret,
) -> Result<(), OpError> {
    let reference = resolved
        .stored
        .as_ref()
        .is_some_and(|s| s.expose().starts_with(OP_REFERENCE_PREFIX));
    if reference || is_reference_copy(request) {
        return Err(already_in_1password());
    }
    if config.onepassword.is_none() {
        return Err(OpError::invalid_input(
            "store_secret_in_1password needs a 1Password vault in config.toml",
        )
        .with_hint(
            "configure one first with `rail secrets set 1password --vault <vault>` or rail_secrets_configure, or add [onepassword] vault = \"Rail\"",
        ));
    }
    if resolved.login.is_none() {
        return Err(OpError::invalid_input(
            "store_secret_in_1password needs a client secret to store",
        ));
    }
    Ok(())
}

/// A request to store in 1Password a client secret given as an `op://` reference.
fn is_reference_copy(request: &AccountAddRequest) -> bool {
    request.store_secret_in_1password
        && matches!(request.client_secret, Some(ClientSecretInput::Reference(_)))
}

fn already_in_1password() -> OpError {
    OpError::invalid_input(
        "the client secret is already in 1Password; drop store_secret_in_1password",
    )
}

/// The address, colour and tenant are shown on the terminal and in confirmations: refuse
/// spaces and control characters (an escape sequence, say) without echoing the value.
fn check_shown_fields(account: &AccountConfig) -> Result<(), OpError> {
    let fields = [
        ("address", Some(account.address.as_str())),
        ("color", account.color.as_deref()),
        ("tenant", account.tenant.as_deref()),
    ];
    match fields
        .iter()
        .find(|(_, value)| value.is_some_and(|v| !config::is_plain(v)))
    {
        Some((field, _)) => Err(OpError::invalid_input(config::not_plain(field))),
        None => Ok(()),
    }
}

fn build_account(
    request: &AccountAddRequest,
    resolved: &ResolvedSecret,
    config: &Config,
) -> Result<AccountConfig, OpError> {
    let client_id = match (&request.client_id, &resolved.client_id) {
        (Some(given), Some(found)) if given.trim() != found.trim() => {
            return Err(OpError::invalid_input(
                "the client id does not match the one in the client secret input",
            ));
        }
        (Some(id), _) | (None, Some(id)) => id.trim().to_string(),
        (None, None) => return Err(OpError::invalid_input("a client id is needed")),
    };
    let address = request.address.trim().to_ascii_lowercase();
    if address.chars().count() > config::MAX_ADDRESS_CHARS {
        return Err(OpError::invalid_input(format!(
            "the address is longer than {} characters",
            config::MAX_ADDRESS_CHARS
        )));
    }
    Ok(AccountConfig {
        name: request
            .name
            .clone()
            .unwrap_or_else(|| config::default_account_name(&address, config)),
        kind: request.kind,
        address,
        color: request.color.clone(),
        label: None,
        client_id,
        client_secret: resolved.stored.as_ref().map(|s| s.expose().to_string()),
        tenant: match request.kind {
            AccountKind::M365 => request.tenant.clone().or_else(|| resolved.tenant.clone()),
            _ => None,
        },
        signature: None,
        from_name: None,
        ai_send: None,
        history_days: None,
        body_cache_days: None,
        ..Default::default()
    })
}

/// `text` with `account` appended, after refusing an address already configured.
fn appended(
    text: &str,
    path: &Path,
    config: &Config,
    account: &AccountConfig,
) -> Result<String, OpError> {
    refuse_taken(config, path, &account.address)?;
    Ok(config::append_account(text, path, account)?)
}

/// Refuse an address that config already holds.
fn refuse_taken(config: &Config, path: &Path, address: &str) -> Result<(), OpError> {
    let taken = config
        .accounts
        .iter()
        .any(|a| a.address.eq_ignore_ascii_case(address));
    if taken {
        return Err(OpError::new(
            ErrorCode::Duplicate,
            ErrorClass::Permanent,
            format!(
                "{address} is already configured in {}; run `rail account login {address}` to sign in again",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// A read-only reachability probe, so a broken secret store fails before the browser opens.
pub fn probe_secrets(secrets: &dyn SecretStore, address: &str) -> Result<(), OpError> {
    // Presence only: a 1Password store must not reveal the tokens just to be probed.
    secrets
        .has_tokens(address)
        .map(drop)
        .map_err(|e| context("secret storage is not usable", e))
}

/// Read the config and build the `op` runner on the blocking pool (the latter walks the file
/// system), refusing a configured address first: a duplicate must not cost an unlock prompt.
async fn load_for_prepare(paths: &Paths, address: &str) -> Result<(Config, OpCli), OpError> {
    let (paths, address) = (paths.clone(), address.trim().to_ascii_lowercase());
    blocking(move || {
        let path = paths.config_file();
        let (_, config) = load_text_or_empty(&path)?;
        refuse_taken(&config, &path, &address)?;
        let op = OpCli::from_config(config.onepassword.as_ref(), &paths.runtime_dir);
        Ok((config, op))
    })
    .await
}

/// Resolve the request's client secret and validate the account against config, without
/// touching the secret store. The config it read is returned for the caller's store.
async fn resolve_and_prepare(
    paths: &Paths,
    request: &AccountAddRequest,
    http: &reqwest::Client,
) -> Result<(PreparedAdd, Config), OpError> {
    if is_reference_copy(request) {
        return Err(already_in_1password());
    }
    let (config, op) = load_for_prepare(paths, &request.address).await?;
    let resolved = resolve_client_secret(request.client_secret.as_ref(), http, &op).await?;
    let paths = paths.clone();
    let request = request.clone();
    let prepared = blocking(move || prepare_add(&paths, &request, resolved)).await?;
    Ok((prepared, config))
}

/// Resolve the client secret, run every offline check of `prepare_add`, then probe the
/// secret store, so a broken store fails before any browser opens. The one place that
/// sequence lives; the daemon runs it for every front end.
pub async fn prepare_and_probe(
    paths: &Paths,
    ids: &ItemIdCache,
    request: &AccountAddRequest,
    http: &reqwest::Client,
) -> Result<PreparedAdd, OpError> {
    let (prepared, config) = resolve_and_prepare(paths, request, http).await?;
    let (paths, ids) = (paths.clone(), ids.clone());
    blocking(move || {
        let secrets = open_secret_store(&config, &paths, &ids);
        probe_secrets(secrets.as_ref(), &prepared.account.address)?;
        Ok(prepared)
    })
    .await
}

/// Tokens, then the store row, then (if asked) the client secret into 1Password, then
/// config.toml last. The config is read again first: another front end may have added an
/// account since `prepare_add`. The caller holds the config lock.
pub fn persist_new(
    paths: &Paths,
    ids: &ItemIdCache,
    prepared: &PreparedAdd,
    tokens: &TokenSet,
) -> Result<SavedAccount, OpError> {
    let path = &prepared.config_path;
    let (text, config) = load_text_or_empty(path)?;
    let mut account = prepared.account.clone();
    appended(&text, path, &config, &account)?;
    let secrets = open_secret_store(&config, paths, ids);
    secrets
        .set_tokens(&account.address, tokens)
        .map_err(|e| context("storing tokens", e))?;
    Store::open(&paths.db_file())?.upsert_account(&account)?;
    if prepared.store_in_1password
        && let (Some(op), Some(secret)) = (&config.onepassword, &prepared.login_secret)
    {
        let cli = OpCli::from_config(Some(op), &paths.runtime_dir);
        let reference = store_client_secret(&cli, &op.vault, &account.client_id, secret)
            .map_err(|e| context("storing the client secret in 1Password", e))?;
        account.client_secret = Some(reference);
    }
    let new_text = appended(&text, path, &config, &account)?;
    write_config_text(path, &new_text)?;
    Ok(saved_account(&config, paths, &account))
}

#[cfg(test)]
mod tests;
