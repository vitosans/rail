//! What Outlook keeps on its own, through a real daemon over fake Graph: a forward carries only
//! its source's plain files (Outlook's `createForward` copies the inline picture, the attached
//! message and the link), and an imported Outlook draft fetches only its plain files. The save
//! that follows writes the body as plain text, so it deletes the inline picture, and uploads,
//! deletes and fetches nothing else.

use std::time::Duration;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::m365_draft_tests::{Corp, corp, save};
use super::m365_tests::BOUND;
use crate::ops::op::{DraftDiscard, DraftImport, DraftPush, DraftSave};
use crate::ops::{DraftDiscardRequest, DraftImportRequest, DraftPushRequest, ReplyTo};
use crate::provider::graph::fixtures::ROOT;
use crate::store::Store;
use crate::store::drafts::{Attachment, DraftKind};

const FILE: &str = "#microsoft.graph.fileAttachment";

/// A plain file, an inline picture, an item attachment (an attached message) and a reference
/// attachment (a OneDrive link), with ids `{prefix}1`, 2, 3 and 4.
fn listing(prefix: &str) -> Value {
    json!({"value": [
        {"@odata.type": FILE, "id": format!("{prefix}1"), "name": "plan.pdf",
         "contentType": "application/pdf", "size": 2048, "isInline": false},
        {"@odata.type": FILE, "id": format!("{prefix}2"), "name": "logo.png",
         "contentType": "image/png", "size": 10, "isInline": true},
        {"@odata.type": "#microsoft.graph.itemAttachment", "id": format!("{prefix}3"),
         "name": "Re: plan", "contentType": null, "size": 300, "isInline": false},
        {"@odata.type": "#microsoft.graph.referenceAttachment", "id": format!("{prefix}4"),
         "name": "Budget.xlsx", "contentType": null, "size": 0, "isInline": false}
    ]})
}

async fn mount(c: &Corp, verb: &str, at: &str, answer: ResponseTemplate) {
    Mock::given(method(verb))
        .and(path(format!("{ROOT}/{at}")))
        .respond_with(answer)
        .mount(&c.g.server)
        .await;
}

impl Corp {
    /// Push `draft`, make it due now and wait until its row is done (gone) or failed.
    pub(super) async fn pushed(&self, draft: i64, token: &str) {
        let request = DraftPushRequest {
            draft,
            client_token: token.into(),
        };
        let row = self.client.call::<DraftPush>(&request).await.unwrap().row;
        self.finished(row).await;
    }

    /// Make the outbox row `row` due now and wait until it is done (gone) or failed.
    pub(super) async fn finished(&self, row: i64) {
        Store::open(&self.paths.db_file())
            .unwrap()
            .conn()
            .execute("UPDATE outbox SET not_before = 0 WHERE id = ?1", [row])
            .unwrap();
        let wake = DraftDiscardRequest { draft: 999_999 };
        self.client.call::<DraftDiscard>(&wake).await.unwrap();
        tokio::time::timeout(BOUND, async {
            loop {
                let state: Option<String> = self
                    .store()
                    .conn()
                    .query_row("SELECT state FROM outbox WHERE id = ?1", [row], |r| {
                        r.get(0)
                    })
                    .ok();
                if state.is_none_or(|s| s == "failed") {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the row finished");
    }

    /// The requests that would add, delete or fetch an attachment.
    async fn attachment_writes_and_fetches(&self) -> Vec<String> {
        let seen = self.g.server.received_requests().await.unwrap();
        seen.iter()
            .filter(|r| {
                let at = r.url.path();
                at.ends_with("/$value")
                    || (r.method.as_str() != "GET" && at.contains("/attachments"))
            })
            .map(|r| format!("{} {}", r.method, r.url.path()))
            .collect()
    }

    fn remote_draft(&self, draft: i64) -> Option<String> {
        self.store().draft(draft).unwrap().unwrap().remote_draft_id
    }
}

#[tokio::test]
async fn a_forward_carries_only_plain_files_and_its_save_drops_only_the_inline_picture() {
    let c = corp().await;
    let m1 = c.message("m1");
    let store = Store::open(&c.paths.db_file()).unwrap();
    for (part, name, size, plain) in [
        ("a1", "plan.pdf", 2048, 1),
        ("a2", "logo.png", 10, 0),
        ("a3", "Re: plan", 300, 0),
        ("a4", "Budget.xlsx", 0, 0),
    ] {
        store
            .conn()
            .execute(
                "INSERT INTO attachments (message_id, remote_part_id, filename, mime_type, size,
                                          plain_file)
                 VALUES (?1, ?2, ?3, 'application/octet-stream', ?4, ?5)",
                rusqlite::params![m1, part, name, size, plain],
            )
            .unwrap();
    }
    drop(store);
    let request = save(DraftKind::Forward, Some(ReplyTo { message: m1 }), "fwd-k");
    let forward = c.client.call::<DraftSave>(&request).await.unwrap().draft;
    let parts: Vec<String> = c
        .store()
        .draft(forward)
        .unwrap()
        .unwrap()
        .attachments
        .iter()
        .map(|a| match a {
            Attachment::Part { part, .. } => part.clone(),
            Attachment::File { .. } => "file".into(),
        })
        .collect();
    assert_eq!(parts, ["a1"], "only the plain file is carried");
    mount(
        &c,
        "GET",
        "me/messages",
        ResponseTemplate::new(200).set_body_json(json!({"value": []})),
    )
    .await;
    let created = json!({"id": "g-f1", "isDraft": true});
    mount(
        &c,
        "POST",
        "me/messages/m1/createForward",
        ResponseTemplate::new(201).set_body_json(created),
    )
    .await;
    mount(
        &c,
        "PATCH",
        "me/messages/g-f1",
        ResponseTemplate::new(200).set_body_json(json!({"id": "g-f1"})),
    )
    .await;
    mount(
        &c,
        "GET",
        "me/messages/g-f1/attachments",
        ResponseTemplate::new(200).set_body_json(listing("c")),
    )
    .await;
    c.pushed(forward, "push-fk").await;
    assert_eq!(
        c.attachment_writes_and_fetches().await,
        [format!("DELETE {ROOT}/me/messages/g-f1/attachments/c2")],
        "the plain file, the attached message and the link stay"
    );
    assert_eq!(c.remote_draft(forward).as_deref(), Some("g-f1"), "saved");
}

#[tokio::test]
async fn an_imported_outlook_draft_fetches_only_plain_files_and_its_save_drops_its_picture() {
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
            "id": "g-od1", "hasAttachments": true,
            "body": {"contentType": "html", "content": "<p>see the logo</p>"}
        })))
        .mount(&c.g.server)
        .await;
    let mut kept = listing("o");
    kept["value"].as_array_mut().unwrap().retain(|a| {
        let id = a["id"].as_str().unwrap();
        id == "o2" || id == "o4"
    });
    mount(
        &c,
        "GET",
        "me/messages/g-od1/attachments",
        ResponseTemplate::new(200).set_body_json(kept),
    )
    .await;
    mount(
        &c,
        "PATCH",
        "me/messages/g-od1",
        ResponseTemplate::new(200).set_body_json(json!({"id": "g-od1"})),
    )
    .await;
    let import = DraftImportRequest {
        account: "corp".into(),
        message: c.message("g-od1"),
        client_token: "imp-k".into(),
    };
    let draft = c.client.call::<DraftImport>(&import).await.unwrap().draft;
    let d = c.store().draft(draft).unwrap().unwrap();
    assert!(d.attachments.is_empty(), "nothing Outlook keeps is fetched");
    c.pushed(draft, "push-ik").await;
    assert_eq!(
        c.attachment_writes_and_fetches().await,
        [format!("DELETE {ROOT}/me/messages/g-od1/attachments/o2")],
        "the link stays"
    );
    let patches = c.g.server.received_requests().await.unwrap();
    let patched = patches.iter().any(|r| {
        r.method.as_str() == "PATCH" && r.url.path() == format!("{ROOT}/me/messages/g-od1")
    });
    assert!(patched, "the first save went to the Outlook draft");
}
