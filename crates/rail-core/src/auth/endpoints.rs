use oauth2::AuthType;
use url::Url;

use crate::config::AccountKind;

/// Gmail API scope for reading and modifying mail.
pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// Microsoft Graph delegated scopes Rail asks for.
pub const GRAPH_SCOPES: [&str; 4] = [
    "https://graph.microsoft.com/Mail.ReadWrite",
    "https://graph.microsoft.com/Mail.Send",
    "https://graph.microsoft.com/User.Read",
    "offline_access",
];

/// The scope the v2 admin-consent endpoint takes: every permission the app registration lists.
pub const ADMIN_CONSENT_SCOPE: &str = "https://graph.microsoft.com/.default";

const MICROSOFT_AUTHORITY: &str = "https://login.microsoftonline.com/";

/// Authority aliases that name no single organisation, so no admin can consent through them.
const MULTI_TENANT_ALIASES: [&str; 3] = ["common", "organizations", "consumers"];

/// Provider-specific OAuth endpoints and parameters.
#[derive(Debug, Clone)]
pub struct OAuthEndpoints {
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub auth_type: AuthType,
    /// Extra query parameters on the authorization URL.
    pub extra_auth_params: Vec<(String, String)>,
}

/// Endpoints for an account kind; `tenant` only applies to Microsoft 365.
pub fn endpoints_for(kind: AccountKind, tenant: Option<&str>) -> OAuthEndpoints {
    match kind {
        AccountKind::Gmail => OAuthEndpoints {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            scopes: vec![GMAIL_SCOPE.to_string()],
            auth_type: AuthType::BasicAuth,
            // Google only issues a refresh token with offline access and an explicit consent prompt.
            extra_auth_params: vec![
                ("access_type".to_string(), "offline".to_string()),
                ("prompt".to_string(), "consent".to_string()),
            ],
        },
        AccountKind::M365 => {
            let tenant = tenant.unwrap_or("common");
            OAuthEndpoints {
                auth_url: format!(
                    "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"
                ),
                token_url: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
                scopes: GRAPH_SCOPES.iter().map(|s| (*s).to_string()).collect(),
                // Public client: credentials go in the body, never HTTP basic auth.
                auth_type: AuthType::RequestBody,
                extra_auth_params: Vec::new(),
            }
        }
        AccountKind::ImapSmtp => panic!("IMAP/SMTP accounts do not use OAuth endpoints"),
    }
}

/// Whether `tenant` names one organisation (a tenant id or domain), as admin consent needs.
pub(crate) fn is_concrete_tenant(tenant: &str) -> bool {
    !tenant.is_empty()
        && !MULTI_TENANT_ALIASES
            .iter()
            .any(|alias| tenant.eq_ignore_ascii_case(alias))
}

/// Microsoft's v2 admin-consent URL. The redirect comes back to `redirect_uri` with
/// `admin_consent=True&tenant=...&state=...` or with `error=...&error_description=...`.
/// The tenant goes in as one escaped path segment, so a hostile value cannot change the host
/// or the path. The URL carries no secret.
///
/// Precondition: `tenant` is concrete (see `is_concrete_tenant`); `begin_admin_consent`
/// enforces it before calling this.
pub fn admin_consent_url(client_id: &str, tenant: &str, redirect_uri: &str, state: &str) -> Url {
    debug_assert!(
        is_concrete_tenant(tenant),
        "admin consent needs a concrete tenant"
    );
    let mut url = Url::parse(MICROSOFT_AUTHORITY).expect("constant authority URL parses");
    url.path_segments_mut()
        .expect("an https URL can have path segments")
        .pop_if_empty()
        .push(tenant)
        .push("v2.0")
        .push("adminconsent");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("scope", ADMIN_CONSENT_SCOPE)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_requests_offline_access_and_consent() {
        let e = endpoints_for(AccountKind::Gmail, None);
        assert_eq!(e.token_url, "https://oauth2.googleapis.com/token");
        assert_eq!(e.scopes, vec![GMAIL_SCOPE.to_string()]);
        assert!(
            e.extra_auth_params
                .contains(&("access_type".to_string(), "offline".to_string()))
        );
        assert!(
            e.extra_auth_params
                .contains(&("prompt".to_string(), "consent".to_string()))
        );
    }

    #[test]
    fn m365_defaults_to_common_tenant_and_honours_override() {
        let e = endpoints_for(AccountKind::M365, None);
        assert_eq!(
            e.auth_url,
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        );
        assert!(e.scopes.iter().any(|s| s == "offline_access"));
        assert!(e.scopes.iter().any(|s| s.ends_with("/Mail.Send")));
        let e = endpoints_for(AccountKind::M365, Some("contoso.onmicrosoft.com"));
        assert_eq!(
            e.token_url,
            "https://login.microsoftonline.com/contoso.onmicrosoft.com/oauth2/v2.0/token"
        );
    }

    #[test]
    fn admin_consent_url_targets_the_tenant_and_default_scope() {
        let url = admin_consent_url(
            "cid",
            "contoso.onmicrosoft.com",
            "http://127.0.0.1:5000",
            "st",
        );
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
        assert_eq!(url.path(), "/contoso.onmicrosoft.com/v2.0/adminconsent");
        let query: std::collections::HashMap<String, String> =
            url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "cid");
        assert_eq!(query["scope"], "https://graph.microsoft.com/.default");
        assert_eq!(query["redirect_uri"], "http://127.0.0.1:5000");
        assert_eq!(query["state"], "st");
    }

    #[test]
    fn admin_consent_url_escapes_a_hostile_tenant() {
        let url = admin_consent_url("cid", "a/b?c", "http://127.0.0.1", "st");
        assert_eq!(url.path(), "/a%2Fb%3Fc/v2.0/adminconsent");
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
    }
}
