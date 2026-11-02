//! An Outlook draft keeps its immutable id when its body or attachments change.

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::m365_draft_tests::{Corp, corp, save};
use crate::error::ErrorClass;
use crate::ops::op::{DraftImport, DraftSave};
use crate::ops::{DraftImportRequest, ErrorCode};
use crate::provider::graph::fixtures::ROOT;
use crate::provider::{AttachmentMeta, Body};
use crate::store::Store;
use crate::store::drafts::{Attachment, DraftKind};
use crate::sync::batch::Batch;

fn cache(c: &Corp, attachments: Vec<AttachmentMeta>) {
    let batch = Batch {
        bodies: vec![(
            "g-od1".into(),
            Body {
                text: "old Outlook text".into(),
                attachments,
                ..Body::default()
            },
        )],
        ..Batch::new(1)
    };
    Store::open(&c.paths.db_file())
        .unwrap()
        .apply(&batch, 1)
        .unwrap();
}

fn import(c: &Corp, token: &str) -> DraftImportRequest {
    DraftImportRequest {
        account: "corp".into(),
        message: c.message("g-od1"),
        client_token: token.into(),
    }
}

async fn remote_body(c: &Corp, response: ResponseTemplate) {
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
        .respond_with(response)
        .mount(&c.g.server)
        .await;
}

fn body(has_attachments: bool) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({"id": "g-od1", "hasAttachments": has_attachments,
        "body": {"contentType": "text", "content": "current Outlook text"}}))
}

#[tokio::test]
async fn importing_an_outlook_draft_refreshes_its_cached_body_and_attachments() {
    let c = corp().await;
    cache(&c, vec![]);
    remote_body(&c, body(true)).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-od1/attachments")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [{
            "@odata.type": "#microsoft.graph.fileAttachment", "id": "new-file", "name": "new.txt",
            "contentType": "text/plain", "size": 200, "isInline": false}]})))
        .expect(1)
        .mount(&c.g.server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "{ROOT}/me/messages/g-od1/attachments/new-file/$value"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new file contents".to_vec()))
        .expect(1)
        .mount(&c.g.server)
        .await;
    let id = c
        .client
        .call::<DraftImport>(&import(&c, "refresh-1"))
        .await
        .unwrap()
        .draft;
    let draft = c.store().draft(id).unwrap().unwrap();
    assert_eq!(draft.body, "current Outlook text");
    assert_eq!(draft.attachments.len(), 1);
    let Attachment::File { path, name, .. } = &draft.attachments[0] else {
        panic!("an imported file")
    };
    assert_eq!(name, "new.txt");
    assert_eq!(std::fs::read(path).unwrap(), b"new file contents");
    let cached = c
        .store()
        .import_source(c.message("g-od1"))
        .unwrap()
        .unwrap();
    assert_eq!(cached.body_text.as_deref(), Some("current Outlook text"));
    assert_eq!(cached.attachments[0].part_id, "new-file");
}

#[tokio::test]
async fn an_attachment_removed_in_outlook_is_not_imported_from_the_cache() {
    let c = corp().await;
    cache(
        &c,
        vec![AttachmentMeta {
            part_id: "gone-file".into(),
            filename: "gone.txt".into(),
            mime_type: "text/plain".into(),
            size: 12,
            plain_file: true,
            content_id: None,
        }],
    );
    remote_body(&c, body(false)).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "{ROOT}/me/messages/g-od1/attachments/gone-file/$value"
        )))
        .respond_with(ResponseTemplate::new(404))
        .expect(0)
        .mount(&c.g.server)
        .await;
    let id = c
        .client
        .call::<DraftImport>(&import(&c, "removed-1"))
        .await
        .unwrap()
        .draft;
    let draft = c.store().draft(id).unwrap().unwrap();
    assert!(draft.attachments.is_empty());
    assert_eq!(draft.body, "current Outlook text");
}

#[tokio::test]
async fn a_failed_outlook_refresh_does_not_import_the_cached_body() {
    for (status, code, class) in [
        (200, ErrorCode::Provider, ErrorClass::Permanent),
        (403, ErrorCode::Provider, ErrorClass::Permanent),
        (404, ErrorCode::NotFound, ErrorClass::Permanent),
        (503, ErrorCode::Network, ErrorClass::Transient),
    ] {
        let c = corp().await;
        cache(&c, vec![]);
        remote_body(
            &c,
            ResponseTemplate::new(status).set_body_string("not JSON"),
        )
        .await;
        let error = c
            .client
            .call::<DraftImport>(&import(&c, "failed-1"))
            .await
            .unwrap_err();
        assert_eq!((error.code, error.class), (code, class));
        assert!(c.store().draft_token_owner("failed-1").unwrap().is_none());
        let cached = c
            .store()
            .import_source(c.message("g-od1"))
            .unwrap()
            .unwrap();
        assert_eq!(cached.body_text.as_deref(), Some("old Outlook text"));
        assert!(!c.paths.import_dir("failed-1").unwrap().exists());
    }
}

#[tokio::test]
async fn reopening_an_imported_outlook_draft_keeps_local_edits() {
    let c = corp().await;
    cache(&c, vec![]);
    remote_body(&c, body(false)).await;
    let before = c.g.server.received_requests().await.unwrap().len();
    let id = c
        .client
        .call::<DraftImport>(&import(&c, "first-1"))
        .await
        .unwrap()
        .draft;
    let mut edited = save(DraftKind::New, None, "edit-1");
    edited.draft = Some(id);
    edited.body = "edited locally".into();
    c.client.call::<DraftSave>(&edited).await.unwrap();
    let again = c
        .client
        .call::<DraftImport>(&import(&c, "again-1"))
        .await
        .unwrap()
        .draft;
    assert_eq!(again, id);
    assert_eq!(c.store().draft(id).unwrap().unwrap().body, "edited locally");
    let bodies =
        c.g.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .skip(before)
            .filter(|r| {
                r.url.path() == format!("{ROOT}/me/messages/g-od1")
                    && r.url.query_pairs().any(|(key, value)| {
                        key == "$select" && value == crate::provider::graph::BODY_FIELDS
                    })
            })
            .count();
    assert_eq!(bodies, 1, "only the first import refreshes the remote body");
}

#[tokio::test]
async fn an_outlook_draft_whose_attachment_list_is_refused_is_not_imported() {
    let c = corp().await;
    cache(&c, vec![]);
    remote_body(&c, body(true)).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-od1/attachments")))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": {"code": "BadRequest", "message": "no"}})),
        )
        .mount(&c.g.server)
        .await;
    let error = c
        .client
        .call::<DraftImport>(&import(&c, "refused-1"))
        .await
        .unwrap_err();
    assert_eq!(
        (error.code, error.class),
        (ErrorCode::Provider, ErrorClass::Permanent)
    );
    assert!(c.store().draft_token_owner("refused-1").unwrap().is_none());
    assert!(!c.paths.import_dir("refused-1").unwrap().exists());
}
