//! Error types for qork.
//!
//! Small surface: a bad URL, an API/network failure, or local I/O. Messages
//! are written for an end user reading them in a terminal.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    /// The URL failed a client-side check before we ever called the server.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// The API returned an error, or we couldn't reach / parse it. The string
    /// is already user-facing (often the server's own `error` message).
    #[error("{0}")]
    Api(String),

    /// Local I/O failed (e.g. removing the binary during uninstall).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn invalid_url(message: impl Into<String>) -> Self {
        Self::InvalidUrl(message.into())
    }

    pub fn api(message: impl Into<String>) -> Self {
        Self::Api(message.into())
    }
}
