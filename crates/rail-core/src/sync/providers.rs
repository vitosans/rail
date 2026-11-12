//! Building an account's provider by its kind: the factory the daemon's sync manager uses.

use std::sync::Arc;

use url::Url;

use super::manager::ProviderRequest;
use crate::accounts::{endpoints_for_login, login_secret};
use crate::auth::{TokenSource, build_client, http_client, open_secret_store};
use crate::config::{AccountConfig, AccountKind, SyncSettings};
use crate::ops::OpError;
use crate::provider::Provider;
use crate::provider::gmail::{GMAIL_API, GmailProvider};
use crate::provider::graph::{GRAPH_API, GRAPH_CONCURRENCY, GraphProvider};
use crate::provider::http::{ApiClient, Dialect, Quota, api_http_client};
use crate::provider::tokens::OAuthTokens;

/// Graph's pace for one account: quota units a minute (one per request, one per batch item),
/// under Outlook's 10,000 requests per 10 minutes per app and mailbox.
pub const GRAPH_QUOTA_PER_MINUTE: u32 = 900;
/// Units a Graph account may spend at once before the pacing starts.
pub const GRAPH_QUOTA_BURST: u32 = 100;

/// Whether Rail syncs accounts of `kind`: every kind has a provider in this version
/// (`SyncPhase::Unsupported` stays for a kind without one).
pub fn syncs(kind: AccountKind) -> bool {
    matches!(
        kind,
        AccountKind::Gmail | AccountKind::M365 | AccountKind::ImapSmtp
    )
}

/// The account's provider, by its kind.
pub fn provider_for(request: ProviderRequest) -> Result<Arc<dyn Provider>, OpError> {
    match request.account.kind {
        AccountKind::Gmail => gmail_for(request),
        AccountKind::M365 => graph_for(request),
        AccountKind::ImapSmtp => imap_smtp_for(request),
    }
}

/// A Gmail provider for the account.
pub fn gmail_for(request: ProviderRequest) -> Result<Arc<dyn Provider>, OpError> {
    if request.account.kind != AccountKind::Gmail {
        return Err(OpError::invalid_input(
            "this account is not a Gmail account",
        ));
    }
    let root = request.gmail_url.as_deref().unwrap_or(GMAIL_API);
    let api = api_for(&request, root, "Gmail")?;
    Ok(Arc::new(GmailProvider::new(api)))
}

/// A Microsoft 365 provider for the account: Graph's dialect, four requests in flight,
/// Graph's pace, and initial deltas within the account's `history_days`.
pub fn graph_for(request: ProviderRequest) -> Result<Arc<dyn Provider>, OpError> {
    if request.account.kind != AccountKind::M365 {
        return Err(OpError::invalid_input(
            "this account is not a Microsoft 365 account",
        ));
    }
    let root = request.graph_url.as_deref().unwrap_or(GRAPH_API);
    let api = api_for(&request, root, "Microsoft Graph")?
        .with_dialect(Dialect::Graph)
        .with_concurrency(GRAPH_CONCURRENCY)
        .with_quota(Quota::new(GRAPH_QUOTA_PER_MINUTE, GRAPH_QUOTA_BURST));
    Ok(Arc::new(
        GraphProvider::new(api)
            .with_history_days(window_days(&request))
            .with_own_address(&request.account.address),
    ))
}

/// An IMAP/SMTP provider for the account.
pub fn imap_smtp_for(request: ProviderRequest) -> Result<Arc<dyn Provider>, OpError> {
    if request.account.kind != AccountKind::ImapSmtp {
        return Err(OpError::invalid_input(
            "this account is not an IMAP/SMTP account",
        ));
    }
    let imap_host = request
        .account
        .imap_host
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing imap_host for IMAP/SMTP account"))?;
    let imap_port = request.account.imap_port.unwrap_or(993);
    let imap_tls_mode = request
        .account
        .imap_tls
        .unwrap_or(crate::config::model::TlsMode::Required);

    let smtp_host = request
        .account
        .smtp_host
        .clone()
        .ok_or_else(|| OpError::invalid_input("missing smtp_host for IMAP/SMTP account"))?;
    let smtp_port = request.account.smtp_port;
    let smtp_tls_mode = request
        .account
        .smtp_tls
        .unwrap_or(crate::config::model::TlsMode::Required);

    let secrets = open_secret_store(&request.config, &request.paths, &request.ids);
    let password = secrets
        .get_password(&request.account.address)
        .ok()
        .flatten()
        .or_else(|| {
            secrets
                .get_password(&format!("{}:imap", request.account.address))
                .ok()
                .flatten()
        })
        .or_else(|| secrets.get_password(&request.account.name).ok().flatten())
        .ok_or_else(|| {
            OpError::new(
                crate::ops::ErrorCode::Auth,
                crate::error::ErrorClass::Permanent,
                "no password stored for this account; run 'rail secrets password'",
            )
        })?;

    let username = request.account.address.clone();
    let sent_mailbox_name = request.account.sent_folder.clone();

    Ok(Arc::new(crate::provider::imap_smtp::ImapSmtpProvider::new(
        imap_host,
        imap_port,
        imap_tls_mode,
        username.clone(),
        password.clone(),
        smtp_host,
        smtp_port,
        smtp_tls_mode,
        Some(username),
        Some(password),
        sent_mailbox_name,
    )))
}

/// The account's `history_days` as its engine gets it: `SyncSettings` from the same config
/// the request carries. A provider whose window were narrower than the engine's would let
/// the caught-up sweep delete the stored mail between the two cutoffs.
pub(crate) fn window_days(request: &ProviderRequest) -> u32 {
    SyncSettings::for_account(&request.config.general, &request.account).history_days
}

/// The API client for the account at `root`, refreshing its tokens through the configured
/// secret store. The client secret (an `op://` reference is read now) stays in the OAuth
/// client.
fn api_for(request: &ProviderRequest, root: &str, what: &str) -> Result<ApiClient, OpError> {
    let secret = login_secret(&request.account, &request.config, &request.paths)?;
    let account = AccountConfig {
        client_secret: secret.map(|s| s.expose().to_string()),
        ..request.account.clone()
    };
    let endpoints = endpoints_for_login(&account, request.token_url.as_deref());
    let client = build_client(&account, &endpoints, None)?;
    let secrets = open_secret_store(&request.config, &request.paths, &request.ids);
    let source = TokenSource::new(account.address.clone(), client, http_client()?, secrets);
    let base = Url::parse(root)
        .map_err(|_| OpError::internal(format!("the {what} API address does not parse")))?;
    Ok(
        ApiClient::new(api_http_client()?, base, Arc::new(OAuthTokens::new(source)))
            .with_observer(request.observer.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ItemIdCache;
    use crate::config::{Config, General, SecretsBackend};
    use crate::paths::Paths;
    use crate::test_support::sample_account;

    fn request(account: AccountConfig, paths: Paths) -> ProviderRequest {
        ProviderRequest {
            paths,
            config: Config {
                general: General {
                    secrets: SecretsBackend::File,
                    ..General::default()
                },
                ..Config::default()
            },
            account,
            ids: ItemIdCache::default(),
            token_url: Some("http://127.0.0.1:9/token".into()),
            gmail_url: Some("http://127.0.0.1:9/gmail/v1/users/me/".into()),
            graph_url: Some("http://127.0.0.1:9/v1.0/".into()),
            observer: Arc::new(|_| {}),
        }
    }

    #[test]
    fn a_microsoft_account_gets_no_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let account = AccountConfig {
            kind: AccountKind::M365,
            ..sample_account()
        };
        let err = gmail_for(request(account, Paths::all_in(tmp.path())))
            .err()
            .expect("refused");
        assert_eq!(err.code, crate::ops::ErrorCode::InvalidInput);
    }

    #[test]
    fn a_gmail_account_builds_without_touching_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let built = gmail_for(request(sample_account(), Paths::all_in(tmp.path())));
        assert!(built.is_ok());
        let mut bad = request(sample_account(), Paths::all_in(tmp.path()));
        bad.gmail_url = Some("not a url".into());
        assert!(gmail_for(bad).is_err());
    }

    #[test]
    fn each_kind_gets_its_provider_without_touching_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = || Paths::all_in(tmp.path());
        let m365 = AccountConfig {
            kind: AccountKind::M365,
            ..sample_account()
        };
        assert!(provider_for(request(sample_account(), paths())).is_ok());
        assert!(provider_for(request(m365.clone(), paths())).is_ok());
        let refused = graph_for(request(sample_account(), paths()))
            .err()
            .expect("a Gmail account is not Graph's");
        assert_eq!(refused.code, crate::ops::ErrorCode::InvalidInput);
        let mut bad = request(m365, paths());
        bad.graph_url = Some("not a url".into());
        assert!(graph_for(bad).is_err());
        assert!(
            syncs(AccountKind::Gmail) && syncs(AccountKind::M365) && syncs(AccountKind::ImapSmtp)
        );
    }

    #[test]
    fn imap_smtp_provider_building() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::all_in(tmp.path());
        let mut account = sample_account();
        account.kind = AccountKind::ImapSmtp;
        account.imap_host = Some("imap.example.com".into());
        account.smtp_host = Some("smtp.example.com".into());

        // Password not stored yet -> ErrorCode::Auth
        let req = request(account.clone(), paths.clone());
        let err = imap_smtp_for(req).err().expect("auth required");
        assert_eq!(err.code, crate::ops::ErrorCode::Auth);

        // Store password
        let secrets = open_secret_store(
            &Config {
                general: General {
                    secrets: SecretsBackend::File,
                    ..General::default()
                },
                ..Config::default()
            },
            &paths,
            &ItemIdCache::default(),
        );
        secrets
            .set_password(
                &account.name,
                &crate::ops::Secret::new("testpass".to_string()),
            )
            .unwrap();

        // Now provider_for and imap_smtp_for succeed
        let built = imap_smtp_for(request(account.clone(), paths.clone()));
        assert!(built.is_ok());
        let provider = provider_for(request(account, paths));
        assert!(provider.is_ok());
    }
}
