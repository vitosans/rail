//! `general.field_keys` on the mail screen: `:field-keys` switches and saves it, and the open
//! fields (the `/` filter, compose's prompts) edit with the mode in force.

use rail_core::config::FieldKeys;

use super::compose::{compose_fixture, editor_types_above, saves, type_text};
use super::*;
use crate::tui::test_util::ctrl;

async fn press_ctrl(f: &mut Fixture, c: char) {
    let effects = f.screen.update(Msg::Key(ctrl(c)));
    settle_capturing(&mut f.screen, effects, &f.last_theme, &f.editor).await;
}

async fn run_field_keys(f: &mut Fixture) {
    press(f, KeyCode::Char(':')).await;
    type_text(f, "field-keys").await;
    press(f, KeyCode::Enter).await;
}

#[tokio::test]
async fn field_keys_palette_switches_and_saves_without_losing_comments() {
    let mut f = fixture(3).await;
    let path = f.paths.config_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[general]\neditor = \"vi\" # mine\n").unwrap();
    for expected in [FieldKeys::Basic, FieldKeys::Readline] {
        run_field_keys(&mut f).await;
        assert!(f.screen.modal.is_none());
        assert_eq!(f.screen.ctx.field_keys, expected);
        let line = f.screen.status_line();
        assert!(
            line.contains(&format!("field keys: {}", expected.as_str())),
            "{line}"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("editor = \"vi\" # mine"), "{text}");
        let config = rail_core::config::parse(&text, &path).unwrap();
        assert_eq!(config.general.field_keys, expected);
        assert_eq!(
            text.contains("field_keys"),
            expected != FieldKeys::Readline,
            "the default removes the key: {text}"
        );
    }
}

#[tokio::test]
async fn the_filter_line_edits_with_the_field_keys_in_force() {
    for (keys, left) in [
        (FieldKeys::Readline, "from:alice "),
        (FieldKeys::Basic, "from:alice bob"),
    ] {
        let mut f = fixture(3).await;
        f.screen.ctx.field_keys = keys;
        press(&mut f, KeyCode::Char('/')).await;
        type_text(&mut f, "from:alice bob").await;
        press_ctrl(&mut f, 'w').await;
        assert_eq!(f.screen.filter_text, left, "{keys:?}");
    }
}

/// The prompt is open before the switch: the keys in force at each key press count.
#[tokio::test]
async fn compose_prompts_edit_with_the_field_keys_in_force() {
    for (keys, to) in [
        (FieldKeys::Readline, "sam@example.com"),
        (FieldKeys::Basic, "xsam@example.com"),
    ] {
        let mut f = compose_fixture().await;
        editor_types_above(&f, "Hello\n");
        press(&mut f, KeyCode::Char('1')).await;
        press(&mut f, KeyCode::Char('c')).await;
        type_text(&mut f, "x").await;
        f.screen.ctx.field_keys = keys;
        press_ctrl(&mut f, 'u').await;
        type_text(&mut f, "sam@example.com").await;
        press(&mut f, KeyCode::Enter).await;
        type_text(&mut f, "Lunch").await;
        press(&mut f, KeyCode::Enter).await;
        let saved = saves(&f);
        assert!(
            matches!(&saved[0], FakeCall::DraftSave { to: got, .. } if got == &[to.to_string()]),
            "{keys:?}: {saved:?}"
        );
    }
}
