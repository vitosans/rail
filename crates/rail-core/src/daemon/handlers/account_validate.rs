//! `account.validate`: test IMAP and SMTP connectivity and credentials.

use crate::auth::{open_secret_store, password_key};
use crate::config::load_text_or_empty;
use crate::config::model::TlsMode;
use crate::daemon::registry::Call;
use crate::error::ErrorClass;
use crate::ops::{AccountValidateRequest, AccountValidateResponse, ErrorCode, OpError, Secret};
use crate::provider::imap_smtp::client::connect_and_login_imap;
use crate::provider::imap_smtp::folders::discover_folders;
use crate::provider::imap_smtp::smtp::{build_smtp_transport, validate_smtp_connection};

/// Validates IMAP and SMTP connections and credentials.
pub(super) async fn validate(
    call: Call,
    request: AccountValidateRequest,
) -> Result<AccountValidateResponse, OpError> {
    let (imap_params, smtp_params) = resolve_params(&call, &request).await?;

    let (mut session, caps) = connect_and_login_imap(
        &imap_params.host,
        imap_params.port,
        imap_params.tls,
        &imap_params.username,
        &imap_params.password,
    )
    .await
    .map_err(OpError::from)?;

    let discovered = discover_folders(&mut session, None)
        .await
        .map_err(OpError::from)?;

    let transport = build_smtp_transport(
        &smtp_params.host,
        Some(smtp_params.port),
        smtp_params.tls,
        Some(&smtp_params.username),
        Some(&smtp_params.password),
    )
    .map_err(OpError::from)?;
    validate_smtp_connection(&transport)
        .await
        .map_err(OpError::from)?;

    let mut cap_strings = Vec::new();
    if caps.move_support {
        cap_strings.push("MOVE".to_string());
    }
    if caps.uidplus_support {
        cap_strings.push("UIDPLUS".to_string());
    }
    if caps.idle_support {
        cap_strings.push("IDLE".to_string());
    }

    let special_mailboxes = discovered
        .folders
        .iter()
        .map(|f| format!("{}:{}", f.name, f.kind.as_str()))
        .collect();

    let mut warnings = Vec::new();
    if imap_params.tls == TlsMode::Off {
        warnings.push(
            "IMAP TLS is off: your password and mail travel in clear text. Use required or opportunistic."
                .to_string(),
        );
    }
    if smtp_params.tls == TlsMode::Off {
        warnings.push(
            "SMTP TLS is off: your password and mail travel in clear text. Use required or opportunistic."
                .to_string(),
        );
    }

    Ok(AccountValidateResponse {
        imap_ok: true,
        smtp_ok: true,
        imap_capabilities: cap_strings,
        special_use_mailboxes: special_mailboxes,
        warnings,
    })
}

struct EndpointParams {
    host: String,
    port: u16,
    tls: TlsMode,
    username: String,
    password: Secret<String>,
}

async fn resolve_params(
    call: &Call,
    req: &AccountValidateRequest,
) -> Result<(EndpointParams, EndpointParams), OpError> {
    if let Some(ref account_name) = req.account {
        resolve_from_account(call, account_name, req).await
    } else {
        resolve_from_inline(req)
    }
}

async fn resolve_from_account(
    call: &Call,
    account_name: &str,
    req: &AccountValidateRequest,
) -> Result<(EndpointParams, EndpointParams), OpError> {
    let config_path = call.ctx.paths.config_file();
    let (_, config) = load_text_or_empty(&config_path)?;
    let account = config
        .accounts
        .iter()
        .find(|a| {
            a.name.eq_ignore_ascii_case(account_name)
                || a.address.eq_ignore_ascii_case(account_name)
        })
        .ok_or_else(|| OpError::not_found(format!("account '{account_name}' not found")))?;

    let store = open_secret_store(&config, &call.ctx.paths, &call.ctx.op_ids);

    let stored = |service: Option<&str>| {
        store
            .get_password(&password_key(&account.address, service))
            .map_err(|e| OpError::internal(e.to_string()))
    };
    let imap_pass = match req.password {
        Some(ref p) => p.clone(),
        None => stored(Some("imap"))?.or(stored(None)?).ok_or_else(|| {
            OpError::new(
                ErrorCode::Auth,
                ErrorClass::Permanent,
                "IMAP password not found in secret store",
            )
        })?,
    };

    let smtp_pass = match req.smtp_password.as_ref() {
        Some(p) => p.clone(),
        None => match stored(Some("smtp"))? {
            Some(p) => p,
            None => match req.password.as_ref() {
                Some(p) => p.clone(),
                None => stored(None)?.unwrap_or_else(|| imap_pass.clone()),
            },
        },
    };

    let imap = EndpointParams {
        host: account.imap_host.clone().unwrap_or_default(),
        port: account.imap_port(),
        tls: account.imap_tls(),
        username: account.imap_user().to_string(),
        password: imap_pass,
    };

    let smtp = EndpointParams {
        host: account.smtp_host.clone().unwrap_or_default(),
        port: account.smtp_port(),
        tls: account.smtp_tls(),
        username: account.smtp_user().to_string(),
        password: smtp_pass,
    };

    Ok((imap, smtp))
}

fn resolve_from_inline(
    req: &AccountValidateRequest,
) -> Result<(EndpointParams, EndpointParams), OpError> {
    let imap_host = req
        .imap_host
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing imap_host"))?;
    let imap_user = req
        .username
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing username"))?;
    let imap_pass = req
        .password
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing password"))?;

    let smtp_host = req
        .smtp_host
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing smtp_host"))?;
    let smtp_user = req
        .smtp_username
        .clone()
        .unwrap_or_else(|| imap_user.clone());
    let smtp_pass = req
        .smtp_password
        .clone()
        .unwrap_or_else(|| imap_pass.clone());

    let imap = EndpointParams {
        host: imap_host,
        port: req.imap_port.unwrap_or(993),
        tls: req.imap_tls.unwrap_or(TlsMode::Required),
        username: imap_user,
        password: imap_pass,
    };

    let smtp = EndpointParams {
        host: smtp_host,
        port: req.smtp_port.unwrap_or(465),
        tls: req.smtp_tls.unwrap_or(TlsMode::Required),
        username: smtp_user,
        password: smtp_pass,
    };

    Ok((imap, smtp))
}
