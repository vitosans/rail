//! The guarded fetch: which addresses count as public, which URLs are fetched at all, a name
//! that resolves only to private addresses, redirects checked like the first address, the cap,
//! and the type read from the bytes rather than the server's word.

use std::net::IpAddr;
use std::str::FromStr;

use reqwest::dns::{Name, Resolve};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::remote_fetch::{Guarded, Ipv6, MAX_REDIRECTS, RemoteFetch, Rules, public};
use crate::error::ErrorClass;
use crate::ops::{CONTENT_MISMATCH, ErrorCode};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn only_public_unicast_addresses_are_public() {
    for open in [
        "93.184.215.14",
        "8.8.8.8",
        "2606:4700::1111",
        "2a00:1450:4001:80b::200e",
        "64:ff9b::808:808",
        "2002:808:808::1",
        "::ffff:8.8.8.8",
    ] {
        assert!(public(ip(open)), "{open}");
    }
    for closed in [
        "0.0.0.0",
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "192.0.0.8",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::127.0.0.1",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "fec0::1",
        "ff02::1",
        "2001::1",
        "2001:db8::1",
        "2002:7f00:1::1",
        "2002:a00:1::1",
        "64:ff9b::7f00:1",
        "64:ff9b::a9fe:a9fe",
        "3fff::1",
    ] {
        assert!(!public(ip(closed)), "{closed}");
    }
}

#[test]
fn only_https_on_its_own_port_to_a_name_or_a_public_address_is_fetched() {
    let strict = Rules::STRICT;
    let url = |text: &str| Url::parse(text).unwrap();
    for fetched in [
        "https://cdn.example.com/a.png",
        "https://cdn.example.com:443/a.png",
        "https://93.184.215.14/a.png",
        "https://[2606:4700::1111]/a.png",
    ] {
        assert!(strict.url(&url(fetched)), "{fetched}");
    }
    for refused in [
        "http://cdn.example.com/a.png",
        "https://cdn.example.com:8443/a.png",
        "https://user:pw@cdn.example.com/a.png",
        "https://user@cdn.example.com/a.png",
        "https://127.0.0.1/a.png",
        "https://0x7f.1/a.png",
        "https://2130706433/a.png",
        "https://[::1]/a.png",
        "https://[::ffff:169.254.169.254]/latest",
        "https://10.0.0.1/a.png",
        "ftp://cdn.example.com/a.png",
        "file:///etc/passwd",
        "data:image/png;base64,iVBORw0KGgo=",
    ] {
        assert!(!strict.url(&url(refused)), "{refused}");
    }
}

#[tokio::test]
async fn a_name_that_resolves_only_to_this_machine_is_refused_before_any_connect() {
    let found = Guarded(Rules::STRICT)
        .resolve(Name::from_str("localhost").unwrap())
        .await;
    assert!(found.is_err(), "localhost has no public address");
    let tests = Guarded(Rules::LOOPBACK)
        .resolve(Name::from_str("localhost").unwrap())
        .await
        .unwrap();
    assert!(tests.into_iter().all(|a| a.ip().is_loopback()));
}

#[tokio::test]
async fn the_strict_fetch_refuses_a_local_server_without_asking_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .expect(0)
        .mount(&server)
        .await;
    let fetch = RemoteFetch::new().unwrap();
    for address in [
        format!("{}/a.png", server.uri()),
        format!("{}/a.png", server.uri().replace("http:", "https:")),
    ] {
        let error = fetch.fetch(&address, 1024).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{error:?}");
        assert_eq!(error.class, ErrorClass::Permanent);
        assert!(!error.message.contains("127.0.0.1"), "{}", error.message);
    }
}

#[tokio::test]
async fn a_picture_comes_back_with_the_type_its_bytes_carry() {
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_bytes(PNG),
        )
        .mount(&server)
        .await;
    let fetch = RemoteFetch::loopback().unwrap();
    let (bytes, mime) = fetch
        .fetch(&format!("{}/a", server.uri()), 1024)
        .await
        .unwrap();
    assert_eq!(bytes, PNG);
    assert_eq!(mime, "image/png");
    let asked = &server.received_requests().await.unwrap()[0];
    assert!(asked.headers.get("cookie").is_none());
    assert!(asked.headers.get("referer").is_none());
}

#[tokio::test]
async fn bytes_that_are_no_picture_rail_draws_are_refused() {
    let server = MockServer::start().await;
    Mock::given(path("/a.svg"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(&b"<svg xmlns='http://www.w3.org/2000/svg'/>"[..]),
        )
        .mount(&server)
        .await;
    let error = RemoteFetch::loopback()
        .unwrap()
        .fetch(&format!("{}/a.svg", server.uri()), 1024)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, CONTENT_MISMATCH);
}

#[tokio::test]
async fn a_picture_over_the_cap_is_refused_whether_or_not_its_length_says_so() {
    let server = MockServer::start().await;
    let mut big = PNG.to_vec();
    big.resize(2048, 0);
    Mock::given(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(big))
        .mount(&server)
        .await;
    let error = RemoteFetch::loopback()
        .unwrap()
        .fetch(&format!("{}/big", server.uri()), 1024)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.class, ErrorClass::Permanent);
}

#[tokio::test]
async fn a_server_error_is_transient_and_a_missing_picture_permanent() {
    let server = MockServer::start().await;
    Mock::given(path("/busy"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(path("/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let fetch = RemoteFetch::loopback().unwrap();
    let busy = fetch
        .fetch(&format!("{}/busy", server.uri()), 1024)
        .await
        .unwrap_err();
    assert_eq!(
        (busy.code, busy.class),
        (ErrorCode::Network, ErrorClass::Transient)
    );
    let gone = fetch
        .fetch(&format!("{}/gone", server.uri()), 1024)
        .await
        .unwrap_err();
    assert_eq!(
        (gone.code, gone.class),
        (ErrorCode::Network, ErrorClass::Permanent)
    );
}

#[tokio::test]
async fn a_redirect_is_checked_as_the_first_address_was() {
    let server = MockServer::start().await;
    for (from, to) in [
        ("/metadata", "http://169.254.169.254/latest/meta-data/"),
        ("/lan", "http://10.0.0.1/a.png"),
        ("/scheme", "file:///etc/passwd"),
    ] {
        Mock::given(path(from))
            .respond_with(ResponseTemplate::new(302).insert_header("location", to))
            .mount(&server)
            .await;
    }
    let fetch = RemoteFetch::loopback().unwrap();
    for from in ["/metadata", "/lan", "/scheme"] {
        let error = fetch
            .fetch(&format!("{}{from}", server.uri()), 1024)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{from}: {error:?}");
        assert_eq!(error.class, ErrorClass::Permanent);
    }
}

#[tokio::test]
async fn a_few_redirects_are_followed_and_a_longer_chain_is_refused() {
    let server = MockServer::start().await;
    for hop in 0..=MAX_REDIRECTS {
        Mock::given(path(format!("/hop{hop}")))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/hop{}", server.uri(), hop + 1)),
            )
            .mount(&server)
            .await;
    }
    Mock::given(path(format!("/hop{}", MAX_REDIRECTS + 1)))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG))
        .mount(&server)
        .await;
    let fetch = RemoteFetch::loopback().unwrap();
    let last = MAX_REDIRECTS + 1;
    let within = fetch
        .fetch(&format!("{}/hop1", server.uri()), 1024)
        .await
        .unwrap();
    assert_eq!(
        within.1, "image/png",
        "{MAX_REDIRECTS} redirects reach /hop{last}"
    );
    let error = fetch
        .fetch(&format!("{}/hop0", server.uri()), 1024)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

/// `remote_images_ipv6 = false`: on an IPv6-only network with NAT64, a public-looking IPv6
/// address can be translated to a private IPv4 one, so every IPv6 address is refused, mapped
/// ones too, and names and public IPv4 addresses are fetched as before.
#[test]
fn without_ipv6_every_ipv6_address_is_refused_and_ipv4_is_not() {
    let rules = Rules::STRICT.without_ipv6();
    let url = |text: &str| Url::parse(text).unwrap();
    for fetched in [
        "https://cdn.example.com/a.png",
        "https://93.184.215.14/a.png",
    ] {
        assert!(rules.url(&url(fetched)), "{fetched}");
    }
    for refused in [
        "https://[2606:4700::1111]/a.png",
        "https://[64:ff9b::5db8:d70e]/a.png",
        "https://[::ffff:93.184.215.14]/a.png",
    ] {
        assert!(!rules.url(&url(refused)), "{refused}");
    }
    assert!(Rules::STRICT.url(&url("https://[2606:4700::1111]/a.png")));
}

#[tokio::test]
async fn an_ipv6_address_is_refused_before_any_connect_when_ipv6_is_refused() {
    let fetch = RemoteFetch::loopback().unwrap();
    let refused = fetch
        .fetch_as("http://[::1]:9/a.png", 1024, Ipv6::Refused)
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput, "{refused:?}");
    let tried = fetch
        .fetch_as("http://[::1]:9/a.png", 1024, Ipv6::Allowed)
        .await
        .unwrap_err();
    assert_eq!(
        tried.code,
        ErrorCode::Network,
        "allowed, so it was tried: {tried:?}"
    );
}
