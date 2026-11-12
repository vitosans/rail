//! Verify newly issued tokens before any account or credential write. Provider responses and
//! transport errors are never echoed: they could contain credentials or hostile text.

use serde::Deserialize;

use crate::auth::TokenSet;
use crate::config::{AccountConfig, AccountKind};
use crate::error::ErrorClass;
use crate::ops::{ErrorCode, OpError};
use crate::provider::{gmail::GMAIL_API, graph::GRAPH_API};

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

const MAX_PROFILE_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct GmailProfile {
    #[serde(rename = "emailAddress")]
    address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphProfile {
    mail: Option<String>,
    user_principal_name: Option<String>,
    #[serde(default)]
    proxy_addresses: Vec<String>,
}

fn unreadable() -> OpError {
    OpError::new(
        ErrorCode::Auth,
        ErrorClass::Permanent,
        "could not read the signed-in mailbox's identity",
    )
}

fn unavailable() -> OpError {
    OpError::new(
        ErrorCode::Network,
        ErrorClass::Transient,
        "could not reach the provider to verify the signed-in mailbox",
    )
}

/// `api_root` comes from the daemon's existing debug-only provider overrides. Production uses
/// the provider's fixed root. The HTTP client has redirects disabled and a request timeout.
pub(crate) async fn verify_mailbox(
    http: &reqwest::Client,
    account: &AccountConfig,
    tokens: &TokenSet,
    api_root: Option<&str>,
) -> Result<(), OpError> {
    let root = match api_root {
        Some(r) => r,
        None => match account.kind {
            AccountKind::Gmail => GMAIL_API,
            AccountKind::M365 => GRAPH_API,
            AccountKind::ImapSmtp => return Ok(()),
        },
    };
    let root = url::Url::parse(root).map_err(|_| unreadable())?;
    let path = match account.kind {
        AccountKind::Gmail => "profile",
        AccountKind::M365 => "me?$select=mail,userPrincipalName,proxyAddresses",
        AccountKind::ImapSmtp => return Ok(()),
    };
    let url = root.join(path).map_err(|_| unreadable())?;
    let response = http
        .get(url)
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| unavailable())?;
    let bytes = read_profile(response).await?;
    let matches = match account.kind {
        AccountKind::Gmail => {
            let profile: GmailProfile = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
            gmail_matches(&account.address, &profile.address)
        }
        AccountKind::M365 => {
            let profile: GraphProfile = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
            profile.matches(&account.address)
        }
        AccountKind::ImapSmtp => true,
    };
    if matches {
        Ok(())
    } else {
        Err(OpError::new(ErrorCode::Auth, ErrorClass::Permanent,
            "the signed-in mailbox does not match this account's address")
            .with_hint("log in again and select this account's mailbox; for Google Workspace, use its primary address"))
    }
}

async fn read_profile(mut response: reqwest::Response) -> Result<Vec<u8>, OpError> {
    let status = response.status();
    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(unavailable());
    }
    if !status.is_success() {
        return Err(OpError::new(
            ErrorCode::Auth,
            ErrorClass::Permanent,
            "the provider refused to verify the signed-in mailbox",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if chunk.len() > MAX_PROFILE_BYTES.saturating_sub(bytes.len()) {
            return Err(unreadable());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn gmail_matches(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual)
        || consumer_name(expected)
            .zip(consumer_name(actual))
            .is_some_and(|(expected, actual)| expected == actual)
}

/// Only consumer Gmail addresses have these equivalences. Workspace dots remain significant;
/// custom aliases need the primary address because gmail.modify cannot list directory aliases.
fn consumer_name(address: &str) -> Option<String> {
    let (local, domain) = address.rsplit_once('@')?;
    if !domain.eq_ignore_ascii_case("gmail.com") && !domain.eq_ignore_ascii_case("googlemail.com") {
        return None;
    }
    let name = local
        .split('+')
        .next()?
        .replace('.', "")
        .to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

impl GraphProfile {
    fn matches(&self, address: &str) -> bool {
        let direct = self.mail.iter().chain(&self.user_principal_name);
        direct.into_iter().any(|a| a.eq_ignore_ascii_case(address))
            || self.proxy_addresses.iter().any(|proxy| {
                proxy.split_once(':').is_some_and(|(kind, a)| {
                    kind.eq_ignore_ascii_case("smtp") && a.eq_ignore_ascii_case(address)
                })
            })
    }
}
