//! IMAP connection lifecycle, TLS/STARTTLS, capabilities, and error mapping.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_imap::error::Error as ImapError;
use async_imap::types::Capabilities;
use async_imap::{Client, Session};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::config::model::TlsMode;
use crate::ops::Secret;
use crate::provider::ProviderError;

/// Default timeout for IMAP connect and handshakes.
pub const IMAP_TIMEOUT: Duration = Duration::from_secs(30);

/// Stream abstraction supporting plain TCP, TLS, and mock streams for testing.
pub enum ImapStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
    #[cfg(test)]
    Mock(tokio::io::DuplexStream),
}

impl fmt::Debug for ImapStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(_) => write!(f, "ImapStream::Plain"),
            Self::Tls(_) => write!(f, "ImapStream::Tls"),
            #[cfg(test)]
            Self::Mock(_) => write!(f, "ImapStream::Mock"),
        }
    }
}

impl AsyncRead for ImapStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            #[cfg(test)]
            Self::Mock(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ImapStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
            #[cfg(test)]
            Self::Mock(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
            #[cfg(test)]
            Self::Mock(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
            #[cfg(test)]
            Self::Mock(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// IMAP capabilities detected during connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImapCapabilities {
    pub move_support: bool,
    pub uidplus_support: bool,
    pub idle_support: bool,
    pub starttls_support: bool,
}

impl ImapCapabilities {
    pub fn from_capabilities(caps: &Capabilities) -> Self {
        Self {
            move_support: caps.has_str("MOVE"),
            uidplus_support: caps.has_str("UIDPLUS"),
            idle_support: caps.has_str("IDLE"),
            starttls_support: caps.has_str("STARTTLS"),
        }
    }

    pub fn from_strs<'a>(iter: impl IntoIterator<Item = &'a str>) -> Self {
        let mut caps = Self::default();
        for s in iter {
            if s.eq_ignore_ascii_case("MOVE") {
                caps.move_support = true;
            } else if s.eq_ignore_ascii_case("UIDPLUS") {
                caps.uidplus_support = true;
            } else if s.eq_ignore_ascii_case("IDLE") {
                caps.idle_support = true;
            } else if s.eq_ignore_ascii_case("STARTTLS") {
                caps.starttls_support = true;
            }
        }
        caps
    }
}

/// Builds a TLS connector trust-anchored by `webpki-roots`.
pub fn build_tls_connector() -> Result<TlsConnector, ProviderError> {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Parse server name for TLS handshake.
pub fn parse_server_name(host: &str) -> Result<ServerName<'static>, ProviderError> {
    ServerName::try_from(host.to_string()).map_err(|e| {
        ProviderError::permanent("the IMAP server hostname is not valid for TLS")
            .with_detail(e.to_string())
    })
}

/// Connect to IMAP host and negotiate TLS/STARTTLS per configured mode.
pub async fn connect_imap(
    host: &str,
    port: u16,
    tls_mode: TlsMode,
) -> Result<Client<ImapStream>, ProviderError> {
    tokio::time::timeout(IMAP_TIMEOUT, connect_imap_timed(host, port, tls_mode))
        .await
        .map_err(|_| ProviderError::transient("IMAP connection timed out"))?
}

async fn connect_imap_timed(
    host: &str,
    port: u16,
    tls_mode: TlsMode,
) -> Result<Client<ImapStream>, ProviderError> {
    let direct_tls = port == 993 && tls_mode != TlsMode::Off;
    if direct_tls {
        connect_direct_tls(host, port).await
    } else {
        connect_plain_and_upgrade(host, port, tls_mode).await
    }
}

async fn connect_direct_tls(host: &str, port: u16) -> Result<Client<ImapStream>, ProviderError> {
    let connector = build_tls_connector()?;
    let server_name = parse_server_name(host)?;
    let tcp = TcpStream::connect((host, port)).await.map_err(|e| {
        ProviderError::transient("failed to connect to the IMAP server").with_detail(e.to_string())
    })?;
    let tls = connector.connect(server_name, tcp).await.map_err(|e| {
        ProviderError::transient("the IMAP TLS handshake failed").with_detail(e.to_string())
    })?;
    let mut client = Client::new(ImapStream::Tls(Box::new(tls)));
    read_greeting(&mut client).await?;
    Ok(client)
}

async fn connect_plain_and_upgrade(
    host: &str,
    port: u16,
    tls_mode: TlsMode,
) -> Result<Client<ImapStream>, ProviderError> {
    let tcp = TcpStream::connect((host, port)).await.map_err(|e| {
        ProviderError::transient("failed to connect to the IMAP server").with_detail(e.to_string())
    })?;
    let mut client = Client::new(ImapStream::Plain(tcp));
    read_greeting(&mut client).await?;

    match tls_mode {
        // Opportunistic means "use STARTTLS when the server offers it" only in the sense
        // that the port may be 143; credentials are never sent over a connection that is not
        // TLS unless the user explicitly chose Off. A failed STARTTLS is final, never a
        // reason to reconnect in clear.
        TlsMode::Required | TlsMode::Opportunistic => upgrade_starttls(client, host).await,
        TlsMode::Off => Ok(client),
    }
}

async fn upgrade_starttls(
    mut client: Client<ImapStream>,
    host: &str,
) -> Result<Client<ImapStream>, ProviderError> {
    client
        .run_command_and_check_ok("STARTTLS", None)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    let plain_tcp = match client.into_inner() {
        ImapStream::Plain(s) => s,
        _ => {
            return Err(ProviderError::permanent(
                "unexpected stream type during STARTTLS upgrade",
            ));
        }
    };
    let connector = build_tls_connector()?;
    let server_name = parse_server_name(host)?;
    let tls = connector
        .connect(server_name, plain_tcp)
        .await
        .map_err(|e| {
            ProviderError::transient("the IMAP TLS handshake failed").with_detail(e.to_string())
        })?;
    Ok(Client::new(ImapStream::Tls(Box::new(tls))))
}

async fn read_greeting(client: &mut Client<ImapStream>) -> Result<(), ProviderError> {
    client
        .read_response()
        .await
        .map_err(|e| {
            ProviderError::transient("failed to read the IMAP server greeting")
                .with_detail(e.to_string())
        })?
        .ok_or_else(|| ProviderError::transient("unexpected EOF waiting for IMAP greeting"))?;
    Ok(())
}

/// Authenticate an IMAP client, returning the active session and capabilities.
pub async fn login_imap(
    client: Client<ImapStream>,
    user: &str,
    pass: &Secret<String>,
) -> Result<(Session<ImapStream>, ImapCapabilities), ProviderError> {
    tokio::time::timeout(IMAP_TIMEOUT, async {
        let (mut session, caps_opt) = client
            .login_with_capabilities(user, pass.expose())
            .await
            .map_err(|(err, _)| classify_imap_error(&err))?;
        let caps = match caps_opt {
            Some(c) => ImapCapabilities::from_capabilities(&c),
            None => query_session_capabilities(&mut session).await?,
        };
        Ok((session, caps))
    })
    .await
    .map_err(|_| ProviderError::transient("IMAP login timed out"))?
}

/// Connect and log in to an IMAP server, returning the authenticated session and capabilities.
pub async fn connect_and_login_imap(
    host: &str,
    port: u16,
    tls_mode: TlsMode,
    user: &str,
    pass: &Secret<String>,
) -> Result<(Session<ImapStream>, ImapCapabilities), ProviderError> {
    let client = connect_imap(host, port, tls_mode).await?;
    login_imap(client, user, pass).await
}

/// Query capabilities on an authenticated session.
pub async fn query_session_capabilities(
    session: &mut Session<ImapStream>,
) -> Result<ImapCapabilities, ProviderError> {
    let caps = session
        .capabilities()
        .await
        .map_err(|e| classify_imap_error(&e))?;
    Ok(ImapCapabilities::from_capabilities(&caps))
}

/// Classify an `async_imap::error::Error` into Rail's `ProviderError`. Server text is detail
/// only; `message` is always Rail's own words.
pub fn classify_imap_error(err: &ImapError) -> ProviderError {
    match err {
        ImapError::Io(io_err) => {
            ProviderError::transient("IMAP network error").with_detail(io_err.to_string())
        }
        ImapError::ConnectionLost => ProviderError::transient("IMAP connection lost"),
        ImapError::No(msg) | ImapError::Bad(msg) => {
            let lower = msg.to_lowercase();
            if is_auth_failure(&lower) {
                ProviderError::NeedsAuth {
                    detail: Some(sanitize_detail(msg)),
                }
            } else {
                ProviderError::permanent("the IMAP server refused the command")
                    .with_detail(sanitize_detail(msg))
            }
        }
        ImapError::Parse(e) => ProviderError::permanent("the IMAP response could not be parsed")
            .with_detail(e.to_string()),
        ImapError::Validate(e) => {
            ProviderError::permanent("the IMAP command was invalid").with_detail(e.to_string())
        }
        ImapError::Append => ProviderError::transient("failed to append message to IMAP mailbox"),
        _ => ProviderError::permanent("IMAP protocol error").with_detail(err.to_string()),
    }
}

fn is_auth_failure(lower: &str) -> bool {
    lower.contains("authenticationfailed")
        || lower.contains("authentication failed")
        || lower.contains("invalid credentials")
        || lower.contains("login failed")
        || lower.contains("invalid user")
        || lower.contains("bad username or password")
        || lower.contains("logindisabled")
}

fn sanitize_detail(msg: &str) -> String {
    let clean: String = msg
        .chars()
        .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
        .take(200)
        .collect();
    clean.trim().to_string()
}
