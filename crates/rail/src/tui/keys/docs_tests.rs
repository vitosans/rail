//! docs/keys.md lists every action. Its table sits between `<!-- actions:begin -->` and
//! `<!-- actions:end -->`, one row per action in `Action::ALL` order, with five columns:
//! the id, the views, the default keys, the description and free notes. The first four must
//! say exactly what `actions.rs` says; the notes column is prose and is not checked.

use super::{Action, View};

const BEGIN: &str = "<!-- actions:begin -->";
const END: &str = "<!-- actions:end -->";
/// What the default-keys cell says for an action with none.
const NO_KEYS: &str = "(none)";

/// One table row as the docs write it.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    id: String,
    views: Vec<String>,
    keys: Vec<String>,
    description: String,
}

/// The row `actions.rs` implies for `action`.
fn expected(action: Action) -> Row {
    Row {
        id: action.id().to_string(),
        views: action
            .views()
            .iter()
            .map(|v| View::name(*v).to_string())
            .collect(),
        keys: action
            .default_keys()
            .iter()
            .map(|k| k.to_string())
            .collect(),
        description: format!("{}.", action.description()),
    }
}

/// The backticked spans of a cell, in order.
fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn parse_row(line: &str) -> Row {
    let cells: Vec<&str> = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    assert_eq!(
        cells.len(),
        5,
        "docs/keys.md: a row needs five cells: {line}"
    );
    let ids = backticked(cells[0]);
    assert_eq!(
        ids.len(),
        1,
        "docs/keys.md: one backticked id per row: {line}"
    );
    let keys = if cells[2] == NO_KEYS {
        Vec::new()
    } else {
        backticked(cells[2])
    };
    Row {
        id: ids[0].clone(),
        views: cells[1].split(", ").map(str::to_string).collect(),
        keys,
        description: cells[3].to_string(),
    }
}

/// The action rows of the docs' table: the lines between the markers that start with a
/// backticked id (the header and the separator do not).
fn documented() -> Vec<Row> {
    let docs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/keys.md"))
        .expect("docs/keys.md at the workspace root");
    let start = docs
        .find(BEGIN)
        .expect("docs/keys.md has the actions:begin marker");
    let end = docs
        .find(END)
        .expect("docs/keys.md has the actions:end marker");
    assert!(
        start < end,
        "docs/keys.md: actions:begin comes before actions:end"
    );
    docs[start + BEGIN.len()..end]
        .lines()
        .filter(|line| line.trim_start().starts_with("| `"))
        .map(parse_row)
        .collect()
}

#[test]
fn the_docs_action_table_lists_every_action_in_order() {
    let ids: Vec<String> = documented().into_iter().map(|row| row.id).collect();
    let want: Vec<String> = Action::ALL.iter().map(|a| a.id().to_string()).collect();
    assert_eq!(
        ids, want,
        "docs/keys.md's action ids differ from actions.rs"
    );
}

#[test]
fn the_docs_action_table_matches_each_actions_views_keys_and_description() {
    for row in documented() {
        let action = Action::from_id(&row.id)
            .unwrap_or_else(|| panic!("docs/keys.md lists {}, which Rail does not know", row.id));
        assert_eq!(row, expected(action), "docs/keys.md's row for {}", row.id);
    }
}

#[test]
fn a_row_reads_its_cells_and_ignores_the_notes() {
    let row = parse_row(
        "| `list.trash_now` | list, reader | (none) | Move to the trash. | `D` in `vim`. |",
    );
    assert_eq!(
        row,
        Row {
            id: "list.trash_now".into(),
            views: vec!["list".into(), "reader".into()],
            keys: Vec::new(),
            description: "Move to the trash.".into(),
        }
    );
    assert_eq!(
        parse_row("| `list.top` | list | `home` `g g` | Newest thread. | |").keys,
        vec!["home".to_string(), "g g".to_string()]
    );
}
