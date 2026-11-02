//! The mail screen: the header (the scope, the search slot, the sync summary, the clock), the
//! sidebar of merged mailboxes, accounts and folders, the thread list or the Drafts pane, the
//! reader, the hint bar, and the modals over them. Keys go through the keymap; every read and
//! daemon call is a job whose answer comes back as a `MailEvent`.

mod actions;
mod actions_pick;
mod attachments;
mod bulk;
mod clock_toggle;
mod composing;
mod composing_answers;
mod composing_flow;
mod dim_read_toggle;
mod drafts_flow;
mod drafts_pane;
mod events;
mod header;
#[cfg(test)]
mod header_tests;
mod hints;
mod hints_toggle;
mod images_toggle;
mod keymap_switch;
mod layout;
mod list;
mod list_view;
mod m365;
#[cfg(test)]
mod modal_tests;
mod palette;
mod reading;
mod rich_html_toggle;
mod saved;
mod search_counts_toggle;
mod selection;
mod sidebar;
#[cfg(test)]
mod sidebar_tests;
mod sidebar_tree;
#[cfg(test)]
mod sidebar_tree_tests;
mod sidebar_view;
mod snooze_flow;
#[cfg(test)]
mod snooze_tests;
mod status;
#[cfg(test)]
mod tests;
mod themes;
mod thread_order_toggle;
mod time_toggle;
mod view;
#[cfg(test)]
mod view_tests;

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use rail_core::config::CompiledLinkAction;
use rail_core::config::SavedSearch;
use rail_core::config::{AccountKind, Config, DimRead, Icons, ThreadOrder};
use rail_core::ops::OpError;
use rail_core::paths::Paths;
use rail_core::store::filter::{Filter, parse};
use rail_core::store::view::PageAt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
pub(crate) use actions::UNDO_TICKS;
#[cfg(test)]
pub use list::MAX_WINDOW;
pub use list::{ListWindow, Place};
pub use sidebar::Sidebar;
pub use status::{DaemonState, StatusBar, Summary, error_line};
pub use view::Glyphs;
pub(crate) use view::visible_offset;

use crate::tui::app::{Effect, Msg, TaskResult};
use crate::tui::clock::Clock;
use crate::tui::data::{DaemonLink, ListRequest, MailEvent, ScopeChoice, StoreReader};
use crate::tui::effects::Opener;
use crate::tui::keys::{Action, Chord, Keymap, Resolved, View};
use crate::tui::modals::{FilterLine, Help, Modal, Palette};
use crate::tui::open::{OpenFiles, OpenedAttachments, older_than, sweep_dir};
use crate::tui::screen::Screen;
use crate::tui::screens::compose::Composer;
use crate::tui::screens::reader::Reader;
use crate::tui::theme::Theme;
use crate::tui::theme::choice::{ColorDepth, ThemeChoice};

/// Rows the list is assumed to show before the first frame says.
const FALLBACK_HEIGHT: usize = 20;
/// Compose files older than this at start are a crash's leftovers.
const COMPOSE_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(7 * 86_400);

/// What the mail screen needs from the outside world.
#[derive(Clone)]
pub struct MailContext {
    pub paths: Paths,
    pub keymap: Arc<Keymap>,
    /// The preset `keymap` was built from (`general.keymap`); `:keymap` replaces both.
    pub keymap_preset: rail_core::config::KeymapPreset,
    pub store: StoreReader,
    pub link: DaemonLink,
    /// Starts the browser (`o`, `gx`).
    pub opener: Opener,
    pub clock: Clock,
    pub icons: Icons,
    pub split_threshold: u16,
    /// `[[link_actions]]`, compiled once: the rules `list.open_link` runs over the subject
    /// of the thread under the cursor.
    pub link_actions: Vec<CompiledLinkAction>,
    /// `general.bulk_confirm`: how many threads an archive, star or mark-read may cover
    /// before Rail asks first; `0` never asks.
    pub bulk_confirm: u32,
    /// `general.hints`: the key hints in the bottom bar.
    pub hints: bool,
    /// `general.clock`: the time of day at the right edge of the header.
    pub show_clock: bool,
    /// `general.thread_order`: how the reader runs a thread's older messages; `:thread-order`
    /// flips it for the session.
    pub thread_order: ThreadOrder,
    /// `general.rich_html`: HTML bodies drawn with their styles; `:rich-html` flips it.
    pub rich_html: bool,
    /// `general.images`: rich bodies' pictures drawn; `:images` flips it.
    pub images: bool,
    /// `general.remote_images`: remote pictures fetched without the key; `:remote-images`
    /// flips it.
    pub remote_images: bool,
    /// `general.remote_images_from`: senders whose remote pictures are always fetched; `I`
    /// adds or removes the cursor message's sender.
    pub remote_images_from: Vec<String>,
    /// How this terminal draws pictures, asked once at start; `None` when it was not asked.
    pub picker: Option<ratatui_image::picker::Picker>,
    /// How read list rows are dimmed; `:dim-read` cycles the modes.
    pub dim_read: DimRead,
    /// The terminal shows OSC 8 hyperlinks.
    pub hyperlinks: bool,
    /// What the terminal can show; the theme picker needs it to preview.
    pub depth: ColorDepth,
    /// The scheme and accent in force, which the pickers change one at a time.
    pub theme_choice: ThemeChoice,
    /// The editor's command line, the compose file's path appended when it runs.
    pub editor: Vec<String>,
    /// Each account's signature (in `config.toml`'s order): the file's text, why it could not
    /// be read, or `None` when none is set.
    pub signatures: Vec<Option<Result<String, String>>>,
    /// Where `s`, `S` and `w` save attachments, resolved once at startup from
    /// `[general] attachment_dir` and the user's download directory. `None` means neither
    /// answered, and the three keys say so instead of sending anything.
    pub attachment_dir: Option<PathBuf>,
    /// `[[saved_searches]]` in config order; the ops answer a whole new list.
    pub saved_searches: Vec<SavedSearch>,
    /// `general.saved_search_counts`: the unread count beside each saved search.
    pub saved_search_counts: bool,
}

/// One configured account, in `config.toml`'s order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountEntry {
    pub name: String,
    /// What the screen calls the account: `label` from config when set and not blank, else
    /// `name`. Outside text (the user's config): escaped when drawn.
    pub label: String,
    pub address: String,
    pub kind: AccountKind,
    /// `None` takes the theme's colour by position.
    pub color: Option<Color>,
}

impl AccountEntry {
    /// The sidebar badge's letter: the label's first character, upper-cased, escaped and cut to
    /// one cell.
    pub fn badge(&self) -> String {
        let first = self.label.graphemes(true).next().unwrap_or_default();
        crate::tui::text::fit(&first.to_uppercase(), 1)
    }

    /// The configured accounts; `color` read as a ratatui colour name or `#rrggbb`, else `None`
    /// (the theme's colour by position).
    pub fn from_config(config: &Config) -> Vec<AccountEntry> {
        config
            .accounts
            .iter()
            .map(|account| AccountEntry {
                name: account.name.clone(),
                label: account
                    .label
                    .clone()
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| account.name.clone()),
                address: account.address.to_ascii_lowercase(),
                kind: account.kind,
                color: account
                    .color
                    .as_deref()
                    .and_then(|c| c.parse::<Color>().ok()),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Sidebar,
    Reader,
    /// The composer's review or prompts, over the list or the reader.
    Compose,
}

/// What the list area shows instead of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreState {
    Loading,
    Ready,
    NotSetUp,
    Failed(OpError),
}

pub struct MailScreen {
    ctx: MailContext,
    accounts: Vec<AccountEntry>,
    account_ids: Vec<Option<i64>>,
    scope: ScopeChoice,
    filter: Filter,
    /// The filter line's text, kept while the line is closed.
    filter_text: String,
    /// The newest filter keystroke: only its timer reads.
    filter_token: u64,
    list: ListWindow,
    /// The list's date groups, kept beside the rows.
    list_groups: list::GroupCache,
    /// Where the selection goes when the newest list read answers.
    place: Place,
    sidebar: Sidebar,
    status: StatusBar,
    pub(crate) focus: Focus,
    /// Set by `b` and `Tab`; by the width until then.
    sidebar_shown: Option<bool>,
    store_state: StoreState,
    /// The list area's rows and the frame's width, from the last frame drawn.
    size: Cell<(u16, u16)>,
    /// The first thread the last list frame drew (a window index), for the re-read.
    drawn_top: Cell<Option<usize>>,
    /// The thread list's rows in the last frame drawn (threads and group rows), for a page.
    list_rows: Cell<usize>,
    /// The header's search slot from the last frame drawn, when it was drawn.
    slot_rect: Cell<Option<Rect>>,
    /// The newest list read that replaces the window (a scope change, `Home`, `End`) while it
    /// is in flight: a re-query reuses it instead of overriding it.
    navigating: Option<(PageAt, Place)>,
    /// The open thread, drawn in the pane or full screen.
    reader: Option<Reader>,
    modal: Option<Modal>,
    pub(crate) actions: actions::ActionState,
    /// What the three bulk-select keys keep between presses.
    pub(crate) bulk: bulk::BulkState,
    /// The theme kept and, while `:theme` or `:accent` is open, the one to restore.
    pub(crate) themes: themes::ThemeState,
    /// The key hints are shown: from the config, then whatever `:hints` chose this session.
    hints: bool,
    /// The header's clock is shown: from the config, then whatever `:clock` chose this session.
    show_clock: bool,
    /// The saved searches, from the config and then from every `SavedSearchesChanged`.
    saved: Vec<SavedSearch>,
    /// Saved-search counts are shown: from the config, then whatever `:search-counts` chose.
    search_counts: bool,
    /// The HTML files `o` wrote this session, removed when the reader closes and on drop.
    open_files: OpenFiles,
    /// The browser confirmation was answered yes this session.
    html_confirmed: bool,
    /// What the attachment keys are waiting on.
    attachment: attachments::AttachmentState,
    /// The copies `attachment.open` wrote this session, removed `OPENED_GRACE` after the reader
    /// closes (on a later tick) and at once on drop, so quitting takes them too.
    opened_attachments: OpenedAttachments,
    /// The message `o` is waiting on a body fetch for.
    pending_open: Option<i64>,
    /// The message being written, drawn between the header and the hint bar.
    compose: Option<Composer>,
    composing: composing::ComposeState,
    /// The Drafts folder's pane and what its keys wait on.
    drafts: drafts_flow::DraftsState,
    seq: u64,
    latest_list: u64,
    /// The newest thread read: an older answer is dropped.
    latest_thread: u64,
    latest_sidebar: u64,
    requery_token: u64,
    /// A re-query timer is armed.
    requery_pending: bool,
    /// The running refresh's sequence number: one refresh at a time, ended by its `RefreshEnded`.
    refreshing: Option<u64>,
    /// The store changed while a refresh ran: run once more when it ends.
    dirty_again: bool,
    chord: Chord,
    tick: usize,
    panic_on_key: bool,
}

impl MailScreen {
    /// The dot colour of account `index`: its own, or the theme's by position.
    pub(super) fn account_color(&self, index: usize, theme: &Theme) -> Color {
        self.accounts
            .get(index)
            .and_then(|a| a.color)
            .unwrap_or_else(|| theme.account_color(index))
    }

    pub fn new(ctx: MailContext, accounts: Vec<AccountEntry>) -> Self {
        let open_files = OpenFiles::new(&ctx.paths.cache_dir);
        let themes = themes::ThemeState::new(ctx.theme_choice);
        let hints = ctx.hints;
        let show_clock = ctx.show_clock;
        let saved = ctx.saved_searches.clone();
        let search_counts = ctx.saved_search_counts;
        Self {
            ctx,
            account_ids: vec![None; accounts.len()],
            accounts,
            scope: ScopeChoice::Unified,
            filter: parse(""),
            filter_text: String::new(),
            filter_token: 0,
            list: ListWindow::default(),
            list_groups: list::GroupCache::default(),
            place: Place::Top,
            sidebar: Sidebar::default(),
            status: StatusBar::default(),
            focus: Focus::List,
            sidebar_shown: None,
            store_state: StoreState::Loading,
            size: Cell::new((0, 0)),
            drawn_top: Cell::new(None),
            list_rows: Cell::new(0),
            slot_rect: Cell::new(None),
            navigating: None,
            reader: None,
            modal: None,
            actions: actions::ActionState::default(),
            bulk: bulk::BulkState::default(),
            themes,
            hints,
            show_clock,
            saved,
            search_counts,
            open_files,
            html_confirmed: false,
            attachment: attachments::AttachmentState::default(),
            opened_attachments: OpenedAttachments::default(),
            pending_open: None,
            compose: None,
            composing: composing::ComposeState::default(),
            drafts: drafts_flow::DraftsState::default(),
            seq: 0,
            latest_list: 0,
            latest_thread: 0,
            latest_sidebar: 0,
            requery_token: 0,
            requery_pending: false,
            refreshing: None,
            dirty_again: false,
            chord: Chord::default(),
            tick: 0,
            panic_on_key: false,
        }
    }

    /// Debug builds' test hook (`RAIL_TEST_MAIL_PANIC`): panic on the first key, so a binary
    /// test can check that the terminal is restored.
    pub fn with_panic_on_key(mut self, on: bool) -> Self {
        self.panic_on_key = on;
        self
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn height(&self) -> usize {
        match self.size.get().0 {
            0 => FALLBACK_HEIGHT,
            rows => usize::from(rows),
        }
    }

    /// Shown when set by `b`/`Tab`, else from the last frame's width.
    fn sidebar_visible(&self) -> bool {
        self.sidebar_shown
            .unwrap_or_else(|| layout::sidebar_by_default(self.size.get().1))
    }

    /// The screen's only `ListRequest` literal: what it reads the store with, whichever read
    /// asks. A second copy would let the list and a select-all drift apart.
    fn request(&self, seq: u64, at: PageAt) -> ListRequest {
        ListRequest {
            seq,
            scope: self.scope.clone(),
            addresses: self.accounts.iter().map(|a| a.address.clone()).collect(),
            names: self.accounts.iter().map(|a| a.name.clone()).collect(),
            filter: self.filter.clone(),
            saved: self.saved_filter(),
            saved_all: self.saved_filters(),
            at,
            utc_offset: self.ctx.clock.utc_offset(),
        }
    }

    fn list_request(&mut self, at: PageAt, place: Place) -> ListRequest {
        let seq = self.next_seq();
        self.latest_list = seq;
        self.place = place;
        self.request(seq, at)
    }

    /// The request a select-all reads with. It leaves `place` alone, so a list read in flight
    /// is not cancelled; `at` is unused by the query.
    fn match_request(&mut self) -> ListRequest {
        let seq = self.next_seq();
        self.bulk.latest_match = seq;
        self.bulk.match_shown = Some(self.shown());
        self.request(seq, PageAt::First)
    }

    /// The open scope's saved filter, re-derived on every read and never cached (FR-10). The
    /// scope's position is trusted only while it still carries the scope's name; otherwise the
    /// search is found by that name, and `None` once it is gone — which `list_query` reads as
    /// "no rows", never as "every thread".
    fn saved_filter(&self) -> Option<Filter> {
        match &self.scope {
            ScopeChoice::Saved { index, name } => {
                self.saved_entry(*index, name).map(|s| parse(&s.query))
            }
            _ => None,
        }
    }

    /// Every saved search's filter, in config order; empty while the counts are off (FR-19).
    fn saved_filters(&self) -> Vec<Filter> {
        if self.search_counts {
            self.saved.iter().map(|s| parse(&s.query)).collect()
        } else {
            Vec::new()
        }
    }

    fn read_list(&mut self, at: PageAt, place: Place) -> Effect {
        self.navigating = matches!(at, PageAt::First | PageAt::Last).then_some((at, place));
        let request = self.list_request(at, place);
        self.ctx.store.list(request)
    }

    fn read_sidebar(&mut self) -> Effect {
        let seq = self.next_seq();
        self.latest_sidebar = seq;
        let addresses = self.accounts.iter().map(|a| a.address.clone()).collect();
        let names = self.accounts.iter().map(|a| a.name.clone()).collect();
        self.ctx.store.sidebar(
            seq,
            addresses,
            names,
            self.saved_filters(),
            self.ctx.clock.utc_offset(),
        )
    }

    /// Show `scope`. The old rows stay drawn until the new ones arrive (spec §3); a page
    /// read of the old scope still in flight is dropped by its sequence number. A `Saved`
    /// scope is re-keyed by name first, so every way of opening one — a sidebar row, a
    /// palette entry, FR-17's re-find — lands on the search it named or on All inboxes.
    fn set_scope(&mut self, scope: ScopeChoice) -> Vec<Effect> {
        let scope = self.resolve_saved_scope(scope);
        self.sidebar.point_at(&scope);
        self.scope = scope;
        self.list.cancel_loading();
        if self.in_drafts() {
            return self.open_drafts_pane();
        }
        vec![self.read_list(PageAt::First, Place::Top)]
    }

    /// The views keys are read in, most specific first.
    pub(crate) fn views(&self) -> [View; 2] {
        let view = match self.focus {
            Focus::List => View::List,
            Focus::Sidebar => View::Sidebar,
            Focus::Reader => View::Reader,
            Focus::Compose => View::Compose,
        };
        [view, View::Global]
    }

    fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if self.panic_on_key {
            panic!("rail test panic in the mail screen");
        }
        if let Some(modal) = self.modal.as_mut() {
            let outcome = modal.key(key);
            return self.modal_outcome(outcome);
        }
        if self.focus == Focus::Compose {
            return self.compose_key(key);
        }
        let views = self.views();
        match self.ctx.keymap.resolve(&views, &mut self.chord, key) {
            Resolved::Action(action) => self.act(action),
            Resolved::Pending | Resolved::Unbound => Vec::new(),
        }
    }

    pub(crate) fn act(&mut self, action: Action) -> Vec<Effect> {
        if !matches!(action, Action::ListExtendDown | Action::ListExtendUp) {
            // Any other key ends a Shift range; what it selected stays selected.
            self.bulk.extending = None;
        }
        if let Some(effects) = self.drafts_action(action) {
            return effects;
        }
        match action {
            // The shell quits on its own key; this is `q` read after a chord's first key.
            Action::GlobalQuit => return vec![Effect::Quit],
            Action::GlobalPalette => {
                self.modal = Some(Modal::Palette(Palette::new(self.palette_entries())));
                return Vec::new();
            }
            Action::GlobalHelp => {
                let help = Help::new(&self.ctx.keymap, &self.views(), self.ctx.icons);
                self.modal = Some(Modal::Help(help));
                return Vec::new();
            }
            Action::ListFilter if self.focus == Focus::List => {
                self.modal = Some(Modal::Filter(FilterLine::new(&self.filter_text)));
                return Vec::new();
            }
            Action::ListSaveSearch if self.focus == Focus::List => {
                return self.save_search_prompt();
            }
            _ => {}
        }
        if let Some(effects) = self.compose_action(action) {
            return effects;
        }
        if let Some(effects) = self.mail_action(action) {
            return effects;
        }
        if let Some(effects) = self.shared_action(action) {
            return effects;
        }
        match self.focus {
            Focus::List => self.list_action(action),
            Focus::Sidebar => self.sidebar_action(action),
            Focus::Reader => self.reader_action(action),
            Focus::Compose => self.compose_act(action),
        }
    }

    /// Actions of both the list and the sidebar.
    fn shared_action(&mut self, action: Action) -> Option<Vec<Effect>> {
        if matches!(self.focus, Focus::Reader | Focus::Compose) {
            // Only the refresh key works in the reader: the status bar offers it after the
            // daemon link is lost, whatever is on screen.
            return (action == Action::ListRefresh).then(|| self.refresh());
        }
        let effects = match action {
            Action::ListSidebar => {
                let shown = !self.sidebar_visible();
                self.sidebar_shown = Some(shown);
                if !shown {
                    self.focus = Focus::List;
                }
                Vec::new()
            }
            Action::SidebarFocus => {
                self.sidebar_shown = Some(true);
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::List,
                    _ => Focus::Sidebar,
                };
                Vec::new()
            }
            Action::ListUnified => self.set_scope(ScopeChoice::Unified),
            Action::ListRefresh => self.refresh(),
            other => return palette::account_number(other).map(|n| self.go_to_account(n)),
        };
        Some(effects)
    }

    fn go_to_account(&mut self, number: usize) -> Vec<Effect> {
        if number > self.accounts.len() {
            self.status
                .notify(format!("There is no account {number}"), self.tick);
            return Vec::new();
        }
        self.set_scope(ScopeChoice::Account(number - 1))
    }

    fn list_action(&mut self, action: Action) -> Vec<Effect> {
        let height = self.height();
        let top = self.drawn_top.get().unwrap_or(self.list.offset());
        let wanted = match action {
            Action::ListOpen => return self.open_selected(),
            Action::ListDown => self.list.move_by(1, height),
            Action::ListUp => self.list.move_by(-1, height),
            Action::ListPageDown => self.page(true, top, height),
            Action::ListPageUp => self.page(false, top, height),
            Action::ListTop => self.list.top(height),
            Action::ListBottom => self.list.bottom(height),
            _ => None,
        };
        match wanted {
            Some(at) => {
                let place = match at {
                    PageAt::First => Place::Top,
                    PageAt::Last => Place::Bottom,
                    _ => Place::Keep,
                };
                vec![self.read_list(at, place)]
            }
            None => Vec::new(),
        }
    }

    /// A page of the list from a screen starting at thread `top` that showed `threads`
    /// threads, each screen worked out from the window's groups and the pane's rows.
    fn page(&mut self, forward: bool, top: usize, threads: usize) -> Option<PageAt> {
        let groups = self.list_groups.groups(&self.list, &self.ctx.clock);
        let lines = list_view::list_lines(&groups);
        let rows = match self.list_rows.get() {
            0 => FALLBACK_HEIGHT,
            rows => rows,
        };
        self.list.page(forward, top, threads, |start| {
            list_view::last_thread_from(&lines, start, rows)
        })
    }

    fn sidebar_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::SidebarDown => self.sidebar.down(),
            Action::SidebarUp => self.sidebar.up(),
            Action::SidebarPageDown => self.sidebar.page_down(),
            Action::SidebarPageUp => self.sidebar.page_up(),
            Action::SidebarFold => self.sidebar.toggle_fold(),
            Action::SidebarRenameSearch => return self.rename_cursor_search(),
            Action::SidebarRemoveSearch => return self.remove_cursor_search(),
            Action::SidebarOpen => {
                if let Some(scope) = self.sidebar.open() {
                    self.focus = Focus::List;
                    return self.set_scope(scope);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// `Ctrl-r`: sync now and re-read when connected; subscribe again only after the daemon
    /// link reported itself lost (a live watch would forward every update twice); while the
    /// first subscription is still being made there is nothing to ask the daemon.
    fn refresh(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        match self.status.daemon {
            DaemonState::Connected => effects.push(self.ctx.link.refresh()),
            DaemonState::Unreachable | DaemonState::Reachable => {
                self.status.connecting();
                effects.push(self.ctx.link.watch());
            }
            DaemonState::Connecting => {}
        }
        effects.extend(self.requery());
        effects
    }

    /// The notice, then each account, as text: what the hint bar shows without the hints;
    /// for tests.
    #[cfg(test)]
    pub(crate) fn status_line(&self) -> String {
        self.status
            .line(&self.accounts, &Glyphs::of(self.ctx.icons))
    }

    fn apply_filter(&mut self) -> Vec<Effect> {
        self.filter = parse(&self.filter_text);
        self.list.cancel_loading();
        vec![self.read_list(PageAt::First, Place::Top)]
    }
}

impl Screen for MailScreen {
    fn start(&mut self) -> Vec<Effect> {
        match self.open_files.sweep(std::time::SystemTime::now()) {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "removed message files a previous session left"),
            Err(error) => {
                tracing::warn!(error = %error, "could not clean the opened-messages directory");
            }
        }
        let cutoff = older_than(std::time::SystemTime::now(), COMPOSE_STALE_AFTER);
        match sweep_dir(&self.ctx.paths.compose_dir(), cutoff, Some("eml")) {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "removed compose files a previous session left"),
            Err(error) => {
                tracing::warn!(kind = ?error.kind(), "could not clean the compose directory");
            }
        }
        if self.ctx.depth == ColorDepth::Ansi && self.themes.current.scheme.is_some() {
            self.status.notify(themes::NEEDS_TRUECOLOR, self.tick);
        }
        self.load_last_snooze();
        vec![
            self.ctx.link.watch(),
            self.read_list(PageAt::First, Place::Top),
            self.read_sidebar(),
        ]
    }

    fn update(&mut self, msg: Msg) -> Vec<Effect> {
        let focus = self.focus;
        let effects = match msg {
            Msg::Key(key) => self.on_key(key),
            Msg::Tick => {
                self.tick = self.tick.wrapping_add(1);
                self.status.tick(self.tick);
                self.opened_attachments.expire(self.tick);
                Vec::new()
            }
            Msg::Resize(..) => Vec::new(),
            Msg::Task(TaskResult::Mail(event)) => self.mail_event(event),
            Msg::Task(TaskResult::EditorDone(result)) => {
                self.mail_event(MailEvent::EditorDone(result))
            }
            Msg::Task(TaskResult::JobPanicked(message)) => self.job_panicked(&message),
            Msg::Task(_) => Vec::new(),
        };
        if self.focus != focus {
            // A chord's first key belongs to the view it was typed in.
            self.chord.clear();
        }
        effects
    }

    fn view(&self, frame: &mut Frame, theme: &Theme) {
        self.render(frame, theme);
    }

    fn captures_text(&self) -> bool {
        match &self.modal {
            Some(modal) => modal.captures_text(),
            None => self.compose.as_ref().is_some_and(Composer::captures_text),
        }
    }

    /// The active view's own keys and a waiting chord are the screen's; the global quit key
    /// is the shell's unless the view binds it (`q` closes the reader).
    fn claims_key(&self, key: KeyEvent) -> bool {
        if self.modal.is_some() || self.chord.is_pending() || self.captures_text() {
            return true;
        }
        let mut chord = self.chord;
        let [view, _] = self.views();
        !matches!(
            self.ctx.keymap.resolve(&[view], &mut chord, key),
            Resolved::Unbound
        )
    }
}
