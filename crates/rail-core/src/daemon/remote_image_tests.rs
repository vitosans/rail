//! `image.remote` through a real daemon: the cached copy (private, named by a hash, with its
//! bytes' type) answered again without a second request, a stale one fetched again, and a
//! refusal that writes nothing. The address rules themselves are `remote_fetch_tests`'.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::attachments_tests::{age, entries, nothing_written};
use super::draft_tests::DaemonHarness;
use crate::ops::op::ImageRemote;
use crate::ops::{
    ErrorCode, IMAGE_STALE_AFTER, MAX_REMOTE_URL_BYTES, OpError, RemoteImageRequest,
    RemoteImageResponse,
};

const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";

async fn remote(h: &DaemonHarness, url: &str) -> Result<RemoteImageResponse, OpError> {
    let request = RemoteImageRequest { url: url.into() };
    h.call::<ImageRemote>(&request).await
}

async fn serving(body: &[u8]) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/logo"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_remote_picture_is_cached_privately_and_answered_again_without_a_request() {
    let h = DaemonHarness::new().await;
    let server = serving(GIF).await;
    let url = format!("{}/logo", server.uri());

    let first = remote(&h, &url).await.unwrap();
    let path = PathBuf::from(&first.path);
    assert_eq!(first.mime_type, "image/gif");
    assert_eq!(std::fs::read(&path).unwrap(), GIF);
    assert_eq!(path.parent().unwrap(), h.paths.images_dir());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with(".gif") && !name.contains("logo"), "{name}");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&h.paths.images_dir()), 0o700);

    let again = remote(&h, &url).await.unwrap();
    assert_eq!(again.path, first.path);
    assert_eq!(again.mime_type, "image/gif");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_stale_copy_is_swept_by_the_next_write_and_fetched_again() {
    let h = DaemonHarness::new().await;
    let server = serving(GIF).await;
    let old = remote(&h, &format!("{}/logo", server.uri())).await.unwrap();
    age(&PathBuf::from(&old.path), IMAGE_STALE_AFTER.as_secs() + 60);
    Mock::given(path("/other"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(GIF))
        .mount(&server)
        .await;
    let other = remote(&h, &format!("{}/other", server.uri()))
        .await
        .unwrap();
    assert_eq!(
        entries(&h.paths.images_dir()),
        vec![
            PathBuf::from(&other.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ],
        "the stale copy went"
    );
}

#[tokio::test]
async fn a_refused_or_overlong_address_writes_nothing() {
    let h = DaemonHarness::new().await;
    let server = serving(b"<html>not a picture</html>").await;
    let mismatch = remote(&h, &format!("{}/logo", server.uri()))
        .await
        .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::Unsupported);
    let refused = remote(&h, "file:///etc/passwd").await.unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    let long = format!("https://example.com/{}", "a".repeat(MAX_REMOTE_URL_BYTES));
    let overlong = remote(&h, &long).await.unwrap_err();
    assert_eq!(overlong.code, ErrorCode::InvalidInput);
    assert!(!overlong.message.contains("example.com"));
    nothing_written(&h.paths.images_dir());
}

/// The daemon reads `remote_images_ipv6` from config.toml at each ask, so a change made while
/// it runs applies to the next picture.
#[tokio::test]
async fn remote_images_ipv6_false_in_the_config_refuses_an_ipv6_address() {
    let h = DaemonHarness::new().await;
    let url = "http://[::1]:9/logo";
    let tried = remote(&h, url).await.unwrap_err();
    assert_eq!(
        tried.code,
        ErrorCode::Network,
        "allowed by default: {tried:?}"
    );

    let config = std::fs::read_to_string(h.paths.config_file()).unwrap();
    let off = config.replace("[general]\n", "[general]\nremote_images_ipv6 = false\n");
    std::fs::write(h.paths.config_file(), off).unwrap();
    let refused = remote(&h, url).await.unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput, "{refused:?}");
    nothing_written(&h.paths.images_dir());
}
