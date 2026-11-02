//! Bodies and attachments against fake Graph: HTML rendered with its links, text kept as it
//! is, the attachment list fetched with the body (only a file that is not inline is a plain
//! file), a body for another message, bytes, a reference attachment refused, a gone one, and
//! ids Rail would not send.

use serde_json::{Value, json};
use wiremock::matchers::{headers, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::attachment::REFERENCE_ATTACHMENT;
use super::body::{BODY_FIELDS, HTML_BODY};
use super::fixtures::{Graph, ROOT};
use crate::provider::http::IMMUTABLE_IDS;
use crate::provider::{AttachmentMeta, Provider, ProviderError};

/// What the attachment listing asks for.
/// What the attachment listing asks for: `contentId` is a file attachment's own property, so
/// it is asked for through a type cast (Graph refuses it on the base type with HTTP 400).
const LISTED: &str = "id,name,contentType,size,isInline,microsoft.graph.fileAttachment/contentId";

async fn serve_body(g: &Graph, id: &str, answer: Value) {
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/{id}")))
        .and(query_param("$select", BODY_FIELDS))
        .and(headers("Prefer", vec![IMMUTABLE_IDS, HTML_BODY]))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .expect(1)
        .mount(&g.server)
        .await;
}

#[tokio::test]
async fn an_html_body_is_rendered_with_its_links_and_kept() {
    let g = Graph::start().await;
    let html = "<p>Hello <a href=\"https://example.com/x\">there</a></p><script>steal()</script>";
    let answer = json!({"id": "m1", "hasAttachments": false,
                        "body": {"contentType": "html", "content": html}});
    serve_body(&g, "m1", answer).await;
    let body = g.provider().body("m1").await.unwrap();
    assert!(body.text.contains("Hello there[1]"), "{:?}", body.text);
    assert!(!body.text.contains("steal"), "{:?}", body.text);
    assert_eq!(body.links, ["https://example.com/x"]);
    assert_eq!(body.html.as_deref(), Some(html));
    assert!(body.attachments.is_empty() && !body.truncated);
}

#[tokio::test]
async fn a_text_body_is_used_as_it_is() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": false, "body": {"contentType": "text",
                        "content": "Plain words\r\nsee https://example.com/y"}});
    serve_body(&g, "m1", answer).await;
    let body = g.provider().body("m1").await.unwrap();
    assert!(
        body.text
            .starts_with("Plain words\nsee https://example.com/y"),
        "{:?}",
        body.text
    );
    assert_eq!(body.links, ["https://example.com/y"]);
    assert_eq!(body.html, None);
}

#[tokio::test]
async fn attachments_are_listed_with_the_body_inline_ones_too() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": true,
                        "body": {"contentType": "html", "content": "<p>see attached</p>"}});
    serve_body(&g, "m1", answer).await;
    let listed = json!({"value": [
        {"@odata.type": "#microsoft.graph.fileAttachment", "id": "a1", "name": "plan.pdf",
         "contentType": "application/pdf", "size": 2048, "isInline": false},
        {"@odata.type": "#microsoft.graph.fileAttachment", "id": "a2", "name": "logo.png",
         "contentType": "image/PNG", "size": 10, "isInline": true, "contentId": "<logo@x>"},
        {"@odata.type": "#microsoft.graph.referenceAttachment", "id": "a3",
         "name": "Budget.xlsx", "contentType": null, "size": 0, "isInline": false},
        {"id": "a/4", "name": "sneaky", "contentType": "text/plain", "size": 1, "isInline": false},
        {"@odata.type": "#microsoft.graph.itemAttachment", "id": "a5", "name": "Re: plan",
         "contentType": null, "size": 300, "isInline": false}
    ]});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments")))
        .and(query_param("$select", LISTED))
        .respond_with(ResponseTemplate::new(200).set_body_json(listed))
        .expect(1)
        .mount(&g.server)
        .await;
    let body = g.provider().body("m1").await.unwrap();
    let meta = |part: &str, name: &str, mime: &str, size: i64, plain_file| AttachmentMeta {
        part_id: part.into(),
        filename: name.into(),
        mime_type: mime.into(),
        size,
        plain_file,
        content_id: None,
    };
    let logo = AttachmentMeta {
        content_id: Some("logo@x".into()),
        ..meta("a2", "logo.png", "image/png", 10, false)
    };
    assert_eq!(
        body.attachments,
        [
            meta("a1", "plan.pdf", "application/pdf", 2048, true),
            logo,
            meta("a3", "Budget.xlsx", "application/octet-stream", 0, false),
            meta("a5", "Re: plan", "application/octet-stream", 300, false),
        ],
        "only a file that is not inline is a plain file"
    );
}

#[tokio::test]
async fn a_body_for_another_message_or_an_unsafe_id_is_unreadable() {
    let g = Graph::start().await;
    let answer = json!({"id": "m2", "body": {"contentType": "html", "content": "x"}});
    serve_body(&g, "m1", answer).await;
    let provider = g.provider();
    let other = provider.body("m1").await.unwrap_err();
    assert!(
        matches!(other, ProviderError::Permanent { .. }),
        "{other:?}"
    );
    let unsafe_id = provider.body("../m1").await.unwrap_err();
    assert!(matches!(unsafe_id, ProviderError::Permanent { .. }));
    assert_eq!(g.server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn attachment_bytes_come_from_the_value_endpoint() {
    let g = Graph::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments/a1/$value")))
        .and(headers("Prefer", vec![IMMUTABLE_IDS]))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3]))
        .expect(1)
        .mount(&g.server)
        .await;
    let bytes = g
        .provider()
        .attachment(&"m1".to_string(), "a1")
        .await
        .unwrap();
    assert_eq!(bytes, [1, 2, 3]);
}

#[tokio::test]
async fn a_reference_attachment_is_refused_with_where_to_reach_it() {
    let g = Graph::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments/a3/$value")))
        .respond_with(ResponseTemplate::new(405).set_body_json(
            json!({"error": {"code": "ErrorInvalidRequest", "message": "no value"}}),
        ))
        .mount(&g.server)
        .await;
    let error = g
        .provider()
        .attachment(&"m1".to_string(), "a3")
        .await
        .unwrap_err();
    assert_eq!(error, ProviderError::permanent(REFERENCE_ATTACHMENT));
}

#[tokio::test]
async fn a_gone_attachment_is_not_found_and_unsafe_ids_are_refused_unsent() {
    let g = Graph::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments/a9/$value")))
        .respond_with(ResponseTemplate::new(404))
        .mount(&g.server)
        .await;
    let provider = g.provider();
    let gone = provider.attachment(&"m1".to_string(), "a9").await;
    assert_eq!(gone, Err(ProviderError::NotFound));
    let refused = provider
        .attachment(&"m1".to_string(), "a/1")
        .await
        .unwrap_err();
    assert!(matches!(refused, ProviderError::Permanent { .. }));
    assert_eq!(g.server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_html_body_naming_cid_images_lists_its_inline_parts_though_graph_says_none() {
    let g = Graph::start().await;
    // Graph answers `hasAttachments: false` when every attachment is inline.
    let answer = json!({"id": "m1", "hasAttachments": false,
                        "body": {"contentType": "html", "content": "<img src=\"cid:logo@x\">"}});
    serve_body(&g, "m1", answer).await;
    let listed = json!({"value": [
        {"@odata.type": "#microsoft.graph.fileAttachment", "id": "a1", "name": "logo.png",
         "contentType": "image/png", "size": 10, "isInline": true, "contentId": "logo@x"}
    ]});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments")))
        .and(query_param("$select", LISTED))
        .respond_with(ResponseTemplate::new(200).set_body_json(listed))
        .expect(1)
        .mount(&g.server)
        .await;
    let body = g.provider().body("m1").await.unwrap();
    assert_eq!(body.attachments.len(), 1);
    assert_eq!(body.attachments[0].content_id.as_deref(), Some("logo@x"));
}

async fn refuse_the_listing(g: &Graph) {
    let refusal = json!({"error": {"code": "BadRequest",
                                   "message": "Parsing OData Select and Expand failed"}});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments")))
        .respond_with(ResponseTemplate::new(400).set_body_json(refusal))
        .expect(1)
        .mount(&g.server)
        .await;
}

#[tokio::test]
async fn a_refused_listing_asked_for_on_a_cid_alone_keeps_the_body() {
    let g = Graph::start().await;
    let html = "<p>Hello</p><img src=\"cid:logo@x\">";
    let answer = json!({"id": "m1", "hasAttachments": false,
                        "body": {"contentType": "html", "content": html}});
    serve_body(&g, "m1", answer).await;
    refuse_the_listing(&g).await;
    let body = g.provider().body("m1").await.unwrap();
    assert!(body.text.contains("Hello"), "{:?}", body.text);
    assert_eq!(body.html.as_deref(), Some(html));
    assert!(body.attachments.is_empty() && body.attachments_unknown);
}

#[tokio::test]
async fn a_refused_listing_graph_said_has_attachments_keeps_the_body_with_them_unknown() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": true,
                        "body": {"contentType": "html", "content": "<p>Hello</p>"}});
    serve_body(&g, "m1", answer).await;
    refuse_the_listing(&g).await;
    let body = g.provider().body("m1").await.unwrap();
    assert!(body.text.contains("Hello"), "{:?}", body.text);
    assert!(body.attachments.is_empty() && body.attachments_unknown);
}

#[tokio::test]
async fn an_upper_case_cid_lists_the_inline_parts_too() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": false,
                        "body": {"contentType": "html", "content": "<img src=\"CID:logo@x\">"}});
    serve_body(&g, "m1", answer).await;
    let listed = json!({"value": [
        {"@odata.type": "#microsoft.graph.fileAttachment", "id": "a1", "name": "logo.png",
         "contentType": "image/png", "size": 10, "isInline": true, "contentId": "logo@x"}
    ]});
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/me/messages/m1/attachments")))
        .and(query_param("$select", LISTED))
        .respond_with(ResponseTemplate::new(200).set_body_json(listed))
        .expect(1)
        .mount(&g.server)
        .await;
    let body = g.provider().body("m1").await.unwrap();
    assert_eq!(body.attachments.len(), 1);
    assert!(!body.attachments_unknown);
}

#[tokio::test]
async fn the_topmost_authentication_results_say_which_domains_were_vouched_for() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": false,
    "body": {"contentType": "text", "content": "hi"},
    "internetMessageHeaders": [
        {"name": "Received", "value": "from x"},
        {"name": "Authentication-Results",
         "value": "spf=pass smtp.mailfrom=example.com; dkim=pass header.d=example.com; dmarc=pass action=none header.from=example.com"},
        {"name": "Authentication-Results",
         "value": "dmarc=pass header.from=forged.example"}
    ]});
    serve_body(&g, "m1", answer).await;
    let body = g.provider().body("m1").await.unwrap();
    assert_eq!(body.vouched_domains, ["example.com"]);
}

#[tokio::test]
async fn a_message_without_authentication_results_vouches_for_nothing() {
    let g = Graph::start().await;
    let answer = json!({"id": "m1", "hasAttachments": false,
                        "body": {"contentType": "text", "content": "hi"}});
    serve_body(&g, "m1", answer).await;
    let body = g.provider().body("m1").await.unwrap();
    assert!(body.vouched_domains.is_empty());
}
