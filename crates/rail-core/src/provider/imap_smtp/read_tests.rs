use super::read::*;
use crate::provider::types::Window;

#[test]
fn test_unix_to_imap_date() {
    assert_eq!(unix_to_imap_date(0), "01-Jan-1970");
    assert_eq!(unix_to_imap_date(1704067200), "01-Jan-2024");
    assert_eq!(unix_to_imap_date(1709164800), "29-Feb-2024");
    assert_eq!(unix_to_imap_date(1709251200), "01-Mar-2024");
    assert_eq!(unix_to_imap_date(1735689600), "01-Jan-2025");
}

#[test]
fn test_build_search_query() {
    let empty_win = Window::default();
    assert_eq!(build_search_query(&empty_win), "ALL");

    let after_win = Window {
        after_ms: Some(1_704_067_200_000),
        before_ms: None,
    };
    assert_eq!(build_search_query(&after_win), "SINCE 01-Jan-2024");

    let range_win = Window {
        after_ms: Some(1_704_067_200_000),
        before_ms: Some(1_709_251_200_000),
    };
    assert_eq!(
        build_search_query(&range_win),
        "SINCE 01-Jan-2024 BEFORE 01-Mar-2024"
    );
}

#[test]
fn test_parse_envelope_headers() {
    let raw_header = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Test Subject\r\n\
Message-ID: <msg001@example.com>\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
\r\n";

    let env = parse_envelope(
        "INBOX",
        12345,
        42,
        1_704_110_400_000,
        1024,
        true,
        true,
        false,
        raw_header,
    )
    .expect("should parse envelope");

    assert_eq!(env.remote_id.as_str(), "INBOX:12345:42");
    assert_eq!(env.subject, "Test Subject");
    assert_eq!(env.from.len(), 1);
    assert_eq!(env.from[0].name.as_deref(), Some("Alice"));
    assert_eq!(env.from[0].address, "alice@example.com");
    assert_eq!(env.to.len(), 1);
    assert_eq!(env.to[0].address, "bob@example.com");
    assert_eq!(env.message_id.as_deref(), Some("msg001@example.com"));
    assert!(env.is_read);
    assert!(env.is_starred);
    assert!(!env.is_draft);
    assert_eq!(env.size, 1024);
    assert_eq!(env.folders, vec!["INBOX"]);
}

#[test]
fn test_parse_body_plain_and_html() {
    let plain_msg = b"From: a@b.c\r\n\
To: d@e.f\r\n\
Subject: Plain\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello plain world! Visit https://example.com for more info.\r\n";

    let body = parse_body(plain_msg).expect("should parse plain body");
    assert!(body.text.contains("Hello plain world!"));
    assert!(body.html.is_none());
    assert!(!body.truncated);
    assert!(!body.links.is_empty());
    assert!(body.links[0].contains("example.com"));

    let html_msg = b"From: a@b.c\r\n\
To: d@e.f\r\n\
Subject: HTML\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><body><p>Hello <b>bold</b> <a href=\"https://example.org\">link</a></p></body></html>\r\n";

    let html_body = parse_body(html_msg).expect("should parse html body");
    assert!(html_body.text.contains("Hello"));
    assert!(html_body.html.is_some());
    assert!(!html_body.links.is_empty());
    assert_eq!(html_body.links[0], "https://example.org");
}

#[test]
fn test_parse_body_with_attachments_and_extraction() {
    let raw_mime = b"MIME-Version: 1.0\r\n\
From: sender@example.com\r\n\
To: recipient@example.com\r\n\
Subject: Mixed Content\r\n\
Content-Type: multipart/mixed; boundary=\"BOUNDARY\"\r\n\
\r\n\
--BOUNDARY\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Message text here.\r\n\
--BOUNDARY\r\n\
Content-Type: text/plain; name=\"note.txt\"\r\n\
Content-Disposition: attachment; filename=\"note.txt\"\r\n\
\r\n\
Attachment content here.\r\n\
--BOUNDARY--\r\n";

    let body = parse_body(raw_mime).expect("should parse multipart message");
    assert!(body.text.contains("Message text here."));
    assert_eq!(body.attachments.len(), 1);
    assert_eq!(body.attachments[0].filename, "note.txt");
    assert_eq!(body.attachments[0].part_id, "0");

    let att_bytes = extract_attachment(raw_mime, "0").expect("should extract by index");
    assert_eq!(att_bytes, b"Attachment content here.");

    let att_by_name = extract_attachment(raw_mime, "note.txt").expect("should extract by name");
    assert_eq!(att_by_name, b"Attachment content here.");
}
