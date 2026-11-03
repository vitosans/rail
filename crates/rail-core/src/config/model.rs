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
    /// `general.remote_images_from`: the senders (addresses) whose remote pictures the reader
    /// always fetches. `I` on a message adds its sender, or removes them.
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

#[derive(Clone, Deserialize, PartialEq)]
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
            .finish()
    }
}

/// How an account's mail sits in folders: Gmail's labels (a message carries several) or one
/// folder per message (Microsoft 365). Code branches on this, never on the account kind.
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Gmail,
    M365,
}

impl AccountKind {
    /// The provider's name in Rail's messages.
    pub fn provider_name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::M365 => "Microsoft 365",
        }
    }

    /// How this provider's mail sits in folders.
    pub fn folder_model(self) -> FolderModel {
        match self {
            Self::Gmail => FolderModel::Labels,
            Self::M365 => FolderModel::SingleFolder,
        }
    }

    /// What the trash prompt says of this provider's trash.
    pub fn trash_note(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail keeps trashed mail in its Trash for 30 days.",
            Self::M365 => "Moves to Deleted Items.",
        }
    }

    /// The provider's Drafts folder in what Rail says.
    pub fn drafts_name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail's Drafts",
            Self::M365 => "Outlook's Drafts",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::M365 => "m365",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gmail" => Some(Self::Gmail),
            "m365" => Some(Self::M365),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_account(general: &str, account: &str) -> crate::config::Config {
        let text = format!(
            "[general]\n{general}\n[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{account}"
        );
        crate::config::parse(&text, std::path::Path::new("c.toml")).unwrap()
    }

    /// AC-34: `attachment_dir` reaches `General` exactly as it was typed — the client expands
    /// `~` and absolutizes, not the parser — and an unknown key still refuses the file.
    #[test]
    fn attachment_dir_is_read_verbatim() {
        let config = with_account("attachment_dir = \"~/mail\"", "");
        assert_eq!(
            config.general.attachment_dir,
            Some(std::path::PathBuf::from("~/mail"))
        );
        assert_eq!(with_account("", "").general.attachment_dir, None);
        assert_eq!(General::default().attachment_dir, None);
        let text = "[general]\nattachement_dir = \"~/mail\"\n";
        assert!(crate::config::parse(text, std::path::Path::new("c.toml")).is_err());
    }

    #[test]
    fn each_kind_has_its_folder_model_and_its_move_kinds() {
        use crate::provider::FolderKind;
        assert_eq!(AccountKind::Gmail.folder_model(), FolderModel::Labels);
        assert_eq!(AccountKind::M365.folder_model(), FolderModel::SingleFolder);
        assert_eq!(
            FolderModel::Labels.move_kinds(),
            [FolderKind::Label, FolderKind::User]
        );
        assert_eq!(
            FolderModel::SingleFolder.move_kinds(),
            [
                FolderKind::Inbox,
                FolderKind::Archive,
                FolderKind::Spam,
                FolderKind::User
            ]
        );
    }

    #[test]
    fn sync_settings_apply_overrides_and_the_clamp() {
        let cases = [
            ("", "", (0, 30)),
            ("history_days = 2", "", (2, 2)),
            ("history_days = 10\nbody_cache_days = 5", "", (10, 5)),
            ("history_days = 10", "history_days = 0", (0, 30)),
            ("", "history_days = 7\nbody_cache_days = 9", (7, 7)),
            ("body_cache_days = 3", "body_cache_days = 40", (0, 40)),
        ];
        for (general, account, (history, body)) in cases {
            let config = with_account(general, account);
            let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
            assert_eq!(
                (settings.history_days, settings.body_cache_days),
                (history, body),
                "{general:?} / {account:?}"
            );
            assert_eq!(settings.poll_interval, Duration::from_secs(60));
        }
    }

    #[test]
    fn the_window_starts_history_days_ago_in_milliseconds() {
        let config = with_account("history_days = 2", "");
        let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
        let now = 10 * DAY_SECS;
        assert_eq!(settings.window_start_ms(now), Some(8 * DAY_SECS * 1000));
        assert_eq!(settings.body_since_ms(now), 8 * DAY_SECS * 1000);
        let whole = with_account("", "");
        let whole = SyncSettings::for_account(&whole.general, &whole.accounts[0]);
        assert_eq!(whole.window_start_ms(now), None);
    }

    /// Review minor 7: a window reaching before 1970 is the whole mailbox, never a negative
    /// `after:`.
    #[test]
    fn a_window_before_1970_is_the_whole_mailbox() {
        let config = with_account(&format!("history_days = {}", u32::MAX), "");
        let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
        let now = 20_000 * DAY_SECS;
        assert_eq!(settings.window_start_ms(now), None);
        let config = with_account("history_days = 20000", "");
        let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
        assert_eq!(
            settings.window_start_ms(now),
            Some(0),
            "exactly 1970 is kept"
        );
        let config = with_account("history_days = 20001", "");
        let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
        assert_eq!(settings.window_start_ms(now), None);
    }

    #[test]
    fn account_overrides_debug_print() {
        let config = with_account("", "history_days = 3");
        let debug = format!("{:?}", config.accounts[0]);
        assert!(debug.contains("history_days: Some(3)"), "{debug}");
    }

    /// Parked item 3: bidi overrides and zero-width characters are Unicode format (Cf)
    /// characters; an address holding one could display as another.
    #[test]
    fn format_characters_are_not_plain() {
        for c in [
            '\u{202E}',
            '\u{200B}',
            '\u{200D}',
            '\u{2066}',
            '\u{FEFF}',
            '\u{E0001}',
        ] {
            assert!(!is_plain(&format!("me{c}@example.com")), "{:?}", c);
        }
        assert!(is_plain("ünïcödé@example.com"));
    }

    #[test]
    fn account_debug_hides_the_client_secret() {
        let account = AccountConfig {
            client_secret: Some("GOCSPX-very-secret".into()),
            ..crate::test_support::sample_account()
        };
        let debug = format!("{account:?}");
        assert!(!debug.contains("GOCSPX-very-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(debug.contains("me@example.com"), "{debug}");
    }

    #[test]
    fn ai_send_defaults_to_confirm_and_parses_each_value() {
        use crate::config::parse;
        use std::path::Path;
        let account = |extra: &str| {
            format!(
                "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{extra}"
            )
        };
        let unset = parse(&account(""), Path::new("c.toml")).unwrap();
        assert_eq!(unset.accounts[0].ai_send, None);
        assert_eq!(unset.accounts[0].ai_send(), AiSend::Confirm);
        for (text, expected) in [
            ("confirm", AiSend::Confirm),
            ("drafts", AiSend::Drafts),
            ("client", AiSend::Client),
        ] {
            let line = format!("ai_send = \"{text}\"\n");
            let config = parse(&account(&line), Path::new("c.toml")).unwrap();
            assert_eq!(config.accounts[0].ai_send(), expected);
            assert_eq!(expected.as_str(), text);
        }
    }

    #[test]
    fn from_name_loads_with_spaces_and_refuses_controls_and_length() {
        let config = with_account("", "from_name = \"Dana Reyes\"");
        assert_eq!(config.accounts[0].from_name.as_deref(), Some("Dana Reyes"));
        assert_eq!(with_account("", "").accounts[0].from_name, None);
        assert_eq!(
            with_account("", "from_name = \"\"").accounts[0].from_name,
            None
        );
        let path = std::path::Path::new("c.toml");
        let account = |line: &str| {
            format!(
                "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{line}\n"
            )
        };
        for bad in [
            "from_name = \"Dana\\tReyes\"",
            "from_name = \"Dana\\u202EReyes\"",
        ] {
            let err = crate::config::parse(&account(bad), path).unwrap_err();
            assert!(err.to_string().contains("from_name"), "{err}");
        }
        let long = format!("from_name = \"{}\"", "a".repeat(MAX_FROM_NAME_CHARS + 1));
        let err = crate::config::parse(&account(&long), path).unwrap_err();
        assert!(err.to_string().contains("at most 200"), "{err}");
        let fits = format!("from_name = \"{}\"", "a".repeat(MAX_FROM_NAME_CHARS));
        assert!(crate::config::parse(&account(&fits), path).is_ok());
    }

    #[test]
    fn an_unknown_ai_send_value_reports_its_line() {
        use crate::config::{ConfigError, parse};
        use std::path::Path;
        let text = "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\nai_send = \"always\"\n";
        let err = parse(text, Path::new("/x/config.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { line: 6, .. }), "{err}");
        assert!(err.to_string().contains("always"), "{err}");
    }

    #[test]
    fn the_trash_prompt_names_each_providers_trash() {
        assert_eq!(
            AccountKind::Gmail.trash_note(),
            "Gmail keeps trashed mail in its Trash for 30 days."
        );
        assert_eq!(AccountKind::M365.trash_note(), "Moves to Deleted Items.");
    }

    #[test]
    fn each_kind_names_its_drafts_folder() {
        assert_eq!(AccountKind::Gmail.drafts_name(), "Gmail's Drafts");
        assert_eq!(AccountKind::M365.drafts_name(), "Outlook's Drafts");
    }
}
