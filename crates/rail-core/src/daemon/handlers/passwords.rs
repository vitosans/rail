//! `account.password.set`, `account.password.check`, and `account.password.delete`.

use crate::auth::open_secret_store;
use crate::config::load_text_or_empty;
use crate::daemon::registry::Call;
use crate::ops::{
    AccountPasswordCheckRequest, AccountPasswordCheckResponse, AccountPasswordDeleteRequest,
    AccountPasswordDeleteResponse, AccountPasswordSetRequest, AccountPasswordSetResponse, OpError,
};

fn resolve_key(address: &str, service: Option<&str>) -> String {
    match service {
        Some(s) if !s.trim().is_empty() => format!("{}:{}", address, s.trim().to_ascii_lowercase()),
        _ => address.to_string(),
    }
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

    let key = resolve_key(&account.address, request.service.as_deref());
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

    let key = resolve_key(&account.address, request.service.as_deref());
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

    let key = resolve_key(&account.address, request.service.as_deref());
    let store = open_secret_store(&config, &call.ctx.paths, &call.ctx.op_ids);

    let deleted = store.delete_password(&key).is_ok();
    call.ctx.sync.reconcile(&call.ctx).await;

    Ok(AccountPasswordDeleteResponse {
        account: account.name.clone(),
        service: request.service,
        deleted,
    })
}
