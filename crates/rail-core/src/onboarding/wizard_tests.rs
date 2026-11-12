//! Tests for the wizard's navigation and actions.

use std::path::PathBuf;

use crate::config::AccountConfig;
use crate::config::AccountKind;
use crate::error::ErrorClass;
use crate::onboarding::events::{Failure, Remedy};
use crate::onboarding::view::StepState;
use crate::onboarding::wizard::Consent;
use crate::onboarding::{Action, Busy, Field, Input, Step, Wizard};
use crate::test_support::{
    contoso_preset, done_report, edit, gmail_details, imapsmtp_details, m365_details, own_app,
    walk, wizard,
};

fn concat(parts: Vec<Vec<Input>>) -> Vec<Input> {
    parts.concat()
}

type WalkCase = (&'static str, Vec<Input>, Step, Option<Field>);

fn to_login(details: Vec<Input>) -> Vec<Input> {
    concat(vec![own_app(0), details, vec![Input::Next]])
}

fn check_walks(cases: Vec<WalkCase>) {
    for (name, inputs, step, error) in cases {
        let mut w = wizard();
        walk(&mut w, inputs);
        assert_eq!(w.step(), step, "{name}");
        if let Some(field) = error {
            assert!(w.errors().contains_key(&field), "{name}: {:?}", w.errors());
        }
    }
}

#[test]
fn invalid_details_keep_the_wizard_on_details() {
    let cases: Vec<WalkCase> = vec![
        (
            "own app asks for the provider",
            vec![Input::Choose(1)],
            Step::Provider,
            None,
        ),
        (
            "a provider leads to instructions",
            vec![Input::Choose(1), Input::Choose(0)],
            Step::Instructions,
            None,
        ),
        (
            "instructions lead to details",
            own_app(0),
            Step::Details,
            None,
        ),
        (
            "gmail without a secret stays",
            to_login(vec![
                edit(Field::Address, "me@gmail.com"),
                edit(Field::ClientId, "cid"),
            ]),
            Step::Details,
            Some(Field::ClientSecret),
        ),
        (
            "a duplicate address stays",
            to_login(gmail_details("WORK@example.com")),
            Step::Details,
            Some(Field::Address),
        ),
        (
            "an address without @ stays",
            to_login(gmail_details("me.gmail.com")),
            Step::Details,
            Some(Field::Address),
        ),
        (
            "a bad name stays",
            to_login(concat(vec![
                gmail_details("me@gmail.com"),
                vec![edit(Field::Name, "has space")],
            ])),
            Step::Details,
            Some(Field::Name),
        ),
        (
            "a duplicate name stays",
            to_login(concat(vec![
                gmail_details("me@gmail.com"),
                vec![edit(Field::Name, "WORK")],
            ])),
            Step::Details,
            Some(Field::Name),
        ),
    ];
    check_walks(cases);
}

#[test]
fn walks_land_on_the_expected_step() {
    let cases: Vec<WalkCase> = vec![
        (
            "valid gmail logs in",
            to_login(gmail_details("me@gmail.com")),
            Step::Login,
            None,
        ),
        (
            "microsoft without the admin box logs in",
            concat(vec![
                own_app(1),
                m365_details("me@contoso.com", ""),
                vec![Input::Next],
            ]),
            Step::Login,
            None,
        ),
        (
            "an administrator on common is asked for a tenant",
            concat(vec![
                own_app(1),
                m365_details("me@contoso.com", "common"),
                vec![Input::ToggleAdmin, Input::Next],
            ]),
            Step::ConsentTenant,
            None,
        ),
        (
            "an administrator with a tenant goes to consent",
            concat(vec![
                own_app(1),
                m365_details("me@contoso.com", "contoso.onmicrosoft.com"),
                vec![Input::ToggleAdmin, Input::Next],
            ]),
            Step::AdminConsent,
            None,
        ),
        (
            "esc goes back a step",
            concat(vec![own_app(0), vec![Input::Back]]),
            Step::Instructions,
            None,
        ),
        (
            "esc twice goes back two",
            concat(vec![own_app(0), vec![Input::Back, Input::Back]]),
            Step::Provider,
            None,
        ),
        ("esc at start stays", vec![Input::Back], Step::Start, None),
    ];
    check_walks(cases);
}

#[test]
fn valid_details_ask_for_a_login_with_the_draft_account() {
    let mut w = wizard();
    let inputs = concat(vec![
        own_app(0),
        gmail_details("Me@Gmail.com"),
        vec![edit(Field::Color, "blue"), Input::Next],
    ]);
    let actions = walk(&mut w, inputs);
    let expected = AccountConfig {
        name: "me".into(),
        kind: AccountKind::Gmail,
        address: "me@gmail.com".into(),
        color: Some("blue".into()),
        label: None,
        client_id: "cid".into(),
        client_secret: Some("sec".into()),
        tenant: None,
        signature: None,
        from_name: None,
        ai_send: None,
        history_days: None,
        body_cache_days: None,
        ..Default::default()
    };
    assert_eq!(actions, vec![Action::RunLogin(expected)]);
    assert_eq!(w.busy(), Some(Busy::Login));
    assert!(walk(&mut w, [Input::Next]).is_empty(), "busy: Next waits");
}

#[test]
fn an_administrator_on_common_is_asked_for_a_tenant_before_consent() {
    let mut w = wizard();
    let inputs = concat(vec![
        own_app(1),
        m365_details("me@contoso.com", "common"),
        vec![Input::ToggleAdmin, Input::Next],
    ]);
    assert!(walk(&mut w, inputs).is_empty());
    assert_eq!(w.step(), Step::ConsentTenant);
    assert_eq!(
        w.consent_tenant(),
        "contoso.com",
        "suggested from the address"
    );
    let shared = walk(
        &mut w,
        [edit(Field::ConsentTenant, "organizations"), Input::Next],
    );
    assert!(shared.is_empty());
    assert_eq!(w.step(), Step::ConsentTenant);
    assert!(w.errors().contains_key(&Field::ConsentTenant));
    assert!(
        walk(
            &mut w,
            [edit(Field::ConsentTenant, "contoso.com"), Input::Next]
        )
        .is_empty()
    );
    assert_eq!(w.step(), Step::AdminConsent);
    assert_eq!(
        walk(&mut w, [Input::Next]),
        vec![Action::RunAdminConsent {
            client_id: "cid".into(),
            tenant: "contoso.com".into()
        }]
    );
    assert_eq!(w.busy(), Some(Busy::Consent));
}

#[test]
fn preset_path_skips_provider_and_instructions() {
    let mut w = wizard();
    assert_eq!(walk(&mut w, [Input::Choose(0)]), vec![Action::LoadPresets]);
    assert_eq!(w.step(), Step::Preset);
    assert_eq!(w.busy(), Some(Busy::LoadingPresets));
    assert!(
        walk(&mut w, [Input::Choose(0)]).is_empty(),
        "choices wait for the presets"
    );
    w.presets = vec![contoso_preset()];
    w.busy = None;
    walk(&mut w, [Input::Choose(0)]);
    assert_eq!(w.step(), Step::Details);
    assert_eq!(w.draft().client_id, "contoso-app");
    assert_eq!(w.draft().tenant, "contoso.onmicrosoft.com");
    assert!(!w.draft().fields().contains(&Field::ClientId));
    let steps = w.view_model().steps;
    assert_eq!(steps[1], ("Provider", StepState::Skipped));
    assert_eq!(steps[2], ("Instructions", StepState::Skipped));
    assert_eq!(steps[3], ("Details", StepState::Current));
    walk(&mut w, [Input::Back]);
    assert_eq!(w.step(), Step::Preset, "back returns to the preset list");
}

#[test]
fn cancel_yields_only_quit_at_every_step() {
    let full = concat(vec![
        own_app(1),
        m365_details("me@contoso.com", "contoso.onmicrosoft.com"),
        vec![Input::ToggleAdmin, Input::Next, Input::Next],
    ]);
    let mut seen = Vec::new();
    for taken in 0..=full.len() {
        let mut w = wizard();
        seen.extend(walk(&mut w, full[..taken].to_vec()));
        assert_eq!(
            w.handle(Input::Cancel),
            vec![Action::Quit],
            "after {taken} inputs, at {:?}",
            w.step()
        );
    }
    assert!(
        !seen.iter().any(|a| matches!(a, Action::Persist { .. })),
        "{seen:?}"
    );
    let mut w = wizard();
    walk(
        &mut w,
        concat(vec![
            own_app(0),
            gmail_details("me@gmail.com"),
            vec![Input::Next],
        ]),
    );
    assert_eq!(w.busy(), Some(Busy::Login));
    assert_eq!(
        w.handle(Input::Cancel),
        vec![Action::Quit],
        "also mid-login"
    );
}

#[test]
fn going_back_while_logging_in_cancels_the_login() {
    let mut w = wizard();
    walk(
        &mut w,
        concat(vec![
            own_app(0),
            gmail_details("me@gmail.com"),
            vec![Input::Next],
        ]),
    );
    assert_eq!(w.handle(Input::Back), vec![Action::CancelPending]);
    assert_eq!(w.step(), Step::Details);
    assert_eq!(w.busy(), None);
    assert_eq!(w.draft().address, "me@gmail.com", "the draft survives");
}

#[test]
fn preset_source_goes_to_fetch_or_read() {
    let mut w = wizard();
    walk(&mut w, [Input::Choose(0)]);
    w.busy = None;
    assert_eq!(
        walk(
            &mut w,
            [
                edit(Field::PresetSource, " http://intranet/rail.toml "),
                Input::Next
            ]
        ),
        vec![Action::FetchPreset("http://intranet/rail.toml".into())]
    );
    assert_eq!(w.busy(), Some(Busy::FetchingPreset));
    w.busy = None;
    assert_eq!(
        walk(
            &mut w,
            [edit(Field::PresetSource, "/srv/acme.toml"), Input::Next]
        ),
        vec![Action::ReadPreset(PathBuf::from("/srv/acme.toml"))]
    );
    w.busy = None;
    assert!(walk(&mut w, [edit(Field::PresetSource, "  "), Input::Next]).is_empty());
}

#[test]
fn open_portal_opens_the_provider_console_or_the_preset_help() {
    let mut w = wizard();
    walk(&mut w, [Input::Choose(1), Input::Choose(1)]);
    let actions = walk(&mut w, [Input::OpenPortal]);
    assert!(
        matches!(&actions[..], [Action::OpenUrl(u)] if u.host_str() == Some("portal.azure.com")),
        "{actions:?}"
    );
    let mut w = wizard();
    w.use_preset(&contoso_preset());
    let actions = walk(&mut w, [Input::OpenPortal]);
    assert!(
        matches!(&actions[..], [Action::OpenUrl(u)] if u.as_str() == "https://intranet.contoso.example/rail"),
        "{actions:?}"
    );
}

#[test]
fn a_provider_hint_skips_the_provider_step() {
    let mut w = Wizard::new(None, Some(AccountKind::M365));
    walk(&mut w, [Input::Choose(1)]);
    assert_eq!(w.step(), Step::Instructions);
    assert_eq!(w.draft().kind, Some(AccountKind::M365));
    walk(&mut w, [Input::Next, Input::ToggleAdmin]);
    assert!(w.draft().admin);
    let mut gmail = wizard();
    walk(&mut gmail, own_app(0));
    walk(&mut gmail, [Input::ToggleAdmin]);
    assert!(
        !gmail.draft().admin,
        "the administrator box is Microsoft only"
    );
}

fn tokens() -> crate::auth::TokenSet {
    crate::auth::TokenSet {
        access_token: "a".into(),
        refresh_token: "r".into(),
        expires_at: 0,
    }
}

/// Pretend the in-flight action failed, as `apply` would.
fn fail_pending(w: &mut Wizard) {
    w.busy = None;
    w.failure = Some((Failure::new("boom", ErrorClass::Permanent), Remedy::Retry));
}

#[test]
fn retry_after_a_failed_save_reuses_the_tokens_for_an_unchanged_draft() {
    let mut w = wizard();
    walk(
        &mut w,
        concat(vec![
            own_app(0),
            gmail_details("me@gmail.com"),
            vec![Input::Next],
        ]),
    );
    w.busy = None;
    assert!(matches!(
        &w.start_persist(tokens())[..],
        [Action::Persist { .. }]
    ));
    fail_pending(&mut w);
    let actions = w.handle(Input::Retry);
    assert!(
        matches!(&actions[..], [Action::Persist { .. }]),
        "{actions:?}"
    );
}

#[test]
fn retry_after_a_new_login_never_reuses_old_tokens() {
    let mut w = wizard();
    walk(
        &mut w,
        concat(vec![
            own_app(0),
            gmail_details("me@gmail.com"),
            vec![Input::Next],
        ]),
    );
    w.busy = None;
    w.start_persist(tokens());
    fail_pending(&mut w);
    walk(&mut w, [Input::Back]);
    assert_eq!(w.step(), Step::Details);
    let login = walk(
        &mut w,
        [edit(Field::Address, "other@gmail.com"), Input::Next],
    );
    assert!(matches!(&login[..], [Action::RunLogin(_)]), "{login:?}");
    fail_pending(&mut w);
    let actions = w.handle(Input::Retry);
    assert!(matches!(&actions[..], [Action::RunLogin(_)]), "{actions:?}");
}

fn consented_to(client_id: &str, tenant: &str) -> Option<Consent> {
    Some(Consent {
        client_id: client_id.into(),
        tenant: tenant.into(),
    })
}

#[test]
fn changing_tenant_or_client_id_requires_consent_again() {
    let mut w = wizard();
    let admin_details = concat(vec![
        own_app(1),
        m365_details("me@contoso.com", "contoso.onmicrosoft.com"),
        vec![Input::ToggleAdmin, Input::Next],
    ]);
    walk(&mut w, admin_details);
    assert_eq!(w.step(), Step::AdminConsent);
    w.consented = consented_to("cid", "contoso.onmicrosoft.com");
    assert!(matches!(&w.handle(Input::Next)[..], [Action::RunLogin(_)]));
    back_to_details(&mut w);
    walk(&mut w, [Input::Next]);
    assert_eq!(
        w.step(),
        Step::Login,
        "unchanged identity keeps its consent"
    );
    back_to_details(&mut w);
    walk(
        &mut w,
        [edit(Field::Tenant, "fabrikam.onmicrosoft.com"), Input::Next],
    );
    assert_eq!(w.step(), Step::AdminConsent, "new tenant");
    w.consented = consented_to("cid", "fabrikam.onmicrosoft.com");
    back_to_details(&mut w);
    walk(&mut w, [edit(Field::ClientId, "other-app"), Input::Next]);
    assert_eq!(w.step(), Step::AdminConsent, "new client id");
    assert!(matches!(
        &w.handle(Input::Next)[..],
        [Action::RunAdminConsent { .. }]
    ));
}

fn back_to_details(w: &mut Wizard) {
    while w.step() != Step::Details {
        w.handle(Input::Back);
    }
}

#[test]
fn edits_are_ignored_outside_editable_steps_and_while_busy() {
    let mut w = wizard();
    walk(&mut w, own_app(1));
    walk(&mut w, [Input::Back]);
    assert_eq!(w.step(), Step::Instructions);
    assert!(walk(&mut w, [edit(Field::ClientId, "x"), Input::ToggleAdmin]).is_empty());
    assert!(w.draft().client_id.is_empty() && !w.draft().admin);
    walk(&mut w, [Input::Next]);
    walk(&mut w, m365_details("me@contoso.com", ""));
    walk(&mut w, [Input::Next]);
    assert_eq!(w.busy(), Some(Busy::Login));
    let actions = walk(
        &mut w,
        [edit(Field::Address, "evil@x.com"), Input::ToggleAdmin],
    );
    assert!(actions.is_empty());
    assert_eq!(w.draft().address, "me@contoso.com");
    assert!(!w.draft().admin);
}

#[test]
fn imapsmtp_walk_to_login_and_validate() {
    let mut w = wizard();
    walk(&mut w, own_app(2));
    assert_eq!(w.step(), Step::Details);
    assert_eq!(w.draft().kind, Some(AccountKind::ImapSmtp));

    walk(&mut w, imapsmtp_details("alice@example.com", "topsecret"));
    let actions = w.handle(Input::Next);
    assert_eq!(w.step(), Step::Login);
    assert_eq!(w.busy(), Some(Busy::Login));
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::ValidateImapSmtp {
            account,
            password,
            smtp_password,
        } => {
            assert_eq!(account.address, "alice@example.com");
            assert_eq!(account.imap_host.as_deref(), Some("mail.example.com"));
            assert_eq!(account.imap_port, Some(993));
            assert_eq!(password.expose(), "topsecret");
            assert_eq!(smtp_password, &None);
        }
        other => panic!("expected ValidateImapSmtp, got {other:?}"),
    }
}

#[test]
fn imapsmtp_validation_success_persists_account() {
    let mut w = wizard();
    walk(&mut w, own_app(2));
    walk(&mut w, imapsmtp_details("alice@example.com", "topsecret"));
    w.handle(Input::Next);

    let persist = w.apply(crate::onboarding::Event::ImapSmtpValidated(Ok(())));
    assert_eq!(w.busy(), Some(Busy::Persisting));
    assert_eq!(persist.len(), 1);
    match &persist[0] {
        Action::PersistImapSmtp {
            account,
            password,
            smtp_password,
        } => {
            assert_eq!(account.address, "alice@example.com");
            assert_eq!(password.expose(), "topsecret");
            assert_eq!(smtp_password, &None);
        }
        other => panic!("expected PersistImapSmtp, got {other:?}"),
    }

    let report = done_report();
    w.apply(crate::onboarding::Event::Persisted(Ok(report)));
    assert_eq!(w.busy(), None);
    assert!(matches!(w.step(), Step::Keys | Step::Done));
    assert_eq!(w.added(), &["alice"]);
    assert_eq!(w.existing().accounts.len(), 2);
    assert!(w.existing().accounts.iter().any(|a| a.name == "alice"));
}

#[test]
fn imapsmtp_validation_failure_and_retry() {
    let mut w = wizard();
    walk(&mut w, own_app(2));
    walk(&mut w, imapsmtp_details("alice@example.com", "topsecret"));
    w.handle(Input::Next);

    let failure = Failure::new("invalid credentials", ErrorClass::Permanent);
    let after_fail = w.apply(crate::onboarding::Event::ImapSmtpValidated(Err(failure)));
    assert!(after_fail.is_empty());
    assert_eq!(w.busy(), None);
    assert_eq!(w.failure().unwrap().message, "invalid credentials");
    assert_eq!(w.remedy(), Some(&Remedy::Retry));

    // Retry re-emits ValidateImapSmtp
    let retried = w.handle(Input::Retry);
    assert_eq!(w.busy(), Some(Busy::Login));
    assert!(matches!(&retried[..], [Action::ValidateImapSmtp { .. }]));

    // Back goes back to Details with inputs preserved
    walk(&mut w, [Input::Back]);
    assert_eq!(w.step(), Step::Details);
    assert_eq!(w.draft().address, "alice@example.com");
    assert_eq!(w.draft().imap_host, "mail.example.com");
}

#[test]
fn imapsmtp_provider_hint_skips_provider_step() {
    let mut w = Wizard::new(None, Some(AccountKind::ImapSmtp));
    let actions = walk(&mut w, [Input::Choose(1)]);
    assert!(actions.is_empty());
    assert_eq!(w.step(), Step::Instructions);
    assert_eq!(w.draft().kind, Some(AccountKind::ImapSmtp));
}
