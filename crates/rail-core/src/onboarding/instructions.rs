//! The portal steps the Instructions step shows. docs/accounts.md carries the same text.

use url::Url;

use crate::auth::{GMAIL_SCOPE, GRAPH_SCOPES};
use crate::config::AccountKind;

/// The redirect URI both portals register. Rail listens on a random loopback port, and both
/// providers accept any port for a loopback redirect.
pub const REDIRECT_URI: &str = "http://127.0.0.1";

const GOOGLE_CONSOLE: &str = "https://console.cloud.google.com/auth/overview";
const AZURE_PORTAL: &str =
    "https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade";

pub fn portal_url(kind: AccountKind) -> Url {
    let url = match kind {
        AccountKind::Gmail => GOOGLE_CONSOLE,
        AccountKind::M365 => AZURE_PORTAL,
    };
    Url::parse(url).expect("constant portal URL parses")
}

pub fn instructions(kind: AccountKind) -> String {
    match kind {
        AccountKind::Gmail => gmail(),
        AccountKind::M365 => microsoft(),
    }
}

fn gmail() -> String {
    [
        "Use the browser as the Google account that will own the client; with several signed in, check the account picker on each page.".to_string(),
        String::new(),
        "1. In Google Cloud Console create a project and enable the Gmail API. A Google account new to Google Cloud accepts its terms first.".to_string(),
        "2. Google Auth Platform: Get started, audience External. Under Branding add a homepage URL and a privacy policy URL (their domain under Authorized domains) and no logo, which would need Google's verification. Then Audience, Publish app: the status must be \"In production\". An app left in Testing expires refresh tokens after 7 days.".to_string(),
        "3. Clients: create an OAuth client of type Desktop app. Google shows its secret only in the dialog that follows.".to_string(),
        "4. Copy the client ID and the client secret into the next step.".to_string(),
        String::new(),
        "Scope Rail asks for:".to_string(),
        format!("    {GMAIL_SCOPE}"),
        String::new(),
        format!("Redirect URI: Desktop app clients accept {REDIRECT_URI} on any port; there is nothing to add."),
    ]
    .join("\n")
}

fn microsoft() -> String {
    let mut lines = vec![
        "1. In Azure Portal open App registrations and choose New registration.".to_string(),
        "2. Supported account types: pick what matches your mailbox. For one organisation choose \"Accounts in this organizational directory only\" and note the Directory (tenant) ID.".to_string(),
        "3. Redirect URI: platform \"Mobile and desktop applications\", value:".to_string(),
        format!("    {REDIRECT_URI}"),
        "4. Authentication: enable \"Allow public client flows\".".to_string(),
        "5. API permissions: Microsoft Graph, delegated:".to_string(),
    ];
    lines.extend(GRAPH_SCOPES.iter().map(|scope| format!("    {scope}")));
    lines.push(
        "6. Copy the Application (client) ID into the next step. No client secret is needed."
            .to_string(),
    );
    lines.push("7. If you administer the tenant, tick the administrator box in the next step to grant consent for everyone.".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_steps_name_the_desktop_client_and_the_scope() {
        let text = instructions(AccountKind::Gmail);
        assert!(text.contains("Desktop app"), "{text}");
        assert!(text.contains("In production"), "{text}");
        assert!(
            text.contains("privacy policy URL") && text.contains("no logo"),
            "{text}"
        );
        assert!(text.contains(GMAIL_SCOPE), "{text}");
        assert_eq!(
            portal_url(AccountKind::Gmail).host_str(),
            Some("console.cloud.google.com")
        );
    }

    #[test]
    fn microsoft_steps_name_the_redirect_and_every_scope() {
        let text = instructions(AccountKind::M365);
        assert!(text.contains(REDIRECT_URI), "{text}");
        assert!(text.contains("Allow public client flows"), "{text}");
        for scope in GRAPH_SCOPES {
            assert!(text.contains(scope), "{scope} missing from {text}");
        }
        assert_eq!(
            portal_url(AccountKind::M365).host_str(),
            Some("portal.azure.com")
        );
    }
}
