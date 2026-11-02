//! Graph lists a file attachment larger than its bytes, by an amount Rail cannot predict, so a
//! draft's file is matched by the size Outlook listed for it once Rail uploaded or imported it.
//! Through a real daemon over fake Graph: a second save with no change uploads and deletes
//! nothing, a send uploads a new file once, a replaced file goes up again, and an imported
//! Outlook draft's first save uploads nothing for its files.

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::m365_draft_tests::{Corp, corp, save};
use crate::ops::op::{DraftImport, DraftSave, DraftSend};
use crate::ops::{DraftAttachment, DraftImportRequest, DraftSaveRequest, DraftSendRequest};
use crate::provider::graph::fixtures::ROOT;
use crate::store::drafts::DraftKind;

const FILE: &str = "#microsoft.graph.fileAttachment";
/// The note's bytes: 41 of them, which Graph lists as 239.
const NOTE: &[u8] = b"a note of forty-one bytes for the test..\n";
const NOTE_LISTED: usize = 239;

fn note_listing(id: &str, size: usize) -> Value {
    json!({"value": [{"@odata.type": FILE, "id": id, "name": "note.txt",
                      "contentType": "text/plain", "size": size, "isInline": false}]})
}

async fn mount(c: &Corp, verb: &str, at: &str, answer: ResponseTemplate) {
    Mock::given(method(verb))
        .and(path(format!("{ROOT}/{at}")))
        .respond_with(answer)
        .mount(&c.g.server)
        .await;
}

/// A new Outlook draft `g-n1`: the `Message-ID` lookup finds none, the create makes it, a
/// `PATCH` and a send are accepted. Its attachments list empty until the first upload, then
/// list `listed_after`; an upload answers `att-{n}` with the size Graph lists for the note.
async fn new_draft_at_outlook(c: &Corp, listed_after: Value) {
    mount(
        c,
        "GET",
        "me/messages",
        ResponseTemplate::new(200).set_body_json(json!({"value": []})),
    )
    .await;
    mount(
        c,
        "POST",
        "me/messages",
        ResponseTemplate::new(201).set_body_json(json!({"id": "g-n1", "isDraft": true})),
    )
    .await;
    mount(
        c,
        "PATCH",
        "me/messages/g-n1",
        ResponseTemplate::new(200).set_body_json(json!({"id": "g-n1"})),
    )
    .await;
    mount(
        c,
        "POST",
        "me/messages/g-n1/send",
        ResponseTemplate::new(202),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/g-n1/attachments")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": []})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&c.g.server)
        .await;
    mount(
        c,
        "GET",
        "me/messages/g-n1/attachments",
        ResponseTemplate::new(200).set_body_json(listed_after),
    )
    .await;
    mount(
        c,
        "POST",
        "me/messages/g-n1/attachments",
        ResponseTemplate::new(201)
            .set_body_json(json!({"id": "att-1", "name": "note.txt", "size": NOTE_LISTED})),
    )
    .await;
}

/// The requests that add or delete an attachment, in order.
async fn attachment_writes(c: &Corp) -> Vec<String> {
    let seen = c.g.server.received_requests().await.unwrap();
    seen.iter()
        .filter(|r| r.method.as_str() != "GET" && r.url.path().contains("/attachments"))
        .map(|r| format!("{} {}", r.method, r.url.path()))
        .collect()
}

fn with_file(mut request: DraftSaveRequest, file: &std::path::Path) -> DraftSaveRequest {
    request.attachments = vec![DraftAttachment::File {
        path: file.to_str().unwrap().into(),
    }];
    request
}

fn upload() -> String {
    format!("POST {ROOT}/me/messages/g-n1/attachments")
}

#[tokio::test]
async fn a_second_save_with_no_change_uploads_and_deletes_nothing() {
    let c = corp().await;
    new_draft_at_outlook(&c, note_listing("att-1", NOTE_LISTED)).await;
    let files = tempfile::tempdir().unwrap();
    let note = files.path().join("note.txt");
    std::fs::write(&note, NOTE).unwrap();
    let first = with_file(save(DraftKind::New, None, "s-1"), &note);
    let draft = c.client.call::<DraftSave>(&first).await.unwrap().draft;
    c.pushed(draft, "p-1").await;
    assert_eq!(
        attachment_writes(&c).await,
        [upload()],
        "the first push uploads"
    );
    let mut again = with_file(save(DraftKind::New, None, "s-2"), &note);
    again.draft = Some(draft);
    c.client.call::<DraftSave>(&again).await.unwrap();
    c.pushed(draft, "p-2").await;
    assert_eq!(
        attachment_writes(&c).await,
        [upload()],
        "the second push matches the note by the size Outlook listed"
    );
}

#[tokio::test]
async fn a_send_uploads_a_new_file_once() {
    let c = corp().await;
    new_draft_at_outlook(&c, note_listing("att-1", NOTE_LISTED)).await;
    let files = tempfile::tempdir().unwrap();
    let note = files.path().join("note.txt");
    std::fs::write(&note, NOTE).unwrap();
    let request = with_file(save(DraftKind::New, None, "s-1"), &note);
    let draft = c.client.call::<DraftSave>(&request).await.unwrap().draft;
    let send = DraftSendRequest {
        draft,
        client_token: "send-1".into(),
    };
    let row = c.client.call::<DraftSend>(&send).await.unwrap().row;
    c.finished(row).await;
    let seen = c.g.server.received_requests().await.unwrap();
    assert!(
        seen.iter()
            .any(|r| r.url.path() == format!("{ROOT}/me/messages/g-n1/send")),
        "sent"
    );
    assert_eq!(attachment_writes(&c).await, [upload()], "uploaded once");
}

#[tokio::test]
async fn a_replaced_file_is_uploaded_again() {
    let c = corp().await;
    new_draft_at_outlook(&c, note_listing("att-1", NOTE_LISTED)).await;
    let files = tempfile::tempdir().unwrap();
    let note = files.path().join("note.txt");
    std::fs::write(&note, NOTE).unwrap();
    let first = with_file(save(DraftKind::New, None, "s-1"), &note);
    let draft = c.client.call::<DraftSave>(&first).await.unwrap().draft;
    c.pushed(draft, "p-1").await;
    std::fs::write(&note, b"another note, of other bytes\n").unwrap();
    let mut replaced = with_file(save(DraftKind::New, None, "s-2"), &note);
    replaced.draft = Some(draft);
    c.client.call::<DraftSave>(&replaced).await.unwrap();
    c.pushed(draft, "p-2").await;
    assert_eq!(
        attachment_writes(&c).await,
        [
            upload(),
            format!("DELETE {ROOT}/me/messages/g-n1/attachments/att-1"),
            upload()
        ],
        "the old copy goes and the new bytes go up"
    );
}

#[tokio::test]
async fn an_imported_outlook_drafts_saves_upload_nothing_for_its_files() {
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
            "body": {"contentType": "text", "content": "written in Outlook"}
        })))
        .mount(&c.g.server)
        .await;
    mount(
        &c,
        "GET",
        "me/messages/g-od1/attachments",
        ResponseTemplate::new(200).set_body_json(note_listing("o1", NOTE_LISTED)),
    )
    .await;
    mount(
        &c,
        "GET",
        "me/messages/g-od1/attachments/o1/$value",
        ResponseTemplate::new(200).set_body_bytes(NOTE.to_vec()),
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
        client_token: "imp-1".into(),
    };
    let draft = c.client.call::<DraftImport>(&import).await.unwrap().draft;
    let d = c.store().draft(draft).unwrap().unwrap();
    assert_eq!(d.attachments.len(), 1, "the note is fetched");
    c.pushed(draft, "p-1").await;
    assert_eq!(
        attachment_writes(&c).await,
        Vec::<String>::new(),
        "first save"
    );
    let kept = d.attachments[0].clone();
    let crate::store::drafts::Attachment::File { path: file, .. } = kept else {
        panic!("a file");
    };
    let mut edited = with_file(save(DraftKind::New, None, "s-1"), file.as_ref());
    edited.draft = Some(draft);
    edited.body = "edited in Rail".into();
    c.client.call::<DraftSave>(&edited).await.unwrap();
    c.pushed(draft, "p-2").await;
    assert_eq!(attachment_writes(&c).await, Vec::<String>::new(), "an edit");
}

#[tokio::test]
async fn a_file_overwritten_at_the_same_length_is_uploaded_again() {
    use base64::Engine as _;
    let c = corp().await;
    new_draft_at_outlook(&c, note_listing("att-1", NOTE_LISTED)).await;
    let files = tempfile::tempdir().unwrap();
    let note = files.path().join("note.txt");
    std::fs::write(&note, NOTE).unwrap();
    let first = with_file(save(DraftKind::New, None, "s-1"), &note);
    let draft = c.client.call::<DraftSave>(&first).await.unwrap().draft;
    c.pushed(draft, "p-1").await;
    let same_length = b"A NOTE OF FORTY-ONE BYTES FOR THE TEST..\n";
    assert_eq!(same_length.len(), NOTE.len());
    std::fs::write(&note, same_length).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&note)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let mut overwritten = with_file(save(DraftKind::New, None, "s-2"), &note);
    overwritten.draft = Some(draft);
    c.client.call::<DraftSave>(&overwritten).await.unwrap();
    c.pushed(draft, "p-2").await;
    assert_eq!(
        attachment_writes(&c).await,
        [
            upload(),
            format!("DELETE {ROOT}/me/messages/g-n1/attachments/att-1"),
            upload()
        ],
        "same size, newer time: the old copy goes and the new bytes go up"
    );
    let seen = c.g.server.received_requests().await.unwrap();
    let last_upload = seen
        .iter()
        .rfind(|r| r.method.as_str() == "POST" && r.url.path().ends_with("/attachments"))
        .unwrap();
    let body: Value = serde_json::from_slice(&last_upload.body).unwrap();
    let sent = base64::engine::general_purpose::STANDARD
        .decode(body["contentBytes"].as_str().unwrap())
        .unwrap();
    assert_eq!(sent, same_length, "the new bytes");
}
