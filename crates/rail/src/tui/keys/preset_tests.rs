//! Keymap presets: each is free of clashes, `keymap` picks one, `[keys]` goes over it, and
//! `F2` opens the palette in every one of them.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::KeymapPreset;

use super::*;

const PATH: &str = "/home/me/.config/rail/config.toml";

fn configured(text: &str) -> Result<Keymap, KeymapError> {
    let path = Path::new(PATH);
    let config = rail_core::config::parse(text, path).expect("valid TOML");
    Keymap::from_config(&config.keys, config.general.keymap, text, path)
}

fn resolve(map: &Keymap, view: View, keys: &[KeyEvent]) -> Resolved {
    let mut chord = Chord::default();
    let mut last = Resolved::Unbound;
    for key in keys {
        last = map.resolve(&[view, View::Global], &mut chord, *key);
    }
    last
}

fn char_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

const F2: KeyEvent = KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE);

#[test]
fn every_preset_parses_and_no_view_clashes() {
    for &preset in KeymapPreset::ALL {
        let text = format!("[general]\nkeymap = \"{}\"\n", preset.as_str());
        let map = configured(&text).unwrap_or_else(|e| panic!("{}: {e}", preset.as_str()));
        for (action, keys) in overrides(preset) {
            assert_eq!(
                map.bindings(*action).len(),
                keys.len(),
                "{}: {} has a key Rail cannot read",
                preset.as_str(),
                action.id()
            );
        }
    }
}

#[test]
fn rail_is_the_default_and_leaves_every_key_as_it_was() {
    let map = configured("").unwrap();
    assert_eq!(
        resolve(&map, View::List, &[char_key('h')]),
        Resolved::Action(Action::ListSnooze)
    );
    assert_eq!(
        resolve(&map, View::List, &[char_key('u')]),
        Resolved::Action(Action::ListRead)
    );
    assert_eq!(
        resolve(&map, View::List, &[char_key('z')]),
        Resolved::Action(Action::GlobalUndo)
    );
    assert_eq!(
        resolve(&map, View::List, &[char_key('D')]),
        Resolved::Unbound
    );
    assert!(
        map.bindings(Action::ListTrashNow).is_empty(),
        "no key in the default keymap"
    );
}

#[test]
fn the_vim_preset_moves_panes_to_h_and_l_and_undo_to_u() {
    let map = configured("[general]\nkeymap = \"vim\"\n").unwrap();
    let cases = [
        (View::List, char_key('l'), Action::ListOpen),
        (View::List, char_key('h'), Action::SidebarFocus),
        (View::Sidebar, char_key('l'), Action::SidebarOpen),
        (View::Sidebar, char_key('h'), Action::SidebarFocus),
        (View::Reader, char_key('h'), Action::ReaderBack),
        (View::Reader, char_key('l'), Action::ReaderUnfold),
        (View::List, char_key('Z'), Action::ListSnooze),
        (View::Reader, char_key('Z'), Action::ListSnooze),
        (View::List, char_key('L'), Action::ListLabels),
        (View::List, char_key('u'), Action::GlobalUndo),
        (View::Reader, char_key('u'), Action::GlobalUndo),
        (View::List, char_key('U'), Action::ListRead),
        (View::List, char_key('d'), Action::ListTrash),
        (View::List, char_key('D'), Action::ListTrashNow),
        (View::Reader, char_key('D'), Action::ListTrashNow),
        (View::List, ctrl('f'), Action::ListPageDown),
        (View::List, ctrl('b'), Action::ListPageUp),
        (View::Reader, ctrl('f'), Action::ReaderPageDown),
        (View::Reader, char_key('N'), Action::ReaderPrev),
        (View::Reader, char_key('p'), Action::ReaderPrev),
        (
            View::List,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            Action::SidebarFocus,
        ),
    ];
    for (view, key, action) in cases {
        assert_eq!(
            resolve(&map, view, &[key]),
            Resolved::Action(action),
            "{view:?} {key:?}"
        );
    }
    assert_eq!(
        resolve(&map, View::List, &[char_key('z')]),
        Resolved::Unbound
    );
}

#[test]
fn keys_entries_go_over_the_preset() {
    let map = configured("[general]\nkeymap = \"vim\"\n\n[keys]\n\"list.snooze\" = \"ctrl-z\"\n")
        .unwrap();
    assert_eq!(
        resolve(&map, View::List, &[ctrl('z')]),
        Resolved::Action(Action::ListSnooze)
    );
    assert_eq!(
        resolve(&map, View::List, &[char_key('Z')]),
        Resolved::Unbound
    );
    assert!(map.is_overridden(Action::ListSnooze));
    assert!(
        !map.is_overridden(Action::ListLabels),
        "a preset's keys are not the user's"
    );
}

#[test]
fn a_keys_entry_that_clashes_with_the_preset_names_its_own_line() {
    let text = "[general]\nkeymap = \"vim\"\n\n[keys]\n\"list.star\" = \"h\"\n";
    let error = configured(text).unwrap_err();
    assert_eq!(error.line, 5, "{error}");
    assert!(error.message.contains("\"list.star\""), "{error}");
    assert!(error.message.contains("\"sidebar.focus\""), "{error}");
}

#[test]
fn an_unknown_preset_is_a_config_error() {
    let path = Path::new(PATH);
    let error = rail_core::config::parse("[general]\nkeymap = \"vi\"\n", path).unwrap_err();
    let text = error.to_string();
    assert!(text.contains("config.toml:2:"), "{text}");
    for &preset in KeymapPreset::ALL {
        assert!(text.contains(&format!("`{}`", preset.as_str())), "{text}");
    }
}

#[test]
fn f2_opens_the_palette_in_every_preset_and_after_an_override() {
    for &preset in KeymapPreset::ALL {
        let text = format!("[general]\nkeymap = \"{}\"\n", preset.as_str());
        let map = configured(&text).unwrap();
        for view in View::ALL.into_iter().filter(|v| *v != View::Wizard) {
            assert_eq!(
                resolve(&map, view, &[F2]),
                Resolved::Action(Action::GlobalPalette),
                "{} {view:?}",
                preset.as_str()
            );
        }
    }
    let map = configured("[keys]\n\"global.palette\" = \"alt-x\"\n").unwrap();
    let alt_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
    assert_eq!(
        resolve(&map, View::List, &[alt_x]),
        Resolved::Action(Action::GlobalPalette)
    );
    assert_eq!(
        resolve(&map, View::List, &[F2]),
        Resolved::Action(Action::GlobalPalette)
    );
}

#[test]
fn f2_cannot_be_bound_to_another_action() {
    let error = configured("[keys]\n\"list.star\" = \"f2\"\n").unwrap_err();
    assert_eq!(error.line, 2);
    assert!(
        error.message.contains("F2 always opens the palette"),
        "{error}"
    );
}

/// `:keymap` shows `<name> — <summary>`: each line fits the picker inside its border, so no
/// preset's summary is cut short.
#[test]
fn every_preset_line_fits_the_keymap_picker() {
    let inside = usize::from(crate::tui::modals::PICKER_WIDTH) - 2;
    for &preset in KeymapPreset::ALL {
        let line = preset.label();
        assert!(crate::tui::text::width(&line) <= inside, "{line}");
    }
}

#[test]
fn the_mutt_preset_follows_mutts_index_and_pager_keys() {
    let map = configured("[general]\nkeymap = \"mutt\"\n").unwrap();
    let chord = |a: char, b: char| [char_key(a), char_key(b)];
    let cases = [
        (View::List, vec![char_key('d')], Action::ListTrash),
        (View::List, vec![char_key('D')], Action::ListTrashNow),
        (View::List, vec![char_key('u')], Action::GlobalUndo),
        (View::List, vec![char_key('N')], Action::ListRead),
        (View::List, vec![char_key('s')], Action::ListMove),
        (View::List, vec![char_key('F')], Action::ListStar),
        (View::List, vec![char_key('m')], Action::ListCompose),
        (View::List, vec![char_key('c')], Action::ListCompose),
        (View::List, vec![char_key('g')], Action::ListReplyAll),
        (View::Reader, vec![char_key('g')], Action::ListReplyAll),
        (View::List, vec![char_key('t')], Action::ListSelect),
        (View::List, vec![char_key('l')], Action::ListFilter),
        (View::List, vec![char_key('Y')], Action::ListLabels),
        (View::List, vec![char_key('Z')], Action::ListSnooze),
        (View::List, vec![char_key('$')], Action::ListRefresh),
        (View::List, vec![char_key('=')], Action::ListTop),
        (View::List, vec![char_key('*')], Action::ListBottom),
        (View::List, vec![ctrl('o')], Action::ListOpenLink),
        (View::List, chord('T', 'a').to_vec(), Action::ListSelectAll),
        (
            View::List,
            chord('T', 'f').to_vec(),
            Action::ListSelectSender,
        ),
        (View::Reader, vec![char_key('h')], Action::ReaderHeaders),
        (View::Reader, vec![char_key('i')], Action::ReaderBack),
        (View::Reader, vec![char_key('P')], Action::ReaderLoadImages),
        (View::Reader, vec![char_key('v')], Action::ReaderAttachments),
        (View::Reader, vec![char_key('-')], Action::ReaderPageUp),
        (View::Reader, vec![char_key('=')], Action::ReaderTop),
        (View::Reader, vec![ctrl('b')], Action::ReaderFollowLink),
        (View::Compose, vec![char_key('D')], Action::ComposeRemove),
        (View::Compose, vec![char_key('y')], Action::ComposeSend),
    ];
    for (view, keys, action) in cases {
        assert_eq!(
            resolve(&map, view, &keys),
            Resolved::Action(action),
            "{view:?} {keys:?}"
        );
    }
    // `g` replies to everyone, so nothing may start with it; `*` is the last thread.
    assert_eq!(
        resolve(&map, View::List, &chord('g', 'g')),
        Resolved::Action(Action::ListReplyAll)
    );
    assert_eq!(
        resolve(&map, View::List, &[char_key('h')]),
        Resolved::Unbound
    );
}

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

#[test]
fn the_emacs_preset_adds_emacs_keys_beside_rails_own() {
    let map = configured("[general]\nkeymap = \"emacs\"\n").unwrap();
    let cases = [
        (View::List, vec![ctrl('n')], Action::ListDown),
        (View::List, vec![ctrl('p')], Action::ListUp),
        (View::List, vec![char_key('n')], Action::ListDown),
        (View::List, vec![char_key('p')], Action::ListUp),
        (View::List, vec![ctrl('v')], Action::ListPageDown),
        (View::List, vec![alt('v')], Action::ListPageUp),
        (View::List, vec![alt('<')], Action::ListTop),
        (View::List, vec![alt('>')], Action::ListBottom),
        (View::List, vec![ctrl('s')], Action::ListFilter),
        (View::List, vec![ctrl('g')], Action::ListClearSelection),
        (View::List, vec![alt('x')], Action::GlobalPalette),
        (
            View::List,
            vec![ctrl('x'), char_key('u')],
            Action::GlobalUndo,
        ),
        (
            View::List,
            vec![ctrl('x'), char_key('o')],
            Action::SidebarFocus,
        ),
        (
            View::Sidebar,
            vec![ctrl('x'), char_key('o')],
            Action::SidebarFocus,
        ),
        (View::Sidebar, vec![ctrl('n')], Action::SidebarDown),
        (View::Sidebar, vec![alt('v')], Action::SidebarPageUp),
        (View::Reader, vec![ctrl('n')], Action::ReaderDown),
        (View::Reader, vec![ctrl('v')], Action::ReaderPageDown),
        (View::Reader, vec![alt('>')], Action::ReaderBottom),
        (View::Reader, vec![ctrl('g')], Action::ReaderBack),
        (View::Reader, vec![char_key('n')], Action::ReaderNext),
        (
            View::Reader,
            vec![ctrl('x'), char_key('u')],
            Action::GlobalUndo,
        ),
        (
            View::Compose,
            vec![ctrl('x'), ctrl('s')],
            Action::ComposePush,
        ),
        (
            View::Compose,
            vec![ctrl('x'), char_key('i')],
            Action::ComposeAttach,
        ),
        (
            View::Compose,
            vec![ctrl('x'), char_key('k')],
            Action::ComposeDiscard,
        ),
        (View::Compose, vec![ctrl('g')], Action::ComposeClose),
        (View::Compose, vec![ctrl('p')], Action::ComposeUp),
    ];
    for (view, keys, action) in cases {
        assert_eq!(
            resolve(&map, view, &keys),
            Resolved::Action(action),
            "{view:?} {keys:?}"
        );
    }
    // Every key of Rail's own preset is still there.
    for &action in Action::ALL {
        for name in action.default_keys() {
            assert!(
                map.bindings(action)
                    .iter()
                    .any(|b| Some(*b) == parse_binding(name)),
                "{} lost {name}",
                action.id()
            );
        }
    }
}

#[test]
fn the_gmail_preset_follows_gmails_web_shortcuts() {
    let map = configured("[general]\nkeymap = \"gmail\"\n").unwrap();
    let chord = |a: char, b: char| vec![char_key(a), char_key(b)];
    let cases = [
        (View::List, vec![char_key('o')], Action::ListOpen),
        (View::List, vec![char_key('I')], Action::ListMarkRead),
        (View::List, vec![char_key('U')], Action::ListMarkUnread),
        (View::Reader, vec![char_key('I')], Action::ListMarkRead),
        (View::Reader, vec![char_key('U')], Action::ListMarkUnread),
        (View::List, vec![char_key('#')], Action::ListTrashNow),
        (View::List, vec![char_key('d')], Action::ListTrash),
        (View::List, vec![char_key('v')], Action::ListMove),
        (View::List, vec![char_key('b')], Action::ListSnooze),
        (View::List, vec![char_key('B')], Action::ListSidebar),
        (View::List, vec![char_key('x')], Action::ListSelect),
        (View::List, vec![char_key('V')], Action::ListSelectRange),
        (View::List, vec![char_key('a')], Action::ListReplyAll),
        (View::Reader, vec![char_key('a')], Action::ListReplyAll),
        (View::List, vec![char_key('N')], Action::ListRefresh),
        (View::List, chord('g', 'i'), Action::ListUnified),
        (View::List, chord('*', 'a'), Action::ListSelectAll),
        (View::List, chord('*', 'n'), Action::ListClearSelection),
        (View::List, vec![char_key('z')], Action::GlobalUndo),
        (View::Reader, vec![char_key('u')], Action::ReaderBack),
        (View::Reader, vec![char_key('A')], Action::ReaderAttachments),
        (View::Reader, vec![char_key('T')], Action::ReaderTrustImages),
        (View::Reader, vec![char_key('i')], Action::ReaderLoadImages),
        (View::Compose, vec![ctrl('s')], Action::ComposePush),
    ];
    for (view, keys, action) in cases {
        assert_eq!(
            resolve(&map, view, &keys),
            Resolved::Action(action),
            "{view:?} {keys:?}"
        );
    }
    // Gmail has no read toggle: `u` in the list is free.
    assert_eq!(
        resolve(&map, View::List, &[char_key('u')]),
        Resolved::Unbound
    );
}

#[test]
fn the_outlook_preset_follows_outlooks_shortcuts() {
    let map = configured("[general]\nkeymap = \"outlook\"\n").unwrap();
    let named = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let cases = [
        (
            View::List,
            vec![named(KeyCode::Delete)],
            Action::ListTrashNow,
        ),
        (
            View::Reader,
            vec![named(KeyCode::Delete)],
            Action::ListTrashNow,
        ),
        (
            View::List,
            vec![named(KeyCode::Backspace)],
            Action::ListArchive,
        ),
        (View::List, vec![named(KeyCode::Insert)], Action::ListStar),
        (View::List, vec![ctrl('q')], Action::ListMarkRead),
        (View::List, vec![ctrl('u')], Action::ListMarkUnread),
        (View::Reader, vec![ctrl('u')], Action::ListMarkUnread),
        (View::List, vec![ctrl('z')], Action::GlobalUndo),
        (View::List, vec![ctrl('n')], Action::ListCompose),
        (View::Reader, vec![char_key('n')], Action::ReaderNext),
        (View::List, vec![ctrl('r')], Action::ListReply),
        (View::Reader, vec![ctrl('r')], Action::ListReply),
        (View::List, vec![ctrl('f')], Action::ListForward),
        (View::List, vec![char_key('F')], Action::ListForward),
        (View::List, vec![char_key('v')], Action::ListMove),
        (View::List, vec![char_key('V')], Action::ListSelectRange),
        (View::List, vec![ctrl('e')], Action::ListFilter),
        (View::List, vec![ctrl('a')], Action::ListSelectAll),
        (View::List, vec![ctrl('o')], Action::ListOpen),
        (View::List, vec![named(KeyCode::F(9))], Action::ListRefresh),
        (View::List, vec![char_key('d')], Action::ListTrash),
        (View::List, vec![char_key('u')], Action::ListRead),
        (View::Sidebar, vec![ctrl('u')], Action::SidebarPageUp),
        (View::Compose, vec![alt('s')], Action::ComposeSend),
        (View::Compose, vec![ctrl('s')], Action::ComposePush),
    ];
    for (view, keys, action) in cases {
        assert_eq!(
            resolve(&map, view, &keys),
            Resolved::Action(action),
            "{view:?} {keys:?}"
        );
    }
}
