use super::*;
use crate::ops::{ErrorCode, OpError};

fn request(kind: AccountKind, inputs: GuideInputs) -> GuideRequest {
    GuideRequest { kind, inputs }
}

fn kinds(steps: &[GuideStep]) -> Vec<StepKind> {
    steps.iter().map(|s| s.kind).collect()
}

#[test]
fn the_google_steps_are_pinned() {
    let response = guide(&request(AccountKind::Gmail, GuideInputs::default())).unwrap();
    assert_eq!(response.version, GUIDE_VERSION);
    use StepKind::*;
    assert_eq!(
        kinds(&response.steps),
        [
            Run, Browser, Run, Run, Run, Browser, Browser, Ask, Rail, Produce
        ]
    );
    let commands: Vec<_> = response
        .steps
        .iter()
        .filter_map(|s| s.command.as_deref())
        .collect();
    assert_eq!(
        commands,
        [
            "gcloud auth login",
            "gcloud projects create <project-id>",
            "gcloud config set project <project-id>",
            "gcloud services enable gmail.googleapis.com",
        ]
    );
    let terms = &response.steps[1];
    assert!(
        terms.optional,
        "only a new Google account must accept the terms"
    );
    assert!(terms.explanation.contains("TOS"));
    let consent = &response.steps[5].explanation;
    for needed in [
        "In production",
        "7 days",
        "privacy policy URL",
        "Authorized domains",
        "logo",
    ] {
        assert!(consent.contains(needed), "consent step lacks {needed}");
    }
    let client = &response.steps[6].explanation;
    assert!(client.contains("Desktop app") && client.contains("Download JSON"));
    let add = &response.steps[8];
    assert_eq!(add.tool.as_deref(), Some("rail_account_add"));
    assert!(add.explanation.contains("store_secret_in_1password"));
    assert!(add.explanation.contains("browser profile"));
    assert!(response.steps[9].optional);
}

#[test]
fn every_google_console_page_names_the_account_to_use() {
    let steps = template(AccountKind::Gmail, &GuideInputs::default());
    let pages: Vec<_> = steps.iter().filter_map(|s| s.url.as_deref()).collect();
    assert_eq!(pages.len(), 3);
    for page in pages {
        assert!(page.contains("authuser=<address>"), "{page}");
    }
}

#[test]
fn the_microsoft_steps_are_pinned_and_never_hard_code_permission_ids() {
    let response = guide(&request(AccountKind::M365, GuideInputs::default())).unwrap();
    use StepKind::*;
    assert_eq!(
        kinds(&response.steps),
        [Run, Run, Run, Run, Run, Run, Run, Run, Rail, Produce]
    );
    let create = response.steps[3].command.as_deref().unwrap();
    for flag in [
        "--public-client-redirect-uris http://127.0.0.1",
        "--is-fallback-public-client true",
        "--sign-in-audience <audience>",
    ] {
        assert!(create.contains(flag), "{flag} missing from {create}");
    }
    let add = response.steps[5].command.as_deref().unwrap();
    assert!(add.contains("<mail-readwrite-id>=Scope"), "{add}");
    for scope in ["Mail.ReadWrite", "Mail.Send", "User.Read", "offline_access"] {
        assert!(
            response.steps[4]
                .command
                .as_deref()
                .unwrap()
                .contains(scope)
        );
    }
    assert!(
        response.steps[6].optional && response.steps[7].optional,
        "service principal and consent are optional for non-administrators"
    );
    assert_eq!(
        response.steps[6].command.as_deref(),
        Some("az ad sp create --id <app-id>")
    );
    let admin = GuideInputs {
        admin: true,
        ..GuideInputs::default()
    };
    let steps = guide(&request(AccountKind::M365, admin)).unwrap().steps;
    assert!(!steps[6].optional && !steps[7].optional);
    assert!(steps[7].explanation.contains("wait a minute"));
}

#[test]
fn inputs_fill_their_placeholders_and_nothing_else() {
    let inputs = GuideInputs {
        project_id: Some("rail-mail-123".into()),
        tenant: Some("contoso.onmicrosoft.com".into()),
        audience: Some(Audience::SingleTenant),
        admin: false,
    };
    let google = guide(&request(AccountKind::Gmail, inputs.clone())).unwrap();
    assert_eq!(
        google.steps[2].command.as_deref(),
        Some("gcloud projects create rail-mail-123")
    );
    assert!(
        google.steps[5]
            .url
            .as_deref()
            .unwrap()
            .ends_with("?project=rail-mail-123&authuser=<address>")
    );
    let microsoft = guide(&request(AccountKind::M365, inputs)).unwrap();
    assert!(
        microsoft.steps[3]
            .explanation
            .contains("audience is AzureADMyOrg for one organisation"),
        "explanations are prose and are not substituted"
    );
    assert_eq!(
        microsoft.steps[1].command.as_deref(),
        Some("az login --tenant contoso.onmicrosoft.com")
    );
    let create = microsoft.steps[3].command.as_deref().unwrap();
    assert!(
        create.contains("--sign-in-audience AzureADMyOrg"),
        "{create}"
    );
    assert!(
        microsoft.steps[5]
            .command
            .as_deref()
            .unwrap()
            .contains("<app-id>")
    );
}

/// docs/accounts.md documents the same steps: its setup sections carry the marker of this guide
/// version, every command of every `run` step verbatim (placeholders included), and every
/// `browser` page up to its query string.
#[test]
fn the_account_docs_setup_sections_agree_with_the_guides() {
    let docs = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/accounts.md"
    ))
    .expect("docs/accounts.md at the workspace root");
    let inputs = GuideInputs {
        admin: true,
        ..GuideInputs::default()
    };
    for (kind, marker) in [
        (AccountKind::Gmail, "google"),
        (AccountKind::M365, "microsoft"),
    ] {
        let tag = format!("<!-- guide:{marker} v{GUIDE_VERSION} -->");
        assert!(docs.contains(&tag), "docs/accounts.md lacks {tag}");
        for step in template(kind, &inputs) {
            if let Some(command) = &step.command {
                assert!(
                    docs.contains(command.as_str()),
                    "docs/accounts.md lacks `{command}`"
                );
            }
            if let Some(url) = &step.url {
                let page = url.split('?').next().unwrap_or(url);
                assert!(mentions_page(&docs, page), "docs/accounts.md lacks {page}");
            }
        }
    }
}

fn refused(inputs: GuideInputs) -> OpError {
    guide(&request(AccountKind::M365, inputs)).unwrap_err()
}

#[test]
fn inputs_that_could_run_other_commands_are_refused_without_being_echoed() {
    for bad in [
        "x; rm -rf ~",
        "abcde$(id)",
        "rail-mail-123 && ls",
        "Rail-Mail-123",
        "abc",
        "-rail-mail-1",
        "rail-mail-123-",
    ] {
        let inputs = GuideInputs {
            project_id: Some(bad.into()),
            ..GuideInputs::default()
        };
        let err = guide(&request(AccountKind::Gmail, inputs)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad}");
        assert!(err.message.contains("project_id"), "{}", err.message);
        assert!(!format!("{err:?}").contains(bad), "{bad} was echoed");
    }
    for bad in [
        "contoso.com; id",
        "$(id)",
        "a b",
        "-x.example.com",
        "contoso..com",
        "",
    ] {
        let err = refused(GuideInputs {
            tenant: Some(bad.into()),
            ..GuideInputs::default()
        });
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        assert!(err.message.contains("tenant"), "{}", err.message);
        assert!(bad.is_empty() || !format!("{err:?}").contains(bad));
    }
}

#[test]
fn valid_inputs_are_substituted_and_unset_placeholders_stay_literal() {
    for tenant in [
        "contoso.onmicrosoft.com",
        "contoso",
        "12345678-1234-1234-1234-123456789abc",
    ] {
        let inputs = GuideInputs {
            tenant: Some(tenant.into()),
            ..GuideInputs::default()
        };
        let steps = guide(&request(AccountKind::M365, inputs)).unwrap().steps;
        assert_eq!(
            steps[1].command.as_deref(),
            Some(format!("az login --tenant {tenant}").as_str())
        );
    }
    let steps = guide(&request(AccountKind::M365, GuideInputs::default()))
        .unwrap()
        .steps;
    assert_eq!(
        steps[1].command.as_deref(),
        Some("az login --tenant <tenant>")
    );
}

#[test]
fn no_step_prints_a_token_or_a_secret() {
    for kind in [AccountKind::Gmail, AccountKind::M365] {
        for step in template(kind, &GuideInputs::default()) {
            for text in [&step.command, &step.check].into_iter().flatten() {
                for banned in [
                    "print-access-token",
                    "get-access-token",
                    "client-secret",
                    "--password",
                ] {
                    assert!(!text.contains(banned), "{text} contains {banned}");
                }
            }
        }
    }
}

/// The page appears as a whole token, not as the prefix of a longer URL.
fn mentions_page(docs: &str, page: &str) -> bool {
    docs.match_indices(page).any(|(at, _)| {
        docs[at + page.len()..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || "`?)>".contains(c))
    })
}

#[test]
fn a_page_that_is_only_the_prefix_of_another_url_does_not_count() {
    assert!(!mentions_page("see `https://x/a/b?p=1`", "https://x/a"));
    assert!(mentions_page("see `https://x/a?p=1` and", "https://x/a"));
    assert!(mentions_page("(https://x/a)", "https://x/a"));
}

#[test]
fn project_ids_of_30_characters_pass_and_31_do_not() {
    let id = |len: usize| format!("a{}", "b".repeat(len - 1));
    let ok = GuideInputs {
        project_id: Some(id(30)),
        ..GuideInputs::default()
    };
    assert!(guide(&request(AccountKind::Gmail, ok)).is_ok());
    let long = GuideInputs {
        project_id: Some(id(31)),
        ..GuideInputs::default()
    };
    assert!(guide(&request(AccountKind::Gmail, long)).is_err());
}

#[test]
fn a_malformed_guid_like_tenant_is_refused() {
    for bad in ["1234-", "12345678-1234-1234-1234-", "1234-5678-"] {
        let err = refused(GuideInputs {
            tenant: Some(bad.into()),
            ..GuideInputs::default()
        });
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad}");
    }
}

#[test]
fn a_project_id_given_to_a_microsoft_guide_is_still_validated() {
    let err = refused(GuideInputs {
        project_id: Some("x; id".into()),
        ..GuideInputs::default()
    });
    assert!(err.message.contains("project_id"));
}
