//! Keymap presets on the mail screen: `D` trashes without asking in the vim preset, `:keymap`
//! switches and asks "Keep these keys?", saving only what is kept and going back by itself
//! when nobody answers, and a switch that `[keys]` clashes with is refused, keeping the keys in
//! force.

use rail_core::config::KeymapPreset;
use rail_core::ops::Action as MailAction;

use super::*;
use crate::tui::modals::Target;

/// The keys of `preset` with `[keys]` from `text`, as startup builds them.
fn keymap(text: &str) -> Arc<Keymap> {
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap())
}

fn write_config(f: &Fixture, text: &str) {
    let path = f.paths.config_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn config_text(f: &Fixture) -> String {
    std::fs::read_to_string(f.paths.config_file()).unwrap_or_default()
}

fn trashed(f: &Fixture) -> bool {
    applies(f).iter().any(|c| {
        matches!(
            c,
            FakeCall::Apply {
                action: MailAction::Trash {},
                ..
            }
        )
    })
}

/// Pick `name` in the open `:keymap` picker.
async fn choose(f: &mut Fixture, name: &str) {
    let effects = f.screen.run_palette(Target::Keymap);
    settle(&mut f.screen, effects).await;
    assert!(matches!(f.screen.modal, Some(Modal::Picker(_))));
    for c in name.chars() {
        press(f, KeyCode::Char(c)).await;
    }
    press(f, KeyCode::Enter).await;
}

#[tokio::test]
async fn capital_d_in_the_vim_preset_trashes_without_asking_and_can_be_undone() {
    let mut f = fixture(3).await;
    f.screen.ctx.keymap = keymap("[general]\nkeymap = \"vim\"\n");
    press(&mut f, KeyCode::Char('D')).await;
    assert!(f.screen.modal.is_none(), "no question");
    assert!(trashed(&f));
    assert!(
        f.screen.status_line().contains("u undoes"),
        "{}",
        f.screen.status_line()
    );

    press(&mut f, KeyCode::Char('d')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "d still asks"
    );
}

/// `D` skips `d`'s question, not `bulk_confirm`'s: a selection over the limit is asked about,
/// and one at the limit is not.
#[tokio::test]
async fn capital_d_still_asks_about_a_selection_over_bulk_confirm() {
    let mut f = fixture(3).await;
    f.screen.ctx.keymap = keymap("[general]\nkeymap = \"vim\"\n");
    f.screen.ctx.bulk_confirm = 1;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char('D')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "two threads, limit one"
    );
    assert!(!trashed(&f), "nothing before the answer");
    press(&mut f, KeyCode::Char('y')).await;
    assert!(trashed(&f));

    let mut f = fixture(3).await;
    f.screen.ctx.keymap = keymap("[general]\nkeymap = \"vim\"\n");
    f.screen.ctx.bulk_confirm = 2;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char('D')).await;
    assert!(f.screen.modal.is_none(), "two threads, limit two");
    assert!(trashed(&f));
}

/// The "Keep these keys?" question, if it is open.
fn keep_question(f: &Fixture) -> Option<Vec<String>> {
    match &f.screen.modal {
        Some(Modal::Confirm(confirm)) => Some(confirm.lines().to_vec()),
        _ => None,
    }
}

async fn tick(f: &mut Fixture, times: usize) {
    for _ in 0..times {
        let effects = f.screen.update(Msg::Tick);
        settle(&mut f.screen, effects).await;
    }
}

#[tokio::test]
async fn keymap_switches_saves_and_says_how_to_reach_the_palette() {
    let mut f = fixture(3).await;
    write_config(&f, "[general]\nhints = true\n");
    choose(&mut f, "vim").await;
    let lines = keep_question(&f).expect("asks to keep the keys");
    assert!(lines.join(" ").contains("vim"), "{lines:?}");
    assert!(
        !config_text(&f).contains("keymap"),
        "nothing saved before the answer"
    );
    press(&mut f, KeyCode::Char('y')).await;
    assert!(f.screen.modal.is_none());
    assert!(
        f.screen
            .status_line()
            .contains("keys: vim · F2 opens the palette"),
        "{}",
        f.screen.status_line()
    );
    let text = config_text(&f);
    assert!(text.contains("keymap = \"vim\""), "{text}");
    assert!(text.contains("hints = true"), "the rest is kept: {text}");

    // The vim keys are in force: `u` undoes, so with nothing to undo it says so.
    press(&mut f, KeyCode::Char('u')).await;
    assert!(
        f.screen.status_line().contains("Nothing to undo"),
        "{}",
        f.screen.status_line()
    );

    choose(&mut f, "rail").await;
    press(&mut f, KeyCode::Enter).await;
    assert!(config_text(&f).contains("keymap = \"rail\""));
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

/// The new keys are tried while the question is open; `Esc` goes back to the keys before,
/// and nothing is saved.
#[tokio::test]
async fn esc_on_keep_these_keys_goes_back_to_the_keys_before() {
    let mut f = fixture(3).await;
    write_config(&f, "[general]\nhints = true\n");
    let rail = f.screen.ctx.keymap.clone();
    choose(&mut f, "vim").await;
    assert_eq!(
        f.screen.ctx.keymap_preset,
        KeymapPreset::Vim,
        "tried at once"
    );
    press(&mut f, KeyCode::Esc).await;
    assert!(f.screen.modal.is_none());
    assert_eq!(f.screen.ctx.keymap_preset, KeymapPreset::Rail);
    assert!(Arc::ptr_eq(&rail, &f.screen.ctx.keymap));
    assert!(!config_text(&f).contains("keymap"), "nothing saved");
    let line = f.screen.status_line();
    assert!(line.contains("keys: rail kept"), "{line}");
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

/// Unanswered, the question counts down and then goes back by itself, so keys that leave
/// someone stuck undo themselves.
#[tokio::test]
async fn an_unanswered_keep_these_keys_goes_back_when_the_time_is_up() {
    let mut f = fixture(3).await;
    write_config(&f, "");
    choose(&mut f, "vim").await;
    let first = keep_question(&f).unwrap().join(" ");
    assert!(first.contains("15 s"), "{first}");
    tick(&mut f, KEEP_KEYS_TICKS - 4).await;
    let later = keep_question(&f).expect("still asking").join(" ");
    assert!(later.contains("1 s"), "{later}");
    tick(&mut f, 4).await;
    assert!(keep_question(&f).is_none(), "the question closed");
    assert_eq!(f.screen.ctx.keymap_preset, KeymapPreset::Rail);
    assert!(!config_text(&f).contains("keymap"), "nothing saved");
}

/// Choosing the preset in force picks up `[keys]` as it is now, with nothing to ask.
#[tokio::test]
async fn choosing_the_preset_in_force_does_not_ask() {
    let mut f = fixture(3).await;
    write_config(&f, "");
    choose(&mut f, "rail").await;
    assert!(keep_question(&f).is_none());
    assert_eq!(f.screen.ctx.keymap_preset, KeymapPreset::Rail);
}

#[tokio::test]
async fn the_picker_marks_the_preset_in_force() {
    let mut f = fixture(3).await;
    f.screen.ctx.keymap = keymap("[general]\nkeymap = \"vim\"\n");
    f.screen.ctx.keymap_preset = KeymapPreset::Vim;
    let effects = f.screen.run_palette(Target::Keymap);
    settle(&mut f.screen, effects).await;
    let Some(Modal::Picker(picker)) = &f.screen.modal else {
        panic!("no picker")
    };
    let checked: Vec<_> = picker
        .items()
        .iter()
        .filter(|i| i.checked)
        .map(|i| i.name.clone())
        .collect();
    assert_eq!(checked.len(), 1);
    assert!(checked[0].starts_with("vim"), "{checked:?}");
}

#[tokio::test]
async fn a_switch_that_keys_clashes_with_is_refused_and_keeps_the_keys() {
    let mut f = fixture(3).await;
    write_config(&f, "[keys]\n\"list.star\" = \"U\"\n");
    f.screen.ctx.keymap = keymap("[keys]\n\"list.star\" = \"U\"\n");
    choose(&mut f, "vim").await;
    let line = f.screen.status_line();
    assert!(line.contains("list.star"), "{line}");
    assert!(!config_text(&f).contains("keymap"), "nothing saved");
    assert_eq!(f.screen.ctx.keymap_preset, KeymapPreset::Rail);
    // `z` still undoes: the rail keys are kept.
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

/// The shell reads the quit key with its own copy of the keymap: a switch hands it the very
/// keymap the screen now uses, and a refused switch hands it nothing.
#[tokio::test]
async fn a_switch_hands_the_shell_the_screens_new_keymap() {
    let mut f = fixture(3).await;
    let vim = keymap("[general]\nkeymap = \"vim\"\n");
    let effects = f.screen.keymap_built(KeymapPreset::Vim, Ok(vim.clone()));
    let handed: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            crate::tui::app::Effect::Keymap(k) => Some(k.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(handed.len(), 1);
    assert!(Arc::ptr_eq(&handed[0], &f.screen.ctx.keymap));
    assert!(Arc::ptr_eq(&handed[0], &vim));
    // Going back hands the shell the keys from before.
    let rail = keymap("");
    f.screen.keymap_trial.as_mut().unwrap().previous = rail.clone();
    let effects = f.screen.update(Msg::Key(key(KeyCode::Esc)));
    assert!(effects.iter().any(|e| matches!(
        e,
        crate::tui::app::Effect::Keymap(k) if Arc::ptr_eq(k, &rail)
    )));
    f.screen.ctx.keymap = vim.clone();
    f.screen.ctx.keymap_preset = KeymapPreset::Vim;

    let effects = f
        .screen
        .keymap_built(KeymapPreset::Rail, Err("clash".into()));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, crate::tui::app::Effect::Keymap(_))),
        "a refused switch changes nothing"
    );
    assert!(Arc::ptr_eq(&vim, &f.screen.ctx.keymap));
    assert_eq!(f.screen.ctx.keymap_preset, KeymapPreset::Vim);
}
