//! Keymap presets on the mail screen: `D` trashes without asking in the vim preset, `:keymap`
//! switches, saves and says how to get back, and a switch that `[keys]` clashes with is
//! refused, keeping the keys in force.

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

#[tokio::test]
async fn keymap_switches_saves_and_says_how_to_reach_the_palette() {
    let mut f = fixture(3).await;
    write_config(&f, "[general]\nhints = true\n");
    choose(&mut f, "vim").await;
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
    assert!(config_text(&f).contains("keymap = \"rail\""));
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
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
