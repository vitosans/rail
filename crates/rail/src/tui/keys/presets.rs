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

/// The keys `preset` gives in place of the defaults.
pub fn overrides(preset: KeymapPreset) -> Overrides {
    match preset {
        KeymapPreset::Rail => &[],
        KeymapPreset::Vim => VIM,
    }
}

/// What `:keymap` says about `preset` beside its name: the keys that differ from Rail's most,
/// short enough that the line fits the picker (`preset_tests`).
pub fn summary(preset: KeymapPreset) -> &'static str {
    match preset {
        KeymapPreset::Rail => "Rail's own keys",
        KeymapPreset::Vim => "h/l panes · u undo · U read · Z snooze · D trash",
    }
}
