//! Microsoft: the Azure CLI can do the whole registration. The flags follow the Azure CLI
//! reference for `az login`, `az account show`, `az ad app create`, `az ad sp show`,
//! `az ad app permission add` and `az ad app permission admin-consent`; `az` was not installed
//! where this was written, so check them with `az <command> --help` before a release.
//! No step prints a token or a client secret.

use super::step;
use crate::ops::{GuideStep, StepKind};

/// Microsoft Graph's application id, the same in every tenant.
const GRAPH_APP_ID: &str = "00000003-0000-0000-c000-000000000000";

pub(super) fn steps(admin: bool) -> Vec<GuideStep> {
    vec![
        check_cli(),
        login(),
        read_tenant(),
        register_app(),
        look_up_permissions(),
        add_permissions(),
        create_service_principal(admin),
        admin_consent(admin),
        add_account(),
        preset(),
    ]
}

fn check_cli() -> GuideStep {
    GuideStep {
        command: Some("az version".into()),
        ..step(
            StepKind::Run,
            "Check for the Azure CLI",
            "If az is missing, the user can install it (https://learn.microsoft.com/cli/azure/install-azure-cli) or follow the portal steps in Rail's docs/accounts.md instead.",
        )
    }
}

fn login() -> GuideStep {
    GuideStep {
        command: Some("az login --tenant <tenant>".into()),
        check: Some("az account show --query tenantId --output tsv".into()),
        ..step(
            StepKind::Run,
            "Sign in to the organisation",
            "Signs az in to the tenant (its id or a domain it owns, such as contoso.onmicrosoft.com). A browser page opens for the user.",
        )
    }
}

fn read_tenant() -> GuideStep {
    GuideStep {
        command: Some("az account show --query tenantId --output tsv".into()),
        ..step(
            StepKind::Run,
            "Read the tenant id",
            "Keep this id: single-tenant apps must log in through it.",
        )
    }
}

fn register_app() -> GuideStep {
    GuideStep {
        command: Some(
            "az ad app create --display-name Rail --sign-in-audience <audience> --public-client-redirect-uris http://127.0.0.1 --is-fallback-public-client true --query appId --output tsv".into(),
        ),
        ..step(
            StepKind::Run,
            "Register the app",
            "Creates a public-client registration with the loopback redirect Rail listens on. The sign-in audience is AzureADMyOrg for one organisation or AzureADMultipleOrgs for several. The output is the Application (client) ID: keep it as <app-id>.",
        )
    }
}

fn look_up_permissions() -> GuideStep {
    GuideStep {
        command: Some(format!(
            "az ad sp show --id {GRAPH_APP_ID} --query \"oauth2PermissionScopes[?value=='Mail.ReadWrite' || value=='Mail.Send' || value=='User.Read' || value=='offline_access'].{{value:value, id:id}}\" --output table"
        )),
        ..step(
            StepKind::Run,
            "Look up the Graph permission ids",
            "Lists the ids of the four delegated permissions Rail needs. They are looked up, never hard-coded.",
        )
    }
}

fn add_permissions() -> GuideStep {
    GuideStep {
        command: Some(format!(
            "az ad app permission add --id <app-id> --api {GRAPH_APP_ID} --api-permissions <mail-readwrite-id>=Scope <mail-send-id>=Scope <user-read-id>=Scope <offline-access-id>=Scope"
        )),
        ..step(
            StepKind::Run,
            "Add the permissions",
            "Adds Mail.ReadWrite, Mail.Send, User.Read and offline_access, using the ids from the previous step.",
        )
    }
}

fn create_service_principal(admin: bool) -> GuideStep {
    GuideStep {
        command: Some("az ad sp create --id <app-id>".into()),
        optional: !admin,
        ..step(
            StepKind::Run,
            "Create the app's identity (administrators)",
            "Creates the app's service principal, its identity in your tenant, which admin consent needs. An \"already exists\" error means it is done.",
        )
    }
}

fn admin_consent(admin: bool) -> GuideStep {
    GuideStep {
        command: Some("az ad app permission admin-consent --id <app-id>".into()),
        tool: Some("rail_admin_consent".into()),
        optional: !admin,
        ..step(
            StepKind::Run,
            "Grant admin consent (administrators)",
            "An administrator approves Rail once for the whole organisation, with this command or with rail_admin_consent. Someone who is not an administrator skips this; when their login fails with AADSTS65001 they send the consent URL Rail shows to an administrator. If the command reports that the permissions or the service principal are not found, wait a minute and run it again: directory changes take a moment to propagate.",
        )
    }
}

fn add_account() -> GuideStep {
    GuideStep {
        tool: Some("rail_account_add".into()),
        content: Some(
            "{\"kind\": \"m365\", \"address\": \"<address>\", \"client_id\": \"<app-id>\", \"tenant\": \"<tenant>\"}".into(),
        ),
        ..step(
            StepKind::Rail,
            "Add the account",
            "Call rail_account_add with the user's address, the app id and the tenant (a single-tenant app needs the tenant id or domain). No client secret is needed. Show the user the URL it returns, then call rail_wait. An administrator who skipped the consent step is offered Microsoft's \"Consent on behalf of your organization\" box during this login.",
        )
    }
}

fn preset() -> GuideStep {
    GuideStep {
        content: Some(
            "name = \"<organisation> Microsoft 365\"\nkind = \"m365\"\nclient_id = \"<app-id>\"\ntenant = \"<tenant>\"\n".into(),
        ),
        optional: true,
        ..step(
            StepKind::Produce,
            "Publish a preset (administrators)",
            "The preset file an administrator can publish over https or in /etc/rail/presets so colleagues skip these steps. Rail never writes presets.",
        )
    }
}
