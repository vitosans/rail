//! `rail account add|login|list|remove` as daemon clients. Output and exit codes are those of
//! the in-process commands they replace, with the columns of `list` sized to their entries.

use std::time::Duration;

use color_eyre::eyre::{bail, eyre};
use rail_core::config::{AccountKind, SecretsBackend};
use rail_core::daemon::DaemonClient;
use rail_core::ops::op::{
    AccountAddBegin, AccountLoginBegin, AccountRemove, AccountsList, PendingCancel, PendingWait,
};
use rail_core::ops::{
    AccountAddRequest, AccountRemoveRequest, AccountRemoveResponse, AccountSummary,
    AccountsListRequest, BeginResponse, CancelRequest, ClientSecretInput, LoginBeginRequest,
    OpError, PendingId, PendingOutcome, PendingState, SavedAccount, TokenState, WaitRequest,
    WaitResponse,
};
use rail_core::paths::Paths;
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::confirm;
use crate::cli::{AddArgs, KindArg};
use crate::daemon_link;

impl From<KindArg> for AccountKind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::Gmail => AccountKind::Gmail,
            KindArg::M365 => AccountKind::M365,
            KindArg::ImapSmtp => AccountKind::ImapSmtp,
        }
    }
}

/// An operation error as the terminal shows it: see `error_text`.
pub(crate) fn report(error: OpError) -> color_eyre::Report {
    eyre!(error_text(&error))
}

/// Rail's message, then the outside text (`detail`: a provider's, the system's or a parser's
/// words) escaped and quoted on a line of its own that says where it came from, then the hint.
pub(crate) fn error_text(error: &OpError) -> String {
    let mut lines = vec![escaped(&error.message)];
    if let Some(detail) = &error.detail {
        lines.push(outside(detail.get()));
    }
    if let Some(hint) = &error.hint {
        lines.push(format!("hint: {}", escaped(hint)));
    }
    lines.join("\n")
}

/// Rail's own text with any control character escaped, so even a value that slipped into a
/// message can neither break the line nor reach the terminal as an escape sequence.
pub(crate) fn escaped(text: &str) -> String {
    escape_chars(text, char::is_control)
}

/// `text` with every character `needs_escape` accepts written as its `escape_debug` form
/// (`\n`, `\u{1b}`), the rest unchanged. The one loop behind each escaping helper.
pub(crate) fn escape_chars(text: &str, needs_escape: impl Fn(char) -> bool) -> String {
    text.chars()
        .map(|c| {
            if needs_escape(c) {
                c.escape_debug().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Text Rail did not write, labelled as such and quoted.
pub(crate) fn outside(text: &str) -> String {
    format!(
        "from outside Rail (the provider or the system): {}",
        quoted(text)
    )
}

/// `text` in double quotes with control characters escaped, so outside text can neither break
/// the line nor send escape sequences to the terminal.
pub(crate) fn quoted(text: &str) -> String {
    format!("{text:?}")
}

#[derive(Debug, PartialEq, Eq)]
pub enum NewAccount {
    OAuth(AccountAddRequest),
    ImapSmtp(ImapSmtpAddArgs),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImapSmtpAddArgs {
    pub address: String,
    pub name: Option<String>,
    pub color: Option<String>,
    pub imap_host: String,
    pub imap_port: Option<u16>,
    pub imap_user: Option<String>,
    pub smtp_host: String,
    pub smtp_port: Option<u16>,
    pub smtp_user: Option<String>,
}

/// The flag form of `rail account add`, or `None` when no flag asks for it and the wizard
/// should open (no arguments, or only a provider). A partial set of flags is an error that
/// names the missing one. `--client-secret op://...` is a 1Password reference.
pub(crate) fn new_account_from_args(args: AddArgs) -> color_eyre::Result<Option<NewAccount>> {
    let Some(address) = args.address else {
        let others = [
            ("--client-id", args.client_id.is_some()),
            ("--client-secret", args.client_secret.is_some()),
            ("--tenant", args.tenant.is_some()),
            ("--name", args.name.is_some()),
            ("--color", args.color.is_some()),
            (
                "--store-secret-in-1password",
                args.store_secret_in_1password,
            ),
            ("--imap-host", args.imap_host.is_some()),
            ("--smtp-host", args.smtp_host.is_some()),
        ];
        if let Some((flag, _)) = others.iter().find(|(_, given)| *given) {
            bail!("{flag} needs --address");
        }
        return Ok(None);
    };
    let Some(kind) = args.kind else {
        bail!("--address needs a provider: `rail account add <gmail|m365|imapsmtp> --address ...`");
    };
    if kind == crate::cli::KindArg::ImapSmtp {
        let Some(imap_host) = args.imap_host else {
            bail!("imapsmtp accounts need --imap-host");
        };
        let Some(smtp_host) = args.smtp_host else {
            bail!("imapsmtp accounts need --smtp-host");
        };
        return Ok(Some(NewAccount::ImapSmtp(ImapSmtpAddArgs {
            address,
            name: args.name,
            color: args.color,
            imap_host,
            imap_port: args.imap_port,
            imap_user: args.imap_user,
            smtp_host,
            smtp_port: args.smtp_port,
            smtp_user: args.smtp_user,
        })));
    }
    let Some(client_id) = args.client_id else {
        bail!("--address needs --client-id");
    };
    Ok(Some(NewAccount::OAuth(AccountAddRequest {
        kind: kind.into(),
        address,
        client_id: Some(client_id),
        client_secret: args.client_secret.map(ClientSecretInput::from_flag),
        tenant: args.tenant,
        name: args.name,
        color: args.color,
        store_secret_in_1password: args.store_secret_in_1password,
        open_browser: true,
    })))
}

async fn client(paths: &Paths) -> color_eyre::Result<DaemonClient> {
    daemon_link::connector(paths).client().await.map_err(report)
}

/// The daemon has already asked the browser to open; the URL is printed too, so a failed
/// opener still leaves the user a way in.
fn show(begun: &BeginResponse) {
    println!(
        "Open this URL in your browser if it does not open by itself:\n\n  {}\n",
        begun.url
    );
    if !begun.opened {
        eprintln!("rail: could not start the browser; open the URL above by hand");
    }
}

/// Ctrl-C from now on, delivered to `wait_or_cancel` instead of ending the process. Taken just
/// before the login begins, so a Ctrl-C that arrives while it starts still cancels it; one while
/// the daemon is still being reached ends the command as usual, with nothing begun.
fn interrupts() -> color_eyre::Result<Signal> {
    Ok(signal(SignalKind::interrupt())?)
}

/// Wait for the login to finish. Ctrl-C cancels it, unless the save has already started, in
/// which case the save is waited for: it is never interrupted.
async fn wait_or_cancel(
    client: &DaemonClient,
    id: &PendingId,
    interrupt: &mut Signal,
) -> color_eyre::Result<WaitResponse> {
    let wait = WaitRequest {
        id: id.clone(),
        timeout_secs: None,
    };
    loop {
        tokio::select! {
            answer = client.call::<PendingWait>(&wait) => {
                let answer = answer.map_err(report)?;
                if answer.state.is_finished() {
                    return Ok(answer);
                }
            }
            _ = interrupt.recv() => {
                let answer = cancel(client, id).await?;
                if answer.state != PendingState::Saving {
                    return Ok(answer);
                }
            }
        }
    }
}

/// The longest a Ctrl-C waits for the daemon to confirm the cancel.
pub(crate) const CANCEL_TIMEOUT: Duration = Duration::from_secs(5);

/// Send `pending.cancel`, bounded: a daemon that does not answer must not trap the user.
async fn cancel(client: &DaemonClient, id: &PendingId) -> color_eyre::Result<WaitResponse> {
    let request = CancelRequest { id: id.clone() };
    match tokio::time::timeout(CANCEL_TIMEOUT, client.call::<PendingCancel>(&request)).await {
        Ok(answer) => answer.map_err(report),
        Err(_) => Err(cancel_unanswered(CANCEL_TIMEOUT)),
    }
}

fn cancel_unanswered(waited: Duration) -> color_eyre::Report {
    eyre!(
        "the daemon did not answer the cancel within {} seconds; the login may still be pending\nhint: run `rail daemon status` to see it, or `rail daemon stop` to end it",
        waited.as_secs()
    )
}

/// The saved account, or the reason there is none.
fn saved(finished: WaitResponse) -> color_eyre::Result<SavedAccount> {
    match finished.state {
        PendingState::Done {
            outcome: PendingOutcome::Added { account } | PendingOutcome::LoggedIn { account },
        } => Ok(account),
        PendingState::Failed { error } => Err(eyre!("login failed: {}", error_text(&error))),
        PendingState::Cancelled => bail!("login cancelled; nothing was saved"),
        other => bail!("the login ended in an unexpected state: {other:?}"),
    }
}

fn warn_if_file_backend(paths: &Paths, account: &SavedAccount) {
    if account.token_backend == SecretsBackend::File.as_str() {
        eprintln!(
            "warning: tokens are stored in {} (mode 0600) because secrets = \"file\"; prefer the OS keyring where available",
            paths.secrets_file().display()
        );
    }
}

/// `rail account add` with flags: the daemon validates, logs in and saves.
pub async fn add(paths: &Paths, new: NewAccount) -> color_eyre::Result<()> {
    match new {
        NewAccount::OAuth(request) => add_oauth(paths, request).await,
        NewAccount::ImapSmtp(args) => add_imapsmtp(paths, args).await,
    }
}

async fn add_oauth(paths: &Paths, request: AccountAddRequest) -> color_eyre::Result<()> {
    let client = client(paths).await?;
    let mut interrupt = interrupts()?;
    let begun = client
        .call::<AccountAddBegin>(&request)
        .await
        .map_err(report)?;
    show(&begun);
    let account = saved(wait_or_cancel(&client, &begun.id, &mut interrupt).await?)?;
    warn_if_file_backend(paths, &account);
    println!(
        "Added {} ({}) as \"{}\". Tokens stored in the {}.",
        account.address,
        account.kind.as_str(),
        account.name,
        account.token_backend
    );
    Ok(())
}

async fn add_imapsmtp(paths: &Paths, args: ImapSmtpAddArgs) -> color_eyre::Result<()> {
    let client = client(paths).await?;
    let pass =
        crate::cmd::password_prompt::read_password(&format!("Password for {}: ", args.address))?;

    let validate_req = rail_core::ops::AccountValidateRequest {
        account: None,
        imap_host: Some(args.imap_host.clone()),
        imap_port: args.imap_port.or(Some(993)),
        imap_tls: Some(rail_core::config::model::TlsMode::Required),
        username: args
            .imap_user
            .clone()
            .or_else(|| Some(args.address.clone())),
        password: Some(rail_core::ops::Secret::new(pass.clone())),
        smtp_host: Some(args.smtp_host.clone()),
        smtp_port: args.smtp_port.or(Some(465)),
        smtp_tls: Some(rail_core::config::model::TlsMode::Required),
        smtp_username: args
            .smtp_user
            .clone()
            .or_else(|| Some(args.address.clone())),
        smtp_password: None,
    };
    client
        .call::<rail_core::ops::op::AccountValidate>(&validate_req)
        .await
        .map_err(report)?;

    let name = args.name.unwrap_or_else(|| {
        let (name, _) = args.address.split_once('@').unwrap_or((&args.address, ""));
        name.to_string()
    });
    let account = rail_core::config::AccountConfig {
        name: name.clone(),
        kind: rail_core::config::AccountKind::ImapSmtp,
        address: args.address.trim().to_ascii_lowercase(),
        color: args.color,
        label: None,
        client_id: String::new(),
        client_secret: None,
        tenant: None,
        signature: None,
        from_name: None,
        ai_send: None,
        history_days: None,
        body_cache_days: None,
        imap_host: Some(args.imap_host),
        imap_port: args.imap_port.or(Some(993)),
        imap_tls: Some(rail_core::config::model::TlsMode::Required),
        imap_user: args.imap_user,
        smtp_host: Some(args.smtp_host),
        smtp_port: args.smtp_port.or(Some(465)),
        smtp_tls: Some(rail_core::config::model::TlsMode::Required),
        smtp_user: args.smtp_user,
        sent_folder: None,
    };

    // Write the account before its password: the password operation resolves the account by
    // name against the config, and refuses one that is not there yet.
    let config_path = paths.config_file();
    let store_path = paths.db_file();
    let account_clone = account.clone();
    tokio::task::spawn_blocking(move || -> color_eyre::Result<()> {
        let (text, _) = rail_core::config::load_text_or_empty(&config_path)?;
        let new_text = rail_core::config::append_account(&text, &config_path, &account_clone)?;
        if let Some(dir) = config_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty() && !d.is_dir())
        {
            rail_core::paths::create_private_dir(dir)?;
        }
        rail_core::paths::replace_private_atomic(&config_path, new_text.as_bytes())?;
        rail_core::store::Store::open(&store_path)?.upsert_account(&account_clone)?;
        Ok(())
    })
    .await??;

    let set_pw = rail_core::ops::AccountPasswordSetRequest {
        account: account.name.clone(),
        password: rail_core::ops::Secret::new(pass),
        service: None,
    };
    let pw_res = client
        .call::<rail_core::ops::op::AccountPasswordSet>(&set_pw)
        .await
        .map_err(report)?;

    println!(
        "Added {} (imapsmtp) as \"{}\". Password stored in the {}.",
        account.address, account.name, pw_res.backend
    );
    Ok(())
}

/// `rail account login <name-or-address>`: log in again and replace the stored tokens.
/// An unknown account fails before the browser opens.
pub async fn relogin(paths: &Paths, who: &str) -> color_eyre::Result<()> {
    let client = client(paths).await?;
    let listed = client
        .call::<AccountsList>(&AccountsListRequest {})
        .await
        .map_err(report)?;
    if let Some(acc) = listed
        .accounts
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(who) || a.address.eq_ignore_ascii_case(who))
        && acc.kind == rail_core::config::AccountKind::ImapSmtp
    {
        let pass =
            crate::cmd::password_prompt::read_password(&format!("Password for {}: ", acc.address))?;
        let validate = rail_core::ops::AccountValidateRequest {
            account: Some(acc.name.clone()),
            password: Some(rail_core::ops::Secret::new(pass.clone())),
            ..Default::default()
        };
        client
            .call::<rail_core::ops::op::AccountValidate>(&validate)
            .await
            .map_err(report)?;
        let set_res = client
            .call::<rail_core::ops::op::AccountPasswordSet>(
                &rail_core::ops::AccountPasswordSetRequest {
                    account: acc.name.clone(),
                    password: rail_core::ops::Secret::new(pass),
                    service: None,
                },
            )
            .await
            .map_err(report)?;
        println!(
            "Updated password for {} (imapsmtp) as \"{}\". Password stored in the {}.",
            acc.address, acc.name, set_res.backend
        );
        return Ok(());
    }

    let mut interrupt = interrupts()?;
    let request = LoginBeginRequest {
        account: who.to_string(),
        open_browser: true,
    };
    let begun = client
        .call::<AccountLoginBegin>(&request)
        .await
        .map_err(report)?;
    show(&begun);
    let account = saved(wait_or_cancel(&client, &begun.id, &mut interrupt).await?)?;
    warn_if_file_backend(paths, &account);
    println!(
        "Logged in to {} ({}) as \"{}\". Tokens stored in the {}.",
        account.address,
        account.kind.as_str(),
        account.name,
        account.token_backend
    );
    Ok(())
}

/// `rail account list`. Token presence comes from the daemon, which asks the store whether
/// tokens exist without reading them.
pub async fn list(paths: &Paths) -> color_eyre::Result<()> {
    let listed = client(paths)
        .await?
        .call::<AccountsList>(&AccountsListRequest {})
        .await
        .map_err(report)?;
    if listed.accounts.is_empty() {
        println!("No accounts configured. Run `rail account add --help`.");
        return Ok(());
    }
    print!("{}", table(&listed.accounts));
    Ok(())
}

/// `rail account remove <name-or-address>`: removes only after a person at a terminal read
/// the daemon's text and said yes (see `confirm`).
pub async fn remove(paths: &Paths, who: &str) -> color_eyre::Result<()> {
    confirm::require_terminal(&REMOVE_WORDS)?;
    let client = client(paths).await?;
    let probe = AccountRemoveRequest {
        account: who.to_string(),
        confirm: false,
        plan: None,
    };
    let confirmed = |probe: AccountRemoveRequest, plan| AccountRemoveRequest {
        confirm: true,
        plan: Some(plan),
        ..probe
    };
    let removed =
        confirm::confirmed_call::<AccountRemove>(&client, probe, confirmed, &REMOVE_WORDS).await?;
    print_removed(&removed);
    Ok(())
}

const REMOVE_WORDS: confirm::Words = confirm::Words {
    not_a_terminal: "refusing to remove an account without asking: stdin and stdout are not both a terminal; nothing was removed\nhint: run `rail account remove` yourself in a terminal",
    question: "Remove? [y/N] ",
    declined: "nothing was removed",
    unexplained: "the daemon did not say what would be removed; nothing was removed",
    // The daemon refused the yes, which was for the account as it was.
    plan_changed: "the account changed while you answered; nothing was removed — run `rail account remove` again",
};

fn print_removed(removed: &AccountRemoveResponse) {
    let entry = format!(
        "Removed \"{}\" <{}> from config.toml",
        removed.name, removed.address
    );
    match &removed.token_error {
        None => println!("{entry} and deleted its tokens. Its mail stays in Rail's store."),
        Some(error) => {
            println!("{entry}. Its mail stays in Rail's store.");
            eprintln!(
                "warning: its tokens could not be deleted\n{}",
                outside(error.get())
            );
        }
    }
}

/// The account table, its name and address columns as wide as their longest entry.
pub(crate) fn table(accounts: &[AccountSummary]) -> String {
    let width = |header: &str, cell: fn(&AccountSummary) -> &str| {
        accounts
            .iter()
            .map(|a| cell(a).chars().count())
            .chain([header.len()])
            .max()
            .unwrap_or(0)
    };
    let name = width("NAME", |a| a.name.as_str());
    let address = width("ADDRESS", |a| a.address.as_str());
    let mut out = format!(
        "{:<name$} {:<6} {:<address$} TOKENS\n",
        "NAME", "KIND", "ADDRESS"
    );
    for account in accounts {
        let tokens = match &account.tokens {
            TokenState::Present => "yes".to_string(),
            TokenState::Missing => "missing".to_string(),
            TokenState::Error { message } => format!("error {}", outside(message.get())),
        };
        out += &format!(
            "{:<name$} {:<6} {:<address$} {tokens}\n",
            account.name,
            account.kind.as_str(),
            account.address
        );
    }
    out
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
