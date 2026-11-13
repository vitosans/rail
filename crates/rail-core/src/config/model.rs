use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::link::LinkAction;
use super::saved::SavedSearch;
use crate::provider::FolderKind;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    /// `[[saved_searches]]`: named filters the sidebar lists. Rail rewrites these tables when
    /// one is saved, renamed or removed, keeping the rest of the file.
    #[serde(default)]
    pub saved_searches: Vec<SavedSearch>,
    /// `[[link_actions]]`: a pattern over a thread's subject and the address it stands for,
    /// so one key reaches the record the thread is about.
    #[serde(default)]
    pub link_actions: Vec<LinkAction>,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    /// `[keys]`: action id to key name. The values keep their position in the file, so the
    /// TUI's keymap can name the line of a bad one.
    #[serde(default)]
    pub keys: BTreeMap<String, toml::Spanned<String>>,
    /// Where `secrets = "1password"` keeps tokens, and how to run `op`.
    #[serde(default)]
    pub onepassword: Option<OnePasswordConfig>,
}

/// The `[onepassword]` table.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OnePasswordConfig {
    /// The vault Rail keeps its items in.
    pub vault: String,
    /// The 1Password account, when several are signed in.
    #[serde(default)]
    pub account: Option<String>,
    /// The `op` binary; `op` on PATH when absent. Set by hand only.
    #[serde(default)]
    pub op_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    pub editor: Option<String>,
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    pub body_cache_days: u32,
    /// 0 means the whole mailbox.
    pub history_days: u32,
    pub layout: Layout,
    pub split_threshold: u16,
    pub icons: Icons,
    pub secrets: SecretsBackend,
    /// `general.theme`: a scheme name as `:theme` lists it; absent is Rail's default look.
    /// The TUI checks the name against its table at startup.
    pub theme: Option<String>,
    /// `general.accent`: a palette slot 0-15, a colour name, or `#rrggbb`; absent is cyan.
    pub accent: Option<AccentSpec>,
    /// `general.hints`: the key hints in the bottom bar; `false` keeps only notices and each
    /// account's state there. `:hints` toggles it.
    pub hints: bool,
    /// `general.clock`: the time of day at the right edge of the header. `:clock` toggles it.
    pub clock: bool,
    /// `general.time`: how a time of day is written on screen. `:time` toggles it.
    pub time: TimeFormat,
    /// `general.thread_order`: how the reader runs a thread's older messages.
    /// `:thread-order` toggles it.
    pub thread_order: ThreadOrder,
    /// How read threads are dimmed in the list; `:dim-read` cycles the modes.
    pub dim_read: DimRead,
    /// `general.keymap`: the preset the keys start from (`rail` by default). `:keymap`
    /// switches it.
    pub keymap: KeymapPreset,
    /// `general.field_keys`: how text fields edit (`readline` by default), whatever the
    /// keymap. `:field-keys` cycles it.
    pub field_keys: FieldKeys,
    /// `general.bulk_confirm`: how many threads an archive, star, mark-read or trash without
    /// asking (`list.trash_now`) may cover before Rail asks first; `0` never asks. `d` asks at
    /// every size. A number, not a two-state
    /// choice, so it has no palette command (CONTRIBUTING's opinionated-defaults rule).
    pub bulk_confirm: u32,
    /// `general.attachment_dir`: where `s`, `S` and `w` save attachments. Read verbatim: the
    /// client expands a leading `~` and refuses a relative path, because the daemon's working
    /// directory is `/`. Unset is the user's download directory.
    pub attachment_dir: Option<PathBuf>,
    /// `general.saved_search_counts`: the unread count beside each saved search in the
    /// sidebar. `false` leaves them out and skips the count queries. `:search-counts`
    /// toggles it.
    pub saved_search_counts: bool,
    /// `general.rich_html`: the reader draws an HTML body's bold, italic, quoted and linked
    /// text in their styles. `false` shows the plain text. `:rich-html` toggles it.
    pub rich_html: bool,
    /// `general.images`: in rich mode the reader draws an HTML body's pictures. `false`
    /// leaves them out. `:images` toggles it.
    pub images: bool,
    /// `general.remote_images`: the reader fetches every HTML body's remote pictures, which
    /// tells their senders the mail was read. `false` (the default) holds them back until a
    /// key loads one thread's. `:remote-images` toggles it.
    pub remote_images: bool,
    /// `general.remote_images_from`: the senders whose remote pictures the reader always
    /// fetches: an address, or `@domain` for everyone at exactly that domain. `I` on a message
    /// adds its sender or their domain, or removes the entry that matched.
    pub remote_images_from: Vec<String>,
    /// `general.remote_images_ipv6`: the daemon may fetch remote pictures over IPv6. `false`
    /// refuses every IPv6 address, for an IPv6-only network whose NAT64 prefix could reach
    /// private IPv4 space.
    pub remote_images_ipv6: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            editor: None,
            poll_interval: Duration::from_secs(60),
            body_cache_days: 30,
            history_days: 0,
            layout: Layout::Auto,
            split_threshold: 140,
            icons: Icons::Unicode,
            secrets: SecretsBackend::Keyring,
            theme: None,
            accent: None,
            hints: true,
            clock: true,
            time: TimeFormat::H24,
            thread_order: ThreadOrder::Oldest,
            dim_read: DimRead::Dot,
            keymap: KeymapPreset::Rail,
            field_keys: FieldKeys::Readline,
            bulk_confirm: 50,
            attachment_dir: None,
            saved_search_counts: true,
            rich_html: true,
            images: true,
            remote_images: false,
            remote_images_from: Vec::new(),
            remote_images_ipv6: true,
        }
    }
}

/// `general.accent`: a slot of the scheme's palette, or a colour of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccentSpec {
    Slot(u8),
    /// A ratatui colour name or `#rrggbb`; the TUI parses it.
    Color(String),
}

impl<'de> Deserialize<'de> for AccentSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Slot(i64),
            Color(String),
        }
        const ACCENT_HELP: &str = "accent must be 0 to 15, a colour name or \"#rrggbb\"";
        match Raw::deserialize(d).map_err(|_| serde::de::Error::custom(ACCENT_HELP))? {
            Raw::Slot(n) => u8::try_from(n)
                .ok()
                .filter(|n| *n <= 15)
                .map(Self::Slot)
                .ok_or_else(|| serde::de::Error::custom("accent must be 0 to 15")),
            Raw::Color(text) if text.is_empty() => Err(serde::de::Error::custom(ACCENT_HELP)),
            Raw::Color(text) => {
                plain_text("accent", &text).map_err(serde::de::Error::custom)?;
                Ok(Self::Color(text))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Auto,
    Split,
    Stack,
}

/// `general.icons`: the glyphs of the mail screen (`ascii` for terminals or fonts without
/// them).
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    #[default]
    Unicode,
    Ascii,
}

/// `general.time`: `24h` writes `14:05`, `12h` writes `2:05 pm`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub enum TimeFormat {
    #[default]
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

impl TimeFormat {
    /// The config value: `24h` or `12h`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H24 => "24h",
            Self::H12 => "12h",
        }
    }
}

/// `general.dim_read`: retain today's dot, dim read subjects, or dim all read row text.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DimRead {
    #[default]
    Dot,
    Subject,
    All,
}

impl DimRead {
    /// The config value: `dot`, `subject` or `all`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::Subject => "subject",
            Self::All => "all",
        }
    }
}

/// `general.keymap`: the preset every action's keys start from, before `[keys]`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeymapPreset {
    /// Rail's own keys.
    #[default]
    Rail,
    /// vim's: `h`/`l` between panes, `u` undo.
    Vim,
    /// mutt's: `s` saves (moves), `g` replies to everyone, `t` tags, `N` toggles new.
    Mutt,
    /// emacs's: `Ctrl-n`/`Ctrl-p`, `Ctrl-v`/`Alt-v`, `Ctrl-g` and `Ctrl-x` chords.
    Emacs,
    /// Gmail's web shortcuts: `o`, `u` back, `I`/`U`, `#`, `v`, `b`, `x`.
    Gmail,
    /// Outlook's: `Delete`, `Backspace` archive, `Insert` flag, `Ctrl-q`/`Ctrl-u`, `Ctrl-z`.
    Outlook,
}

impl KeymapPreset {
    /// Every preset, in the order `:keymap` lists them.
    pub const ALL: &'static [KeymapPreset] = &[
        Self::Rail,
        Self::Vim,
        Self::Mutt,
        Self::Emacs,
        Self::Gmail,
        Self::Outlook,
    ];

    /// The config value: `rail`, `vim`, `mutt`, `emacs`, `gmail` or `outlook`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rail => "rail",
            Self::Vim => "vim",
            Self::Mutt => "mutt",
            Self::Emacs => "emacs",
            Self::Gmail => "gmail",
            Self::Outlook => "outlook",
        }
    }

    /// What `:keymap` and the setup wizard say beside the name: the keys that differ from
    /// Rail's most, short enough that the line fits the picker (`preset_tests` in `rail`).
    pub fn summary(self) -> &'static str {
        match self {
            Self::Rail => "Rail's own keys",
            Self::Vim => "h/l panes · u undo · U read · Z snooze · D trash",
            Self::Mutt => "s move · g reply all · t tag · N read · u undo",
            Self::Emacs => "C-n/C-p · C-v/M-v page · C-s filter · C-g back",
            Self::Gmail => "I/U read · # trash · v move · b snooze · x select",
            Self::Outlook => "Del trash · Bksp archive · Ins flag · C-q read",
        }
    }

    /// `as_str` and `summary` as one line: `vim — h/l panes · …`.
    pub fn label(self) -> String {
        format!("{} — {}", self.as_str(), self.summary())
    }
}

/// `general.field_keys`: the editing keys of every text field. Separate from `keymap`:
/// fields take raw keys, so no preset or `[keys]` entry reaches them.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FieldKeys {
    /// Bash's default (emacs) keys: `Ctrl-a`/`Ctrl-e`, `Ctrl-w`, `Alt-b`/`Alt-f` and so on.
    #[default]
    Readline,
    /// A shell's `set -o vi`: fields open in insert mode and `Esc` goes to normal mode.
    Vi,
    /// Arrows, `Home`/`End`, `Backspace` and `Delete` only; Ctrl and Alt keys do nothing.
    Basic,
}

impl FieldKeys {
    /// The config value: `readline`, `vi` or `basic`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Readline => "readline",
            Self::Vi => "vi",
            Self::Basic => "basic",
        }
    }

    /// The mode `:field-keys` switches to: `readline`, `vi`, `basic`, then round again.
    pub fn next(self) -> Self {
        match self {
            Self::Readline => Self::Vi,
            Self::Vi => Self::Basic,
            Self::Basic => Self::Readline,
        }
    }
}

/// `general.thread_order`: how the reader runs a thread's older messages under the newest one.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThreadOrder {
    /// The oldest first, as the conversation went.
    #[default]
    Oldest,
    /// The most recent first, right under the newest message.
    Newest,
}

impl ThreadOrder {
    /// The config value: `oldest` or `newest`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oldest => "oldest",
            Self::Newest => "newest",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SecretsBackend {
    Keyring,
    File,
    /// Items in a 1Password vault, through the `op` CLI.
    #[serde(rename = "1password")]
    OnePassword,
}

impl SecretsBackend {
    /// The spelling in `config.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::File => "file",
            Self::OnePassword => "1password",
        }
    }
}

/// TLS configuration mode for IMAP and SMTP connections.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    #[default]
    Required,
    Opportunistic,
    Off,
}

impl TlsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Opportunistic => "opportunistic",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    pub name: String,
    pub kind: AccountKind,
    #[serde(deserialize_with = "plain_address")]
    pub address: String,
    #[serde(default, deserialize_with = "plain_color")]
    pub color: Option<String>,
    /// The account's name on screen (the sidebar, the header, `Write from`, the account
    /// states); `name` when unset or blank. At most `MAX_LABEL_CHARS` characters.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    /// Microsoft only. Defaults to `common`.
    #[serde(default, deserialize_with = "plain_tenant")]
    pub tenant: Option<String>,
    #[serde(default)]
    pub signature: Option<PathBuf>,
    /// The name put before `address` in `From:` of mail Rail sends; spaces allowed.
    #[serde(default, deserialize_with = "plain_from_name")]
    pub from_name: Option<String>,
    /// What an AI agent may do with this account's mail over MCP. Unset means `confirm`.
    #[serde(default)]
    pub ai_send: Option<AiSend>,
    /// This account's `history_days`, instead of `[general]`'s. 0 is the whole mailbox.
    #[serde(default)]
    pub history_days: Option<u32>,
    /// This account's `body_cache_days`, instead of `[general]`'s.
    #[serde(default)]
    pub body_cache_days: Option<u32>,
    /// IMAP host for `imapsmtp` accounts.
    #[serde(default)]
    pub imap_host: Option<String>,
    /// IMAP port for `imapsmtp` accounts. Defaults to 993 (required TLS) or 143.
    #[serde(default)]
    pub imap_port: Option<u16>,
    /// IMAP TLS mode (required, opportunistic, off). Defaults to required.
    #[serde(default)]
    pub imap_tls: Option<TlsMode>,
    /// IMAP username override when different from `address`.
    #[serde(default)]
    pub imap_user: Option<String>,
    /// SMTP host for `imapsmtp` accounts.
    #[serde(default)]
    pub smtp_host: Option<String>,
    /// SMTP port for `imapsmtp` accounts. Defaults to 465 (required TLS) or 587.
    #[serde(default)]
    pub smtp_port: Option<u16>,
    /// SMTP TLS mode (required, opportunistic, off). Defaults to required.
    #[serde(default)]
    pub smtp_tls: Option<TlsMode>,
    /// SMTP username override when different from `address`.
    #[serde(default)]
    pub smtp_user: Option<String>,
    /// Optional Sent mailbox name override for `imapsmtp` accounts.
    #[serde(default)]
    pub sent_folder: Option<String>,
    /// Whether Rail appends a sent message to the IMAP Sent mailbox. Unset means true. Set
    /// false for servers such as Gmail or iCloud that already file their own copy.
    #[serde(default)]
    pub append_to_sent: Option<bool>,
}

/// True when `text` holds no whitespace, no control character and no Unicode format
/// character (general category Cf: bidi overrides such as U+202E, zero-width joiners and
/// spaces, the byte-order mark). An account's address, colour and tenant are shown on the
/// terminal and in confirmations; none of these belongs in any of them.
pub fn is_plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || is_format_char(c))
}

/// True for a Unicode format character (general category Cf): invisible, and able to reorder
/// or hide what is shown around it.
pub fn is_format_char(c: char) -> bool {
    use icu_properties::CodePointMapData;
    use icu_properties::props::GeneralCategory;
    CodePointMapData::<GeneralCategory>::new().get(c) == GeneralCategory::Format
}

/// Why a value that is not `is_plain` is refused. Never quotes the value.
pub fn not_plain(field: &str) -> String {
    format!("the {field} must not contain spaces, control characters or format characters")
}

/// A plain string, or a deserialization error naming `field` (the parser adds the position).
fn plain<'de, D: serde::Deserializer<'de>>(field: &str, d: D) -> Result<String, D::Error> {
    let text = String::deserialize(d)?;
    plain_text(field, &text).map_err(serde::de::Error::custom)?;
    Ok(text)
}

/// `Ok` when `text` is a scheme name: ordinary spaces are allowed ("Catppuccin Mocha"), other
/// whitespace, control characters and format characters are not. Never quotes the value.
pub(super) fn plain_theme(text: &str) -> Result<(), String> {
    let bad = |c: char| c != ' ' && (c.is_whitespace() || c.is_control() || is_format_char(c));
    if text.chars().any(bad) {
        Err("the theme must not contain control characters, format characters or whitespace other than spaces".to_string())
    } else {
        Ok(())
    }
}

/// `Ok` when `text` is plain, else the refusal naming `field`.
pub(super) fn plain_text(field: &str, text: &str) -> Result<(), String> {
    if is_plain(text) {
        Ok(())
    } else {
        Err(not_plain(field))
    }
}

/// The longest `from_name`, in characters.
pub const MAX_FROM_NAME_CHARS: usize = 200;

/// The longest account `label`, in characters.
pub const MAX_LABEL_CHARS: usize = 40;

/// `Ok` when `text` is a display name: ordinary spaces are allowed, other whitespace, control
/// characters and format characters are not, and it is at most `MAX_FROM_NAME_CHARS` long.
/// Never quotes the value.
pub(super) fn plain_name(text: &str) -> Result<(), String> {
    let bad = |c: char| c != ' ' && (c.is_whitespace() || c.is_control() || is_format_char(c));
    if text.chars().any(bad) {
        Err("the from_name must not contain control characters, format characters or whitespace other than spaces".to_string())
    } else if text.chars().count() > MAX_FROM_NAME_CHARS {
        Err(format!(
            "the from_name must be at most {MAX_FROM_NAME_CHARS} characters"
        ))
    } else {
        Ok(())
    }
}

fn plain_from_name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let text = String::deserialize(d)?;
    plain_name(&text).map_err(serde::de::Error::custom)?;
    // An empty name is no name: `From:` carries the bare address.
    Ok((!text.trim().is_empty()).then_some(text))
}

fn plain_address<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    plain("address", d)
}

fn plain_color<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    plain("color", d).map(Some)
}

fn plain_tenant<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    plain("tenant", d).map(Some)
}

impl AccountConfig {
    /// The account's send policy, `confirm` when the key is absent.
    pub fn ai_send(&self) -> AiSend {
        self.ai_send.unwrap_or_default()
    }

    pub fn imap_port(&self) -> u16 {
        self.imap_port.unwrap_or_else(|| match self.imap_tls() {
            TlsMode::Required => 993,
            TlsMode::Opportunistic | TlsMode::Off => 143,
        })
    }

    pub fn imap_tls(&self) -> TlsMode {
        self.imap_tls.unwrap_or(TlsMode::Required)
    }

    pub fn imap_user(&self) -> &str {
        self.imap_user.as_deref().unwrap_or(&self.address)
    }

    pub fn smtp_port(&self) -> u16 {
        self.smtp_port.unwrap_or_else(|| match self.smtp_tls() {
            TlsMode::Required => 465,
            TlsMode::Opportunistic | TlsMode::Off => 587,
        })
    }

    pub fn smtp_tls(&self) -> TlsMode {
        self.smtp_tls.unwrap_or(TlsMode::Required)
    }

    pub fn smtp_user(&self) -> &str {
        self.smtp_user.as_deref().unwrap_or(&self.address)
    }
}

/// Seconds in a day.
pub const DAY_SECS: i64 = 86_400;

/// What one account's sync uses: `[general]` with the account's overrides, and
/// `body_cache_days` clamped to a non-zero `history_days` (bodies are only kept for mail the
/// store keeps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncSettings {
    /// 0 means the whole mailbox.
    pub history_days: u32,
    pub body_cache_days: u32,
    pub poll_interval: Duration,
}

impl SyncSettings {
    pub fn for_account(general: &General, account: &AccountConfig) -> Self {
        let history_days = account.history_days.unwrap_or(general.history_days);
        let mut body_cache_days = account.body_cache_days.unwrap_or(general.body_cache_days);
        if history_days != 0 {
            body_cache_days = body_cache_days.min(history_days);
        }
        Self {
            history_days,
            body_cache_days,
            poll_interval: general.poll_interval,
        }
    }

    /// The oldest `internalDate` (milliseconds) inside the window at `now` (Unix seconds),
    /// or `None` when the window is the whole mailbox: `history_days = 0`, or a window that
    /// reaches back before 1970 (Gmail's `after:` takes no negative time).
    pub fn window_start_ms(&self, now: i64) -> Option<i64> {
        if self.history_days == 0 {
            return None;
        }
        let start = now.saturating_sub(i64::from(self.history_days).saturating_mul(DAY_SECS));
        (start >= 0).then(|| start.saturating_mul(1000))
    }

    /// The oldest `internalDate` (milliseconds) whose thread gets bodies prefetched.
    pub fn body_since_ms(&self, now: i64) -> i64 {
        now.saturating_sub(i64::from(self.body_cache_days).saturating_mul(DAY_SECS))
            .saturating_mul(1000)
    }
}

/// What an agent may do through MCP with an account's mail: send, reply, forward and delete.
/// Parsed and reported now; enforced when the send tools arrive.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiSend {
    /// The user confirms each action through MCP elicitation; without elicitation, drafts only.
    #[default]
    Confirm,
    /// Drafts may be created and edited, never sent.
    Drafts,
    /// Rely on the MCP client's own per-call approval prompt. The weakest option.
    Client,
}

impl AiSend {
    /// The value as written in `config.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirm => "confirm",
            Self::Drafts => "drafts",
            Self::Client => "client",
        }
    }
}

/// Hides `client_secret` so `{:?}` and tracing fields never print it.
impl std::fmt::Debug for AccountConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountConfig")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("address", &self.address)
            .field("color", &self.color)
            .field("label", &self.label)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("tenant", &self.tenant)
            .field("signature", &self.signature)
            .field("from_name", &self.from_name.is_some())
            .field("ai_send", &self.ai_send)
            .field("history_days", &self.history_days)
            .field("body_cache_days", &self.body_cache_days)
            .field("imap_host", &self.imap_host)
            .field("imap_port", &self.imap_port)
            .field("imap_tls", &self.imap_tls)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_tls", &self.smtp_tls)
            .finish()
    }
}

/// How an account's mail sits in folders: Gmail's labels (a message carries several) or one
/// folder per message (Microsoft 365, IMAP/SMTP). Code branches on this, never on the account kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderModel {
    Labels,
    SingleFolder,
}

impl FolderModel {
    /// The folder kinds `m` offers and `action.apply` accepts as a move target.
    pub fn move_kinds(self) -> &'static [FolderKind] {
        match self {
            Self::Labels => &[FolderKind::Label, FolderKind::User],
            Self::SingleFolder => &[
                FolderKind::Inbox,
                FolderKind::Archive,
                FolderKind::Spam,
                FolderKind::User,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    #[default]
    Gmail,
    M365,
    #[serde(rename = "imapsmtp")]
    ImapSmtp,
}

impl AccountKind {
    /// The provider's name in Rail's messages.
    pub fn provider_name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::M365 => "Microsoft 365",
            Self::ImapSmtp => "IMAP/SMTP",
        }
    }

    /// How this provider's mail sits in folders.
    pub fn folder_model(self) -> FolderModel {
        match self {
            Self::Gmail => FolderModel::Labels,
            Self::M365 | Self::ImapSmtp => FolderModel::SingleFolder,
        }
    }

    /// What the trash prompt says of this provider's trash.
    pub fn trash_note(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail keeps trashed mail in its Trash for 30 days.",
            Self::M365 => "Moves to Deleted Items.",
            Self::ImapSmtp => "Moves to Trash.",
        }
    }

    /// The provider's Drafts folder in what Rail says.
    pub fn drafts_name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail's Drafts",
            Self::M365 => "Outlook's Drafts",
            Self::ImapSmtp => "Drafts",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::M365 => "m365",
            Self::ImapSmtp => "imapsmtp",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gmail" => Some(Self::Gmail),
            "m365" => Some(Self::M365),
            "imapsmtp" => Some(Self::ImapSmtp),
            _ => None,
        }
    }
}
