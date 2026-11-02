//! The engine with the Graph provider against fake Graph, over a real store and writer: the
//! first load shown as a backfill with its count and no total, a resync that keeps what Graph
//! still lists (with its body) and sweeps the rest only once caught up, and a grown window that
//! starts every folder over with the new filter; a load that leaves a folder unwalked (given
//! up on, or in a skipped subtree) sweeps nothing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::sync::Notify;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::engine::{Engine, EngineParts};
use super::status::StatusBoard;
use super::writer::{WriterHandle, WriterTask, spawn_writer};
use crate::config::{AccountConfig, AccountKind, SyncSettings};
use crate::ops::{AccountSyncStatus, SyncPhase};
use crate::provider::graph::fixtures::{Graph, ROOT, folder, message, recent};
use crate::provider::graph::{GraphProvider, MAX_LINK_BYTES};
use crate::provider::{Provider, SyncCursor};
use crate::store::{Store, SyncStateRow};
use crate::test_support::sample_account;

const ADDRESS: &str = "me@corp.example";

pub(super) struct Harness {
    _tmp: tempfile::TempDir,
    pub(super) db: PathBuf,
    writer: WriterHandle,
    _task: WriterTask,
    board: StatusBoard,
}

pub(super) fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("rail.db");
    Store::open(&db)
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('corp', 'm365', ?1)",
            [ADDRESS],
        )
        .unwrap();
    let (writer, task) = spawn_writer(&db).unwrap();
    Harness {
        _tmp: tmp,
        db,
        writer,
        _task: task,
        board: StatusBoard::default(),
    }
}

/// An engine over `provider` whose clock says `now` (each pass gets its own, later, clock).
pub(super) fn engine(h: &Harness, provider: GraphProvider, history_days: u32, now: i64) -> Engine {
    engine_on(h, Arc::new(provider), history_days, now)
}

/// `engine` over a provider that outlives it, as the daemon's does across passes.
fn engine_on(h: &Harness, provider: Arc<GraphProvider>, history_days: u32, now: i64) -> Engine {
    let account = AccountConfig {
        name: "corp".into(),
        address: ADDRESS.into(),
        kind: AccountKind::M365,
        ..sample_account()
    };
    if h.board.get(ADDRESS).is_none() {
        h.board
            .insert(AccountSyncStatus::new("corp", ADDRESS, AccountKind::M365));
    }
    Engine::new(EngineParts {
        account,
        account_id: 1,
        settings: SyncSettings {
            history_days,
            body_cache_days: 30,
            poll_interval: Duration::from_secs(60),
        },
        provider,
        writer: h.writer.clone(),
        reader: Store::open_reader(&h.db).unwrap(),
        board: h.board.clone(),
        wake: Arc::new(Notify::new()),
        outbox: Arc::new(Notify::new()),
        clock: Arc::new(move || now),
    })
}

fn stored(h: &Harness) -> Vec<String> {
    let store = Store::open_reader(&h.db).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT remote_id FROM messages ORDER BY remote_id")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn state(h: &Harness) -> SyncStateRow {
    Store::open_reader(&h.db).unwrap().sync_state(1).unwrap()
}

pub(super) fn body_of(h: &Harness, id: &str) -> Option<String> {
    Store::open_reader(&h.db)
        .unwrap()
        .conn()
        .query_row(
            "SELECT body_text FROM messages WHERE remote_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

pub(super) async fn inbox(g: &Graph) {
    g.mount_root(json!([folder("f-inbox", "Inbox", 0)])).await;
    g.mount_well_known(&[("inbox", "f-inbox")]).await;
}

#[tokio::test]
async fn the_first_load_shows_as_a_backfill_with_its_count_and_no_total() {
    let g = Graph::start().await;
    inbox(&g).await;
    let at = recent(60);
    let first = g.page(vec![message("m1", "c1", "f-inbox", &at)], Some("p2"), None);
    g.mount_initial("f-inbox", first).await;
    let rest = g.page(vec![message("m2", "c2", "f-inbox", &at)], None, Some("d1"));
    g.mount_link("p2", rest).await;
    let h = harness();
    let phases = Arc::new(Mutex::new(Vec::new()));
    let seen = phases.clone();
    h.board.subscribe(Box::new(move |s| {
        seen.lock().unwrap().push(s.phase);
        true
    }));
    let now = crate::auth::unix_now();
    engine(&h, g.provider(), 0, now).pass().await.unwrap();
    assert_eq!(stored(&h), ["m1", "m2"]);
    let status = h.board.get(ADDRESS).unwrap();
    assert_eq!(
        (status.backfill_fetched, status.backfill_estimate),
        (2, None)
    );
    let phases = phases.lock().unwrap();
    assert!(phases.contains(&SyncPhase::Backfill));
    assert!(
        !phases.contains(&SyncPhase::Delta),
        "the first load never shows as a delta"
    );
    let state = state(&h);
    assert!(state.cursor.is_some());
    assert_eq!(
        state.backfill_started_at, None,
        "the sweep ran once caught up"
    );
}

#[tokio::test]
async fn a_resync_keeps_what_graph_still_lists_with_its_body_and_drops_the_rest() {
    let g = Graph::start().await;
    inbox(&g).await;
    let at = recent(60);
    let both = vec![
        message("m1", "c1", "f-inbox", &at),
        message("m2", "c2", "f-inbox", &at),
    ];
    g.mount_initial("f-inbox", g.page(both, None, Some("d1")))
        .await;
    let body = json!({"id": "m1", "hasAttachments": false,
                      "body": {"contentType": "text", "content": "kept body"}});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1")))
        .and(query_param("$select", crate::provider::graph::BODY_FIELDS))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&g.server)
        .await;
    let h = harness();
    let now = crate::auth::unix_now();
    engine(&h, g.provider(), 0, now).pass().await.unwrap();
    assert_eq!(stored(&h), ["m1", "m2"]);
    assert_eq!(body_of(&h, "m1").as_deref(), Some("kept body"));
    // Graph forgot the link, and m2 is gone meanwhile.
    g.reset().await;
    inbox(&g).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/d1")))
        .respond_with(ResponseTemplate::new(410))
        .mount(&g.server)
        .await;
    let again = g.page(vec![message("m1", "c1", "f-inbox", &at)], None, Some("d2"));
    g.mount_initial("f-inbox", again).await;
    engine(&h, g.provider(), 0, now + 60).pass().await.unwrap();
    assert_eq!(stored(&h), ["m1"]);
    assert_eq!(
        body_of(&h, "m1").as_deref(),
        Some("kept body"),
        "never deleted, never fetched again"
    );
    assert_eq!(state(&h).backfill_started_at, None);
}

#[tokio::test]
async fn a_grown_window_starts_every_folder_over_with_the_new_filter() {
    let g = Graph::start().await;
    inbox(&g).await;
    let (fresh, old) = (recent(60), recent(20 * 24 * 60));
    let first = g.page(
        vec![message("m1", "c1", "f-inbox", &fresh)],
        None,
        Some("d1"),
    );
    g.mount_initial("f-inbox", first).await;
    let h = harness();
    let now = crate::auth::unix_now();
    engine(&h, g.provider_with_days(7), 7, now)
        .pass()
        .await
        .unwrap();
    assert_eq!(stored(&h), ["m1"]);
    g.reset().await;
    inbox(&g).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/d1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(g.page(vec![], None, Some("d1"))))
        .expect(0)
        .mount(&g.server)
        .await;
    let all = vec![
        message("m1", "c1", "f-inbox", &fresh),
        message("m-old", "c2", "f-inbox", &old),
    ];
    g.mount_initial("f-inbox", g.page(all, None, Some("d2")))
        .await;
    engine(&h, g.provider_with_days(30), 30, now + 60)
        .pass()
        .await
        .unwrap();
    assert_eq!(stored(&h), ["m-old", "m1"]);
}

/// Page `n` of a twelve-page inbox walk of one message each (`m01` to `m12`): page 1 is the
/// initial delta, the last gives the delta link `d2`.
fn walk_page(g: &Graph, n: usize, at: &str) -> serde_json::Value {
    let item = message(&format!("m{n:02}"), &format!("c{n}"), "f-inbox", at);
    match n {
        12 => g.page(vec![item], None, Some("d2")),
        _ => g.page(vec![item], Some(&format!("p{}", n + 1)), None),
    }
}

#[tokio::test]
async fn a_first_load_over_two_passes_and_a_restart_sweeps_once_at_the_end_from_its_first_start() {
    let g = Graph::start().await;
    inbox(&g).await;
    let at = recent(60);
    let before = g.page(
        vec![message("m-gone", "c0", "f-inbox", &at)],
        None,
        Some("d1"),
    );
    g.mount_initial("f-inbox", before).await;
    let h = harness();
    let now = crate::auth::unix_now();
    engine(&h, g.provider(), 0, now).pass().await.unwrap();
    assert_eq!(stored(&h), ["m-gone"]);
    // Graph forgets the link; the reload walks twelve pages, ten to a call, and the pass
    // that began it fails on page 11 (a stand-in for the daemon stopping).
    g.reset().await;
    inbox(&g).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/d1")))
        .respond_with(ResponseTemplate::new(410))
        .mount(&g.server)
        .await;
    g.mount_initial("f-inbox", walk_page(&g, 1, &at)).await;
    for n in 2..=10 {
        g.mount_link(&format!("p{n}"), walk_page(&g, n, &at)).await;
    }
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/p11")))
        .respond_with(ResponseTemplate::new(400))
        .mount(&g.server)
        .await;
    let first_start = now + 60;
    assert!(
        engine(&h, g.provider(), 0, first_start)
            .pass()
            .await
            .is_err()
    );
    let midway = stored(&h);
    assert!(
        midway.contains(&"m-gone".to_string()),
        "no sweep yet: {midway:?}"
    );
    assert!(midway.contains(&"m10".to_string()), "{midway:?}");
    let mid = state(&h);
    assert_eq!(mid.backfill_started_at, Some(first_start));
    assert!(mid.cursor.is_some());
    // A fresh engine, later, finishes the load.
    g.reset().await;
    inbox(&g).await;
    for n in 11..=12 {
        g.mount_link(&format!("p{n}"), walk_page(&g, n, &at)).await;
    }
    engine(&h, g.provider(), 0, now + 120).pass().await.unwrap();
    let expected: Vec<String> = (1..=12).map(|n| format!("m{n:02}")).collect();
    assert_eq!(
        stored(&h),
        expected,
        "m-gone swept; pages 1 to 10, seen only in the first pass, kept: the first start rules"
    );
    assert_eq!(state(&h).backfill_started_at, None);
}

/// Inbox and Projects at the root, Projects with the subfolder Sub; the Inbox well-known.
async fn with_projects(g: &Graph, sub_listed: bool) {
    g.mount_root(json!([
        folder("f-inbox", "Inbox", 0),
        folder("f-proj", "Projects", 1),
    ]))
    .await;
    if sub_listed {
        g.mount_children("f-proj", json!([folder("f-sub", "Sub", 0)]))
            .await;
    }
    g.mount_well_known(&[("inbox", "f-inbox")]).await;
}

/// A first load of `m1` in the inbox, `mp` in Projects and `ms` in Sub, swept clean, on a
/// provider kept for the passes after it.
async fn loaded(g: &Graph, h: &Harness, now: i64) -> Arc<GraphProvider> {
    with_projects(g, true).await;
    let at = recent(60);
    for (folder, id) in [("f-inbox", "m1"), ("f-proj", "mp"), ("f-sub", "ms")] {
        let page = g.page(
            vec![message(id, id, folder, &at)],
            None,
            Some(&format!("d-{folder}")),
        );
        g.mount_initial(folder, page).await;
    }
    let provider = Arc::new(g.provider());
    engine_on(h, provider.clone(), 0, now).pass().await.unwrap();
    assert_eq!(stored(h), ["m1", "mp", "ms"]);
    assert_eq!(state(h).backfill_started_at, None, "a clean load sweeps");
    provider
}

/// Graph forgets the inbox's link: the next pass resyncs, and its load walks the inbox anew.
async fn inbox_link_forgotten(g: &Graph) {
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/d-f-inbox")))
        .respond_with(ResponseTemplate::new(410))
        .mount(&g.server)
        .await;
    let at = recent(60);
    let page = g.page(vec![message("m1", "m1", "f-inbox", &at)], None, Some("d2"));
    g.mount_initial("f-inbox", page).await;
}

/// One pass of `engine` that goes through, recorded as the daemon records it.
async fn recorded_pass(engine: &Engine) {
    let pass = engine.pass().await;
    assert!(pass.is_ok(), "{pass:?}");
    engine.record(pass).await;
}

/// The load ended without a sweep (`stored` shows what it kept): no load is left open, and
/// the account is idle, not in a backfill.
fn assert_load_ended(h: &Harness) {
    assert_eq!(state(h).backfill_started_at, None, "the load ended");
    let status = h.board.get(ADDRESS).unwrap();
    assert_eq!(status.phase, SyncPhase::Idle);
    assert!(!status.pass_running);
}

#[tokio::test]
async fn a_resync_with_a_folder_given_up_on_sweeps_none_of_its_mail() {
    let g = Graph::start().await;
    let h = harness();
    let now = crate::auth::unix_now();
    let provider = loaded(&g, &h, now).await;
    // Projects' links run over the cap from now on: the next pass gives it up.
    let long = "l".repeat(MAX_LINK_BYTES);
    g.reset().await;
    with_projects(&g, true).await;
    for folder in ["f-inbox", "f-sub"] {
        let link = format!("d-{folder}");
        g.mount_link(&link, g.page(vec![], None, Some(&link))).await;
    }
    g.mount_link("d-f-proj", g.page(vec![], None, Some(&long)))
        .await;
    g.mount_initial("f-proj", g.page(vec![], None, Some(&long)))
        .await;
    engine_on(&h, provider.clone(), 0, now + 60)
        .pass()
        .await
        .unwrap();
    assert!(provider.standing_error().is_some());
    g.reset().await;
    with_projects(&g, true).await;
    inbox_link_forgotten(&g).await;
    g.mount_initial(
        "f-sub",
        g.page(
            vec![message("ms", "ms", "f-sub", &recent(60))],
            None,
            Some("d3"),
        ),
    )
    .await;
    recorded_pass(&engine_on(&h, provider, 0, now + 120)).await;
    assert_eq!(
        stored(&h),
        ["m1", "mp", "ms"],
        "Projects' mail is not swept"
    );
    assert_load_ended(&h);
    assert!(
        h.board.get(ADDRESS).unwrap().last_pass_failed,
        "the standing error"
    );
}

#[tokio::test]
async fn a_resync_with_a_subtree_skipped_sweeps_none_of_its_mail() {
    let g = Graph::start().await;
    let h = harness();
    let now = crate::auth::unix_now();
    let provider = loaded(&g, &h, now).await;
    // Projects' child listing answers not found: Sub is carried over and skipped.
    g.reset().await;
    with_projects(&g, false).await;
    inbox_link_forgotten(&g).await;
    let at = recent(60);
    let page = g.page(vec![message("mp", "mp", "f-proj", &at)], None, Some("d3"));
    g.mount_initial("f-proj", page).await;
    recorded_pass(&engine_on(&h, provider.clone(), 0, now + 60)).await;
    let at = SyncCursor(state(&h).cursor.unwrap());
    assert!(!provider.walked_everything(&at), "Sub was set aside");
    assert_eq!(stored(&h), ["m1", "mp", "ms"], "Sub's mail is not swept");
    assert_load_ended(&h);
    assert!(!h.board.get(ADDRESS).unwrap().last_pass_failed);
}

/// How Aside, Box's subfolder (`Box/Aside`, walked before Projects), fares in a two-pass load.
#[derive(Clone, Copy, PartialEq)]
enum Aside {
    /// Listed and walked.
    Walked,
    /// Its listing answers not found in the first pass: set aside, never walked in the load.
    Skipped,
    /// Its links run over the cap: given up in the first pass; the second pass runs on a new
    /// provider (a daemon restart) that has forgotten it.
    GivenUp,
}

/// Inbox, Box with its subfolder Aside (listed when `listed`), and Projects.
async fn aside_mailbox(g: &Graph, listed: bool) {
    g.mount_root(json!([
        folder("f-inbox", "Inbox", 0),
        folder("f-box", "Box", 1),
        folder("f-proj", "Projects", 0),
    ]))
    .await;
    if listed {
        g.mount_children("f-box", json!([folder("f-aside", "Aside", 0)]))
            .await;
    }
    g.mount_well_known(&[("inbox", "f-inbox")]).await;
}

/// Page `n` of Projects' twelve-page walk (`pr01` to `pr12`), linked as `pr<n>`.
fn projects_page(g: &Graph, n: usize, at: &str) -> serde_json::Value {
    let item = message(&format!("pr{n:02}"), &format!("pr{n}"), "f-proj", at);
    match n {
        12 => g.page(vec![item], None, Some("d-pr")),
        _ => g.page(vec![item], Some(&format!("pr{}", n + 1)), None),
    }
}

/// A clean load of `m-gone` (inbox), `b0` (Box), `a1` (Aside) and `p0` (Projects), then a
/// resync whose load
/// spans two passes: the first fails on Projects' page 10 after one saved answer, the
/// second finishes it. What the store holds at the end; the load has ended.
async fn two_pass_resync(aside: Aside) -> Vec<String> {
    let g = Graph::start().await;
    let h = harness();
    let now = crate::auth::unix_now();
    let at = recent(60);
    aside_mailbox(&g, true).await;
    for (folder, id) in [
        ("f-inbox", "m-gone"),
        ("f-box", "b0"),
        ("f-proj", "p0"),
        ("f-aside", "a1"),
    ] {
        let page = g.page(
            vec![message(id, id, folder, &at)],
            None,
            Some(&format!("d-{folder}")),
        );
        g.mount_initial(folder, page).await;
    }
    let provider = Arc::new(g.provider());
    recorded_pass(&engine_on(&h, provider.clone(), 0, now)).await;
    assert_eq!(stored(&h), ["a1", "b0", "m-gone", "p0"]);
    // Graph forgets the inbox's link; m-gone is gone meanwhile.
    g.reset().await;
    aside_mailbox(&g, aside != Aside::Skipped).await;
    inbox_link_forgotten(&g).await;
    let box_page = g.page(vec![message("b0", "b0", "f-box", &at)], None, Some("d-b"));
    g.mount_initial("f-box", box_page).await;
    let long = "l".repeat(MAX_LINK_BYTES);
    match aside {
        Aside::Walked => {
            let page = g.page(vec![message("a1", "a1", "f-aside", &at)], None, Some("d-a"));
            g.mount_initial("f-aside", page).await;
        }
        Aside::Skipped => {}
        Aside::GivenUp => {
            g.mount_initial("f-aside", g.page(vec![], None, Some(&long)))
                .await;
        }
    }
    g.mount_initial("f-proj", projects_page(&g, 1, &at)).await;
    for n in 2..=9 {
        g.mount_link(&format!("pr{n}"), projects_page(&g, n, &at))
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/links/pr10")))
        .respond_with(ResponseTemplate::new(400))
        .mount(&g.server)
        .await;
    let first = engine_on(&h, provider.clone(), 0, now + 60).pass().await;
    assert!(first.is_err(), "the first pass stops on page 10");
    assert!(state(&h).backfill_started_at.is_some(), "the load goes on");
    // The second pass: Aside lists again; on a new provider when it was given up on.
    g.reset().await;
    aside_mailbox(&g, true).await;
    for n in 2..=12 {
        g.mount_link(&format!("pr{n}"), projects_page(&g, n, &at))
            .await;
    }
    let provider = match aside {
        Aside::GivenUp => Arc::new(g.provider()),
        _ => provider,
    };
    recorded_pass(&engine_on(&h, provider, 0, now + 120)).await;
    assert_load_ended(&h);
    stored(&h)
}

#[tokio::test]
async fn a_clean_two_pass_load_still_sweeps() {
    let stored = two_pass_resync(Aside::Walked).await;
    assert!(!stored.contains(&"m-gone".to_string()), "swept: {stored:?}");
    assert!(stored.contains(&"a1".to_string()), "{stored:?}");
}

#[tokio::test]
async fn a_subtree_set_aside_in_one_pass_of_a_load_is_not_swept_when_it_lists_again() {
    let stored = two_pass_resync(Aside::Skipped).await;
    assert!(
        stored.contains(&"a1".to_string()),
        "Aside's mail stays: {stored:?}"
    );
    assert!(
        stored.contains(&"m-gone".to_string()),
        "no sweep: {stored:?}"
    );
}

#[tokio::test]
async fn a_folder_given_up_in_a_load_is_not_swept_after_a_restart() {
    let stored = two_pass_resync(Aside::GivenUp).await;
    assert!(
        stored.contains(&"a1".to_string()),
        "Aside's mail stays: {stored:?}"
    );
    assert!(
        stored.contains(&"m-gone".to_string()),
        "no sweep: {stored:?}"
    );
}
