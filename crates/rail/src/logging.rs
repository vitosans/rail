//! File logging under the state dir. Never logs tokens or message bodies.

use rail_core::paths::Paths;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

const LOG_FILE: &str = "rail.log";

/// Starts logging to `rail.log` in the state dir. The returned guard must live
/// until exit so buffered lines are flushed. If logging cannot be set up, one
/// warning goes to stderr and the CLI runs without file logs. Never panics.
/// With `mcp` (`rail mcp`), rmcp logs only errors: its warnings about tool arguments that do
/// not parse quote the agent's value, which may be a pasted secret.
pub fn init(paths: &Paths, mcp: bool) -> Option<WorkerGuard> {
    let dir = paths.log_dir();
    match try_init(dir, filter(mcp)) {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!(
                "rail: warning: cannot set up logging in {}: {e}; continuing without file logs",
                dir.display()
            );
            None
        }
    }
}

/// Libraries kept at `warn` whatever `RUST_LOG` says: at debug and trace they log request
/// lines with query strings (Gmail's `q` is the user's search text), headers (the bearer
/// token), bodies (mail), IMAP commands (LOGIN with the password) and SMTP writes (AUTH
/// PLAIN and the message). Rail logs its own requests without those.
const PINNED: &[&str] = &[
    "hyper=warn",
    "hyper_util=warn",
    "h2=warn",
    "reqwest=warn",
    "rustls=warn",
    "async_imap=warn",
    "imap_proto=warn",
    "lettre=warn",
];

/// `RUST_LOG`, or `info`, with `PINNED` on top; for `rail mcp` also `rmcp=error`.
fn filter(mcp: bool) -> EnvFilter {
    let base = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    pinned(base, mcp)
}

fn pinned(base: EnvFilter, mcp: bool) -> EnvFilter {
    let rmcp = mcp.then_some("rmcp=error");
    PINNED
        .iter()
        .copied()
        .chain(rmcp)
        .filter_map(|directive| directive.parse().ok())
        .fold(base, EnvFilter::add_directive)
}

fn try_init(dir: &std::path::Path, filter: EnvFilter) -> Result<WorkerGuard, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = RollingFileAppender::builder()
        .rotation(Rotation::NEVER)
        .filename_prefix(LOG_FILE)
        .build(dir)
        .map_err(|e| e.to_string())?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|e| e.to_string())?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_returns_none_when_log_file_is_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::all_in(dir.path());
        std::fs::create_dir_all(paths.log_dir().join(LOG_FILE)).unwrap();
        assert!(init(&paths, false).is_none());
    }

    /// The HTTP, TLS, IMAP and SMTP libraries stay at `warn` even when `RUST_LOG` asks for
    /// more of them. A trace-level login must not put credentials in the log.
    #[test]
    fn libraries_that_can_log_credentials_stay_at_warn() {
        let filter = pinned(
            EnvFilter::new(
                "trace,hyper=trace,reqwest=debug,async_imap=trace,imap_proto=trace,lettre=trace",
            ),
            false,
        )
        .to_string();
        for pin in PINNED {
            assert!(filter.contains(pin), "{filter}");
        }
        for unpinned in [
            "hyper=trace",
            "reqwest=debug",
            "async_imap=trace",
            "imap_proto=trace",
            "lettre=trace",
        ] {
            assert!(!filter.contains(unpinned), "{filter}");
        }
    }

    #[test]
    fn a_trace_level_login_is_not_written_to_the_log() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Buffer(Arc<Mutex<Vec<u8>>>);
        impl Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(pinned(EnvFilter::new("trace"), false))
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!(target: "async_imap::client", "LOGIN \"user\" \"hunter2\"");
            tracing::debug!(target: "lettre::transport::smtp", "AUTH PLAIN AHVzZXIAaHVudGVyMg==");
            tracing::info!(target: "rail", "this line is kept");
        });
        let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(!log.contains("hunter2"), "{log}");
        assert!(!log.contains("AUTH PLAIN"), "{log}");
        assert!(log.contains("this line is kept"), "{log}");
    }

    #[test]
    fn rail_mcp_logs_only_rmcp_errors() {
        assert!(filter(true).to_string().contains("rmcp=error"));
        assert!(!filter(false).to_string().contains("rmcp"));
    }
}
