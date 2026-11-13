//! `rail secrets set <keyring|file|1password>`: the terminal's way to change where tokens are
//! kept, as `rail_secrets_configure` is the agent's. It changes nothing until a person at a
//! terminal has read the daemon's description of the change and said yes (see `confirm`).

use rail_core::config::SecretsBackend;
use rail_core::ops::SecretsConfigureRequest;
use rail_core::ops::op::SecretsConfigure;
use rail_core::paths::Paths;

use super::account::report;
use super::confirm;
use super::mcp::summary;
use crate::cli::{BackendArg, SecretsSetArgs};
use crate::daemon_link;

impl From<BackendArg> for SecretsBackend {
    fn from(backend: BackendArg) -> Self {
        match backend {
            BackendArg::Keyring => SecretsBackend::Keyring,
            BackendArg::File => SecretsBackend::File,
            BackendArg::OnePassword => SecretsBackend::OnePassword,
        }
    }
}

const WORDS: confirm::Words = confirm::Words {
    not_a_terminal: "refusing to change where tokens are kept without asking: stdin and stdout are not both a terminal; nothing changed\nhint: run `rail secrets set` yourself in a terminal",
    question: "Change? [y/N] ",
    declined: "nothing changed",
    unexplained: "the daemon did not say what would change; nothing changed",
    plan_changed: "the settings changed while you answered; nothing changed — run it again",
};

/// The unconfirmed request the flags describe.
pub(crate) fn request(args: &SecretsSetArgs) -> SecretsConfigureRequest {
    SecretsConfigureRequest {
        backend: args.backend.into(),
        vault: args.vault.clone(),
        op_account: args.account.clone(),
        migrate: !args.no_migrate,
        confirm: false,
        plan: None,
    }
}

/// Ask the daemon what the change would do, show it, ask, and only on a yes make the change
/// the person read. A setting already in place changes nothing and needs no question.
pub async fn set(paths: &Paths, args: &SecretsSetArgs) -> color_eyre::Result<()> {
    confirm::require_terminal(&WORDS)?;
    let client = daemon_link::connector(paths)
        .client()
        .await
        .map_err(report)?;
    let confirmed = |probe: SecretsConfigureRequest, plan| SecretsConfigureRequest {
        confirm: true,
        plan: Some(plan),
        ..probe
    };
    let done =
        confirm::confirmed_call::<SecretsConfigure>(&client, request(args), confirmed, &WORDS)
            .await?;
    println!("{}", summary::secrets(&done));
    Ok(())
}

/// Set or update the stored password for an account or service.
pub async fn password(paths: &Paths, who: &str, service: Option<String>) -> color_eyre::Result<()> {
    let client = daemon_link::connector(paths)
        .client()
        .await
        .map_err(report)?;
    let prompt_label = match &service {
        Some(s) => format!("Password for {who} ({s}): "),
        None => format!("Password for {who}: "),
    };
    let pass = crate::cmd::password_prompt::read_password(&prompt_label)?;
    let res = client
        .call::<rail_core::ops::op::AccountPasswordSet>(
            &rail_core::ops::AccountPasswordSetRequest {
                account: who.to_string(),
                password: rail_core::ops::Secret::new(pass),
                service,
            },
        )
        .await
        .map_err(report)?;
    let svc = res
        .service
        .as_ref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    println!(
        "Stored password for \"{}\"{}. Stored in {}.",
        res.account, svc, res.backend
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command, SecretsCommand};
    use clap::Parser;

    fn parse(args: &[&str]) -> SecretsConfigureRequest {
        let mut all = vec!["rail", "secrets", "set"];
        all.extend_from_slice(args);
        match Cli::try_parse_from(all).unwrap().command {
            Some(Command::Secrets {
                command: SecretsCommand::Set(set),
            }) => request(&set),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_flags_become_an_unconfirmed_request() {
        let to_op = parse(&[
            "1password",
            "--vault",
            "Rail",
            "--account",
            "my.1password.com",
        ]);
        assert_eq!(to_op.backend, SecretsBackend::OnePassword);
        assert_eq!(to_op.vault.as_deref(), Some("Rail"));
        assert_eq!(to_op.op_account.as_deref(), Some("my.1password.com"));
        assert!(to_op.migrate);
        assert!(!to_op.confirm && to_op.plan.is_none());
        let to_file = parse(&["file", "--no-migrate"]);
        assert_eq!(to_file.backend, SecretsBackend::File);
        assert!(!to_file.migrate);
        assert_eq!(to_file.vault, None);
        assert_eq!(parse(&["keyring"]).backend, SecretsBackend::Keyring);
    }

    #[test]
    fn there_is_no_flag_that_confirms() {
        for flag in ["--yes", "--confirm", "--plan=x", "--op-path=/bin/sh"] {
            assert!(
                Cli::try_parse_from(["rail", "secrets", "set", "file", flag]).is_err(),
                "{flag}"
            );
        }
        assert!(Cli::try_parse_from(["rail", "secrets", "set", "vault"]).is_err());
    }

    #[test]
    fn secrets_password_parses_account_and_service() {
        match Cli::try_parse_from(["rail", "secrets", "password", "my-acc", "--service", "smtp"])
            .unwrap()
            .command
        {
            Some(Command::Secrets {
                command: SecretsCommand::Password { account, service },
            }) => {
                assert_eq!(account, "my-acc");
                assert_eq!(service.as_deref(), Some("smtp"));
            }
            other => panic!("{other:?}"),
        }
    }
}
