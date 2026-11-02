//! The guarded fetch behind `image.remote`: a picture a mail names by its address, fetched by
//! the daemon for the reader. Mail is untrusted, and the address could point the daemon at this
//! machine or its network, so only `https` on its own port is fetched, and only from public
//! unicast addresses (`public`): an address in the URL is checked as written, a name by every
//! address it resolves to, the rest dropped before the connect (so a name cannot answer one
//! address to the check and another to the connect), and each redirect (at most
//! `MAX_REDIRECTS`) again the same way. No proxy (it would resolve names itself), no cookies,
//! no referrer, a timeout, and a body cap; the type is read from the bytes, never from the
//! server. With `Ipv6::Refused` (`remote_images_ipv6 = false`) no IPv6 address is fetched at
//! all: on an IPv6-only network a NAT64 gateway's own prefix (inside `2000::/3`, so public to
//! `public`) can be translated to a private IPv4 address, and only the network knows its
//! prefix. Errors and logs carry kinds, never the address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::{Host, Url};

use crate::error::ErrorClass;
use crate::ops::{CONTENT_MISMATCH, ErrorCode, IMAGE_TYPES, OpError, content_matches};

/// The most redirects one picture may take.
pub const MAX_REDIRECTS: usize = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(15);

/// Which URLs and addresses are fetched: `STRICT` always but in tests, which serve on loopback
/// over plain HTTP.
#[derive(Clone, Copy)]
pub(super) struct Rules {
    https_only: bool,
    loopback: bool,
    ipv6: bool,
}

impl Rules {
    pub(super) const STRICT: Self = Self {
        https_only: true,
        loopback: false,
        ipv6: true,
    };
    #[cfg(any(test, feature = "test-gates"))]
    pub(super) const LOOPBACK: Self = Self {
        https_only: false,
        loopback: true,
        ipv6: true,
    };

    /// These rules, refusing every IPv6 address (an IPv4-mapped one too).
    pub(super) const fn without_ipv6(self) -> Self {
        Self {
            ipv6: false,
            ..self
        }
    }

    fn address(self, ip: IpAddr) -> bool {
        if ip.is_ipv6() && !self.ipv6 {
            return false;
        }
        public(ip) || (self.loopback && ip.is_loopback())
    }

    /// `https` (or, in tests, `http`) on the scheme's own port, no credentials, and a name or
    /// an address `address` allows. `url` already reads `0x7f.1` and such as addresses.
    pub(super) fn url(self, url: &Url) -> bool {
        let scheme = url.scheme() == "https" || (!self.https_only && url.scheme() == "http");
        if !scheme || !url.username().is_empty() || url.password().is_some() {
            return false;
        }
        if self.https_only && url.port().is_some() {
            return false;
        }
        match url.host() {
            Some(Host::Domain(name)) => !name.is_empty(),
            Some(Host::Ipv4(ip)) => self.address(ip.into()),
            Some(Host::Ipv6(ip)) => self.address(ip.into()),
            None => false,
        }
    }
}

/// Whether `ip` is a public unicast address: never this machine, its networks, link-local,
/// shared, multicast, documentation or reserved space, nor an IPv6 form that carries one.
pub(super) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let closed = a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113);
    !closed
}

/// Global unicast (`2000::/3`) only, less its special and documentation blocks; an address
/// that carries an IPv4 one (mapped, 6to4, NAT64) is as public as that address.
fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    let embedded = |hi: u16, lo: u16| {
        let [a, b] = hi.to_be_bytes();
        let [c, d] = lo.to_be_bytes();
        public_v4(Ipv4Addr::new(a, b, c, d))
    };
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return embedded(s[6], s[7]);
    }
    if s[0] & 0xe000 != 0x2000 {
        return false;
    }
    match (s[0], s[1]) {
        (0x2001, second) => second >= 0x0200 && second != 0x0db8,
        (0x2002, _) => embedded(s[1], s[2]),
        (0x3fff, second) => second >= 0x1000,
        _ => true,
    }
}

/// Name resolution that keeps only the addresses `Rules` allows, and fails when none is left.
pub(super) struct Guarded(pub(super) Rules);

impl Resolve for Guarded {
    fn resolve(&self, name: Name) -> Resolving {
        let rules = self.0;
        Box::pin(async move {
            let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let kept: Vec<SocketAddr> = found.filter(|a| rules.address(a.ip())).collect();
            if kept.is_empty() {
                return Err("the name has no public address".into());
            }
            Ok(Box::new(kept.into_iter()) as Addrs)
        })
    }
}

/// Whether a fetch may use IPv6 addresses (`remote_images_ipv6`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ipv6 {
    Allowed,
    Refused,
}

/// The client `image.remote` fetches with: one HTTP client for each `Ipv6`, since the address
/// rules are fixed into a client's name resolution and redirect policy.
pub struct RemoteFetch {
    client: reqwest::Client,
    rules: Rules,
    ipv4_only: reqwest::Client,
}

impl RemoteFetch {
    /// # Errors
    /// `Internal` when the HTTP client cannot be built.
    pub fn new() -> Result<Self, OpError> {
        Self::with_rules(Rules::STRICT)
    }

    /// A fetch that also takes loopback over plain HTTP, for tests' local servers.
    ///
    /// # Errors
    /// As `new`.
    #[cfg(any(test, feature = "test-gates"))]
    pub fn loopback() -> Result<Self, OpError> {
        Self::with_rules(Rules::LOOPBACK)
    }

    fn with_rules(rules: Rules) -> Result<Self, OpError> {
        Ok(Self {
            client: client(rules)?,
            rules,
            ipv4_only: client(rules.without_ipv6())?,
        })
    }

    /// `fetch_as` with IPv6 allowed.
    ///
    /// # Errors
    /// As `fetch_as`.
    pub async fn fetch(&self, url: &str, cap: usize) -> Result<(Vec<u8>, &'static str), OpError> {
        self.fetch_as(url, cap, Ipv6::Allowed).await
    }

    /// The picture at `url`, at most `cap` bytes, and its type (one of `IMAGE_TYPES`), over
    /// IPv6 only when `ipv6` allows it.
    ///
    /// # Errors
    /// `InvalidInput` for an address Rail does not fetch, before or after a redirect;
    /// `Network` when the server cannot be reached or refuses; `Unsupported` for bytes over
    /// `cap` or that are no picture Rail draws.
    pub async fn fetch_as(
        &self,
        url: &str,
        cap: usize,
        ipv6: Ipv6,
    ) -> Result<(Vec<u8>, &'static str), OpError> {
        let (rules, client) = match ipv6 {
            Ipv6::Allowed => (self.rules, &self.client),
            Ipv6::Refused => (self.rules.without_ipv6(), &self.ipv4_only),
        };
        let parsed = Url::parse(url).map_err(|_| refused())?;
        if !rules.url(&parsed) {
            return Err(refused());
        }
        let mut response = client
            .get(parsed)
            .header(
                reqwest::header::ACCEPT,
                "image/png,image/jpeg,image/gif,image/webp",
            )
            .send()
            .await
            .map_err(|e| transport(&e))?;
        let status = response.status();
        // A redirect still standing is one reqwest would not follow, such as to `file:`.
        if status.is_redirection() {
            return Err(refused());
        }
        if !status.is_success() {
            let class = if status.is_server_error() || status.as_u16() == 429 {
                ErrorClass::Transient
            } else {
                ErrorClass::Permanent
            };
            return Err(OpError::new(
                ErrorCode::Network,
                class,
                format!("the picture's server answered HTTP {}", status.as_u16()),
            ));
        }
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| transport(&e))? {
            if bytes.len() + chunk.len() > cap {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        let Some((mime, _)) = IMAGE_TYPES.iter().find(|(m, _)| content_matches(m, &bytes)) else {
            tracing::warn!("a remote picture's bytes were no type Rail draws; not kept");
            return Err(OpError::new(
                ErrorCode::Unsupported,
                ErrorClass::Permanent,
                CONTENT_MISMATCH,
            ));
        };
        Ok((bytes, mime))
    }
}

/// The HTTP client for `rules`: names resolved by `Guarded`, redirects checked by the same rules.
fn client(rules: Rules) -> Result<reqwest::Client, OpError> {
    let redirects = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if !rules.url(attempt.url()) {
            attempt.error("a redirect to an address Rail does not fetch")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .no_proxy()
        .dns_resolver(Arc::new(Guarded(rules)))
        .redirect(redirects)
        .referer(false)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| OpError::internal("could not build the picture client"))
}

fn refused() -> OpError {
    OpError::invalid_input("Rail fetches pictures only over https from public addresses")
}

/// A refused redirect is a refusal; anything else on the way is the network's. reqwest's text
/// can quote the address, so only the kind goes out.
fn transport(error: &reqwest::Error) -> OpError {
    if error.is_redirect() {
        return refused();
    }
    let kind = if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "could not connect"
    } else {
        "failed"
    };
    OpError::new(
        ErrorCode::Network,
        ErrorClass::Transient,
        format!("fetching the picture {kind}"),
    )
}

fn too_large() -> OpError {
    OpError::new(
        ErrorCode::Unsupported,
        ErrorClass::Permanent,
        "this picture is larger than Rail draws",
    )
}
