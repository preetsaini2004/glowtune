//! Synced lyrics: LRCLIB client, .lrc parser, cache and sync clock.

/// Persistent lyrics and per-song timing adjustments.
pub mod cache;
/// Metadata cleanup for lyrics queries.
pub mod clean;
/// Playback position interpolation.
pub mod clock;
/// Discovery of lyrics beside local audio files.
pub mod local;
/// Line-based LRC parsing and lookup.
pub mod lrc;
/// Asynchronous LRCLIB requests and result selection.
pub mod lrclib;

pub use cache::{CacheEntry, CachedLyrics, LyricsCache};
pub use clean::{Query, clean_query};
pub use clock::SyncClock;
pub use local::find_local_lrc;
pub use lrc::{Line, Lyrics};
pub use lrclib::{LrclibClient, LrclibRecord, pick_best};

/// Errors from fetching or storing lyrics.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An HTTP request, response status, or response decoding failed.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Cached JSON could not be encoded or decoded.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The API base URL is not an HTTP(S) URL without query or fragment.
    #[error("invalid LRCLIB base URL: {0}")]
    InvalidBaseUrl(String),
    /// A cache key contains unsafe filename characters or is too long.
    #[error("invalid lyrics cache key")]
    InvalidCacheKey,
}

/// Result type used by this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
