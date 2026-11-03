//! Google: `gcloud` creates the project and enables the Gmail API (flags checked against
//! Google Cloud SDK 586.0.0), but it cannot create the desktop OAuth client Gmail needs (its
//! only OAuth-client commands are for Identity-Aware Proxy), so those are browser steps.

use super::step;
use crate::ops::{GuideStep, StepKind};

pub(super) fn steps() -> Vec<GuideStep> {
    vec![
        login(),
        accept_terms(),
        create_project(),
        use_project(),
        enable_api(),
        consent_screen(),
        desktop_client(),
        find_file(),
        add_account(),
        preset(),
    ]
}

fn login() -> GuideStep {
    GuideStep {
        command: Some("gcloud auth login".into()),
        check: Some(
            "gcloud auth list --filter=status:ACTIVE --format=\"value(account)\" | grep -q ."
                .into(),
        ),
        ..step(
            StepKind::Run,
            "Sign in to Google Cloud",
            "Signs gcloud in to the user's Google account. A browser page opens for them. The check passes for any signed-in account, so say which account is active and confirm it is the one that should own the project, not a work account. To leave another account's gcloud setup alone, use a separate configuration: `gcloud config configurations create rail --no-activate`, then run this and every later gcloud command with `CLOUDSDK_ACTIVE_CONFIG_NAME=rail`.",
        )
    }
}

fn accept_terms() -> GuideStep {
    GuideStep {
        url: Some("https://console.cloud.google.com/?authuser=<address>".into()),
        optional: true,
        ..step(
            StepKind::Browser,
            "Accept the Google Cloud terms",
            "Only for a Google account that has never used Google Cloud: until it accepts the terms of service in the console, creating a project fails with a `TOS` precondition error. The user opens the console as that account and accepts; no billing account is needed.",
        )
    }
}

fn create_project() -> GuideStep {
    GuideStep {
        command: Some("gcloud projects create <project-id>".into()),
        check: Some("gcloud projects describe <project-id>".into()),
        ..step(
            StepKind::Run,
            "Create a project",
            "A Cloud project holds the OAuth client. The id must be globally unique, 6 to 30 lowercase letters, digits or hyphens.",
        )
    }
}

fn use_project() -> GuideStep {
    GuideStep {
        command: Some("gcloud config set project <project-id>".into()),
        ..step(
            StepKind::Run,
            "Use the project",
            "Makes the new project the default for the next commands.",
        )
    }
}

fn enable_api() -> GuideStep {
    GuideStep {
        command: Some("gcloud services enable gmail.googleapis.com".into()),
        check: Some(
            "gcloud services list --enabled --format=\"value(config.name)\" | grep -qx gmail.googleapis.com"
                .into(),
        ),
        ..step(
            StepKind::Run,
            "Enable the Gmail API",
            "Rail reads and sends mail through the Gmail API.",
        )
    }
}

fn consent_screen() -> GuideStep {
    GuideStep {
        url: Some(
            "https://console.cloud.google.com/auth/overview?project=<project-id>&authuser=<address>"
                .into(),
        ),
        ..step(
            StepKind::Browser,
            "Set up the consent screen",
            "The user takes over in the browser, signed in as the account that owns the project (`authuser` picks it when the browser holds several; a \"You need additional access\" page means the wrong one): Get started, the app name and their address, audience External, their address as contact. Before Google lets them publish, Branding needs a homepage URL and a privacy policy URL, with each URL's domain under Authorized domains. Leave the logo out: an app with a logo needs Google's verification before it can go to production. Then Audience, Publish app, so the status is \"In production\". An app left in Testing expires refresh tokens after 7 days, and Rail would keep asking them to log in. Google shows an \"unverified app\" warning for their own app; that is expected.",
        )
    }
}

fn desktop_client() -> GuideStep {
    GuideStep {
        url: Some(
            "https://console.cloud.google.com/auth/clients/create?project=<project-id>&authuser=<address>"
                .into(),
        ),
        ..step(
            StepKind::Browser,
            "Create a desktop client",
            "The user takes over in the browser: application type Desktop app, then Create. Google offers the JSON file only in the dialog that follows, and the secret cannot be downloaded later, so they click Download JSON before closing it. They should not paste the client secret anywhere.",
        )
    }
}

fn find_file() -> GuideStep {
    GuideStep {
        ask: Some("the path of the downloaded client_secret_*.json file".into()),
        ..step(
            StepKind::Ask,
            "Find the downloaded file",
            "Usually in the Downloads folder, named client_secret_<id>.json.",
        )
    }
}

fn add_account() -> GuideStep {
    GuideStep {
        tool: Some("rail_account_add".into()),
        content: Some(
            "{\"kind\": \"gmail\", \"address\": \"<address>\", \"client_secret_file\": \"<path>\"}"
                .into(),
        ),
        ..step(
            StepKind::Rail,
            "Add the account",
            "Call rail_account_add with the user's address and the file path. If config.toml has a 1Password vault (rail_status), ask whether to keep the client secret there and pass store_secret_in_1password; otherwise it is written to config.toml. Rail opens the login in the default browser, which may be signed in to another account or browser profile: show the user the URL it returns so they can open it where they are signed in as that address, then call rail_wait with its id while they log in. Afterwards the downloaded JSON file is no longer needed.",
        )
    }
}

fn preset() -> GuideStep {
    GuideStep {
        content: Some(
            "name = \"<organisation> Gmail\"\nkind = \"gmail\"\nclient_id = \"<client-id>\"\nclient_secret = \"<client-secret>\"\n"
                .into(),
        ),
        optional: true,
        ..step(
            StepKind::Produce,
            "Publish a preset (administrators)",
            "If an administrator wants colleagues to skip these steps, this is the preset file to publish over https or in /etc/rail/presets. The administrator fills in the client id and secret from the JSON file themselves; Rail never writes presets.",
        )
    }
}
