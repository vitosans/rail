//! `rail` with an account configured: the mail screen. It needs a terminal. The daemon is
//! reached from the screen's first effects, in the background, so the list reads from the
//! store at once and a daemon that cannot be reached only shows in the status bar.

use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use rail_core::config::Config;
use rail_core::ops::BrowserEnv;
use rail_core::paths::Paths;

use crate::tui::clock::Clock;
use crate::tui::data::{DaemonLink, StoreReader};
use crate::tui::editor;
use crate::tui::effects::Opener;
use crate::tui::keys::Keymap;
use crate::tui::run::{finish_run, run_app};
use crate::tui::screens::mail::{AccountEntry, MailContext, MailScreen};
use crate::tui::text::hyperlinks_supported;
use crate::tui::theme::Theme;
use crate::tui::theme::choice::{ColorDepth, ThemeChoice, ThemeError, color_depth};
use crate::{browser, daemon_link, test_hooks};

/// The most of a signature file a message carries.
const MAX_SIGNATURE_BYTES: usize = 4096;

/// Printed instead of the mail screen in a pipe.
pub const MAIL_PIPE_HINT: &str = "rail: the mail screen needs an interactive terminal; `rail search <query>` and `rail sync` work in scripts";

/// Open the mail screen over `config` (read from `text`, for `[keys]` line numbers).
pub async fn run(paths: &Paths, text: &str, config: &Config) -> color_eyre::Result<ExitCode> {
    let keymap = Arc::new(Keymap::from_config(
        &config.keys,
        config.general.keymap,
        text,
        &paths.config_file(),
    )?);
    let choice =
        ThemeChoice::from_config(&config.general, text, &paths.config_file()).map_err(|e| {
            color_eyre::eyre::eyre!(
                "{e}
{}",
                hint_for(&e)
            )
        })?;
    let depth = color_depth(|name| std::env::var(name).ok());
    let theme = Theme::from_choice(choice, depth);
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        eprintln!("{MAIL_PIPE_HINT}");
        return Ok(ExitCode::from(2));
    }
    let accounts = AccountEntry::from_config(config);
    let screen = MailScreen::new(
        context(paths, config, keymap.clone(), depth, choice),
        accounts,
    )
    .with_panic_on_key(test_hooks::mail_panic());
    // The screen is dropped (it removes the files `o` wrote) before a signal may kill Rail.
    drop(run_app(screen, keymap, theme).await?);
    finish_run().await;
    Ok(ExitCode::SUCCESS)
}

fn context(
    paths: &Paths,
    config: &Config,
    keymap: Arc<Keymap>,
    depth: ColorDepth,
    theme_choice: ThemeChoice,
) -> MailContext {
    // The TUI opens a URL only when the user asks; it runs where the user is, so the browser
    // inherits this process's own display.
    let opener: Opener = Arc::new(|url| browser::launch(url, &BrowserEnv::default()));
    MailContext {
        paths: paths.clone(),
        keymap,
        keymap_preset: config.general.keymap,
        store: StoreReader::new(paths.db_file()),
        link: DaemonLink::new(daemon_link::connector(paths)),
        opener,
        clock: Clock::system(config.general.time),
        icons: config.general.icons,
        split_threshold: config.general.split_threshold,
        bulk_confirm: config.general.bulk_confirm,
        // Validated at load, so this cannot fail; an empty list only costs the feature.
        link_actions: rail_core::config::compile_link_actions(config).unwrap_or_else(|err| {
            tracing::warn!(%err, "link actions could not be compiled");
            Vec::new()
        }),
        hints: config.general.hints,
        show_clock: config.general.clock,
        thread_order: config.general.thread_order,
        rich_html: config.general.rich_html,
        images: config.general.images,
        remote_images: config.general.remote_images,
        remote_images_from: config.general.remote_images_from.clone(),
        picker: picker(),
        dim_read: config.general.dim_read,
        field_keys: config.general.field_keys,
        hyperlinks: hyperlinks_supported(|name| std::env::var(name).ok()),
        depth,
        theme_choice,
        editor: editor::command_line(config.general.editor.as_deref(), |n| std::env::var(n).ok()),
        signatures: config
            .accounts
            .iter()
            .map(|a| a.signature.as_deref().map(read_signature))
            .collect(),
        // The daemon's working directory is `/`, so the client resolves the destination. A
        // path that is not UTF-8 counts as unresolved: `dir` is a `String` on the wire.
        attachment_dir: rail_core::paths::attachment_target(
            config.general.attachment_dir.as_deref(),
            home_dir().as_deref(),
            rail_core::paths::download_dir().as_deref(),
        )
        .filter(|dir| dir.to_str().is_some()),
        saved_searches: config.saved_searches.clone(),
        saved_search_counts: config.general.saved_search_counts,
    }
}

/// This process's home directory, read and never mutated.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// A signature file's text (a leading `~` is the home directory), trailing whitespace
/// trimmed, at most `MAX_SIGNATURE_BYTES`; or why it could not be read.
fn read_signature(path: &Path) -> Result<String, String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let path = match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    };
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut text = text.trim_end().to_string();
    if text.len() > MAX_SIGNATURE_BYTES {
        let mut end = MAX_SIGNATURE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(text)
}

/// The `hint:` line printed under a theme error.
fn hint_for(error: &ThemeError) -> &'static str {
    match error {
        ThemeError::UnknownTheme { .. } => {
            "hint: pick a theme with :theme inside Rail, or remove the key"
        }
        ThemeError::BadAccent { .. } => {
            "hint: accent takes a palette slot 0 to 15, a colour name such as \"blue\", or \"#rrggbb\"; :accent inside Rail picks one"
        }
    }
}

/// How this terminal draws pictures (kitty, iTerm2 or sixel, and its cell size), asked once
/// before the screen starts reading keys, since the answer comes on stdin. The picture query
/// must never go unanswered: on a timeout it leaves its reader behind, which takes every key
/// and later turns raw mode off (#155). So it is not sent to a terminal that cannot answer
/// (`asks_terminal`), to one that answers nothing (`answers_queries`, after up to 2 s), nor
/// inside tmux with no client attached, where tmux answers queries itself but passes the
/// picture query through to an outer terminal that is not there (`tmux_has_client`). Each of
/// those gets half blocks.
fn picker() -> Option<ratatui_image::picker::Picker> {
    if test_hooks::no_image_query() {
        return None;
    }
    let halfblocks = ratatui_image::picker::Picker::halfblocks;
    let in_tmux = std::env::var_os("TMUX").is_some();
    if !asks_terminal(std::env::var("TERM").ok().as_deref())
        || !answers_queries()
        || (in_tmux && !tmux_has_client(tmux_clients_attached().as_deref()))
    {
        return Some(halfblocks());
    }
    let picker = ratatui_image::picker::Picker::from_query_stdio();
    Some(picker.unwrap_or_else(|_| halfblocks()))
}

/// How many clients tmux says this session has (`#{session_attached}`), as tmux printed it;
/// `None` when tmux could not be asked.
fn tmux_clients_attached() -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{session_attached}"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether tmux's answer (`tmux_clients_attached`) names at least one attached client. No
/// answer, or one that is not a number, counts as none: half blocks are the safe side.
fn tmux_has_client(attached: Option<&str>) -> bool {
    attached
        .and_then(|text| text.trim().parse::<u32>().ok())
        .is_some_and(|clients| clients > 0)
}

/// Whether the terminal answers a query at all: crossterm's own probe (the keyboard
/// enhancement query, then primary device attributes) waits up to 2 s on this thread and
/// leaves nothing reading afterwards; keys typed meanwhile stay queued for the screen. Any
/// answer will do, supported or not; only silence fails.
fn answers_queries() -> bool {
    crossterm::terminal::supports_keyboard_enhancement().is_ok()
}

/// Whether a terminal named `term` (`$TERM`) can answer the picture query: not when it is
/// unset, empty or `dumb`.
fn asks_terminal(term: Option<&str>) -> bool {
    term.is_some_and(|term| !term.is_empty() && term != "dumb")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #155: a tmux session with no client passes the picture query to nobody, so it gets half
    /// blocks; an unreadable answer counts as no client.
    #[test]
    fn only_a_tmux_session_with_a_client_attached_is_given_the_picture_query() {
        assert!(tmux_has_client(Some("1\n")));
        assert!(tmux_has_client(Some("2")));
        assert!(!tmux_has_client(Some("0\n")));
        assert!(!tmux_has_client(Some("")));
        assert!(!tmux_has_client(Some("attached")));
        assert!(!tmux_has_client(None));
    }

    fn error(text: &str) -> ThemeError {
        let config = rail_core::config::parse(text, std::path::Path::new("/c")).unwrap();
        ThemeChoice::from_config(&config.general, text, std::path::Path::new("/c")).unwrap_err()
    }

    #[test]
    fn only_a_terminal_that_can_answer_is_asked_how_it_draws_pictures() {
        assert!(asks_terminal(Some("xterm-kitty")));
        assert!(asks_terminal(Some("xterm-256color")));
        assert!(!asks_terminal(None));
        assert!(!asks_terminal(Some("")));
        assert!(!asks_terminal(Some("dumb")));
    }

    #[test]
    fn a_signature_is_trimmed_capped_and_a_missing_file_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sig.txt");
        std::fs::write(&path, "Dana\n\n  \n").unwrap();
        assert_eq!(read_signature(&path).unwrap(), "Dana");
        std::fs::write(&path, "é".repeat(MAX_SIGNATURE_BYTES)).unwrap();
        let capped = read_signature(&path).unwrap();
        assert!(capped.len() <= MAX_SIGNATURE_BYTES && capped.len() > MAX_SIGNATURE_BYTES - 2);
        assert!(read_signature(&dir.path().join("none")).is_err());
    }

    /// The mail context `rail` would build over `text`.
    fn built(paths: &Paths, text: &str) -> MailContext {
        let file = paths.config_file();
        let config = rail_core::config::parse(text, &file).unwrap();
        let keymap = Arc::new(
            Keymap::from_config(&config.keys, config.general.keymap, text, &file).unwrap(),
        );
        context(
            paths,
            &config,
            keymap,
            ColorDepth::TrueColor,
            ThemeChoice::default(),
        )
    }

    /// AC-43: the client resolves the destination once, at startup. The environment is read,
    /// never mutated.
    #[test]
    fn context_resolves_the_attachment_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::all_in(tmp.path());
        let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME is set"));
        let account = "\n[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n";
        let with_key = built(
            &paths,
            &format!("[general]\nattachment_dir = \"~/mail\"{account}"),
        );
        assert_eq!(with_key.attachment_dir, Some(home.join("mail")));
        let unset = built(&paths, &format!("[general]{account}"))
            .attachment_dir
            .expect("a download directory");
        assert!(unset.is_absolute(), "{unset:?}");
        assert_eq!(
            unset,
            rail_core::paths::download_dir().unwrap_or_else(|| home.join("Downloads"))
        );
    }

    #[test]
    fn each_theme_error_has_its_own_hint() {
        assert!(hint_for(&error("[general]\ntheme = \"qqqqqqqqqq\"\n")).contains(":theme"));
        assert!(hint_for(&error("[general]\naccent = \"nope\"\n")).contains("slot 0 to 15"));
    }
}
