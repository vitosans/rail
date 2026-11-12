//! The two attachment op names and their place outside the retry-safe set, and the count-only
//! `Debug` every attachment type on the wire has.

use super::attachment_image::{AttachmentImageRequest, AttachmentImageResponse};
use super::attachments::*;
use super::{OpError, names};

/// A populated save request.
fn save_request() -> AttachmentSaveRequest {
    AttachmentSaveRequest {
        account: "work".into(),
        message_id: "m1".into(),
        part_ids: vec!["0.1".into()],
        dir: "/x".into(),
    }
}

/// A populated save response: one part written into `/x`.
fn save_response() -> AttachmentSaveResponse {
    AttachmentSaveResponse {
        results: vec![AttachmentResult {
            part_id: "0.1".into(),
            outcome: AttachmentOutcome::Saved {
                path: "/x/plan.pdf".into(),
                size: 1,
            },
        }],
    }
}

/// AC-29a: both names are in `ALL` at the pinned slots, and neither may be resent.
#[test]
fn the_two_names_exist_and_are_not_retry_safe() {
    assert_eq!(names::ATTACHMENT_SAVE, "attachment.save");
    assert_eq!(names::ATTACHMENT_OPEN, "attachment.open");
    // Slots 27 and 28: appended after `snooze.clear` and adjacent in that order.
    assert_eq!(names::ALL[26], names::ATTACHMENT_SAVE);
    assert_eq!(names::ALL[27], names::ATTACHMENT_OPEN);
    let after_clear = names::ALL
        .iter()
        .position(|name| *name == names::SNOOZE_CLEAR)
        .expect("snooze.clear is in ALL");
    assert_eq!(after_clear + 1, 26, "the pair follows snooze.clear");
    assert!(!crate::daemon::retry_safe(names::ATTACHMENT_SAVE));
    assert!(!crate::daemon::retry_safe(names::ATTACHMENT_OPEN));
}

/// AC-32 (the `ops` half): no request, response, result or outcome prints a filename, a part
/// id, a directory, a path, a mime type or an account — only counts, sizes and variant names.
#[test]
fn debug_prints_counts_only() {
    let printed = [
        format!("{:?}", save_request()),
        format!("{:?}", save_response()),
        format!(
            "{:?}",
            AttachmentOpenRequest {
                account: "work".into(),
                message_id: "m1".into(),
                part_id: "0.1".into(),
            }
        ),
        format!(
            "{:?}",
            AttachmentOpenResponse {
                path: "/x/plan.pdf".into(),
            }
        ),
        format!("{:?}", save_response().results[0]),
        format!("{:?}", save_response().results[0].outcome),
        format!(
            "{:?}",
            AttachmentResult {
                part_id: "0.2".into(),
                outcome: AttachmentOutcome::Missing,
            }
        ),
        format!(
            "{:?}",
            AttachmentResult {
                part_id: "0.3".into(),
                outcome: AttachmentOutcome::Failed(OpError::invalid_input(
                    "dir: not an absolute path"
                )),
            }
        ),
    ];
    for text in &printed {
        for secret in [
            "plan",
            "/x",
            "work",
            "m1",
            "0.1",
            "0.2",
            "0.3",
            "application/pdf",
            "absolute",
        ] {
            assert!(!text.contains(secret), "{text} carries {secret}");
        }
    }
    assert!(printed[1].contains('1'), "the result count: {}", printed[1]);
    assert!(
        printed[5].contains("Saved") && printed[5].contains('1'),
        "the variant and the size: {}",
        printed[5]
    );
    assert!(printed[6].contains("Missing"), "{}", printed[6]);
    assert!(printed[7].contains("Failed"), "{}", printed[7]);
}

/// A save of `part_ids` into an absolute destination.
fn save_of(message_id: &str, part_ids: &[&str]) -> AttachmentSaveRequest {
    AttachmentSaveRequest {
        account: "work".into(),
        message_id: message_id.into(),
        part_ids: part_ids.iter().map(|p| (*p).to_string()).collect(),
        dir: "/x".into(),
    }
}

fn refused(error: &OpError) -> bool {
    error.code == super::ErrorCode::InvalidInput
        && error.class == crate::error::ErrorClass::Permanent
}

/// AC-30: the part-id rule takes both providers' ids — Gmail's empty one included, which is a
/// single-part message's own payload — and refuses the rest, on a part id and on the message id
/// alike (a message id is never empty), and a part named twice is kept once.
#[test]
fn part_id_validation_accepts_both_providers_and_refuses_the_rest() {
    let graph = "AAMkAGI2THVSAAA=";
    let long = "a".repeat(MAX_PART_ID_BYTES + 1);
    let many: Vec<String> = (0..=MAX_SAVE_PARTS).map(|n| format!("0.{n}")).collect();
    let named: Vec<&str> = many.iter().map(String::as_str).collect();
    for bad in [long.as_str(), "bell\u{7}", "zero\u{200b}width"] {
        a_refused_part_id(bad);
    }
    let error = save_of("m1", &named)
        .checked_parts()
        .expect_err("too many parts");
    assert!(refused(&error), "{error:?}");

    both_providers_ids_are_accepted(graph);
    for bad in [long.as_str(), ""] {
        let error = save_of(bad, &["0.1"])
            .checked_parts()
            .expect_err("a message id Rail does not pass on");
        assert!(refused(&error), "{bad:?}: {error:?}");
    }

    // FR-13: one result per distinct part, first occurrence kept.
    assert_eq!(
        save_of("m1", &["0.1", "0.1", "0.2"])
            .checked_parts()
            .unwrap(),
        ["0.1", "0.2"]
    );

    a_destination_is_checked_first();
}

/// One part id Rail does not pass on: refused on a save and on an open alike, and never quoted
/// back in the refusal.
fn a_refused_part_id(bad: &str) {
    let error = save_of("m1", &[bad])
        .checked_parts()
        .expect_err("a part id Rail does not pass on");
    assert!(refused(&error), "{bad:?}: {error:?}");
    assert!(
        !error.message.contains(bad),
        "the id is never quoted: {error:?}"
    );
    let open = AttachmentOpenRequest {
        account: "work".into(),
        message_id: "m1".into(),
        part_id: bad.to_string(),
    };
    assert!(refused(&open.checked().expect_err("the same rule")));
}

/// Both providers' part ids, Gmail's empty one included, taken on a part id and on a message id.
fn both_providers_ids_are_accepted(graph: &str) {
    assert_eq!(
        save_of("m1", &["0.1", graph]).checked_parts().unwrap(),
        ["0.1", graph]
    );
    assert_eq!(save_of(graph, &["0.1"]).checked_parts().unwrap(), ["0.1"]);
    // Gmail's own part id for a single-part message whose top part is the file.
    assert_eq!(save_of("m1", &[""]).checked_parts().unwrap(), [""]);
    assert!(
        AttachmentOpenRequest {
            account: "work".into(),
            message_id: "m1".into(),
            part_id: String::new(),
        }
        .checked()
        .is_ok()
    );
    assert!(
        AttachmentOpenRequest {
            account: "work".into(),
            message_id: graph.into(),
            part_id: "0.1".into(),
        }
        .checked()
        .is_ok()
    );
}

/// The destination is checked before anything else is asked of the store or the provider.
fn a_destination_is_checked_first() {
    for dir in ["relative/path", "", "~/Downloads"] {
        let mut request = save_of("m1", &["0.1"]);
        request.dir = dir.to_string();
        let error = request.checked_parts().expect_err("not an absolute path");
        assert!(refused(&error), "{dir:?}: {error:?}");
        assert_eq!(error.message, "dir: not an absolute path");
    }
    let mut request = save_of("m1", &["0.1"]);
    request.dir = format!("/{}", "d".repeat(MAX_DIR_BYTES));
    assert!(refused(&request.checked_parts().expect_err("too long")));
}

/// AC-31: `openable` strips the parameters, ignores case, and leaves every scripted format
/// out.
#[test]
fn openable_strips_parameters_and_ignores_case() {
    assert_eq!(openable("application/pdf"), Some("pdf"));
    assert_eq!(openable("text/plain; charset=utf-8"), Some("txt"));
    assert_eq!(openable("IMAGE/PNG"), Some("png"));
    assert_eq!(
        openable("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        Some("xlsx")
    );
    for closed in [
        "text/html",
        "image/svg+xml",
        "text/markdown",
        "application/zip",
        "",
        "application/octet-stream",
    ] {
        assert_eq!(openable(closed), None, "{closed}");
    }
    assert_eq!(OPENABLE.len(), 21);
}

/// `attachment.image` follows the saved-search pair, may be resent (it only reads and caches),
/// and prints nothing of the request or the answer.
#[test]
fn the_image_operation_is_registered_retry_safe_and_prints_nothing() {
    assert_eq!(names::ATTACHMENT_IMAGE, "attachment.image");
    assert_eq!(names::ALL[30], names::ATTACHMENT_IMAGE);
    assert!(crate::daemon::retry_safe(names::ATTACHMENT_IMAGE));
    let request = AttachmentImageRequest {
        account: "work".into(),
        message_id: "m1".into(),
        content_id: "logo@x".into(),
    };
    let response = AttachmentImageResponse {
        path: "/cache/images/ab.png".into(),
        mime_type: "image/png".into(),
    };
    let printed = format!("{request:?} {response:?}");
    for mail in ["work", "m1", "logo", "cache", "png"] {
        assert!(!printed.contains(mail), "{mail} in {printed}");
    }
    assert_eq!(super::image_extension(" IMAGE/JPEG "), Some("jpg"));
    assert_eq!(super::image_extension("image/svg+xml"), None);
}

/// `image.remote` follows `attachment.image`, may be resent as it may, and prints nothing of
/// the address.
#[test]
fn the_remote_image_operation_is_registered_retry_safe_and_prints_nothing() {
    assert_eq!(names::IMAGE_REMOTE, "image.remote");
    assert!(names::ALL.contains(&names::IMAGE_REMOTE));
    assert!(crate::daemon::retry_safe(names::IMAGE_REMOTE));
    let request = super::RemoteImageRequest {
        url: "https://track.example.com/open?to=sam".into(),
    };
    let printed = format!("{request:?}");
    for mail in ["track", "example", "sam"] {
        assert!(!printed.contains(mail), "{mail} in {printed}");
    }
    assert!(request.checked().is_ok());
    let empty = super::RemoteImageRequest { url: String::new() };
    assert_eq!(
        empty.checked().unwrap_err().code,
        super::ErrorCode::InvalidInput
    );
}
