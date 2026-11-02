//! Writing from a Microsoft 365 account through a real daemon over fake Graph: saves are
//! accepted and a reply or forward keeps the message it answers, a send and a push are queued;
//! an Outlook draft is imported bound to its Graph id and discarding it deletes it in Outlook;
//! the refusals name Microsoft 365.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::client::DaemonClient;
use super::m365_tests::{BOUND, CORP, graph_factory, until};
use super::{Connector, ServeConfig};
use crate::error::ErrorClass;
use crate::ops::op::{DraftDiscard, DraftImport, DraftPush, DraftSave, DraftSend};
use crate::ops::{
    DraftDiscardRequest, DraftImportRequest, DraftPushRequest, DraftSaveRequest, DraftSendRequest,
    ReplyTo,
};
use crate::paths::Paths;
use crate::provider::graph::fixtures::{Graph, ROOT, folder, message, recent};
use crate::store::Store;
use crate::store::drafts::{DraftKind, DraftState};
use crate::test_support::{daemon_context, start_daemon};

pub(super) struct Corp {
    pub(super) g: Graph,
    pub(super) paths: Paths,
    pub(super) client: DaemonClient,
    _tmp: tempfile::TempDir,
}

/// Inbox (m1 in c1) and Drafts (the Outlook drafts g-od1 and g-od2), synced once.
pub(super) async fn corp() -> Corp {
    let g = Graph::start().await;
    g.mount_root(json!([
        folder("f-inbox", "Inbox", 0),
        folder("f-drafts", "Drafts", 0)
    ]))
    .await;
    g.mount_well_known(&[("inbox", "f-inbox"), ("drafts", "f-drafts")])
        .await;
    let at = recent(5);
    let inbox = g.page(
        vec![message("m1", "c1", "f-inbox", &at)],
        None,
        Some("d-in"),
    );
    g.mount_initial("f-inbox", inbox).await;
    g.mount_link("d-in", g.page(vec![], None, Some("d-in")))
        .await;
    let mut drafts = Vec::new();
    for id in ["g-od1", "g-od2"] {
        let mut draft = message(id, "c9", "f-drafts", &at);
        draft["isDraft"] = json!(true);
        drafts.push(draft);
    }
    g.mount_initial("f-drafts", g.page(drafts, None, Some("d-dr")))
        .await;
    g.mount_link("d-dr", g.page(vec![], None, Some("d-dr")))
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), CORP).unwrap();
    let (ctx, _) = daemon_context(&paths);
    start_daemon(
        ctx.with_provider_factory(graph_factory(g.base())),
        ServeConfig::default(),
    )
    .await;
    let client = Connector::new(paths.clone(), Arc::new(|| Ok(())))
        .connect()
        .await
        .unwrap();
    until(&client, |s| s.passes_completed >= 1).await;
    Corp {
        g,
        paths,
        client,
        _tmp: tmp,
    }
}

impl Corp {
    pub(super) fn store(&self) -> Store {
        Store::open_reader(&self.paths.db_file()).unwrap()
    }

    pub(super) fn message(&self, remote: &str) -> i64 {
        self.store()
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE remote_id = ?1",
                [remote],
                |r| r.get(0),
            )
            .unwrap()
    }

    async fn until_request(&self, verb: &str, at: &str) {
        let wanted = format!("{ROOT}/{at}");
        tokio::time::timeout(BOUND, async {
            loop {
                let seen = self.g.server.received_requests().await.unwrap();
                if seen
                    .iter()
                    .any(|r| r.method.as_str() == verb && r.url.path() == wanted)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the request reached Graph");
    }
}

pub(super) fn save(kind: DraftKind, reply_to: Option<ReplyTo>, token: &str) -> DraftSaveRequest {
    DraftSaveRequest {
        account: "corp".into(),
        draft: None,
        kind,
        reply_to,
        to: vec!["ann@example.com".into()],
        cc: vec![],
        bcc: vec![],
        subject: "hello".into(),
        body: "hi".into(),
        attachments: vec![],
        client_token: token.into(),
    }
}

#[tokio::test]
async fn writing_from_a_microsoft_365_account_is_accepted_and_keeps_its_source() {
    let c = corp().await;
    let m1 = c.message("m1");
    let new = c
        .client
        .call::<DraftSave>(&save(DraftKind::New, None, "new-1"))
        .await
        .unwrap();
    let request = save(DraftKind::Reply, Some(ReplyTo { message: m1 }), "reply-1");
    let reply = c.client.call::<DraftSave>(&request).await.unwrap();
    let request = save(DraftKind::Forward, Some(ReplyTo { message: m1 }), "fwd-1");
    let forward = c.client.call::<DraftSave>(&request).await.unwrap();
    let store = c.store();
    let n = store.draft(new.draft).unwrap().unwrap();
    assert_eq!(
        (n.kind, n.source_remote_id.as_deref()),
        (DraftKind::New, None)
    );
    let r = store.draft(reply.draft).unwrap().unwrap();
    assert_eq!(
        (
            r.kind,
            r.source_remote_id.as_deref(),
            r.remote_thread_id.as_deref()
        ),
        (DraftKind::Reply, Some("m1"), Some("c1"))
    );
    assert_eq!(
        (r.remote_draft_id.as_deref(), r.remote_message_id.as_deref()),
        (None, None)
    );
    let f = store.draft(forward.draft).unwrap().unwrap();
    assert_eq!(
        (f.kind, f.source_remote_id.as_deref()),
        (DraftKind::Forward, Some("m1"))
    );
    let sent = DraftSendRequest {
        draft: reply.draft,
        client_token: "send-1".into(),
    };
    let sent = c.client.call::<DraftSend>(&sent).await.unwrap();
    let pushed = DraftPushRequest {
        draft: new.draft,
        client_token: "push-1".into(),
    };
    let pushed = c.client.call::<DraftPush>(&pushed).await.unwrap();
    assert_ne!(sent.row, pushed.row);
    assert_eq!(
        c.store().draft(reply.draft).unwrap().unwrap().state,
        DraftState::Queued
    );
}

#[tokio::test]
async fn an_outlook_draft_is_imported_bound_to_its_id_and_discarding_it_deletes_it() {
    let c = corp().await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-od1")))
        .and(query_param("$select", "id,isDraft"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "g-od1", "isDraft": true})),
        )
        .mount(&c.g.server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-od1")))
        .and(query_param("$select", crate::provider::graph::BODY_FIELDS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "g-od1", "hasAttachments": false,
            "body": {"contentType": "text", "content": "written in Outlook"}
        })))
        .mount(&c.g.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{ROOT}/me/messages/g-od1")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&c.g.server)
        .await;
    let message = c.message("g-od1");
    let import = DraftImportRequest {
        account: "corp".into(),
        message,
        client_token: "imp-1".into(),
    };
    let imported = c.client.call::<DraftImport>(&import).await.unwrap();
    let d = c.store().draft(imported.draft).unwrap().unwrap();
    assert_eq!(
        (d.remote_draft_id.as_deref(), d.remote_message_id.as_deref()),
        (Some("g-od1"), Some("g-od1")),
        "bound to the Graph id"
    );
    assert_eq!(d.source_remote_id, None);
    assert!(d.body.contains("written in Outlook"));
    let discard = DraftDiscardRequest {
        draft: imported.draft,
    };
    c.client.call::<DraftDiscard>(&discard).await.unwrap();
    assert!(c.store().draft(imported.draft).unwrap().is_none());
    c.until_request("DELETE", "me/messages/g-od1").await;
}

#[tokio::test]
async fn refusals_on_a_microsoft_365_draft_name_microsoft_365() {
    let c = corp().await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-od2")))
        .and(query_param("$select", "id,isDraft"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&c.g.server)
        .await;
    let import = DraftImportRequest {
        account: "corp".into(),
        message: c.message("g-od2"),
        client_token: "imp-2".into(),
    };
    let error = c.client.call::<DraftImport>(&import).await.unwrap_err();
    assert_eq!(error.message, "Microsoft 365 no longer has this draft");
    let draft = {
        let store = Store::open(&c.paths.db_file()).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO drafts (account_id, kind, message_id, created_at, updated_at)
                 VALUES (1, 'new', '<p1@corp.example>', 0, 0)",
                [],
            )
            .unwrap();
        let draft = store.conn().last_insert_rowid();
        store
            .conn()
            .execute(
                "INSERT INTO outbox (account_id, kind, payload, state, client_token, draft_id)
                 VALUES (1, 'save_draft', ?1, 'inflight', 'push-x', ?2)",
                rusqlite::params![
                    format!("{{\"kind\":\"push_draft\",\"draft\":{draft}}}"),
                    draft
                ],
            )
            .unwrap();
        draft
    };
    let error = c
        .client
        .call::<DraftDiscard>(&DraftDiscardRequest { draft })
        .await
        .unwrap_err();
    assert_eq!(
        (error.class, error.message.as_str()),
        (
            ErrorClass::Transient,
            "draft: saving to Microsoft 365, try again in a moment"
        )
    );
}
