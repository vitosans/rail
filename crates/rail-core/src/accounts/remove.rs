//! Removing an account: its config entry, then its tokens. Its mail rows stay in the store;
//! Rail never deletes mail.

use super::login::account_for_login;
use crate::auth::{ItemIdCache, open_secret_store, token_location};
use crate::config::{self, SecretsBackend, load_text_or_empty};
use crate::ops::{
    AccountRemoveResponse, ConfirmPlan, OpError, plan_digest, shown, write_config_text,
};
use crate::paths::Paths;

/// The refusal of a removal nobody confirmed.
pub const REMOVE_NEEDS_CONFIRMATION: &str = "removing an account needs the user's confirmation";

/// What a trusted front end shows the person before it sets `confirm`, and the digest of what
/// it describes: which account goes, where its tokens are, and that its mail stays. The name,
/// the address and the vault are quoted and escaped, so none can forge a line or hide text.
pub fn removal_plan(paths: &Paths, who: &str) -> Result<ConfirmPlan, OpError> {
    let (config, account) = account_for_login(paths, who)?;
    let op_account = config
        .onepassword
        .as_ref()
        .and_then(|o| o.account.as_deref())
        .unwrap_or("");
    let place = match config.general.secrets {
        SecretsBackend::Keyring => "the OS keyring".to_string(),
        SecretsBackend::File => paths.secrets_file().display().to_string(),
        SecretsBackend::OnePassword => {
            let vault = config.onepassword.as_ref().map_or("", |o| o.vault.as_str());
            match op_account {
                "" => format!("1Password vault {}", shown(vault)),
                account => format!(
                    "1Password vault {} (account {})",
                    shown(vault),
                    shown(account)
                ),
            }
        }
    };
    let text = format!(
        "Remove account {} ({}, {})?\n\
         Its config entry is removed and its tokens are deleted from {place}.\n\
         Mail already synced stays in the local store ({}); Rail never deletes mail.",
        shown(&account.name),
        shown(&account.address),
        account.kind.as_str(),
        paths.db_file().display()
    );
    let location = token_location(&config, paths, &account.address);
    let plan = plan_digest(&[
        "account.remove",
        &account.name,
        &account.address,
        account.kind.as_str(),
        config.general.secrets.as_str(),
        &location,
        op_account,
    ]);
    Ok(ConfirmPlan { text, plan })
}

/// Remove the account `who` names after the person confirmed `plan`, the digest of the
/// `removal_plan` they were shown. A missing or stale digest changes nothing and is refused
/// with the current text and digest. The caller holds the config lock.
pub fn remove_confirmed(
    paths: &Paths,
    ids: &ItemIdCache,
    who: &str,
    plan: Option<&str>,
) -> Result<AccountRemoveResponse, OpError> {
    let current = removal_plan(paths, who)?;
    if plan != Some(current.plan.as_str()) {
        return Err(OpError::not_confirmed(REMOVE_NEEDS_CONFIRMATION, current).with_hint(
            "nothing was removed; the user confirms in Rail's own terminal or dialog, and their mail stays",
        ));
    }
    remove_account(paths, ids, who)
}

/// Remove the account `who` names. The config entry goes first, so a failure to delete the
/// tokens leaves no entry whose tokens are gone: the response says the tokens remain and the
/// config change stands. The caller holds the config lock and has the person's confirmation;
/// only `remove_confirmed`, which checks it, calls this.
pub(crate) fn remove_account(
    paths: &Paths,
    ids: &ItemIdCache,
    who: &str,
) -> Result<AccountRemoveResponse, OpError> {
    let (config, account) = account_for_login(paths, who)?;
    let path = paths.config_file();
    let (text, _) = load_text_or_empty(&path)?;
    let new_text = config::remove_account(&text, &path, &account.name)?;
    write_config_text(&path, &new_text)?;
    let store = open_secret_store(&config, paths, ids);
    let deleted = store.delete_tokens(&account.address);
    let mut pass_err = None;
    for key in [
        account.address.as_str(),
        &format!("{}:imap", account.address),
        &format!("{}:smtp", account.address),
    ] {
        if let Err(e) = store.delete_password(key) {
            pass_err = Some(e);
        }
    }
    let removed = deleted.is_ok() && pass_err.is_none();
    let err = deleted.err().or(pass_err);
    Ok(AccountRemoveResponse {
        name: account.name,
        kind: account.kind,
        address: account.address,
        tokens_removed: removed,
        token_error: err.map(|e| e.to_string().into()),
        mail_kept: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{FileSecretStore, SecretStore, TokenSet};

    const WORK: &str = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"cid\"\n";

    fn paths_with(tmp: &tempfile::TempDir) -> Paths {
        let paths = Paths::all_in(tmp.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(paths.config_file(), WORK).unwrap();
        paths
    }

    #[test]
    fn the_summary_names_the_account_the_token_place_and_the_kept_mail() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        let text = removal_plan(&paths, "WORK").unwrap().text;
        assert!(
            text.contains("Remove account \"work\" (\"me@example.com\", gmail)?"),
            "{text}"
        );
        assert!(text.contains(&paths.secrets_file().display().to_string()));
        assert!(text.contains("Mail already synced stays"), "{text}");
        assert!(removal_plan(&paths, "nobody").is_err());
    }

    /// Values from config cannot forge a line or hide text in the question. Config
    /// refuses an address with a line break, a space or a format character (a right-to-left
    /// override) outright, so the plan is never built for one.
    #[test]
    fn a_hostile_address_cannot_forge_the_question() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        let forged = WORK.replace(
            "address = \"me@example.com\"",
            "address = \"me@example.com, gmail)?\\nMail is kept.\"",
        );
        std::fs::write(paths.config_file(), forged).unwrap();
        assert!(removal_plan(&paths, "work").is_err());
        let hostile = WORK.replace(
            "address = \"me@example.com\"",
            "address = \"me@example.com\\u202Eevil\"",
        );
        std::fs::write(paths.config_file(), hostile).unwrap();
        assert!(removal_plan(&paths, "work").is_err());
    }

    /// The 1Password account decides which store the tokens are deleted from.
    #[test]
    fn the_removal_plan_binds_the_1password_account() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        let on = |account: &str| {
            WORK.replace("secrets = \"file\"", "secrets = \"1password\"")
                + &format!("\n[onepassword]\nvault = \"Rail\"\naccount = \"{account}\"\n")
        };
        std::fs::write(paths.config_file(), on("A")).unwrap();
        let asked = removal_plan(&paths, "work").unwrap();
        assert!(
            asked
                .text
                .contains("1Password vault \"Rail\" (account \"A\")"),
            "{}",
            asked.text
        );
        std::fs::write(paths.config_file(), on("B")).unwrap();
        assert_ne!(removal_plan(&paths, "work").unwrap().plan, asked.plan);
    }

    #[test]
    fn a_stale_or_missing_plan_removes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        let ids = ItemIdCache::default();
        let asked = removal_plan(&paths, "work").unwrap();
        for plan in [None, Some("0".repeat(64))] {
            let err = remove_confirmed(&paths, &ids, "work", plan.as_deref()).unwrap_err();
            assert_eq!(err.code, crate::ops::ErrorCode::NotConfirmed);
            assert_eq!(*err.confirmation.unwrap(), asked);
        }
        // The account's kind changes after the question: the old answer no longer counts.
        let changed = WORK.replace("kind = \"gmail\"", "kind = \"m365\"");
        std::fs::write(paths.config_file(), &changed).unwrap();
        let err = remove_confirmed(&paths, &ids, "work", Some(&asked.plan)).unwrap_err();
        assert_ne!(err.confirmation.unwrap().plan, asked.plan);
        assert_eq!(
            std::fs::read_to_string(paths.config_file()).unwrap(),
            changed
        );
        let fresh = removal_plan(&paths, "work").unwrap();
        remove_confirmed(&paths, &ids, "work", Some(&fresh.plan)).unwrap();
        assert!(
            config::load(&paths.config_file())
                .unwrap()
                .accounts
                .is_empty()
        );
    }

    #[test]
    fn a_token_delete_failure_is_reported_and_the_config_change_stands() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        std::fs::create_dir_all(paths.secrets_file().parent().unwrap()).unwrap();
        std::fs::write(paths.secrets_file(), "TOP-SECRET not json").unwrap();
        let removed = remove_account(&paths, &ItemIdCache::default(), "work").unwrap();
        assert!(!removed.tokens_removed && removed.mail_kept);
        let reason = removed.token_error.unwrap();
        assert!(!reason.get().contains("TOP-SECRET"), "{reason:?}");
        let left = config::load(&paths.config_file()).unwrap();
        assert!(left.accounts.is_empty());
    }

    #[test]
    fn a_clean_removal_deletes_the_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_with(&tmp);
        let secrets = FileSecretStore::new(paths.secrets_file());
        let tokens = TokenSet {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1,
        };
        secrets.set_tokens("me@example.com", &tokens).unwrap();
        let removed = remove_account(&paths, &ItemIdCache::default(), "work").unwrap();
        assert!(removed.tokens_removed && removed.token_error.is_none());
        assert_eq!(secrets.get_tokens("me@example.com").unwrap(), None);
    }
}
