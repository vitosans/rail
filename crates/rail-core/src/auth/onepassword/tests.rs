use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

use super::*;
use crate::error::ErrorClass;
use crate::test_support::{FakeOp, files_containing};

const ACCESS: &str = "ya29.test-access-token";
const REFRESH: &str = "1//test-refresh-token";

fn tokens() -> TokenSet {
    TokenSet {
        access_token: ACCESS.to_string(),
        refresh_token: REFRESH.to_string(),
        expires_at: 1_700_000_000,
    }
}

fn store(fake: &FakeOp) -> OnePasswordSecretStore {
    OnePasswordSecretStore::new(fake.cli(), "Rail".to_string(), ItemIdCache::default())
}

#[test]
fn tokens_round_trip_through_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    assert_eq!(store.get_tokens("Me@Example.com").unwrap(), None);
    store.set_tokens("Me@Example.com", &tokens()).unwrap();
    assert_eq!(store.get_tokens("me@example.com").unwrap(), Some(tokens()));
    let newer = TokenSet {
        expires_at: 1_800_000_000,
        ..tokens()
    };
    store.set_tokens("me@example.com", &newer).unwrap();
    assert_eq!(
        fake.items().len(),
        1,
        "the second write edits the same item"
    );
    assert_eq!(store.get_tokens("me@example.com").unwrap(), Some(newer));
    store.delete_tokens("me@example.com").unwrap();
    assert_eq!(store.get_tokens("me@example.com").unwrap(), None);
    store.delete_tokens("me@example.com").unwrap();
    assert!(fake.items().is_empty());
}

#[test]
fn passwords_round_trip_through_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    let pass = Secret::new("my-imap-secret");
    assert_eq!(store.get_password("Me@Example.com").unwrap(), None);
    store.set_password("Me@Example.com", &pass).unwrap();
    assert_eq!(
        store
            .get_password("me@example.com")
            .unwrap()
            .as_ref()
            .map(|s| s.expose()),
        Some("my-imap-secret")
    );
    let newer = Secret::new("newer-password");
    store.set_password("me@example.com", &newer).unwrap();
    assert_eq!(
        fake.items().len(),
        1,
        "the second write edits the same item"
    );
    assert_eq!(
        store
            .get_password("me@example.com")
            .unwrap()
            .as_ref()
            .map(|s| s.expose()),
        Some("newer-password")
    );
    store.delete_password("me@example.com").unwrap();
    assert_eq!(store.get_password("me@example.com").unwrap(), None);
    assert!(fake.items().is_empty());
}

/// Final review F2: an `op item edit` that answers success but keeps the old values is caught
/// by reading the item back; the error is fixed text and quotes no token.
#[test]
fn an_edit_that_kept_the_old_values_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    store.set_tokens("me@example.com", &tokens()).unwrap();
    fake.edit_ignores_stdin();
    let newer = TokenSet {
        access_token: "ya29.newer-access".to_string(),
        refresh_token: "1//newer-refresh".to_string(),
        expires_at: 1_800_000_000,
    };
    let err = store.set_tokens("me@example.com", &newer).unwrap_err();
    assert!(
        matches!(err, SecretError::OnePasswordEditUnverified),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Permanent);
    let text = format!("{err} {err:?}");
    for secret in [ACCESS, REFRESH, "ya29.newer-access", "1//newer-refresh"] {
        assert!(!text.contains(secret), "{text}");
    }
    assert_eq!(store.get_tokens("me@example.com").unwrap(), Some(tokens()));
    assert!(
        fake.argv().contains("item get item1"),
        "the edit was read back"
    );
}

#[test]
fn items_are_titled_tagged_and_found_again_by_a_fresh_store() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    store(&fake).set_tokens("a@x.com", &tokens()).unwrap();
    let item = &fake.items()[0];
    assert!(item.contains("\"title\":\"Rail: a@x.com\""), "{item}");
    assert!(item.contains("\"tags\":[\"rail\"]"), "{item}");
    assert!(item.contains("\"type\":\"CONCEALED\""), "{item}");
    assert_eq!(store(&fake).get_tokens("a@x.com").unwrap(), Some(tokens()));
    assert!(
        fake.argv()
            .contains("item list --vault Rail --tags rail --format json")
    );
}

/// Review focus 1: values travel on stdin, never on the command line.
#[test]
fn op_arguments_never_carry_a_token_or_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    store.set_tokens("a@x.com", &tokens()).unwrap();
    store.set_tokens("a@x.com", &tokens()).unwrap();
    store.get_tokens("a@x.com").unwrap();
    store.delete_tokens("a@x.com").unwrap();
    let secret = Secret::new("GOCSPX-client-secret");
    let reference = store_client_secret(&fake.cli(), "Rail", "cid", &secret).unwrap();
    fake.set_read("GOCSPX-client-secret");
    assert_eq!(fake.cli().read_reference(&reference).unwrap(), secret);
    let argv = fake.argv();
    for value in [ACCESS, REFRESH, "GOCSPX-client-secret"] {
        assert!(!argv.contains(value), "{value} on a command line:\n{argv}");
    }
    assert!(
        argv.contains("item create --vault Rail --format json -"),
        "{argv}"
    );
    assert!(
        argv.contains("item edit item1 --vault Rail --format json"),
        "{argv}"
    );
}

#[test]
fn no_template_file_is_left_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    store(&fake).set_tokens("a@x.com", &tokens()).unwrap();
    let leaked = files_containing(tmp.path(), ACCESS, &fake.state);
    assert!(
        leaked.is_empty(),
        "token written outside 1Password: {leaked:?}"
    );
}

#[test]
fn cli_errors_never_echo_the_template() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.fail_on("a@x.com");
    let err = store(&fake).set_tokens("a@x.com", &tokens()).unwrap_err();
    let text = format!("{err} {err:?}");
    assert!(text.contains("could not create"), "{text}");
    for value in [ACCESS, REFRESH] {
        assert!(!text.contains(value), "{text}");
    }
    assert!(text.contains("<redacted>"), "{text}");
}

#[test]
fn a_locked_vault_is_permanent_with_the_unlock_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.lock();
    let err = store(&fake).get_tokens("a@x.com").unwrap_err();
    assert!(matches!(err, SecretError::OnePasswordLocked(_)), "{err:?}");
    assert_eq!(err.class(), ErrorClass::Permanent);
    assert!(err.to_string().contains("op signin"), "{err}");
    assert!(!err.to_string().contains("[ERROR]"), "{err}");
    assert!(store(&fake).probe().is_err());
}

/// The locked fake exits without reading its stdin: the write breaks, the real cause shows.
#[test]
fn a_locked_vault_is_reported_even_when_op_never_reads_the_template() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.lock();
    let secret = Secret::new("GOCSPX-client-secret");
    let err = store_client_secret(&fake.cli(), "Rail", "cid", &secret).unwrap_err();
    assert!(matches!(err, SecretError::OnePasswordLocked(_)), "{err:?}");
}

#[test]
fn escaped_forms_of_a_token_are_redacted_too() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.fail_on("a@x.com");
    let tricky = TokenSet {
        access_token: "ab\"cd\\ef".to_string(),
        ..tokens()
    };
    let err = store(&fake).set_tokens("a@x.com", &tricky).unwrap_err();
    let text = format!("{err} {err:?}");
    assert!(text.contains("could not create"), "{text}");
    assert!(!text.contains("ab\"cd\\ef"), "raw form leaked: {text}");
    assert!(
        !text.contains("ab\\\"cd\\\\ef"),
        "escaped form leaked: {text}"
    );
    assert!(text.contains("<redacted>"), "{text}");
}

/// A stand-in `op` printing `bytes` bytes of `x` and exiting with `code`.
fn printing_cli(tmp: &tempfile::TempDir, bytes: usize, code: i32) -> OpCli {
    let fake = FakeOp::install(tmp.path());
    let script = tmp.path().join("big-op");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nhead -c {bytes} /dev/zero | tr '\\0' 'x'\nexit {code}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = OnePasswordConfig {
        op_path: Some(script),
        ..fake.config()
    };
    OpCli::from_config(Some(&config), &fake.runtime_dir)
}

/// Review: the cap fails closed, whatever the exit status, with a fixed error.
#[test]
fn output_over_the_cap_fails_with_a_fixed_error() {
    let cap = super::MAX_OUTPUT as usize;
    for (bytes, code) in [(cap + 1, 0), (3_000_000, 0), (3_000_000, 1)] {
        let tmp = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let err = printing_cli(&tmp, bytes, code)
            .run(&["whoami"], None, &[])
            .unwrap_err();
        assert!(
            matches!(err, SecretError::OnePasswordOutputTooLarge),
            "{bytes} bytes, exit {code}: {err:?}"
        );
        assert_eq!(err.to_string(), "op's output exceeded 1 MiB");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = printing_cli(&tmp, cap, 0)
        .run(&["whoami"], None, &[])
        .unwrap();
    assert_eq!(out.len(), cap, "exactly the cap is still fine");
}

#[test]
fn a_cli_that_never_reads_a_large_template_still_times_out() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.hang();
    let cli = fake.cli().with_timeout(Duration::from_millis(300));
    let template = vec![b'x'; 4 * 1024 * 1024];
    let started = Instant::now();
    let err = cli
        .run(&["item", "create", "-"], Some(&template), &[])
        .unwrap_err();
    assert!(matches!(err, SecretError::OnePasswordTimeout), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn presence_checks_never_reveal_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    assert!(!store.has_tokens("a@x.com").unwrap());
    store.set_tokens("a@x.com", &tokens()).unwrap();
    assert!(store.has_tokens("a@x.com").unwrap());
    assert!(!fake.argv().contains("--reveal"), "{}", fake.argv());
    std::fs::remove_dir_all(fake.state.join("items")).unwrap();
    assert!(
        !store.has_tokens("a@x.com").unwrap(),
        "a stale cached id is not presence"
    );
}

#[test]
fn a_hung_cli_times_out_as_transient() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    fake.hang();
    let cli = fake.cli().with_timeout(Duration::from_millis(300));
    let started = Instant::now();
    let err = cli.whoami().unwrap_err();
    assert!(matches!(err, SecretError::OnePasswordTimeout), "{err:?}");
    assert_eq!(err.class(), ErrorClass::Transient);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn an_item_deleted_behind_rails_back_means_no_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = store(&fake);
    store.set_tokens("a@x.com", &tokens()).unwrap();
    std::fs::remove_dir_all(fake.state.join("items")).unwrap();
    assert_eq!(
        store.get_tokens("a@x.com").unwrap(),
        None,
        "cached id, gone item"
    );
    store.set_tokens("a@x.com", &tokens()).unwrap();
    assert_eq!(store.get_tokens("a@x.com").unwrap(), Some(tokens()));
}

fn refusal(tmp: &tempfile::TempDir, op_path: &Path) -> SecretError {
    let config = OnePasswordConfig {
        vault: "Rail".into(),
        account: None,
        op_path: Some(op_path.to_path_buf()),
    };
    OpCli::from_config(Some(&config), &tmp.path().join("run"))
        .whoami()
        .unwrap_err()
}

#[test]
fn a_missing_cli_names_op_path() {
    let tmp = tempfile::tempdir().unwrap();
    let err = refusal(&tmp, Path::new("/nonexistent/op"));
    assert!(
        matches!(err, SecretError::OnePasswordUnsafe { .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("op_path"), "{err}");
    let on_path = OpCli::from_config(None, &tmp.path().join("run"));
    assert!(
        format!("{on_path:?}").contains("\"op\""),
        "op from PATH by default"
    );
}

/// Review focus 6: only a file the user controls may be run as `op`.
#[test]
fn a_group_writable_op_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    std::fs::set_permissions(&fake.path, std::fs::Permissions::from_mode(0o775)).unwrap();
    let err = refusal(&tmp, &fake.path);
    assert!(
        err.to_string().contains("writable by group or others"),
        "{err}"
    );
    assert!(fake.argv().is_empty(), "the refused program never ran");
    assert_eq!(err.class(), ErrorClass::Permanent);
}

/// An `op` another user could swap, because the directory it sits in is theirs to write.
#[test]
fn an_op_in_a_group_writable_directory_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let shared = tmp.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    let fake = FakeOp::install(&shared);
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o775)).unwrap();
    let err = refusal(&tmp, &fake.path);
    assert!(
        err.to_string().contains("writable by group or others"),
        "{err}"
    );
    assert!(err.to_string().contains("shared"), "{err}");
    assert!(fake.argv().is_empty(), "the refused program never ran");
    // A sticky directory (like /tmp) is fine: others cannot replace what they do not own.
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777)).unwrap();
    refusal_free(&tmp, &fake.path);
}

fn refusal_free(tmp: &tempfile::TempDir, op_path: &Path) {
    let config = OnePasswordConfig {
        vault: "Rail".into(),
        account: None,
        op_path: Some(op_path.to_path_buf()),
    };
    OpCli::from_config(Some(&config), &tmp.path().join("run"))
        .whoami()
        .unwrap();
}

/// The program that runs is the checked canonical file, not the configured spelling.
#[test]
fn a_symlinked_op_runs_as_its_canonical_target() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let fake = FakeOp::install(&real);
    let link = tmp.path().join("op-link");
    std::os::unix::fs::symlink(&fake.path, &link).unwrap();
    refusal_free(&tmp, &link);
    let canonical = std::fs::canonicalize(&fake.path).unwrap();
    assert_eq!(fake.program(), canonical.display().to_string());
    assert_ne!(fake.program(), link.display().to_string());
}

/// Review focus 6.
#[test]
fn a_relative_op_path_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let err = refusal(&tmp, Path::new("bin/op"));
    assert!(err.to_string().contains("absolute"), "{err}");
}

/// Review focus 6.
#[test]
fn a_non_executable_or_non_file_op_path_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    std::fs::set_permissions(&fake.path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        refusal(&tmp, &fake.path)
            .to_string()
            .contains("not executable")
    );
    assert!(
        refusal(&tmp, tmp.path())
            .to_string()
            .contains("not a regular file")
    );
}

/// Review focus 6.
#[test]
fn a_private_fake_op_owned_by_the_user_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    for mode in [0o700, 0o755] {
        std::fs::set_permissions(&fake.path, std::fs::Permissions::from_mode(mode)).unwrap();
        fake.cli().whoami().unwrap();
    }
    let own = std::fs::metadata(&fake.runtime_dir).unwrap().uid();
    assert!(check_op_path(&fake.path, own).is_ok());
    assert!(
        check_op_path(&fake.path, own + 1)
            .unwrap_err()
            .contains("another user")
    );
}

#[test]
fn the_account_flag_is_passed_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let config = OnePasswordConfig {
        account: Some("my.1password.com".into()),
        ..fake.config()
    };
    OpCli::from_config(Some(&config), &fake.runtime_dir)
        .whoami()
        .unwrap();
    assert!(
        fake.argv()
            .contains("whoami --format json --account my.1password.com")
    );
}

#[test]
fn references_are_validated_before_op_runs() {
    for bad in [
        "op://",
        "op://vault",
        "op://vault/item",
        "op://vault//field",
        "https://x/y/z",
    ] {
        assert!(validate_reference(bad).is_err(), "{bad}");
    }
    assert!(validate_reference("op://Rail/Google client/client_secret").is_ok());
    let canary = "GOCSPX-canary-pasted-secret";
    let err = validate_reference(canary).unwrap_err();
    assert!(!err.to_string().contains(canary), "{err}");
    assert!(!format!("{err:?}").contains(canary), "{err:?}");
    let op = crate::ops::OpError::from(err);
    assert!(
        !serde_json::to_string(&op).unwrap().contains(canary),
        "{op:?}"
    );
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    assert!(fake.cli().read_reference("op://vault").is_err());
    assert!(fake.argv().is_empty(), "op was not run for a bad reference");
}

#[test]
fn an_empty_vault_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeOp::install(tmp.path());
    let store = OnePasswordSecretStore::new(fake.cli(), String::new(), ItemIdCache::default());
    let err = store.get_tokens("a@x.com").unwrap_err();
    assert!(err.to_string().contains("[onepassword]"), "{err}");
}

#[test]
fn log_prefixes_are_stripped() {
    assert_eq!(
        strip_log_prefix("[ERROR] 2026/10/10 08:35:58 You are not currently signed in"),
        "You are not currently signed in"
    );
    assert_eq!(strip_log_prefix("plain text"), "plain text");
}

/// Talks to a real, unlocked 1Password. Run with
/// `RAIL_OP_LIVE_VAULT=<vault> cargo test -p rail-core -- --ignored onepassword_live`.
#[test]
#[ignore]
fn onepassword_live_roundtrip() {
    let vault = std::env::var("RAIL_OP_LIVE_VAULT").expect("set RAIL_OP_LIVE_VAULT");
    let tmp = tempfile::tempdir().unwrap();
    let op = OpCli::from_config(None, &tmp.path().join("run"));
    let store = OnePasswordSecretStore::new(op, vault, ItemIdCache::default());
    let address = "rail-live-test@example.invalid";
    store.set_tokens(address, &tokens()).unwrap();
    assert_eq!(store.get_tokens(address).unwrap(), Some(tokens()));
    // The second write edits the existing item through `op item edit` with the template on
    // stdin; reading it back must give the new values.
    let newer = TokenSet {
        access_token: "ya29.live-test-second-access".to_string(),
        refresh_token: "1//live-test-second-refresh".to_string(),
        expires_at: 1_800_000_000,
    };
    store.set_tokens(address, &newer).unwrap();
    assert_eq!(store.get_tokens(address).unwrap(), Some(newer));
    store.delete_tokens(address).unwrap();
    assert_eq!(store.get_tokens(address).unwrap(), None);
}
