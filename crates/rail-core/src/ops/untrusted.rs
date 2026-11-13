//! Wrappers that say how a value may be handled: `Untrusted` for text from outside the user's
//! own input, `Secret` for a value that must never be printed.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Text that came from outside the user's own input: a preset's name or help link now, email
/// subjects, bodies, sender and attachment names later. It serialises as
/// `{"untrusted": <value>}` so a client can never mistake it for Rail's own words. It is data,
/// never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Untrusted<T> {
    pub untrusted: T,
}

impl<T> Untrusted<T> {
    pub fn new(value: T) -> Self {
        Self { untrusted: value }
    }

    pub fn get(&self) -> &T {
        &self.untrusted
    }

    pub fn into_inner(self) -> T {
        self.untrusted
    }
}

impl From<String> for Untrusted<String> {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for Untrusted<String> {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

/// A value that crosses the socket but must never reach a log, an error or `Debug` output.
/// It serialises as a plain string.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Secret<T = String>(T);

impl<T> Secret<T> {
    pub fn new(value: impl Into<T>) -> Self {
        Self(value.into())
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Secret<String> {
    /// The value itself, for the one place that needs it (an OAuth client, a 1Password write).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
