//! The error type for caller-supplied account fetchers.

use std::fmt;

/// A failure reported by the caller-supplied account fetcher.
///
/// The fetcher is an arbitrary async closure (RPC client, bank client, test
/// map), so its native error type is erased to a message. Keeping a plain
/// string makes [`SplResolveError`] `Clone + PartialEq + Eq`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchError {
    message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

impl From<String> for FetchError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for FetchError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for FetchError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::new(error.to_string())
    }
}

impl From<std::io::Error> for FetchError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}
