//! Switching the token backend (`secrets.configure`). Tokens are copied first and read back
//! from the new backend; only then is config written and the old copies deleted, so a failure
//! part way leaves the old copies and the old config in charge.

use std::path::PathBuf;

use super::onepassword::ItemIdCache;

use super::secrets::{SecretStore, open_secret_store};
use crate::config::{Config, OnePasswordConfig, SecretsBackend, load_text_or_empty, set_secrets};
use crate::error::ErrorClass;
use crate::ops::write_config_text;
use crate::ops::{
    ConfirmPlan, ErrorCode, MAX_NAME_CHARS, OpError, SecretsConfigureRequest,
    SecretsConfigureResponse, plan_digest, shown,
};
use crate::paths::Paths;

const ONEPASSWORD: &str = "1password";

/// The message of the refusal for a change nobody confirmed.
pub const NEEDS_CONFIRMATION: &str =
    "changing where tokens are stored needs the user's confirmation";

/// Shown in a confirmation when tokens end up in the plain file.
pub const FILE_WARNING: &str =
    "Tokens will be stored in a plain 0600 file instead of the system keyring or 1Password.";

/// What a `secrets.configure` request would do, for the confirmation a front end shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchSummary {
    /// False when the request leaves every setting as it is.
    pub changes: bool,
    pub previous: SecretsBackend,
    /// The vault and 1Password account tokens are read and deleted from now: they decide
    /// which old copies the switch touches.
    pub previous_vault: Option<String>,
    pub previous_account: Option<String>,
    pub backend: SecretsBackend,
    pub vault: Option<String>,
    pub account: Option<String>,
    /// Whether tokens would live in another store (another backend, vault or account).
    pub moves_store: bool,
    /// Whether the request asks for the tokens to be moved.
    pub migrate: bool,
    /// Accounts whose tokens would be copied to the new store, sorted.
    pub moving: Vec<String>,
}

impl SwitchSummary {
    /// The text to put in front of the user before they confirm. Every value that came from a
    /// request or config is quoted and escaped, so it can neither forge a line nor hide text.
    pub fn text(&self) -> String {
        let old = store_text(self.previous, &self.previous_vault, &self.previous_account);
        let new = store_text(self.backend, &self.vault, &self.account);
        let mut text = format!("Change token storage from {old} to {new}");
        if self.migrate || !self.moves_store {
            text.push_str(&format!(
                ". Tokens for {} account(s) will be moved.",
                self.moving.len()
            ));
        } else {
            text.push_str(
                ". Tokens stay where they are and are not moved, so each account then needs `rail account login <name>`.",
            );
        }
        if self.backend == SecretsBackend::File {
            text.push(' ');
            text.push_str(FILE_WARNING);
        }
        text
    }

    /// The digest the confirmed request must carry: the old and new backend, the vault, the
    /// 1Password account, the migrate flag and the accounts whose tokens move.
    pub fn plan(&self) -> String {
        let migrate = if self.migrate { "migrate" } else { "keep" };
        let mut parts = vec![
            "secrets.configure",
            self.previous.as_str(),
            self.previous_vault.as_deref().unwrap_or(""),
            self.previous_account.as_deref().unwrap_or(""),
            self.backend.as_str(),
            self.vault.as_deref().unwrap_or(""),
            self.account.as_deref().unwrap_or(""),
            migrate,
        ];
        parts.extend(self.moving.iter().map(String::as_str));
        plan_digest(&parts)
    }

    /// The text and the digest, for a `NotConfirmed` refusal.
    pub fn confirm_plan(&self) -> ConfirmPlan {
        ConfirmPlan {
            text: self.text(),
            plan: self.plan(),
        }
    }
}

struct Plan {
    path: PathBuf,
    text: String,
    config: Config,
    target: Config,
}

impl Plan {
    fn load(paths: &Paths, request: &SecretsConfigureRequest) -> Result<Self, OpError> {
        validate_request(request)?;
        let path = paths.config_file();
        let (text, config) = load_text_or_empty(&path)?;
        let target = target_config(&config, request)?;
        Ok(Self {
            path,
            text,
            config,
            target,
        })
    }

    fn changes(&self) -> bool {
        self.target.general.secrets != self.config.general.secrets
            || self.target.onepassword != self.config.onepassword
    }

    /// Whether tokens would live somewhere else: another backend, or another vault or account.
    fn stores_differ(&self) -> bool {
        if self.target.general.secrets != self.config.general.secrets {
            return true;
        }
        let place = |c: &Config| {
            c.onepassword
                .as_ref()
                .map(|o| (o.vault.clone(), o.account.clone()))
        };
        self.target.general.secrets == SecretsBackend::OnePassword
            && place(&self.target) != place(&self.config)
    }
}

fn validate_request(request: &SecretsConfigureRequest) -> Result<(), OpError> {
    for (field, value) in [
        ("vault", &request.vault),
        ("op_account", &request.op_account),
    ] {
        let Some(value) = value.as_deref() else {
            continue;
        };
        // The confirmation shows these whole: a line break could fake another line and a
        // format character could reorder it. The value itself is never echoed.
        if value
            .chars()
            .any(|c| c.is_control() || crate::config::is_format_char(c))
        {
            return Err(OpError::invalid_input(format!(
                "{field} must not contain control or format characters"
            )));
        }
        if value.chars().count() > MAX_NAME_CHARS {
            return Err(OpError::invalid_input(format!(
                "{field} must be at most {MAX_NAME_CHARS} characters"
            )));
        }
    }
    if request
        .op_account
        .as_deref()
        .is_some_and(|a| a.trim().is_empty())
    {
        return Err(OpError::invalid_input("op_account must not be empty"));
    }
    match request.vault.as_deref() {
        Some(v) if v.trim().is_empty() => Err(OpError::invalid_input("vault must not be empty")),
        Some(v) if v.contains('/') => Err(OpError::invalid_input("vault must not contain '/'")),
        _ => Ok(()),
    }
}

/// A backend as a confirmation names it: for 1Password, with its vault and account shown.
fn store_text(backend: SecretsBackend, vault: &Option<String>, account: &Option<String>) -> String {
    let mut text = backend.as_str().to_string();
    if backend == SecretsBackend::OnePassword {
        text.push_str(&format!(
            " (vault {}",
            shown(vault.as_deref().unwrap_or(""))
        ));
        if let Some(account) = account {
            text.push_str(&format!(", account {}", shown(account)));
        }
        text.push(')');
    }
    text
}

/// What `request` would change and which accounts' tokens would move, without changing
/// anything. A front end shows `SwitchSummary::text` and, after the user agrees, sends the
/// request again with `confirm` and `SwitchSummary::plan`.
pub fn describe_switch(
    paths: &Paths,
    ids: &ItemIdCache,
    request: &SecretsConfigureRequest,
) -> Result<SwitchSummary, OpError> {
    let plan = Plan::load(paths, request)?;
    Ok(summarize(&plan, paths, ids, request))
}

fn summarize(
    plan: &Plan,
    paths: &Paths,
    ids: &ItemIdCache,
    request: &SecretsConfigureRequest,
) -> SwitchSummary {
    let onepassword = plan.target.onepassword.as_ref();
    let old = plan.config.onepassword.as_ref();
    let moves_store = plan.changes() && plan.stores_differ();
    let mut moving = if moves_store && request.migrate {
        let old = open_secret_store(&plan.config, paths, ids);
        present_accounts(&plan.config, old.as_ref())
    } else {
        Vec::new()
    };
    moving.sort();
    SwitchSummary {
        changes: plan.changes(),
        previous: plan.config.general.secrets,
        previous_vault: old.map(|o| o.vault.clone()),
        previous_account: old.and_then(|o| o.account.clone()),
        backend: request.backend,
        vault: onepassword.map(|o| o.vault.clone()),
        account: onepassword.and_then(|o| o.account.clone()),
        moves_store,
        migrate: request.migrate,
        moving,
    }
}

/// Point config at `request.backend` and, when asked, move every account's tokens there.
/// Refuses a change that is not `confirm`ed. The caller holds the config lock.
pub fn switch_backend(
    paths: &Paths,
    ids: &ItemIdCache,
    request: &SecretsConfigureRequest,
) -> Result<SecretsConfigureResponse, OpError> {
    let plan = Plan::load(paths, request)?;
    let mut response = SecretsConfigureResponse {
        backend: request.backend,
        previous: plan.config.general.secrets,
        moved: Vec::new(),
        old_copies_kept: Vec::new(),
        already_in_place: Vec::new(),
        config_path: plan.path.clone(),
        changed: false,
    };
    if !plan.changes() {
        return Ok(response);
    }
    let summary = summarize(&plan, paths, ids, request);
    if !request.confirm || request.plan.as_deref() != Some(summary.plan().as_str()) {
        return Err(OpError::not_confirmed(
            NEEDS_CONFIRMATION,
            summary.confirm_plan(),
        ));
    }
    response.changed = true;
    let new_text = set_secrets(
        &plan.text,
        &plan.path,
        request.backend,
        plan.target.onepassword.as_ref(),
    )?;
    let old = open_secret_store(&plan.config, paths, ids);
    let new = open_secret_store(&plan.target, paths, ids);
    new.probe()?;
    let moving = plan.stores_differ();
    if moving && request.migrate {
        response.moved = copy_tokens(&plan.config, old.as_ref(), new.as_ref())?;
    }
    write_config_text(&plan.path, &new_text)?;
    if moving && request.migrate {
        let retired = retire_old_copies(old.as_ref(), new.as_ref(), &response.moved);
        response.moved.retain(|a| !retired.in_place.contains(a));
        response.old_copies_kept = retired.kept;
        response.already_in_place = retired.in_place;
    } else if moving {
        response.old_copies_kept = present_accounts(&plan.config, old.as_ref());
    }
    Ok(response)
}

/// The accounts that have tokens or passwords in `store`. One it cannot read counts as holding
/// them: an old copy that might exist is reported rather than assumed gone.
fn present_accounts(config: &Config, store: &dyn SecretStore) -> Vec<String> {
    config
        .accounts
        .iter()
        .filter(|a| {
            store
                .has_tokens(&a.address)
                .inspect_err(|e| tracing::warn!(error = %e, "could not check an account's tokens"))
                .unwrap_or(true)
                || store.has_password(&a.address).unwrap_or(false)
                || store
                    .has_password(&format!("{}:imap", a.address))
                    .unwrap_or(false)
                || store
                    .has_password(&format!("{}:smtp", a.address))
                    .unwrap_or(false)
        })
        .map(|a| a.address.clone())
        .collect()
}

fn target_config(config: &Config, request: &SecretsConfigureRequest) -> Result<Config, OpError> {
    let mut target = config.clone();
    target.general.secrets = request.backend;
    let wants_onepassword = request.backend == SecretsBackend::OnePassword
        || request.vault.is_some()
        || request.op_account.is_some();
    if wants_onepassword {
        target.onepassword = Some(onepassword_config(config.onepassword.as_ref(), request)?);
    }
    Ok(target)
}

fn onepassword_config(
    existing: Option<&OnePasswordConfig>,
    request: &SecretsConfigureRequest,
) -> Result<OnePasswordConfig, OpError> {
    let vault = request
        .vault
        .clone()
        .or_else(|| existing.map(|o| o.vault.clone()))
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            OpError::invalid_input("the 1password backend needs a vault").with_hint(
                "pass vault, for example \"Rail\"; Rail keeps one item per account there",
            )
        })?;
    Ok(OnePasswordConfig {
        vault,
        account: request
            .op_account
            .clone()
            .or_else(|| existing.and_then(|o| o.account.clone())),
        // Never from the request: only the user, editing config.toml, picks the program.
        op_path: existing.and_then(|o| o.op_path.clone()),
    })
}

/// Copy each account's tokens and check they read back. The first failure stops the move and
/// says which accounts had been copied: their old copies are untouched and config still points
/// at the old backend.
fn copy_tokens(
    config: &Config,
    old: &dyn SecretStore,
    new: &dyn SecretStore,
) -> Result<Vec<String>, OpError> {
    let mut moved = Vec::new();
    for account in &config.accounts {
        let address = account.address.as_str();
        copy_one(address, old, new).map_err(|e| partial_failure(e, &moved, config))?;
        moved.push(address.to_string());
    }
    Ok(moved)
}

/// Copy one account's tokens, if it has any, and read them back from `new`.
fn copy_one(address: &str, old: &dyn SecretStore, new: &dyn SecretStore) -> Result<(), OpError> {
    if let Some(tokens) = old.get_tokens(address)? {
        new.set_tokens(address, &tokens)?;
        if new.get_tokens(address)?.as_ref() != Some(&tokens) {
            return Err(OpError::new(
                ErrorCode::Secrets,
                ErrorClass::Permanent,
                format!(
                    "the tokens for {address} did not read back from the {} backend",
                    new.backend_name()
                ),
            ));
        }
    }
    for key in [
        address,
        &format!("{address}:imap"),
        &format!("{address}:smtp"),
    ] {
        if let Some(pass) = old.get_password(key)? {
            new.set_password(key, &pass)?;
            if new.get_password(key)?.as_ref() != Some(&pass) {
                return Err(OpError::new(
                    ErrorCode::Secrets,
                    ErrorClass::Permanent,
                    format!(
                        "the password for {key} did not read back from the {} backend",
                        new.backend_name()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// `error`, extended with which accounts were copied before it and which were not reached.
/// Addresses come from the config; no token is involved.
fn partial_failure(mut error: OpError, moved: &[String], config: &Config) -> OpError {
    let rest: Vec<&str> = config
        .accounts
        .iter()
        .map(|a| a.address.as_str())
        .filter(|a| !moved.iter().any(|m| m == a))
        .collect();
    error.message = format!(
        "{}; nothing was switched. Copied before the failure (old copies kept): [{}]; not copied: [{}]",
        error.message,
        moved.join(", "),
        rest.join(", ")
    );
    error
}

/// What became of the old copies after a move.
struct Retired {
    /// Old copies that could not be deleted or confirmed gone.
    kept: Vec<String>,
    /// Accounts whose old and new settings name the same item: nothing was deleted.
    in_place: Vec<String>,
}

/// Delete what was moved, once it is sure that it is a different item from the new copy.
/// A copy that will not delete is reported, not fatal: the new backend already holds the
/// tokens and config points at it.
fn retire_old_copies(old: &dyn SecretStore, new: &dyn SecretStore, moved: &[String]) -> Retired {
    let mut retired = Retired {
        kept: Vec::new(),
        in_place: Vec::new(),
    };
    for address in moved {
        match retire_one(old, new, address) {
            Outcome::Deleted => {}
            Outcome::Kept => retired.kept.push(address.clone()),
            Outcome::InPlace => retired.in_place.push(address.clone()),
        }
    }
    retired
}

enum Outcome {
    Deleted,
    Kept,
    InPlace,
}

fn retire_token_copy(old: &dyn SecretStore, new: &dyn SecretStore, address: &str) -> Outcome {
    let by_item = old.backend_name() == ONEPASSWORD;
    if by_item {
        let new_id = if new.backend_name() == ONEPASSWORD {
            match new.item_id(address) {
                Ok(id) => id,
                Err(e) => return kept(&e, "could not look up the new token copy"),
            }
        } else {
            None
        };
        match old.item_id(address) {
            Err(e) => return kept(&e, "could not look up an old token copy"),
            Ok(Some(id)) if new_id.as_ref() == Some(&id) => return Outcome::InPlace,
            Ok(None) => return Outcome::Deleted,
            Ok(Some(_)) => {}
        }
    }
    if let Err(e) = old.delete_tokens(address) {
        return kept(&e, "could not delete an old token copy");
    }
    if by_item && !matches!(old.item_id(address), Ok(None)) {
        tracing::warn!("an old token copy is still there after its delete");
        return Outcome::Kept;
    }
    Outcome::Deleted
}

fn retire_password_copy(old: &dyn SecretStore, new: &dyn SecretStore, key: &str) -> Outcome {
    if !old.has_password(key).unwrap_or(false) {
        return Outcome::Deleted;
    }
    let by_item = old.backend_name() == ONEPASSWORD;
    if by_item {
        let new_id = if new.backend_name() == ONEPASSWORD {
            match new.item_id(key) {
                Ok(id) => id,
                Err(e) => return kept(&e, "could not look up the new password copy"),
            }
        } else {
            None
        };
        match old.item_id(key) {
            Err(e) => return kept(&e, "could not look up an old password copy"),
            Ok(Some(id)) if new_id.as_ref() == Some(&id) => return Outcome::InPlace,
            Ok(None) => return Outcome::Deleted,
            Ok(Some(_)) => {}
        }
    }
    if let Err(e) = old.delete_password(key) {
        return kept(&e, "could not delete an old password copy");
    }
    if by_item && !matches!(old.item_id(key), Ok(None)) {
        tracing::warn!("an old password copy is still there after its delete");
        return Outcome::Kept;
    }
    Outcome::Deleted
}

/// Item stores are checked with uncached lookups (nothing revealed): the old copy is the
/// new one when the ids match, and "no such item" on delete only counts when a fresh lookup
/// agrees the item is gone.
fn retire_one(old: &dyn SecretStore, new: &dyn SecretStore, address: &str) -> Outcome {
    let mut outcome = Outcome::Deleted;
    match retire_token_copy(old, new, address) {
        Outcome::Kept => return Outcome::Kept,
        Outcome::InPlace => outcome = Outcome::InPlace,
        Outcome::Deleted => {}
    }
    for key in [
        address,
        &format!("{address}:imap"),
        &format!("{address}:smtp"),
    ] {
        match retire_password_copy(old, new, key) {
            Outcome::Kept => return Outcome::Kept,
            Outcome::InPlace => outcome = Outcome::InPlace,
            Outcome::Deleted => {}
        }
    }
    outcome
}

fn kept(error: &super::secrets::SecretError, what: &str) -> Outcome {
    tracing::warn!(error = %error, "{what}");
    Outcome::Kept
}

#[cfg(test)]
mod tests;
