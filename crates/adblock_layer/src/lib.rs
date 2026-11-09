//! Network and cosmetic filtering for service webviews, with a weekly list cache.
//!
//! ```text
//! ListCache -- cached list text --> Blocker -- request --> Verdict
//!                                    |
//!                                    +-- page URL --> hiding CSS
//! ```
//!
//! Call [`ListCache::refresh`] with a seven-day maximum age for weekly updates.
//! Rebuild [`Blocker`] from cached texts after updates; webview integration owns scheduling.

#![warn(missing_docs)]

/// Request filtering, counters, and cosmetic CSS generation.
pub mod filter;
/// Filter list sources, downloads, and persistent freshness metadata.
pub mod lists;

pub use filter::{Blocker, ResourceKind, Verdict};
pub use lists::{DEFAULT_LISTS, ListCache, ListSource, RefreshReport};

/// Errors from filter list downloads and cache operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An HTTP request or response body failed.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// Cache metadata could not be encoded or decoded.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A response had a non-success HTTP status.
    #[error("filter list HTTP status: {0}")]
    HttpStatus(u16),
    /// A downloaded body failed size, encoding, or filter list validation.
    #[error("invalid filter list: {0}")]
    InvalidList(&'static str),
    /// A source ID is not a safe cache filename component.
    #[error("invalid filter list source ID")]
    InvalidSourceId,
}

/// Result type for filter list operations.
pub type Result<T> = std::result::Result<T, Error>;
