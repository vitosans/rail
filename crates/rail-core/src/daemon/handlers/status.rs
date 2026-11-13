//! The handshake, the daemon's own state, `status` and `accounts.list`.

use crate::auth::open_secret_store;
use crate::config;
use crate::daemon::registry::Call;
use crate::daemon::wire::PROTOCOL_VERSION;
use crate::doctor;
use crate::ops::{
    AccountSummary, AccountsListRequest, AccountsListResponse, DaemonInfo, DaemonInfoRequest,
    DaemonStopRequest, DaemonStopResponse, HelloRequest, HelloResponse, OpError, StatusRequest,
    StatusResponse, TokenState,
};

/// Always answers: the client compares versions and decides what to do.
pub(super) async fn hello(call: Call, request: HelloRequest) -> Result<HelloResponse, OpError> {
    if request.protocol != PROTOCOL_VERSION || request.version != call.server.config.version {
        tracing::info!(client = ?request.version, "hello from a client of another version");
    }
    Ok(HelloResponse {
        protocol: PROTOCOL_VERSION,
        version: call.server.config.version.clone(),
        pid: std::process::id(),
        build: Some(call.server.config.build),
        config_dir: call.ctx.paths.config_dir.clone(),
        data_dir: call.ctx.paths.data_dir.clone(),
        state_dir: call.ctx.paths.state_dir.clone(),
    })
}

pub(super) async fn daemon_info(call: Call, _: DaemonInfoRequest) -> Result<DaemonInfo, OpError> {
    Ok(call.server.info(&call.ctx))
}

pub(super) async fn daemon_stop(
    call: Call,
    _: DaemonStopRequest,
) -> Result<DaemonStopResponse, OpError> {
    // Refuse new logins before answering, so a client that heard "stopping" can rely on it;
    // `Server::finish` does the same for the idle and signal paths.
    call.ctx.pending.stop_new();
    call.server.request_shutdown();
    Ok(DaemonStopResponse { stopping: true })
}

pub(super) async fn status(call: Call, _: StatusRequest) -> Result<StatusResponse, OpError> {
    let (paths, ids) = (call.ctx.paths.clone(), call.ctx.op_ids.clone());
    let mut doctor = call
        .ctx
        .blocking(move || Ok(doctor::gather(&paths, &ids)))
        .await?;
    call.ctx.sync.add_to_report(&mut doctor);
    Ok(StatusResponse {
        doctor,
        daemon: call.server.info(&call.ctx),
    })
}

pub(super) async fn accounts_list(
    call: Call,
    _: AccountsListRequest,
) -> Result<AccountsListResponse, OpError> {
    let (paths, ids) = (call.ctx.paths.clone(), call.ctx.op_ids.clone());
    call.ctx
        .blocking(move || {
            let config = config::load(&paths.config_file())?;
            let secrets = open_secret_store(&config, &paths, &ids);
            let accounts = config
                .accounts
                .iter()
                .map(|account| AccountSummary {
                    name: account.name.clone(),
                    kind: account.kind,
                    address: account.address.clone(),
                    color: account.color.clone(),
                    tokens: match account.kind {
                        config::AccountKind::ImapSmtp => {
                            match secrets
                                .has_password(&crate::auth::password_key(&account.address, None))
                            {
                                Ok(true) => TokenState::Present,
                                Ok(false) => {
                                    match secrets.has_password(&crate::auth::password_key(
                                        &account.address,
                                        Some("imap"),
                                    )) {
                                        Ok(true) => TokenState::Present,
                                        Ok(false) => TokenState::Missing,
                                        Err(e) => TokenState::Error {
                                            message: e.to_string().into(),
                                        },
                                    }
                                }
                                Err(e) => TokenState::Error {
                                    message: e.to_string().into(),
                                },
                            }
                        }
                        _ => match secrets.has_tokens(&account.address) {
                            Ok(true) => TokenState::Present,
                            Ok(false) => TokenState::Missing,
                            Err(e) => TokenState::Error {
                                message: e.to_string().into(),
                            },
                        },
                    },
                    ai_send: account.ai_send(),
                })
                .collect();
            Ok(AccountsListResponse { accounts })
        })
        .await
}
