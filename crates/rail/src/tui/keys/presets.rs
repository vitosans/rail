//! Keymap presets: the keys each one gives an action in place of Rail's defaults. An action a
//! preset does not name keeps its default keys, and `[keys]` goes over the preset. Every
//! preset is checked for clashes like `[keys]` is (`preset_tests`).

use rail_core::config::KeymapPreset;

use super::Action;

type Overrides = &'static [(Action, &'static [&'static str])];

/// vim's habits: `h` goes out and `l` goes in (list, sidebar, reader), `u` undoes, and
/// `Ctrl-f`/`Ctrl-b` page. What those keys did moves to capitals: snooze to `Z`, labels to
/// `L`, read or unread to `U`. `D` trashes without asking; `d` still asks.
const VIM: Overrides = &[
    (Action::GlobalUndo, &["u"]),
    (Action::ListOpen, &["enter", "l"]),
    (Action::ListPageDown, &["pagedown", "ctrl-d", "ctrl-f"]),
    (Action::ListPageUp, &["pageup", "ctrl-u", "ctrl-b"]),
    (Action::ListRead, &["U"]),
    (Action::ListLabels, &["L"]),
    (Action::ListSnooze, &["Z"]),
    (Action::ListTrashNow, &["D"]),
    (Action::SidebarFocus, &["tab", "h"]),
    (Action::SidebarOpen, &["enter", "l"]),
    (Action::SidebarPageDown, &["pagedown", "ctrl-d", "ctrl-f"]),
    (Action::SidebarPageUp, &["pageup", "ctrl-u", "ctrl-b"]),
    (Action::ReaderPrev, &["p", "N"]),
    (Action::ReaderUnfold, &["enter", "l"]),
    (Action::ReaderBack, &["q", "esc", "h"]),
    (
        Action::ReaderPageDown,
        &["pagedown", "space", "ctrl-f", "ctrl-d"],
    ),
    (Action::ReaderPageUp, &["pageup", "ctrl-b", "ctrl-u"]),
    (
        Action::ComposePageDown,
        &["pagedown", "space", "ctrl-f", "ctrl-d"],
    ),
    (Action::ComposePageUp, &["pageup", "ctrl-b", "ctrl-u"]),
];

/// mutt's index and pager habits: `s` saves (moves), `F` flags (stars), `N` toggles new
/// (read or unread), `t` tags (selects), `l` limits (filters), `g` replies to everyone, `m`
/// mails, `$` syncs and `u` undoes. `d` still asks, as Rail's does, and `D` trashes without
/// asking. With `g` and `*` single keys, nothing may start with them: top and bottom are `=`
/// and `*`, the links are `Ctrl-o` and `Ctrl-b`, and select all or by sender are `T a` and
/// `T f`. In the reader `h` shows the headers, `i` goes back, `v` lists the attachments and
/// `P` loads the pictures; snooze is `Z` and labels `Y` (neomutt's edit-label).
const MUTT: Overrides = &[
    (Action::GlobalUndo, &["u"]),
    (Action::ListRefresh, &["ctrl-r", "$"]),
    (Action::ListFilter, &["/", "l"]),
    (Action::ListTop, &["home", "="]),
    (Action::ListBottom, &["end", "*"]),
    (Action::ListTrashNow, &["D"]),
    (Action::ListStar, &["F"]),
    (Action::ListRead, &["N"]),
    (Action::ListLabels, &["Y"]),
    (Action::ListMove, &["s"]),
    (Action::ListSnooze, &["Z"]),
    (Action::ListCompose, &["c", "m"]),
    (Action::ListOpenLink, &["ctrl-o"]),
    (Action::ListReplyAll, &["R", "g"]),
    (Action::ListSelect, &["space", "t"]),
    (Action::ListSelectAll, &["T a"]),
    (Action::ListSelectSender, &["T f"]),
    (Action::ReaderHeaders, &["h"]),
    (Action::ReaderAttachments, &["v"]),
    (Action::ReaderLoadImages, &["P"]),
    (Action::ReaderFollowLink, &["ctrl-b"]),
    (Action::ReaderBack, &["q", "esc", "i"]),
    (Action::ReaderPageUp, &["pageup", "-"]),
    (Action::ReaderTop, &["home", "="]),
    (Action::ComposeRemove, &["x", "D"]),
];

/// emacs's habits, added beside Rail's keys rather than in place of them: `Ctrl-n`/`Ctrl-p`
/// move, `Ctrl-v`/`Alt-v` page, `Alt-<`/`Alt->` go to the top and bottom, `Ctrl-s` filters,
/// `Ctrl-g` backs out and `Alt-x` opens the palette. `Ctrl-x` starts the rest: `Ctrl-x u`
/// undoes, `Ctrl-x o` moves between the sidebar and the list, and in the composer `Ctrl-x
/// Ctrl-s` saves to Drafts, `Ctrl-x i` attaches and `Ctrl-x k` discards (asking first). In the
/// list `n` and `p` go to the next and previous thread, as in mu4e and Gnus. `Ctrl-c` quits
/// Rail everywhere, so no `Ctrl-c` chord is bound.
const EMACS: Overrides = &[
    (Action::GlobalPalette, &[":", "f2", "alt-x"]),
    (Action::GlobalUndo, &["z", "ctrl-x u"]),
    (Action::ListFilter, &["/", "ctrl-s"]),
    (Action::ListDown, &["j", "down", "n", "ctrl-n"]),
    (Action::ListUp, &["k", "up", "p", "ctrl-p"]),
    (Action::ListPageDown, &["pagedown", "ctrl-d", "ctrl-v"]),
    (Action::ListPageUp, &["pageup", "ctrl-u", "alt-v"]),
    (Action::ListTop, &["home", "g g", "alt-<"]),
    (Action::ListBottom, &["end", "G", "alt->"]),
    (Action::ListClearSelection, &["esc", "ctrl-g"]),
    (Action::SidebarFocus, &["tab", "ctrl-x o"]),
    (Action::SidebarDown, &["j", "down", "ctrl-n"]),
    (Action::SidebarUp, &["k", "up", "ctrl-p"]),
    (Action::SidebarPageDown, &["pagedown", "ctrl-d", "ctrl-v"]),
    (Action::SidebarPageUp, &["pageup", "ctrl-u", "alt-v"]),
    (Action::ReaderBack, &["q", "esc", "ctrl-g"]),
    (Action::ReaderDown, &["j", "down", "ctrl-n"]),
    (Action::ReaderUp, &["k", "up", "ctrl-p"]),
    (Action::ReaderPageDown, &["pagedown", "space", "ctrl-v"]),
    (Action::ReaderPageUp, &["pageup", "alt-v"]),
    (Action::ReaderTop, &["home", "g g", "alt-<"]),
    (Action::ReaderBottom, &["end", "G", "alt->"]),
    (Action::ComposePush, &["s", "ctrl-x ctrl-s"]),
    (Action::ComposeAttach, &["a", "ctrl-x i"]),
    (Action::ComposeClose, &["q", "esc", "ctrl-g"]),
    (Action::ComposeDiscard, &["d", "ctrl-x k"]),
    (Action::ComposeDown, &["j", "down", "ctrl-n"]),
    (Action::ComposeUp, &["k", "up", "ctrl-p"]),
    (Action::ComposePageDown, &["pagedown", "space", "ctrl-v"]),
    (Action::ComposePageUp, &["pageup", "alt-v"]),
];

/// Gmail's web shortcuts: `o` opens, `u` goes back to the list, `Shift-i`/`Shift-u` mark read
/// and unread, `#` trashes without asking, `v` moves, `b` snoozes, `x` selects, `a` replies to
/// everyone, `Shift-n` syncs, `g i` goes to the inbox and `* n` clears the selection. `Ctrl-s`
/// saves the draft. What those keys did in `rail` moves: the sidebar to `B`, a range to `V`,
/// the attachments to `A` and trusting a sender's pictures to `T`. `d` still asks. Gmail has no
/// read toggle, so `u` in the list does nothing.
const GMAIL: Overrides = &[
    (Action::ListOpen, &["enter", "o"]),
    (Action::ListUnified, &["0", "g i"]),
    (Action::ListRefresh, &["ctrl-r", "N"]),
    (Action::ListSidebar, &["B"]),
    (Action::ListRead, &[]),
    (Action::ListMarkRead, &["I"]),
    (Action::ListMarkUnread, &["U"]),
    (Action::ListTrashNow, &["#"]),
    (Action::ListMove, &["m", "v"]),
    (Action::ListSnooze, &["b"]),
    (Action::ListReplyAll, &["R", "a"]),
    (Action::ListSelect, &["space", "x"]),
    (Action::ListClearSelection, &["esc", "* n"]),
    (Action::ListSelectRange, &["V"]),
    (Action::ReaderAttachments, &["A"]),
    (Action::ReaderTrustImages, &["T"]),
    (Action::ReaderBack, &["q", "esc", "u"]),
    (Action::ComposePush, &["s", "ctrl-s"]),
];

/// Outlook's shortcuts, from Outlook for Windows and on the web: `Delete` trashes without
/// asking, `Backspace` archives, `Insert` flags (stars), `Ctrl-q`/`Ctrl-u` mark read and unread,
/// `Ctrl-z` undoes, `Ctrl-n` writes, `Ctrl-r` replies, `Ctrl-f` or `F` forwards, `v`
/// moves, `Ctrl-e` searches, `Ctrl-a` selects all, `Ctrl-o` opens and `F9` syncs. In the
/// composer `Alt-s` sends and `Ctrl-s` saves to Drafts. What those keys did in `rail` moves:
/// sync from `Ctrl-r` to `F9`, a range from `v` to `V`, and the list's page up loses `Ctrl-u`.
/// `d` still asks. `Ctrl-Shift` keys and `Ctrl-Enter` are left out: most terminals send them
/// as the plain keys.
const OUTLOOK: Overrides = &[
    (Action::GlobalUndo, &["z", "ctrl-z"]),
    (Action::ListOpen, &["enter", "ctrl-o"]),
    (Action::ListFilter, &["/", "ctrl-e"]),
    (Action::ListRefresh, &["f9"]),
    (Action::ListPageUp, &["pageup"]),
    (Action::ListArchive, &["e", "backspace"]),
    (Action::ListTrashNow, &["delete"]),
    (Action::ListStar, &["s", "insert"]),
    (Action::ListMarkRead, &["ctrl-q"]),
    (Action::ListMarkUnread, &["ctrl-u"]),
    (Action::ListMove, &["m", "v"]),
    (Action::ListCompose, &["c", "ctrl-n"]),
    (Action::ListReply, &["r", "ctrl-r"]),
    (Action::ListForward, &["f", "F", "ctrl-f"]),
    (Action::ListSelectAll, &["* a", "ctrl-a"]),
    (Action::ListSelectRange, &["V"]),
    (Action::ComposeSend, &["y", "alt-s"]),
    (Action::ComposePush, &["s", "ctrl-s"]),
];

/// The keys `preset` gives in place of the defaults.
pub fn overrides(preset: KeymapPreset) -> Overrides {
    match preset {
        KeymapPreset::Rail => &[],
        KeymapPreset::Vim => VIM,
        KeymapPreset::Mutt => MUTT,
        KeymapPreset::Emacs => EMACS,
        KeymapPreset::Gmail => GMAIL,
        KeymapPreset::Outlook => OUTLOOK,
    }
}

/// What `:keymap` says about `preset` beside its name: the keys that differ from Rail's most,
/// short enough that the line fits the picker (`preset_tests`).
pub fn summary(preset: KeymapPreset) -> &'static str {
    match preset {
        KeymapPreset::Rail => "Rail's own keys",
        KeymapPreset::Vim => "h/l panes · u undo · U read · Z snooze · D trash",
        KeymapPreset::Mutt => "s move · g reply all · t tag · N read · u undo",
        KeymapPreset::Emacs => "C-n/C-p · C-v/M-v page · C-s filter · C-g back",
        KeymapPreset::Gmail => "I/U read · # trash · v move · b snooze · x select",
        KeymapPreset::Outlook => "Del trash · Bksp archive · Ins flag · C-q read",
    }
}
