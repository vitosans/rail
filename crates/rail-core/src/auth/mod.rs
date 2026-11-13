//! OAuth2 login, token storage and refresh.

pub mod client;
pub mod endpoints;
pub mod flow;
pub mod loopback;
pub mod migrate;
pub mod onepassword;
pub mod secrets;
pub mod token_source;

pub use client::{OAuthClient, build_client, http_client};
pub use endpoints::{
    ADMIN_CONSENT_SCOPE, GMAIL_SCOPE, GRAPH_SCOPES, OAuthEndpoints, admin_consent_url,
    endpoints_for,
};
pub use flow::{
    AuthSession, ConsentSession, begin_admin_consent, begin_auth_code, random_token,
    run_auth_code_flow,
};
pub use loopback::Redirect;
pub use onepassword::{ItemIdCache, OnePasswordSecretStore, OpCli, store_client_secret};
pub use secrets::{
    FileSecretStore, KeyringSecretStore, MemorySecretStore, PROBE_ADDRESS, SecretError,
    SecretStore, TokenSet, open_secret_store, password_key, token_location,
};

pub use token_source::{TokenSource, unix_now};

use crate::error::ErrorClass;

/// Microsoft error codes that mean an administrator has to approve the app first.
const CONSENT_REQUIRED_CODES: [&str; 2] = ["AADSTS65001", "AADSTS90094"];

/// Errors from the OAuth login and token refresh flows.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authorization denied by the provider: {0}")]
    Denied(String),
    #[error("the OAuth redirect did not include an authorization code")]
    MissingCode,
    #[error("the redirect did not confirm admin consent")]
    MissingConsent,
    #[error(
        "the provider returned no refresh token. For Google: set the OAuth app to \"In production\" and revoke Rail at https://myaccount.google.com/permissions, then try again"
    )]
    NoRefreshToken,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("refresh token rejected ({0}); to re-authenticate, run `rail account login <name>`")]
    NeedsAuth(String),
    #[error("network error talking to the token endpoint: {0}")]
    Network(String),
    #[error("invalid OAuth configuration: {0}")]
    Config(String),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("loopback listener: {0}")]
    Io(#[from] std::io::Error),
}

impl AuthError {
    /// Whether retrying later could succeed.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Network(_) | Self::Io(_) => ErrorClass::Transient,
            _ => ErrorClass::Permanent,
        }
    }

    /// True when the provider refused because the organisation has not approved Rail:
    /// a `Denied` whose description carries `AADSTS65001` or `AADSTS90094`.
    pub fn consent_required(&self) -> bool {
        matches!(self, Self::Denied(description)
            if CONSENT_REQUIRED_CODES.iter().any(|code| description.contains(code)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_covers_every_variant() {
        let transient = [
            AuthError::Network("x".into()),
            AuthError::Io(std::io::Error::other("x")),
        ];
        let permanent = [
            AuthError::Denied("x".into()),
            AuthError::MissingCode,
            AuthError::MissingConsent,
            AuthError::NoRefreshToken,
            AuthError::Exchange("x".into()),
            AuthError::NeedsAuth("x".into()),
            AuthError::Config("x".into()),
            AuthError::Secret(SecretError::Keyring("x".into())),
        ];
        assert!(transient.iter().all(|e| e.class() == ErrorClass::Transient));
        assert!(permanent.iter().all(|e| e.class() == ErrorClass::Permanent));
    }

    #[test]
    fn needs_auth_advice_is_actionable() {
        let text = AuthError::NeedsAuth("revoked".into()).to_string();
        assert!(
            text.contains("to re-authenticate, run `rail account login <name>`"),
            "{text}"
        );
    }

    #[test]
    fn consent_required_matches_the_two_aadsts_codes() {
        let needs = |d: &str| AuthError::Denied(d.to_string()).consent_required();
        assert!(needs(
            "AADSTS65001: The user or administrator has not consented"
        ));
        assert!(needs("AADSTS90094: The grant requires admin permission"));
        assert!(!needs("AADSTS50020: User account from a different tenant"));
        assert!(!needs("User said no"));
        assert!(!AuthError::Exchange("AADSTS65001".into()).consent_required());
    }
}
