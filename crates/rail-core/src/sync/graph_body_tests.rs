//! A Microsoft 365 body fetched again over attachments already stored (as schema v8 asks for
//! every such message): a list Graph refuses, whether it said there were attachments or the
//! HTML only names a `cid:`, keeps the new body and the stored attachments; so does a body
//! Rail cannot read, stored as the marker.

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::UNREADABLE_BODY;
use super::graph_engine_tests::{Harness, body_of, engine, harness, inbox};
use crate::provider::graph::fixtures::{Graph, ROOT, message, recent};
use crate::store::Store;

async fn serve_body(g: &Graph, answer: Value) {
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1")))
        .and(query_param("$select", crate::provider::graph::BODY_FIELDS))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .expect(1)
        .mount(&g.server)
        .await;
}

async fn refuse_the_listing(g: &Graph) {
    let refusal = json!({"error": {"code": "BadRequest", "message": "no"}});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments")))
        .respond_with(ResponseTemplate::new(400).set_body_json(refusal))
        .expect(1)
        .mount(&g.server)
        .await;
}

/// m1 loaded with a body and one stored attachment, then marked unfetched as schema v8 does;
/// the second pass fetches its body again from what `serve` mounts. The first body's mock
/// says `.expect(1)`; `reset` drops it unchecked, so `serve`'s own mocks answer the second.
async fn fetched_again(serve: impl AsyncFnOnce(&Graph)) -> Harness {
    let g = Graph::start().await;
    inbox(&g).await;
    let page = g.page(
        vec![message("m1", "c1", "f-inbox", &recent(60))],
        None,
        Some("d1"),
    );
    g.mount_initial("f-inbox", page).await;
    serve_body(
        &g,
        json!({"id": "m1", "hasAttachments": false,
                          "body": {"contentType": "text", "content": "old body"}}),
    )
    .await;
    let h = harness();
    let now = crate::auth::unix_now();
    engine(&h, g.provider(), 0, now).pass().await.unwrap();
    Store::open(&h.db)
        .unwrap()
        .conn()
        .execute_batch(
            "INSERT INTO attachments (message_id, remote_part_id, filename)
             SELECT id, 'a1', 'plan.pdf' FROM messages WHERE remote_id = 'm1';
             UPDATE messages SET body_fetched_at = NULL WHERE remote_id = 'm1';",
        )
        .unwrap();
    g.reset().await;
    inbox(&g).await;
    g.mount_link("d1", g.page(Vec::new(), None, Some("d2")))
        .await;
    serve(&g).await;
    engine(&h, g.provider(), 0, now + 60).pass().await.unwrap();
    h
}

fn stored_attachments(h: &Harness) -> Vec<String> {
    let store = Store::open_reader(&h.db).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT filename FROM attachments")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn a_refused_list_graph_said_was_there_keeps_the_body_and_the_stored_attachments() {
    let h = fetched_again(async |g: &Graph| {
        serve_body(
            g,
            json!({"id": "m1", "hasAttachments": true,
                             "body": {"contentType": "html", "content": "<p>new body</p>"}}),
        )
        .await;
        refuse_the_listing(g).await;
    })
    .await;
    assert_eq!(body_of(&h, "m1").as_deref(), Some("new body"));
    assert_eq!(stored_attachments(&h), ["plan.pdf"]);
}

#[tokio::test]
async fn a_refused_list_asked_for_on_a_cid_alone_keeps_the_stored_attachments() {
    let html = "<p>new body</p><img src=\"cid:logo@x\">";
    let h = fetched_again(async |g: &Graph| {
        serve_body(
            g,
            json!({"id": "m1", "hasAttachments": false,
                             "body": {"contentType": "html", "content": html}}),
        )
        .await;
        refuse_the_listing(g).await;
    })
    .await;
    assert!(body_of(&h, "m1").unwrap().starts_with("new body"));
    assert_eq!(stored_attachments(&h), ["plan.pdf"]);
}

#[tokio::test]
async fn an_unreadable_body_keeps_the_stored_attachments() {
    // A body for another message is one Rail cannot read.
    let h = fetched_again(async |g: &Graph| {
        serve_body(
            g,
            json!({"id": "m2", "body": {"contentType": "text", "content": "x"}}),
        )
        .await;
    })
    .await;
    assert_eq!(body_of(&h, "m1").as_deref(), Some(UNREADABLE_BODY));
    assert_eq!(stored_attachments(&h), ["plan.pdf"]);
}
