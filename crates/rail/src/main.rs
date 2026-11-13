mod browser;
mod cli;
mod cmd;
mod daemon_link;
mod logging;
mod test_hooks;
mod tui;

use std::process::ExitCode;

use clap::Parser;
use cli::{AccountCommand, Cli, Command, DaemonCommand, SecretsCommand};
use rail_core::paths::Paths;

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = color_eyre::install() {
        eprintln!("rail: {e}");
        return ExitCode::FAILURE;
    }
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(report) => {
            eprintln!("{report:?}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the command. Returning (rather than exiting) lets the log guard drop
/// and flush before the process ends.
async fn run(cli: Cli) -> color_eyre::Result<ExitCode> {
    if cli.panic_test {
        tui::terminal::panic_test();
    }
    let paths = Paths::from_env()?;
    let mcp = matches!(cli.command, Some(Command::Mcp));
    let _log_guard = logging::init(&paths, mcp);
    match cli.command {
        None => cmd::setup::first_run(&paths).await,
        Some(Command::Setup) => cmd::setup::wizard(&paths, None).await,
        Some(Command::Account { command }) => account(&paths, command).await,
        Some(Command::Secrets { command }) => match command {
            SecretsCommand::Set(args) => cmd::secrets::set(&paths, &args)
                .await
                .map(|()| ExitCode::SUCCESS),
            SecretsCommand::Password { account, service } => {
                cmd::secrets::password(&paths, &account, service)
                    .await
                    .map(|()| ExitCode::SUCCESS)
            }
        },
        Some(Command::Sync { account, no_wait }) => cmd::sync::run(&paths, account, no_wait).await,
        Some(Command::Search {
            query,
            account,
            limit,
        }) => cmd::search::run(&paths, query.join(" "), account, usize::from(limit)).await,
        Some(Command::Doctor) => cmd::doctor::run(&paths).await,
        Some(Command::Mcp) => cmd::mcp::run(&paths).await,
        Some(Command::Daemon { command }) => match command {
            DaemonCommand::Run => cmd::daemon::run(&paths).await,
            DaemonCommand::Status => cmd::daemon::status(&paths).await,
            DaemonCommand::Stop => cmd::daemon::stop(&paths).await,
            DaemonCommand::Install => cmd::service::install(&paths).await,
            DaemonCommand::Uninstall => cmd::service::uninstall(&paths).await,
        },
    }
}

async fn account(paths: &Paths, command: AccountCommand) -> color_eyre::Result<ExitCode> {
    match command {
        AccountCommand::Add(args) => {
            let kind_hint = args.kind.map(Into::into);
            match cmd::account::new_account_from_args(*args)? {
                Some(new) => cmd::account::add(paths, new)
                    .await
                    .map(|()| ExitCode::SUCCESS),
                None => cmd::setup::wizard(paths, kind_hint).await,
            }
        }
        AccountCommand::Login { account } => cmd::account::relogin(paths, &account)
            .await
            .map(|()| ExitCode::SUCCESS),
        AccountCommand::List => cmd::account::list(paths).await.map(|()| ExitCode::SUCCESS),
        AccountCommand::Remove { account } => cmd::account::remove(paths, &account)
            .await
            .map(|()| ExitCode::SUCCESS),
    }
}
