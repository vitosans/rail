//! The keymap: key names, chords, normalisation, overrides, the config errors and help.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

const PATH: &str = "/home/me/.config/rail/config.toml";

fn configured(text: &str) -> Result<Keymap, KeymapError> {
    let path = Path::new(PATH);
    let config = rail_core::config::parse(text, path).expect("valid TOML");
    Keymap::from_config(&config.keys, config.general.keymap, text, path)
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn with(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn key(code: KeyCode) -> Key {
    Key::new(code, KeyModifiers::NONE)
}

fn resolve(map: &Keymap, views: &[View], keys: &[KeyEvent]) -> Vec<Resolved> {
    let mut chord = Chord::default();
    keys.iter()
        .map(|k| map.resolve(views, &mut chord, *k))
        .collect()
}

#[test]
fn every_default_parses_and_no_view_clashes() {
    let map = Keymap::defaults();
    for &action in Action::ALL {
        assert_eq!(
            map.bindings(action).len(),
            action.default_keys().len(),
            "{} has a default Rail cannot read",
            action.id()
        );
        assert_eq!(Action::from_id(action.id()), Some(action));
    }
    configured("").expect("the defaults are free of clashes");
}

#[test]
fn every_key_name_parses() {
    let cases: &[(&str, Binding)] = &[
        ("a", Binding::Single(key(KeyCode::Char('a')))),
        ("O", Binding::Single(key(KeyCode::Char('O')))),
        ("shift-o", Binding::Single(key(KeyCode::Char('O')))),
        ("/", Binding::Single(key(KeyCode::Char('/')))),
        ("-", Binding::Single(key(KeyCode::Char('-')))),
        ("é", Binding::Single(key(KeyCode::Char('é')))),
        ("enter", Binding::Single(key(KeyCode::Enter))),
        ("Esc", Binding::Single(key(KeyCode::Esc))),
        ("tab", Binding::Single(key(KeyCode::Tab))),
        ("backtab", Binding::Single(key(KeyCode::BackTab))),
        ("shift-tab", Binding::Single(key(KeyCode::BackTab))),
        ("space", Binding::Single(key(KeyCode::Char(' ')))),
        ("up", Binding::Single(key(KeyCode::Up))),
        ("down", Binding::Single(key(KeyCode::Down))),
        ("left", Binding::Single(key(KeyCode::Left))),
        ("right", Binding::Single(key(KeyCode::Right))),
        ("pageup", Binding::Single(key(KeyCode::PageUp))),
        ("pagedown", Binding::Single(key(KeyCode::PageDown))),
        ("home", Binding::Single(key(KeyCode::Home))),
        ("end", Binding::Single(key(KeyCode::End))),
        ("f1", Binding::Single(key(KeyCode::F(1)))),
        ("F12", Binding::Single(key(KeyCode::F(12)))),
        (
            "ctrl-r",
            Binding::Single(Key::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        ),
        (
            "Ctrl-R",
            Binding::Single(Key::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        ),
        (
            "ctrl--",
            Binding::Single(Key::new(KeyCode::Char('-'), KeyModifiers::CONTROL)),
        ),
        (
            "alt-x",
            Binding::Single(Key::new(KeyCode::Char('x'), KeyModifiers::ALT)),
        ),
        (
            "ctrl-alt-enter",
            Binding::Single(Key::new(
                KeyCode::Enter,
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            )),
        ),
        (
            "g x",
            Binding::Chord(key(KeyCode::Char('g')), key(KeyCode::Char('x'))),
        ),
        (
            "ctrl-x ctrl-c",
            Binding::Chord(
                Key::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                Key::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            ),
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(parse_binding(name).as_ref(), Some(expected), "{name:?}");
    }
}

#[test]
fn unreadable_key_names_are_refused() {
    for bad in [
        "",
        " ",
        "ctrl-",
        "shift-",
        "f13",
        "f0",
        "foo",
        "g x y",
        "g  x",
        "shift-1",
        "shift-left",
        "a\u{1b}",
        "\u{7}",
        "ab",
        "\u{ad}",
        "\u{61c}",
        "\u{180e}",
        "\u{fff9}",
        "\u{e0001}",
        "\u{e0041}",
    ] {
        assert_eq!(parse_binding(bad), None, "{bad:?}");
    }
}

#[test]
fn events_are_normalised_like_names() {
    let shifted_o = Key::from_event(with(KeyCode::Char('O'), KeyModifiers::SHIFT));
    assert_eq!(Binding::Single(shifted_o), parse_binding("O").unwrap());
    let backtab = Key::from_event(with(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(Binding::Single(backtab), parse_binding("backtab").unwrap());
    let ctrl_r = Key::from_event(with(
        KeyCode::Char('R'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(Binding::Single(ctrl_r), parse_binding("ctrl-r").unwrap());
    let shift_enter = Key::from_event(with(KeyCode::Enter, KeyModifiers::SHIFT));
    assert_eq!(
        Binding::Single(shift_enter),
        parse_binding("enter").unwrap()
    );
}

#[test]
fn shift_up_and_down_are_keys_of_their_own() {
    let shift = |code| Key::new(code, KeyModifiers::SHIFT);
    assert_eq!(
        parse_binding("shift-up"),
        Some(Binding::Single(shift(KeyCode::Up)))
    );
    assert_eq!(
        parse_binding("Shift-Down"),
        Some(Binding::Single(shift(KeyCode::Down)))
    );
    assert_ne!(shift(KeyCode::Up), key(KeyCode::Up));
    assert_eq!(shift(KeyCode::Up).to_string(), "Shift-↑");
    assert_eq!(shift(KeyCode::Down).to_string(), "Shift-↓");
    // Every other key drops Shift as before.
    assert_eq!(shift(KeyCode::Left), key(KeyCode::Left));
    assert_eq!(shift(KeyCode::PageDown), key(KeyCode::PageDown));
}

#[test]
fn shift_arrows_extend_the_list_and_move_where_nothing_binds_them() {
    let map = Keymap::defaults();
    let down = with(KeyCode::Down, KeyModifiers::SHIFT);
    let up = with(KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(
        resolve(&map, &[View::List, View::Global], &[down, up]),
        [
            Resolved::Action(Action::ListExtendDown),
            Resolved::Action(Action::ListExtendUp)
        ]
    );
    assert_eq!(
        resolve(&map, &[View::Reader, View::Global], &[down]),
        [Resolved::Action(Action::ReaderDown)]
    );
    assert_eq!(
        resolve(&map, &[View::Sidebar, View::Global], &[down]),
        [Resolved::Action(Action::SidebarDown)]
    );
}

#[test]
fn a_view_key_shadows_a_global_one_and_chords_resolve() {
    let map = Keymap::defaults();
    let reader = [View::Reader, View::Global];
    let list = [View::List, View::Global];
    assert_eq!(
        resolve(&map, &reader, &[press(KeyCode::Char('q'))]),
        [Resolved::Action(Action::ReaderBack)]
    );
    assert_eq!(
        resolve(&map, &list, &[press(KeyCode::Char('q'))]),
        [Resolved::Action(Action::GlobalQuit)]
    );
    assert_eq!(
        resolve(
            &map,
            &reader,
            &[press(KeyCode::Char('g')), press(KeyCode::Char('x'))]
        ),
        [
            Resolved::Pending,
            Resolved::Action(Action::ReaderFollowLink)
        ]
    );
    // A prefix that leads nowhere is dropped and the second key is read alone.
    assert_eq!(
        resolve(
            &map,
            &reader,
            &[press(KeyCode::Char('g')), press(KeyCode::Char('j'))]
        ),
        [Resolved::Pending, Resolved::Action(Action::ReaderDown)]
    );
    assert_eq!(
        resolve(&map, &list, &[press(KeyCode::Char('x'))]),
        [Resolved::Unbound]
    );
}

#[test]
fn an_override_replaces_the_defaults() {
    // `x`, a key the list binds to nothing; `v` is `list.select_range`'s own default.
    let map = configured("[keys]\n\"list.filter\" = \"x\"\n").unwrap();
    let list = [View::List, View::Global];
    assert_eq!(
        resolve(&map, &list, &[press(KeyCode::Char('x'))]),
        [Resolved::Action(Action::ListFilter)]
    );
    assert_eq!(
        resolve(&map, &list, &[press(KeyCode::Char('/'))]),
        [Resolved::Unbound]
    );
    assert!(map.is_overridden(Action::ListFilter));
    assert!(!map.is_overridden(Action::ListOpen));
}

#[test]
fn an_unknown_action_is_refused_with_path_and_line() {
    let text = "[general]\nsecrets = \"file\"\n\n[keys]\n\"list.filter\" = \"/\"\n\"list.write\" = \"e\"\n";
    let error = configured(text).err().unwrap();
    assert_eq!(error.line, 6);
    assert_eq!(
        error.to_string(),
        format!(
            "{PATH}:6: [keys] \"list.write\" is not an action Rail knows (the README lists them)"
        )
    );
}

#[test]
fn an_unreadable_key_is_refused_with_path_and_line() {
    let error = configured("[keys]\n\"list.filter\" = \"ctrl-\"\n")
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        format!("{PATH}:2: [keys] \"list.filter\": \"ctrl-\" is not a key Rail can read")
    );
}

#[test]
fn two_actions_on_one_key_in_one_view_are_refused_with_the_line() {
    let error = configured("[keys]\n\n\"list.filter\" = \"enter\"\n")
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        format!(
            "{PATH}:3: [keys] \"list.open\" (Enter) and \"list.filter\" (Enter) cannot share a key in the list view"
        )
    );
    // A key that starts a chord of the same view clashes too.
    let error = configured("[keys]\n\"list.filter\" = \"g\"\n")
        .err()
        .unwrap();
    assert!(error.message.contains("\"list.top\" (g g)"), "{error}");
    // The same key in two views is fine: Tab is sidebar.focus in the list, expand in the reader.
    configured("[keys]\n\"reader.attachments\" = \"b\"\n").unwrap();
}

#[test]
fn ctrl_c_and_a_quit_chord_are_refused() {
    let error = configured("[keys]\n\"list.filter\" = \"ctrl-c\"\n")
        .err()
        .unwrap();
    assert!(
        error.message.contains("Ctrl-C, which always quits Rail"),
        "{error}"
    );
    assert_eq!(error.line, 2);
    let error = configured("[keys]\n\"global.quit\" = \"z z\"\n")
        .err()
        .unwrap();
    assert!(error.message.contains("must be one key"), "{error}");
}

#[test]
fn hostile_text_in_keys_is_escaped_in_the_error() {
    let error = configured("[keys]\n\"a\\u001b[2J\" = \"x\"\n")
        .err()
        .unwrap();
    assert!(!error.message.contains('\u{1b}'), "{:?}", error.message);
    assert!(error.message.contains("\\u{1b}[2J"), "{:?}", error.message);
    let error = configured("[keys]\n\"list.filter\" = \"\\u202e\"\n")
        .err()
        .unwrap();
    assert!(error.message.contains("\\u{202e}"), "{:?}", error.message);
}

#[test]
fn help_reflects_an_override_and_labels_read_as_the_wizard_showed_them() {
    // `x`, a key the list binds to nothing; `v` is `list.select_range`'s own default.
    let map = configured("[keys]\n\"list.filter\" = \"x\"\n").unwrap();
    let rows = map.help(View::List);
    let filter = rows
        .iter()
        .find(|r| r.action == Action::ListFilter)
        .unwrap();
    assert_eq!(filter.keys, "x");
    assert!(filter.overridden);
    let open = rows.iter().find(|r| r.action == Action::ListOpen).unwrap();
    assert_eq!((open.keys.as_str(), open.overridden), ("Enter", false));
    let down = rows.iter().find(|r| r.action == Action::ListDown).unwrap();
    assert_eq!(down.keys, "j, ↓");
    assert!(rows.iter().all(|r| r.action.views().contains(&View::List)));
    let defaults = Keymap::defaults();
    let labels: Vec<String> = [
        Action::WizardUp,
        Action::WizardDown,
        Action::WizardNext,
        Action::WizardBack,
        Action::GlobalQuit,
        Action::WizardToggle,
        Action::WizardNextField,
        Action::WizardOpenPortal,
        Action::WizardRetry,
        Action::WizardCopyUrl,
        Action::ListRefresh,
        Action::ReaderFollowLink,
        Action::WizardPrevField,
    ]
    .iter()
    .map(|a| defaults.label(*a))
    .collect();
    assert_eq!(
        labels,
        [
            "↑",
            "↓",
            "Enter",
            "Esc",
            "q",
            "Space",
            "Tab",
            "o",
            "r",
            "c",
            "Ctrl-r",
            "g x",
            "Shift-Tab"
        ]
    );
}

#[test]
fn text_keys_are_printable_characters_without_ctrl_or_alt() {
    assert!(is_text_key(press(KeyCode::Char('q'))));
    assert!(is_text_key(with(KeyCode::Char('Q'), KeyModifiers::SHIFT)));
    assert!(!is_text_key(with(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL
    )));
    assert!(!is_text_key(press(KeyCode::Enter)));
}

#[test]
fn the_action_keys_have_their_spec_defaults_and_views() {
    let map = Keymap::defaults();
    let both = [View::List, View::Reader];
    for (action, id, key, views) in [
        (Action::ListArchive, "list.archive", "e", &both[..]),
        (Action::ListTrash, "list.trash", "d", &both[..]),
        (Action::ListStar, "list.star", "s", &both[..]),
        (Action::ListRead, "list.read", "u", &both[..]),
        (Action::ListLabels, "list.labels", "l", &both[..]),
        (Action::ListMove, "list.move", "m", &both[..]),
        (
            Action::ListSelect,
            "list.select",
            "Space",
            &[View::List][..],
        ),
        (
            Action::ListClearSelection,
            "list.clear_selection",
            "Esc",
            &[View::List][..],
        ),
        (Action::GlobalUndo, "global.undo", "z", &[View::Global][..]),
    ] {
        assert_eq!(
            (action.id(), map.label(action).as_str(), action.views()),
            (id, key, views)
        );
    }
    // In the reader Esc still closes and Space still pages.
    let reader = [View::Reader, View::Global];
    assert_eq!(
        resolve(&map, &reader, &[press(KeyCode::Esc)]),
        [Resolved::Action(Action::ReaderBack)]
    );
    assert_eq!(
        resolve(&map, &reader, &[press(KeyCode::Char(' '))]),
        [Resolved::Action(Action::ReaderPageDown)]
    );
    assert_eq!(
        resolve(&map, &reader, &[press(KeyCode::Char('e'))]),
        [Resolved::Action(Action::ListArchive)]
    );
    let list = [View::List, View::Global];
    assert_eq!(
        resolve(&map, &list, &[press(KeyCode::Char('z'))]),
        [Resolved::Action(Action::GlobalUndo)]
    );
}

#[test]
fn compose_push_is_s_and_clashes_with_no_other_compose_key() {
    let map = Keymap::defaults();
    assert_eq!(
        (
            Action::ComposePush.id(),
            map.label(Action::ComposePush).as_str(),
            Action::ComposePush.views()
        ),
        ("compose.push", "s", &[View::Compose][..])
    );
    let compose = [View::Compose, View::Global];
    assert_eq!(
        resolve(&map, &compose, &[press(KeyCode::Char('s'))]),
        [Resolved::Action(Action::ComposePush)]
    );
    configured("").expect("the defaults are free of clashes");
}

#[test]
fn the_old_headers_key_h_clashes_with_snooze_in_the_reader() {
    let text = "[keys]\n\"reader.headers\" = \"h\"\n";
    let error = configured(text).unwrap_err();
    assert_eq!(error.line, 2);
    assert_eq!(
        error.message,
        "[keys] \"list.snooze\" (h) and \"reader.headers\" (h) cannot share a key in the reader view"
    );
    let moved = configured("[keys]\n\"reader.headers\" = \"h\"\n\"list.snooze\" = \"z\"\n");
    assert!(moved.is_ok(), "moving snooze frees h");
}
