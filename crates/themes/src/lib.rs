//! Theme schema, loading, validation and hot reload.

#![deny(missing_docs)]

mod parse;
mod schema;
mod store;
mod watcher;

pub use parse::Warning;
pub use schema::*;
pub use store::{ThemeInfo, ThemeStore, bundled};
pub use watcher::ThemeWatcher;

/// Errors that prevent reading or writing a theme.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem or watcher operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The input has invalid TOML syntax, including its source location.
    #[error("{0}")]
    TomlSyntax(String),
    /// A theme id contains unsafe characters or is empty.
    #[error("invalid theme id: {0}")]
    InvalidId(String),
    /// No theme with this id exists.
    #[error("theme not found: {0}")]
    NotFound(String),
    /// Bundled themes cannot be deleted.
    #[error("bundled theme is read-only: {0}")]
    BundledReadOnly(String),
}

/// Result of a theme operation.
pub type Result<T, E = Error> = std::result::Result<T, E>;
