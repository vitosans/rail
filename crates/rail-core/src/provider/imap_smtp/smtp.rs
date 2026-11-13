//! SMTP connection, authentication, and mail submission using Lettre.

use std::str::FromStr;
use std::time::Duration;

use lettre::address::{Address, Envelope};
use lettre::transport::smtp::Error as SmtpError;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::config::model::TlsMode;
use crate::ops::Secret;
use crate::provider::{OutgoingMail, ProviderError, SentReceipt};

/// Default timeout for SMTP connect and submission.
pub const SMTP_TIMEOUT: Duration = Duration::from_secs(30);

pub type SmtpTransport = AsyncSmtpTransport<Tokio1Executor>;

/// Build an SMTP transport for an account's configuration.
pub fn build_smtp_transport(
    host: &str,
    port: Option<u16>,
    tls_mode: TlsMode,
    username: Option<&str>,
    password: Option<&Secret<String>>,
) -> Result<SmtpTransport, ProviderError> {
    let effective_port = port.unwrap_or_else(|| default_smtp_port(tls_mode));

    let mut builder = SmtpTransport::builder_dangerous(host)
        .port(effective_port)
        .timeout(Some(SMTP_TIMEOUT));

    let tls = build_tls_setting(host, effective_port, tls_mode)?;
    builder = builder.tls(tls);

    if let (Some(user), Some(pass)) = (username, password) {
        let creds = Credentials::new(user.to_string(), pass.expose().to_string());
        builder = builder.credentials(creds);
    }

    Ok(builder.build())
}

/// Default port per TLS mode.
pub fn default_smtp_port(tls_mode: TlsMode) -> u16 {
    match tls_mode {
        TlsMode::Required => 465,
        TlsMode::Opportunistic => 587,
        TlsMode::Off => 25,
    }
}

fn build_tls_setting(host: &str, port: u16, tls_mode: TlsMode) -> Result<Tls, ProviderError> {
    match tls_mode {
        TlsMode::Required => {
            let tls_params = TlsParameters::new(host.to_string()).map_err(|e| {
                ProviderError::permanent("the SMTP server hostname is not valid for TLS")
                    .with_detail(e.to_string())
            })?;
            if port == 465 {
                Ok(Tls::Wrapper(tls_params))
            } else {
                Ok(Tls::Required(tls_params))
            }
        }
        // Lettre's Opportunistic silently stays in clear when STARTTLS is stripped. Rail's
        // opportunistic mode may use the STARTTLS port, but it still refuses to authenticate
        // without TLS; only `off` allows a clear connection.
        TlsMode::Opportunistic => {
            let tls_params = TlsParameters::new(host.to_string()).map_err(|e| {
                ProviderError::permanent("the SMTP server hostname is not valid for TLS")
                    .with_detail(e.to_string())
            })?;
            if port == 465 {
                Ok(Tls::Wrapper(tls_params))
            } else {
                Ok(Tls::Required(tls_params))
            }
        }
        TlsMode::Off => Ok(Tls::None),
    }
}

/// Test connection to SMTP server using NOOP.
pub async fn validate_smtp_connection(transport: &SmtpTransport) -> Result<(), ProviderError> {
    tokio::time::timeout(SMTP_TIMEOUT, transport.test_connection())
        .await
        .map_err(|_| ProviderError::transient("SMTP connection test timed out"))?
        .map_err(|e| classify_smtp_error(&e))?;
    Ok(())
}

/// Submit an outgoing email to the SMTP server.
pub async fn send_smtp(
    transport: &SmtpTransport,
    mail: &OutgoingMail,
) -> Result<SentReceipt, ProviderError> {
    tokio::time::timeout(SMTP_TIMEOUT, send_smtp_timed(transport, mail))
        .await
        .map_err(|_| ProviderError::transient("SMTP send timed out"))?
}

async fn send_smtp_timed(
    transport: &SmtpTransport,
    mail: &OutgoingMail,
) -> Result<SentReceipt, ProviderError> {
    let raw = crate::provider::gmail::mime::build(&mail.outgoing)?;
    let from_addr = Address::from_str(&mail.outgoing.from).map_err(|e| {
        ProviderError::permanent("the From address is not valid").with_detail(e.to_string())
    })?;

    let to_addrs = collect_recipient_addresses(mail)?;
    if to_addrs.is_empty() {
        return Err(ProviderError::permanent(
            "cannot send mail with no recipients",
        ));
    }

    let envelope = Envelope::new(Some(from_addr), to_addrs).map_err(|e| {
        ProviderError::permanent("failed to construct the message envelope")
            .with_detail(e.to_string())
    })?;

    transport
        .send_raw(&envelope, &raw)
        .await
        .map_err(|e| classify_smtp_error(&e))?;

    Ok(SentReceipt {
        message_id: Some(mail.outgoing.message_id.clone()),
        thread_id: mail.thread.clone(),
    })
}

/// Parse and collect recipient addresses from `to`, `cc`, and `bcc`.
pub fn collect_recipient_addresses(mail: &OutgoingMail) -> Result<Vec<Address>, ProviderError> {
    let mut to_addrs = Vec::new();
    for mb in mail
        .outgoing
        .to
        .iter()
        .chain(&mail.outgoing.cc)
        .chain(&mail.outgoing.bcc)
    {
        let addr = Address::from_str(&mb.address).map_err(|e| {
            ProviderError::permanent("a recipient address is not valid").with_detail(e.to_string())
        })?;
        to_addrs.push(addr);
    }
    Ok(to_addrs)
}

/// Classify a Lettre `SmtpError` into Rail's `ProviderError`. Server text is detail only.
pub fn classify_smtp_error(err: &SmtpError) -> ProviderError {
    let msg = err.to_string();
    if err.is_timeout() {
        return ProviderError::transient("SMTP connection timed out");
    }
    if err.is_transient() {
        return ProviderError::transient("SMTP transient error").with_detail(sanitize_detail(&msg));
    }
    if let Some(code) = err.status() {
        let num: u16 = code.into();
        if num == 535 || is_smtp_auth_failure(&msg) {
            return ProviderError::NeedsAuth {
                detail: Some(sanitize_detail(&msg)),
            };
        }
    } else if is_smtp_auth_failure(&msg) {
        return ProviderError::NeedsAuth {
            detail: Some(sanitize_detail(&msg)),
        };
    }
    if err.is_permanent() {
        return ProviderError::permanent("the SMTP server refused the message")
            .with_detail(sanitize_detail(&msg));
    }
    ProviderError::transient("SMTP network error").with_detail(sanitize_detail(&msg))
}

fn is_smtp_auth_failure(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("535")
        || lower.contains("authentication failed")
        || lower.contains("invalid credentials")
        || lower.contains("bad username or password")
        || lower.contains("auth failed")
}

fn sanitize_detail(msg: &str) -> String {
    let clean: String = msg
        .chars()
        .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
        .take(200)
        .collect();
    clean.trim().to_string()
}
