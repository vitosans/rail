//! `format=full` bodies against fake Gmail.

use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, ResponseTemplate};

use super::fixtures::{API_PATH, Gmail, b64, full_json, part_json};
use super::*;

/// Serve `payload` as message `id` and read its body.
async fn body_as(
    gmail: &Gmail,
    id: &str,
    payload: serde_json::Value,
) -> Result<crate::provider::Body, ProviderError> {
    gmail.mount_message(full_json(id, payload)).await;
    gmail.provider().full_body(id).await
}

async fn body_of(
    gmail: &Gmail,
    payload: serde_json::Value,
) -> Result<crate::provider::Body, ProviderError> {
    body_as(gmail, "m1", payload).await
}

#[tokio::test]
async fn multipart_alternative_prefers_the_html_rendered_without_scripts() {
    let gmail = Gmail::start().await;
    let html = b"<html><head><style>p{color:red}</style><script>steal()</script></head><body><p>Hello <b>Dana</b></p><script>alert(1)</script></body></html>";
    let payload = json!({
        "partId": "", "mimeType": "multipart/alternative", "filename": "", "headers": [],
        "body": {"size": 0},
        "parts": [
            part_json("text/plain", "text/plain; charset=UTF-8", b"Hello Dana (plain)"),
            part_json("text/html", "text/html; charset=\"utf-8\"", html)
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    assert!(body.text.contains("Hello Dana"), "{:?}", body.text);
    assert!(
        !body.text.contains("plain"),
        "HTML is preferred: {:?}",
        body.text
    );
    for leaked in ["steal", "alert", "color:red", "**"] {
        assert!(!body.text.contains(leaked), "{leaked}: {:?}", body.text);
    }
    assert_eq!(body.html.as_deref().map(str::len), Some(html.len()));
    assert!(!body.truncated);
}

#[tokio::test]
async fn remote_images_and_links_are_never_fetched() {
    let gmail = Gmail::start().await;
    let html = format!(
        "<p>Hi<img src=\"{0}/pixel.gif\" alt=\"logo\"><link rel=stylesheet href=\"{0}/x.css\"><iframe src=\"{0}/frame\"></iframe></p>",
        gmail.server.uri()
    );
    let body = body_of(&gmail, part_json("text/html", "text/html", html.as_bytes()))
        .await
        .unwrap();
    assert!(body.text.contains("Hi"));
    let fetched: Vec<String> = gmail
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert_eq!(
        fetched,
        [format!("{API_PATH}/messages/m1")],
        "only the API was asked"
    );
}

#[tokio::test]
async fn a_plain_body_in_latin1_is_decoded() {
    let gmail = Gmail::start().await;
    let latin1 = b"caf\xe9 cr\xe8me";
    let body = body_of(
        &gmail,
        part_json("text/plain", "text/plain; charset=ISO-8859-1", latin1),
    )
    .await
    .unwrap();
    assert_eq!(body.text, "café crème");
    assert_eq!(body.html, None);
    let unknown = body_as(
        &gmail,
        "m2",
        part_json("text/plain", "text/plain; charset=x-made-up", b"ok \xff"),
    )
    .await
    .unwrap();
    assert!(
        unknown.text.starts_with("ok "),
        "an unknown charset falls back to lossy UTF-8"
    );
}

#[tokio::test]
async fn a_body_over_the_cap_is_truncated_with_a_marker() {
    let gmail = Gmail::start().await;
    let big = vec![b'a'; MAX_BODY_BYTES + 4096];
    let body = body_of(&gmail, part_json("text/plain", "text/plain", &big))
        .await
        .unwrap();
    assert!(body.truncated);
    assert!(body.text.ends_with(TRUNCATED_MARKER));
    assert!(body.text.len() <= MAX_BODY_BYTES + TRUNCATED_MARKER.len());
}

#[tokio::test]
async fn attachments_are_recorded_and_a_large_body_part_is_fetched_separately() {
    let gmail = Gmail::start().await;
    let payload = json!({
        "partId": "", "mimeType": "multipart/mixed", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            {"partId": "0", "mimeType": "text/html", "filename": "",
             "headers": [{"name": "Content-Type", "value": "text/html; charset=utf-8"}],
             "body": {"size": 900000, "attachmentId": "ANGjdJ_big-body"}},
            {"partId": "1", "mimeType": "application/pdf", "filename": "invoice \u{202E}fdp.exe",
             "headers": [], "body": {"size": 12345, "attachmentId": "ANGjdJ_pdf"}}
        ]
    });
    Mock::given(path(format!(
        "{API_PATH}/messages/m1/attachments/ANGjdJ_big-body"
    )))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "size": 20, "data": b64(b"<p>big newsletter</p>")
    })))
    .expect(1)
    .mount(&gmail.server)
    .await;
    let body = body_of(&gmail, payload).await.unwrap();
    assert!(body.text.contains("big newsletter"), "{:?}", body.text);
    assert_eq!(body.attachments.len(), 1);
    let pdf = &body.attachments[0];
    assert_eq!(
        (pdf.part_id.as_str(), pdf.mime_type.as_str(), pdf.size),
        ("1", "application/pdf", 12345)
    );
    assert_eq!(
        pdf.filename, "invoice \u{202E}fdp.exe",
        "kept as outside text; escaped when shown"
    );
}

#[tokio::test]
async fn a_body_rail_cannot_decode_is_a_permanent_error() {
    let gmail = Gmail::start().await;
    let mut part = part_json("text/plain", "text/plain", b"x");
    part["body"]["data"] = json!("***not base64***");
    let err = body_of(&gmail, part).await.unwrap_err();
    assert_eq!(err.class(), crate::error::ErrorClass::Permanent);
    assert_eq!(err.to_string(), "Rail could not read this message's body");
}

#[tokio::test]
async fn a_message_with_no_text_part_has_an_empty_body() {
    let gmail = Gmail::start().await;
    let payload = json!({"partId": "", "mimeType": "image/png", "filename": "a.png", "headers": [],
                         "body": {"size": 3, "attachmentId": "ANGjdJ_png"}});
    let body = body_of(&gmail, payload).await.unwrap();
    assert!(body.text.is_empty());
    assert_eq!(body.attachments.len(), 1);
}

#[tokio::test]
async fn control_and_format_characters_are_removed_but_lines_kept() {
    let gmail = Gmail::start().await;
    let text = "line one\r\nline\ttwo\u{1b}[31m red\0\u{7}\u{202E}rev\u{200B}\u{2066}x\u{FEFF}\u{85}\rthree";
    let body = body_of(
        &gmail,
        part_json("text/plain", "text/plain; charset=utf-8", text.as_bytes()),
    )
    .await
    .unwrap();
    assert_eq!(body.text, "line one\nline\ttwo[31m redrevx\nthree");
    let html = "<p>a\u{1b}]8;;http://x\u{7}b\u{202D}c</p>";
    let body = body_as(
        &gmail,
        "m2",
        part_json("text/html", "text/html", html.as_bytes()),
    )
    .await
    .unwrap();
    assert!(
        body.text.chars().all(|c| c == '\n'
            || c == '\t'
            || !(c.is_control() || crate::config::model::is_format_char(c))),
        "{:?}",
        body.text
    );
    assert!(body.text.contains("bc"), "{:?}", body.text);
}

#[tokio::test]
async fn an_html_body_over_the_cap_is_truncated_too() {
    let gmail = Gmail::start().await;
    let mut html = b"<p>".to_vec();
    html.resize(MAX_BODY_BYTES + 10, b'w');
    let body = body_of(&gmail, part_json("text/html", "text/html", &html))
        .await
        .unwrap();
    assert!(body.truncated);
    assert!(body.text.ends_with(TRUNCATED_MARKER));
    assert!(body.text.len() <= MAX_BODY_BYTES + TRUNCATED_MARKER.len());
    assert!(body.html.unwrap().len() <= MAX_BODY_BYTES);
}

#[tokio::test]
async fn a_latin1_body_that_grows_when_decoded_stays_under_the_cap() {
    let gmail = Gmail::start().await;
    let latin1 = vec![0xe9u8; MAX_BODY_BYTES];
    let body = body_of(
        &gmail,
        part_json("text/plain", "text/plain; charset=iso-8859-1", &latin1),
    )
    .await
    .unwrap();
    assert!(
        body.truncated,
        "two UTF-8 bytes per character is over the cap"
    );
    assert!(body.text.len() <= MAX_BODY_BYTES + TRUNCATED_MARKER.len());
    assert!(body.text.starts_with("éé"));
}

#[tokio::test]
async fn an_unsafe_attachment_id_is_never_requested() {
    let gmail = Gmail::start().await;
    let payload = json!({"partId": "0", "mimeType": "text/plain", "filename": "",
                         "headers": [], "body": {"size": 9, "attachmentId": "../../profile"}});
    let err = body_of(&gmail, payload).await.unwrap_err();
    assert_eq!(err.to_string(), "Rail could not read this message's body");
    let paths: Vec<String> = gmail
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert_eq!(paths, [format!("{API_PATH}/messages/m1")]);
}

#[tokio::test]
async fn an_unsafe_message_id_or_a_reply_for_another_is_unreadable() {
    let gmail = Gmail::start().await;
    let err = gmail.provider().full_body("../profile").await.unwrap_err();
    assert_eq!(err.class(), crate::error::ErrorClass::Permanent);
    let mut other = full_json("m9", part_json("text/plain", "text/plain", b"hi"));
    other["id"] = json!("m8");
    Mock::given(path(format!("{API_PATH}/messages/m9")))
        .respond_with(ResponseTemplate::new(200).set_body_json(other))
        .mount(&gmail.server)
        .await;
    let err = gmail.provider().full_body("m9").await.unwrap_err();
    assert_eq!(err.to_string(), "Rail could not read this message's body");
}

#[tokio::test]
async fn nested_part_ids_are_kept_and_unsafe_ones_dropped() {
    let gmail = Gmail::start().await;
    let attachment = |part: &str, name: &str| {
        json!({"partId": part, "mimeType": "image/png", "filename": name, "headers": [],
               "body": {"size": 1, "attachmentId": "ANGjdJ_x"}})
    };
    let payload = json!({
        "partId": "", "mimeType": "multipart/mixed", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            {"partId": "0", "mimeType": "multipart/related", "filename": "", "headers": [], "body": {"size": 0},
             "parts": [part_json("text/plain", "text/plain", b"body"), attachment("0.1", "a.png")]},
            attachment("1/../2", "b.png"),
            attachment("2 3", "c.png")
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    assert_eq!(body.text, "body");
    let ids: Vec<&str> = body
        .attachments
        .iter()
        .map(|a| a.part_id.as_str())
        .collect();
    assert_eq!(ids, ["0.1"]);
    assert!(
        body.attachments[0].plain_file,
        "a Gmail part is a plain file even in multipart/related: forwards carry it as before"
    );
}

#[test]
fn deeply_nested_html_renders_quickly_and_keeps_its_text() {
    let cases = [
        format!(
            "{}deep{}",
            "<div>".repeat(200_000),
            "</div>".repeat(200_000)
        ),
        format!("{}deep", "<span><div></span>".repeat(100_000)),
        format!("{}deep", "<blockquote>".repeat(100_000)),
        format!("{}deep", "<ul><li>".repeat(50_000)),
        format!("{}<script>steal()</script>deep", "<div>".repeat(100_000)),
    ];
    for html in cases {
        let started = std::time::Instant::now();
        let body = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || body::render(body::Kind::Html, html.as_bytes(), None, Vec::new()))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        // Deep indentation may break the word across lines; its letters are all there.
        let letters: String = body.text.chars().filter(char::is_ascii_lowercase).collect();
        assert!(
            letters.ends_with("deep"),
            "{:?}",
            &letters[letters.len().saturating_sub(40)..]
        );
        assert!(!body.text.contains("steal"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30) * super::SLOWDOWN,
            "{:?}",
            started.elapsed()
        );
    }
}

#[tokio::test]
async fn a_forwarded_html_message_does_not_replace_the_plain_text() {
    let gmail = Gmail::start().await;
    let payload = json!({
        "partId": "", "mimeType": "multipart/mixed", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            part_json("text/plain", "text/plain; charset=utf-8", b"See below."),
            {"partId": "1", "mimeType": "message/rfc822", "filename": "", "headers": [], "body": {"size": 0},
             "parts": [{"partId": "1.0", "mimeType": "multipart/alternative", "filename": "", "headers": [],
                        "body": {"size": 0},
                        "parts": [part_json("text/html", "text/html", b"<p>forwarded</p>")]}]}
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    assert_eq!(body.text, "See below.");
    assert_eq!(body.html, None);
}

#[tokio::test]
async fn the_html_of_each_alternative_is_preferred_inside_related_parts() {
    let gmail = Gmail::start().await;
    let payload = json!({
        "partId": "", "mimeType": "multipart/mixed", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            {"partId": "0", "mimeType": "multipart/alternative", "filename": "", "headers": [], "body": {"size": 0},
             "parts": [
                part_json("text/plain", "text/plain", b"plain"),
                {"partId": "0.1", "mimeType": "multipart/related", "filename": "", "headers": [], "body": {"size": 0},
                 "parts": [part_json("text/html", "text/html", b"<p>rich</p>")]}
             ]},
            part_json("text/html", "text/html", b"<p>a later part</p>")
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    assert_eq!(body.text, "rich");
}

#[tokio::test]
async fn the_charset_is_read_as_a_parameter() {
    let gmail = Gmail::start().await;
    let latin1 = b"caf\xe9";
    let cases = [
        (
            "m1",
            "text/plain; format=flowed; CHARSET = \"iso-8859-1\"",
            "café",
        ),
        (
            "m2",
            "text/plain; name=\"charset=iso-8859-1\"",
            "caf\u{FFFD}",
        ),
        ("m3", "text/plain; xcharset=iso-8859-1", "caf\u{FFFD}"),
    ];
    for (id, content_type, expected) in cases {
        let body = body_as(&gmail, id, part_json("text/plain", content_type, latin1))
            .await
            .unwrap();
        assert_eq!(body.text, expected, "{content_type}");
    }
}

#[tokio::test]
async fn standard_base64_is_read_too() {
    let gmail = Gmail::start().await;
    let mut part = part_json("text/plain", "text/plain", b"");
    // "\u{fb}\u{ff}" in standard base64 holds '+' and '/'.
    let data = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        "a\u{fb}\u{ff}?>".as_bytes(),
    );
    assert!(data.contains('+') || data.contains('/'), "{data}");
    part["body"]["data"] = json!(data);
    let body = body_of(&gmail, part).await.unwrap();
    assert_eq!(body.text, "a\u{fb}\u{ff}?>");
}

/// Review minor 2: an attachment's name and type are one line each, like envelope fields.
#[tokio::test]
async fn attachment_names_and_types_are_one_line_and_capped() {
    let gmail = Gmail::start().await;
    let long = "x".repeat(5_000);
    let payload = json!({
        "partId": "", "mimeType": "multipart/mixed", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            {"partId": "0", "mimeType": "text/plain", "filename": "",
             "headers": [], "body": {"size": 2, "data": b64(b"hi")}},
            {"partId": "1", "mimeType": "application/pdf\r\nX-Evil: 1", "filename": "a\r\nb\u{0}.pdf",
             "headers": [], "body": {"size": 1, "attachmentId": "ANGjdJ_a"}},
            {"partId": "2", "mimeType": long, "filename": long,
             "headers": [], "body": {"size": 1, "attachmentId": "ANGjdJ_b"}}
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    let first = &body.attachments[0];
    assert_eq!(first.filename, "a  b .pdf");
    assert_eq!(first.mime_type, "application/pdf  x-evil: 1");
    let second = &body.attachments[1];
    assert!(second.filename.chars().count() < 5_000);
    assert!(second.mime_type.chars().count() < 5_000);
}

#[test]
fn rendered_bodies_carry_their_links() {
    let html = body::render(
        body::Kind::Html,
        b"<p>Read <a href=\"https://a.example\">this</a></p>",
        None,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(html.text, "Read this[1]");
    assert_eq!(html.links, ["https://a.example"]);
    let plain = body::render(
        body::Kind::Plain,
        b"go to https://a.example now",
        None,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(plain.text, "go to https://a.example [1] now");
    assert_eq!(plain.links, ["https://a.example"]);
}

#[tokio::test]
async fn an_inline_parts_content_id_is_kept_without_its_brackets_and_a_bad_one_is_dropped() {
    let gmail = Gmail::start().await;
    let html = b"<p>Hi</p><img src=\"cid:logo@x\">";
    let payload = json!({
        "partId": "", "mimeType": "multipart/related", "filename": "", "headers": [], "body": {"size": 0},
        "parts": [
            {"partId": "0", "mimeType": "text/html", "filename": "", "headers": [],
             "body": {"size": html.len(), "data": b64(html)}},
            {"partId": "1", "mimeType": "image/png", "filename": "logo.png",
             "headers": [{"name": "Content-ID", "value": " <logo@x> "}],
             "body": {"size": 10, "attachmentId": "ANGjdJ_logo"}},
            {"partId": "2", "mimeType": "image/png", "filename": "odd.png",
             "headers": [{"name": "content-id", "value": "<a b\u{1b}>"}],
             "body": {"size": 10, "attachmentId": "ANGjdJ_odd"}}
        ]
    });
    let body = body_of(&gmail, payload).await.unwrap();
    let ids: Vec<Option<&str>> = body
        .attachments
        .iter()
        .map(|a| a.content_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("logo@x"), None]);
}

/// A message whose top-level headers are `headers`, with a short text body.
fn with_headers(headers: serde_json::Value) -> serde_json::Value {
    json!({
        "partId": "", "mimeType": "text/plain", "filename": "", "headers": headers,
        "body": {"size": 2, "data": b64(b"hi")}
    })
}

#[tokio::test]
async fn gmail_s_own_authentication_results_say_which_domains_it_vouched_for() {
    let gmail = Gmail::start().await;
    let headers = json!([
        {"name": "Authentication-Results",
         "value": "mx.google.com; dkim=pass header.i=@example.com; dmarc=pass header.from=example.com"},
        {"name": "Authentication-Results",
         "value": "mx.google.com; dmarc=pass header.from=forged.example"},
        {"name": "Content-Type", "value": "text/plain"}
    ]);
    let body = body_of(&gmail, with_headers(headers)).await.unwrap();
    assert_eq!(body.vouched_domains, ["example.com"]);
}

#[tokio::test]
async fn results_a_sender_wrote_itself_vouch_for_nothing() {
    let gmail = Gmail::start().await;
    let headers = json!([
        {"name": "Authentication-Results",
         "value": "relay.example; dmarc=pass header.from=example.com"},
        {"name": "Authentication-Results",
         "value": "mx.google.com; dmarc=pass header.from=example.com"}
    ]);
    let body = body_of(&gmail, with_headers(headers)).await.unwrap();
    assert!(body.vouched_domains.is_empty());
    let none = body_of(&gmail, with_headers(json!([]))).await.unwrap();
    assert!(none.vouched_domains.is_empty());
}
