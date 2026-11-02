//! The `:` palette's entries: the active view's key actions and the global ones, Rail's own
//! commands, each account, merged mailbox and label.

use rail_core::config::{DimRead, ThreadOrder, TimeFormat};

use super::MailScreen;
use crate::tui::data::ScopeChoice;
use crate::tui::keys::Action;
use crate::tui::modals::{PaletteEntry, Target};

impl MailScreen {
    /// Every palette entry: each key action of the active view and the global ones with its
    /// key, `compose`, `drafts`, `errors`, `theme`, `accent`, `keymap`, `hints`, `time`, `thread-order`, `dim-read`,
    /// each account, merged mailbox and label.
    pub(crate) fn palette_entries(&self) -> Vec<PaletteEntry> {
        let key = |a: Action| self.ctx.keymap.label(a);
        let views = self.views();
        let mut entries: Vec<PaletteEntry> = Action::ALL
            .iter()
            // `compose` below stands for `c` in every view, and each saved-search action has
            // a command entry of its own — the rename and remove ones one per search, since
            // the palette has no cursor to say which search an index names.
            .filter(|&&a| {
                !matches!(
                    a,
                    Action::ListCompose
                        | Action::ListSaveSearch
                        | Action::SidebarRenameSearch
                        | Action::SidebarRemoveSearch
                )
            })
            .filter(|a| a.views().iter().any(|v| views.contains(v)))
            .map(|&a| PaletteEntry {
                text: a.description().to_string(),
                search: format!("{} {}", a.description(), a.id()),
                key: key(a),
                target: Target::Action(a),
            })
            .collect();
        entries.extend(self.command_entries());
        entries.extend(self.accounts.iter().enumerate().map(|(i, a)| PaletteEntry {
            text: format!("Go to {}'s inbox", a.label),
            search: format!(
                "Go to {}'s inbox account {} {} {}",
                a.label, a.label, a.name, a.address
            ),
            key: account_action(i + 1).map(key).unwrap_or_default(),
            target: Target::Scope(ScopeChoice::Account(i)),
        }));
        entries.extend(mailbox_entries());
        entries.extend(self.saved_entries());
        entries.extend(self.label_entries());
        entries
    }

    /// `Search: <name>` for each saved search, and the `Rename search:` / `Remove search:`
    /// pair that carries its position. `text` is raw — the draw site escapes it, as it does
    /// every label.
    fn saved_entries(&self) -> Vec<PaletteEntry> {
        let key = |a: Action| self.ctx.keymap.label(a);
        self.saved
            .iter()
            .enumerate()
            .flat_map(|(index, search)| {
                let name = search.name.clone();
                [
                    PaletteEntry {
                        text: format!("Search: {name}"),
                        search: format!("search {name}"),
                        key: String::new(),
                        target: Target::Scope(ScopeChoice::Saved {
                            index,
                            name: name.clone(),
                        }),
                    },
                    PaletteEntry {
                        text: format!("Rename search: {name}"),
                        search: format!("rename-search {name}"),
                        key: key(Action::SidebarRenameSearch),
                        target: Target::RenameSearch {
                            index,
                            name: name.clone(),
                        },
                    },
                    PaletteEntry {
                        text: format!("Remove search: {name}"),
                        search: format!("remove-search {name}"),
                        key: key(Action::SidebarRemoveSearch),
                        target: Target::RemoveSearch { index, name },
                    },
                ]
            })
            .collect()
    }

    /// Rail's own commands; `compose` carries the key of `c`, and `hints`, `time` and
    /// `thread-order` say what they will do.
    fn command_entries(&self) -> Vec<PaletteEntry> {
        let compose = self.ctx.keymap.label(Action::ListCompose);
        let mut commands = vec![
            ("Write a new message", "compose", compose, Target::Compose),
            ("Open a draft", "drafts", String::new(), Target::Drafts),
            (
                "Show recent errors",
                "errors",
                String::new(),
                Target::Errors,
            ),
            (
                "Change the colour theme",
                "theme",
                String::new(),
                Target::Theme,
            ),
            (
                "Change the accent colour",
                "accent",
                String::new(),
                Target::Accent,
            ),
            (
                "Change the key preset",
                "keymap",
                String::new(),
                Target::Keymap,
            ),
        ];
        if self.has_filter_text() {
            commands.push((
                Action::ListSaveSearch.description(),
                "save-search",
                self.ctx.keymap.label(Action::ListSaveSearch),
                Target::SaveSearch,
            ));
        }
        commands.extend(
            self.toggles()
                .map(|(text, id, target)| (text, id, String::new(), target)),
        );
        commands
            .into_iter()
            .map(|(text, id, key, target)| PaletteEntry {
                text: text.into(),
                search: format!("{id} {text}"),
                key,
                target,
            })
            .collect()
    }

    /// Display settings, each saying what it will do next.
    fn toggles(&self) -> [(&'static str, &'static str, Target); 9] {
        let hints = if self.hints {
            "Turn the key hints off"
        } else {
            "Turn the key hints on"
        };
        let time = match self.ctx.clock.format() {
            TimeFormat::H24 => "Show times as 12-hour (am/pm)",
            TimeFormat::H12 => "Show times as 24-hour",
        };
        let order = match self.ctx.thread_order {
            ThreadOrder::Oldest => "Show older messages newest first",
            ThreadOrder::Newest => "Show older messages oldest first",
        };
        let clock = if self.show_clock {
            "Hide the clock"
        } else {
            "Show the clock"
        };
        let dim_read = match self.ctx.dim_read {
            DimRead::Dot => "Dim read subjects",
            DimRead::Subject => "Dim all read row text",
            DimRead::All => "Dim only the read dot",
        };
        let counts = if self.search_counts {
            "Hide saved-search counts"
        } else {
            "Show saved-search counts"
        };
        let rich = if self.ctx.rich_html {
            "Show HTML mail as plain text"
        } else {
            "Show HTML mail with its styles"
        };
        let images = if self.ctx.images {
            "Hide pictures in HTML mail"
        } else {
            "Show pictures in HTML mail"
        };
        let remote = if self.ctx.remote_images {
            "Ask before loading remote pictures"
        } else {
            "Always load remote pictures"
        };
        [
            (dim_read, "dim-read", Target::DimRead),
            (hints, "hints", Target::Hints),
            (time, "time", Target::Time),
            (order, "thread-order", Target::ThreadOrder),
            (clock, "clock", Target::Clock),
            (counts, "search-counts", Target::SearchCounts),
            (rich, "rich-html", Target::RichHtml),
            (images, "images", Target::Images),
            (remote, "remote-images", Target::RemoteImages),
        ]
    }

    /// One entry per folder: a parent row that opens a system folder is not that folder's own.
    fn label_entries(&self) -> impl Iterator<Item = PaletteEntry> + '_ {
        self.sidebar.all_rows().iter().filter_map(|row| {
            let scope = row.scope().filter(|s| row.is_row_of(s))?;
            let ScopeChoice::Folder { account, name, .. } = &scope else {
                return None;
            };
            let entry = self.accounts.get(*account);
            let label = entry.map_or("", |a| a.label.as_str());
            let id = entry.map_or("", |a| a.name.as_str());
            Some(PaletteEntry {
                text: format!("Label: {label} · {name}"),
                search: format!("label {label} {id} {name}"),
                key: String::new(),
                target: Target::Scope(scope.clone()),
            })
        })
    }
}

/// The `list.account_n` action for account number `n` (1 to 9).
fn account_action(n: usize) -> Option<Action> {
    let action = match n {
        1 => Action::ListAccount1,
        2 => Action::ListAccount2,
        3 => Action::ListAccount3,
        4 => Action::ListAccount4,
        5 => Action::ListAccount5,
        6 => Action::ListAccount6,
        7 => Action::ListAccount7,
        8 => Action::ListAccount8,
        9 => Action::ListAccount9,
        _ => return None,
    };
    Some(action)
}

/// The account number of a `list.account_n` action.
pub(super) fn account_number(action: Action) -> Option<usize> {
    let number = match action {
        Action::ListAccount1 => 1,
        Action::ListAccount2 => 2,
        Action::ListAccount3 => 3,
        Action::ListAccount4 => 4,
        Action::ListAccount5 => 5,
        Action::ListAccount6 => 6,
        Action::ListAccount7 => 7,
        Action::ListAccount8 => 8,
        Action::ListAccount9 => 9,
        _ => return None,
    };
    Some(number)
}

/// `Mailbox: Starred` and the other merged mailboxes (`0` reaches All inboxes). `Mailbox:
/// Snoozed` is here whether or not the sidebar lists it.
fn mailbox_entries() -> impl Iterator<Item = PaletteEntry> {
    [
        ScopeChoice::Starred,
        ScopeChoice::Snoozed,
        ScopeChoice::Drafts,
        ScopeChoice::Sent,
        ScopeChoice::Trash,
    ]
    .into_iter()
    .map(|scope| {
        let name = scope.unified_name().unwrap_or_default();
        PaletteEntry {
            text: format!("Mailbox: {name}"),
            search: format!("mailbox {} {name}", name.to_lowercase()),
            key: String::new(),
            target: Target::Scope(scope),
        }
    })
}
