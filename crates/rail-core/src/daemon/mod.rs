//! The background daemon that owns the store, the token sources, the loopback listener and
//! every secret-store call, and its clients. Front ends send requests; only the daemon
//! executes operations.

pub mod client;
pub mod connect;
mod connection;
pub mod context;
mod handlers;
mod pending;
mod registry;
mod remote_fetch;
pub mod server;
mod version;
pub mod wire;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod attachment_image_tests;
#[cfg(test)]
mod attachments_content_tests;
#[cfg(test)]
mod attachments_dest_tests;
#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod body_tests;
#[cfg(test)]
mod connect_build_tests;
#[cfg(test)]
mod connect_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod draft_files_tests;
#[cfg(test)]
mod draft_gmail_tests;
#[cfg(test)]
mod draft_tests;
#[cfg(test)]
mod imap_tests;
#[cfg(test)]
mod login_tests;
#[cfg(test)]
mod m365_draft_tests;
#[cfg(test)]
mod m365_import_tests;
#[cfg(test)]
mod m365_kept_tests;
#[cfg(test)]
mod m365_listed_tests;
#[cfg(test)]
mod m365_tests;
#[cfg(test)]
mod manage_tests;
#[cfg(test)]
mod remote_fetch_tests;
#[cfg(test)]
mod remote_image_tests;
#[cfg(test)]
mod saved_tests;
#[cfg(test)]
mod snooze_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;

pub use client::DaemonClient;
pub use connect::{
    BUILD, Connector, DAEMON_EXIT_TIMEOUT, MAX_SOCKET_PATH, START_TIMEOUT, SpawnFn, VERSION,
    retry_safe, wait_for_daemon_exit,
};
#[cfg(any(test, feature = "test-gates"))]
pub use context::{BeginGate, SaveGate};
pub use context::{DaemonContext, Opener};
pub use remote_fetch::{Ipv6, RemoteFetch};
pub use server::{IDLE_TIMEOUT, SAVE_GRACE, ServeConfig, Server, peer_allowed};
pub use wire::{MAX_FRAME_BYTES, PROTOCOL_VERSION};
