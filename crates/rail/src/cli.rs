use clap::{Parser, Subcommand, ValueEnum};

/// Modern email. Native to your terminal.
#[derive(Debug, Parser)]
#[command(name = "rail", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Test only: panic inside the alternate screen to check the terminal is restored.
    #[arg(long, hide = true)]
    pub panic_test: bool,
}

/// `rail search`'s default `--limit`: the daemon's.
const DEFAULT_LIMIT: u8 = 20;
/// `rail search`'s largest `--limit`: the daemon's.
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add an account with the interactive setup wizard.
    Setup,
    /// Manage accounts.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Choose where tokens are kept.
    Secrets {
        #[command(subcommand)]
        command: SecretsCommand,
    },
    /// Sync now: wake an account's (or every account's) sync and show progress until it is
    /// caught up. Exits 0 when caught up, 1 if a sync failed, 2 if an account needs a login.
    Sync {
        /// Account name or email address; every account when absent.
        account: Option<String>,
        /// Wake the sync and return at once.
        #[arg(long)]
        no_wait: bool,
    },
    /// Search the local store and each account's mailbox on its server: one line per thread.
    Search {
        /// What to look for (Gmail's search syntax for Gmail accounts; from:, subject:, to:, body: and words for Microsoft 365).
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        query: Vec<String>,
        /// Only this account (name or address).
        #[arg(long)]
        account: Option<String>,
        /// At most this many threads, 1 to 100.
        #[arg(
            long,
            default_value_t = DEFAULT_LIMIT,
            value_parser = clap::value_parser!(u8).range(1..=MAX_LIMIT)
        )]
        limit: u8,
    },
    /// Check config, store, secrets, accounts, each account's sync and the daemon.
    Doctor,
    /// Serve Rail's tools to an AI coding agent over MCP on stdin and stdout.
    Mcp,
    /// The background daemon that runs every operation. Started on demand, or at login once
    /// `rail daemon install` has run.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Run the daemon in the foreground (Rail starts it this way when needed).
    Run,
    /// Say whether the daemon is running, and what it is doing.
    Status,
    /// Ask the daemon to exit once running saves finish.
    Stop,
    /// Start the daemon at login: write and enable a systemd user unit (Linux) or a launchd
    /// agent (macOS) that runs this binary. Stops a daemon already running first.
    Install,
    /// Stop, disable and remove the login unit.
    Uninstall,
}

#[derive(Debug, Subcommand)]
pub enum SecretsCommand {
    /// Keep tokens in the OS keyring, a private file or 1Password, moving every account's
    /// tokens there. Shows what changes and asks first, so it runs only on an interactive
    /// terminal.
    Set(SecretsSetArgs),
    /// Set or update the stored password for an account.
    Password {
        /// Account name or email address.
        account: String,
        /// Service name: imap or smtp (default: account-level, used for both).
        #[arg(long)]
        service: Option<String>,
    },
}

#[derive(Debug, clap::Args)]
pub struct SecretsSetArgs {
    /// Where tokens go from now on.
    pub backend: BackendArg,
    /// 1Password only: the vault Rail keeps its items in (needed unless config.toml has one).
    #[arg(long)]
    pub vault: Option<String>,
    /// 1Password only: the account, when several are signed in.
    #[arg(long)]
    pub account: Option<String>,
    /// Change the setting without moving tokens; each account then needs `rail account login`.
    #[arg(long)]
    pub no_migrate: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BackendArg {
    Keyring,
    File,
    #[value(name = "1password")]
    OnePassword,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Add an account. With no arguments, or only a provider, the setup wizard opens.
    Add(Box<AddArgs>),
    /// Log in again to an account already in config.toml and replace its tokens.
    Login {
        /// Account name or email address.
        account: String,
    },
    /// List configured accounts and whether tokens are stored.
    List,
    /// Remove an account's config entry and stored tokens. Its mail stays in the store.
    /// Asks first, so it runs only on an interactive terminal.
    Remove {
        /// Account name or email address.
        account: String,
    },
}

/// The flag form needs a provider, `--address` and `--client-id`; every other flag needs
/// `--address`. Without `--address` the wizard opens. The combinations are checked in
/// `cmd::account::new_account_from_args`.
#[derive(Debug, clap::Args)]
pub struct AddArgs {
    /// Provider. Alone, it opens the setup wizard for that provider.
    pub kind: Option<KindArg>,
    /// Email address of the account.
    #[arg(long)]
    pub address: Option<String>,
    /// OAuth client id from Google Cloud or Azure.
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth client secret (Google desktop clients have one; Azure public clients do not).
    #[arg(long)]
    pub client_secret: Option<String>,
    /// Microsoft tenant (default: common).
    #[arg(long)]
    pub tenant: Option<String>,
    /// Short name used in the UI and config (default: derived from the address).
    #[arg(long)]
    pub name: Option<String>,
    /// Account colour in the UI.
    #[arg(long)]
    pub color: Option<String>,
    /// Store the client secret given in --client-secret in 1Password (the `[onepassword]`
    /// vault) and write an op:// reference into config.toml instead of the value.
    #[arg(long)]
    pub store_secret_in_1password: bool,
    /// IMAP server hostname for imapsmtp accounts.
    #[arg(long)]
    pub imap_host: Option<String>,
    /// IMAP server port for imapsmtp accounts (default: 993).
    #[arg(long)]
    pub imap_port: Option<u16>,
    /// IMAP username override when different from address.
    #[arg(long)]
    pub imap_user: Option<String>,
    /// SMTP server hostname for imapsmtp accounts.
    #[arg(long)]
    pub smtp_host: Option<String>,
    /// SMTP server port for imapsmtp accounts (default: 465).
    #[arg(long)]
    pub smtp_port: Option<u16>,
    /// SMTP username override when different from address.
    #[arg(long)]
    pub smtp_user: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    Gmail,
    M365,
    #[value(name = "imapsmtp")]
    ImapSmtp,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The search limits here are the daemon's.
    #[test]
    fn the_search_limits_match_the_daemon() {
        assert_eq!(
            usize::from(DEFAULT_LIMIT),
            rail_core::ops::DEFAULT_SEARCH_LIMIT
        );
        assert_eq!(
            usize::try_from(MAX_LIMIT).unwrap(),
            rail_core::ops::MAX_SEARCH_LIMIT
        );
    }

    #[test]
    fn a_search_limit_is_1_to_100() {
        let parse = |limit: &str| Cli::try_parse_from(["rail", "search", "--limit", limit, "x"]);
        assert!(parse("0").is_err());
        assert!(parse("101").is_err());
        assert!(parse("-1").is_err());
        for ok in ["1", "100"] {
            match parse(ok).unwrap().command {
                Some(Command::Search { limit, .. }) => assert_eq!(limit.to_string(), ok),
                other => panic!("{other:?}"),
            }
        }
        match Cli::try_parse_from(["rail", "search", "x"])
            .unwrap()
            .command
        {
            Some(Command::Search { limit, .. }) => assert_eq!(limit, 20),
            other => panic!("{other:?}"),
        }
    }
}
