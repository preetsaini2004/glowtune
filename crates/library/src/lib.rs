//! Incremental local music index, full-text search, and folder change notifications.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod db;
mod query;
mod scan;
mod tags;
mod watch;

use std::{path::PathBuf, time::Duration};

pub use db::Library;
pub use watch::LibraryWatcher;

/// A stable track identifier within one database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(
    /// SQLite row identifier.
    pub i64,
);

/// An album identifier within one database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlbumId(
    /// SQLite row identifier.
    pub i64,
);

/// Indexed metadata for an audio file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Database identifier.
    pub id: TrackId,
    /// Absolute file path.
    pub path: PathBuf,
    /// Tag title, falling back to the file stem.
    pub title: String,
    /// Performing artist, or an empty string.
    pub artist: String,
    /// Album title, falling back to "Unknown album".
    pub album: String,
    /// Album artist, falling back to the performing artist.
    pub album_artist: String,
    /// Album containing this track.
    pub album_id: AlbumId,
    /// Track number within the disc.
    pub track_no: Option<u32>,
    /// Disc number within the album.
    pub disc_no: Option<u32>,
    /// Release year, when tagged.
    pub year: Option<u32>,
    /// Audio duration, when known.
    pub duration: Option<Duration>,
    /// Audio properties used by the quality badge.
    pub format: AudioFormat,
    /// Whether embedded or adjacent cover art was found during indexing.
    pub has_cover: bool,
}

/// Codec and audio properties reported by Lofty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFormat {
    /// Display codec name.
    pub codec: String,
    /// Whether this codec preserves the source samples.
    pub lossless: bool,
    /// Bits per sample, when known.
    pub bit_depth: Option<u8>,
    /// Samples per second, when known.
    pub sample_rate: Option<u32>,
    /// Channel count, when known.
    pub channels: Option<u8>,
    /// Audio bitrate in kilobits per second, when known.
    pub bitrate_kbps: Option<u32>,
}

/// An album grouped by case-insensitive album artist and title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    /// Database identifier.
    pub id: AlbumId,
    /// Display album title.
    pub title: String,
    /// Display album artist.
    pub artist: String,
    /// Earliest tagged release year among its tracks.
    pub year: Option<u32>,
    /// Number of indexed tracks.
    pub track_count: u32,
    /// One track with cover art, if available.
    pub cover_track: Option<TrackId>,
}

/// Cover image loaded on demand, never stored in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    /// Encoded image bytes.
    pub data: Vec<u8>,
    /// Image MIME type.
    pub mime: String,
}

/// Scan counters delivered at every hundredth file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanProgress {
    /// Audio files encountered so far.
    pub seen: u64,
    /// New or changed files indexed so far.
    pub updated: u64,
}

/// Outcome of an incremental scan or filesystem change batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Newly indexed files.
    pub added: u64,
    /// Previously indexed files whose metadata changed.
    pub updated: u64,
    /// Deleted index entries.
    pub removed: u64,
    /// Files skipped because size and modification time matched.
    pub unchanged: u64,
    /// Files or directories that could not be read, with error details.
    pub failed: Vec<(PathBuf, String)>,
    /// Whether cancellation stopped the scan.
    pub cancelled: bool,
}

/// Aggregate indexed library sizes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryStats {
    /// Indexed track count.
    pub tracks: u64,
    /// Nonempty album count.
    pub albums: u64,
    /// Sum of known track durations.
    pub total_duration: Duration,
}

/// Library setup, database, filesystem, or watcher failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// SQLite failed.
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Native filesystem notifications failed.
    #[error(transparent)]
    Watch(#[from] notify::Error),
    /// A requested root is not a directory or is not registered.
    #[error("Not an added music folder: {0}")]
    NotAFolder(PathBuf),
}

/// A library operation's result.
pub type Result<T> = std::result::Result<T, Error>;
