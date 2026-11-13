//! `account.password.set`, `account.password.check`, and `account.password.delete`.

use crate::auth::{open_secret_store, password_key};
use crate::config::{AccountKind, load_text_or_empty};
use crate::daemon::registry::Call;
use crate::ops::{
    AccountPasswordCheckRequest, AccountPasswordCheckResponse, AccountPasswordDeleteRequest,
    AccountPasswordDeleteResponse, AccountPasswordSetRequest, AccountPasswordSetResponse, OpError,
};

/// Passwords belong to IMAP/SMTP accounts; OAuth accounts keep tokens. `None` is the shared
/// account password, and `imap`/`smtp` name one endpoint's own password.
fn checked_service(service: Option<&str>) -> Result<Option<&str>, OpError> {
    let Some(service) = service.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if service.eq_ignore_ascii_case("imap") || service.eq_ignore_ascii_case("smtp") {
        Ok(Some(if service.eq_ignore_ascii_case("imap") {
            "imap"
        } else {
            "smtp"
        }))
    } else {
        Err(OpError::invalid_input(
            "service must be imap, smtp or omitted for the account password",
        ))
    }
}

fn require_imapsmtp(account: &crate::config::AccountConfig) -> Result<(), OpError> {
    if account.kind == AccountKind::ImapSmtp {
        Ok(())
    } else {
        Err(OpError::invalid_input(
            "passwords are only stored for IMAP/SMTP accounts; OAuth accounts use tokens",
        ))
    }
}

fn resolve_key(address: &str, service: Option<&str>) -> String {
    password_key(address, service)
}

/// Stores an account or service password in the secret store.
pub(super) async fn set(
    call: Call,
    request: AccountPasswordSetRequest,
) -> Result<AccountPasswordSetResponse, OpError> {
    let config_path = call.ctx.paths.config_file();
    let (_, config) = load_text_or_empty(&config_path)?;
    let account = config
        .accounts
        .iter()
        .find(|a| {
            a.name.eq_ignore_ascii_case(&request.account)
                || a.address.eq_ignore_ascii_case(&request.account)
        })
        .ok_or_else(|| OpError::not_found(format!("account '{}' not found", request.account)))?;
    require_imapsmtp(account)?;
    let service = checked_service(request.service.as_deref())?;
    let key = resolve_key(&account.address, service);
    let store = open_secret_store(&config, &call.ctx.paths, &call.ctx.op_ids);
    let backend_name = store.backend_name().to_string();

    store
        .set_password(&key, &request.password)
        .map_err(|e| OpError::internal(e.to_string()))?;

    call.ctx.sync.reconcile(&call.ctx).await;

    Ok(AccountPasswordSetResponse {
        account: account.name.clone(),
        service: request.service,
        backend: backend_name,
    })
}

/// Checks whether a password exists in the secret store without revealing it.
pub(super) async fn check(
    call: Call,
    request: AccountPasswordCheckRequest,
) -> Result<AccountPasswordCheckResponse, OpError> {
    let config_path = call.ctx.paths.config_file();
    let (_, config) = load_text_or_empty(&config_path)?;
    let account = config
        .accounts
        .iter()
        .find(|a| {
            a.name.eq_ignore_ascii_case(&request.account)
                || a.address.eq_ignore_ascii_case(&request.account)
        })
        .ok_or_else(|| OpError::not_found(format!("account '{}' not found", request.account)))?;
    require_imapsmtp(account)?;
    let service = checked_service(request.service.as_deref())?;
    let key = resolve_key(&account.address, service);
    let store = open_secret_store(&config, &call.ctx.paths, &call.ctx.op_ids);
    let backend_name = store.backend_name().to_string();

    let has_password = store
        .get_password(&key)
        .map(|opt| opt.is_some())
        .unwrap_or(false);

    Ok(AccountPasswordCheckResponse {
        account: account.name.clone(),
        service: request.service,
        has_password,
        backend: backend_name,
    })
}

/// Removes a stored password from the secret store.
pub(super) async fn delete(
    call: Call,
    request: AccountPasswordDeleteRequest,
) -> Result<AccountPasswordDeleteResponse, OpError> {
    let config_path = call.ctx.paths.config_file();
    let (_, config) = load_text_or_empty(&config_path)?;
    let account = config
        .accounts
        .iter()
        .find(|a| {
            a.name.eq_ignore_ascii_case(&request.account)
                || a.address.eq_ignore_ascii_case(&request.account)
        })
        .ok_or_else(|| OpError::not_found(format!("account '{}' not found", request.account)))?;
    require_imapsmtp(account)?;
    let service = checked_service(request.service.as_deref())?;
    let key = resolve_key(&account.address, service);
    let store = open_secret_store(&config, &call.ctx.paths, &call.ctx.op_ids);

    let deleted = store.delete_password(&key).is_ok();
    call.ctx.sync.reconcile(&call.ctx).await;

    Ok(AccountPasswordDeleteResponse {
        account: account.name.clone(),
        service: request.service,
        deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_services_are_only_imap_smtp_or_none() {
        assert_eq!(checked_service(None).unwrap(), None);
        assert_eq!(checked_service(Some(" ")).unwrap(), None);
        assert_eq!(checked_service(Some("IMAP")).unwrap(), Some("imap"));
        assert_eq!(checked_service(Some("smtp")).unwrap(), Some("smtp"));
        assert!(checked_service(Some("pop3")).is_err());
    }

    #[test]
    fn password_keys_never_share_an_entry_with_tokens() {
        assert_eq!(password_key("me@example.com", None), "pw:me@example.com");
        assert_eq!(
            password_key("me@example.com", Some("imap")),
            "pw:me@example.com:imap"
        );
        assert_ne!(password_key("me@example.com", None), "me@example.com");
    }
}
