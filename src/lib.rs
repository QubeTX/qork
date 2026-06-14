//! qork — shorten URLs from your terminal.
//!
//! The library exposes the pieces `main.rs` wires together: CLI parsing
//! ([`cli`]), the shorten API client ([`api`]), runtime [`config`], the
//! [`error`] type, and self-[`update`].
//!
//! # Example
//!
//! ```no_run
//! use qork::{config::Config, shorten};
//!
//! let config = Config::new(); // talks to https://qork.me
//! let link = shorten("https://example.com/some/long/path", None, &config)?;
//! println!("{}", link.display_url());
//! # Ok::<(), qork::AppError>(())
//! ```

pub mod api;
pub mod cli;
pub mod command;
pub mod config;
pub mod error;
pub mod update;

pub use api::{shorten, ShortLink};
pub use cli::Cli;
pub use command::Command;
pub use config::{Config, OutputFormat};
pub use error::{AppError, Result};

/// The crate version (`CARGO_PKG_VERSION`), reused for `--version`, the
/// `User-Agent`, and the self-update check.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
