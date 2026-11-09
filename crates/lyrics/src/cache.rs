use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{Error, Result, clean_query};

const NEGATIVE_TTL: u64 = 7 * 24 * 60 * 60;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Cached lyrics text or the outcome of a lyrics lookup.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CachedLyrics {
    /// Original synced LRC text.
    Synced(String),
    /// Original plain lyric text.
    Plain(String),
    /// A confirmed instrumental recording.
    Instrumental,
    /// A negative lookup, expiring after seven days.
    NotFound {
        /// Time of the failed lookup, in seconds since the Unix epoch.
        checked_unix: u64,
    },
}

/// Lyrics plus persistent, per-song display settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CacheEntry {
    /// Lookup outcome and original lyric text.
    pub lyrics: CachedLyrics,
    /// Timing adjustment in milliseconds; positive displays lyrics earlier.
    pub offset_ms: i64,
    /// Source label, such as `lrclib` or `local`.
    pub source: String,
}

/// A directory of JSON entries, written with atomic file replacement.
#[derive(Debug)]
pub struct LyricsCache {
    dir: PathBuf,
}

impl LyricsCache {
    /// Opens a cache directory, creating it and its parents if necessary.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Returns a stable 16-digit FNV-1a hash of cleaned, lowercase metadata.
    /// Duration is rounded to the nearest two seconds (ties up); unknown stays distinct.
    /// Fields are separated with NUL bytes and duration uses decimal seconds.
    pub fn key(title: &str, artist: &str, duration: Option<Duration>) -> String {
        let query = clean_query(title, artist);
        let duration = duration.map_or_else(
            || "unknown".to_owned(),
            |duration| {
                let rounded = (duration.as_nanos() + 1_000_000_000) / 2_000_000_000 * 2;
                rounded.to_string()
            },
        );
        let input = format!(
            "{}\0{}\0{}",
            query.title.to_lowercase(),
            query.artist.to_lowercase(),
            duration
        );
        let hash = input.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        format!("{hash:016x}")
    }

    /// Reads an entry, treating invalid keys, corrupt files and expired negative lookups as misses.
    pub fn get(&self, key: &str) -> Option<CacheEntry> {
        let entry: CacheEntry =
            serde_json::from_slice(&fs::read(self.path(key).ok()?).ok()?).ok()?;
        if let CachedLyrics::NotFound { checked_unix } = entry.lyrics
            && unix_now().saturating_sub(checked_unix) > NEGATIVE_TTL
        {
            return None;
        }
        Some(entry)
    }

    /// Writes a unique temporary file in the cache directory, then atomically replaces the entry.
    /// Failed writes remove their temporary file and leave the previous entry intact.
    pub fn put(&self, key: &str, entry: &CacheEntry) -> Result<()> {
        let path = self.path(key)?;
        let bytes = serde_json::to_vec(entry)?;
        let (temp, mut file) = loop {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let temp = self
                .dir
                .join(format!("{key}.{}.{sequence}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => break (temp, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        };
        let result = (|| -> io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.map_err(Error::from)
    }

    /// Saves an offset while preserving lyrics and source.
    /// A missing entry becomes a fresh negative lookup with an empty source label.
    pub fn set_offset(&self, key: &str, offset_ms: i64) -> Result<()> {
        let mut entry = self.get(key).unwrap_or_else(|| CacheEntry {
            lyrics: CachedLyrics::NotFound {
                checked_unix: unix_now(),
            },
            offset_ms: 0,
            source: String::new(),
        });
        entry.offset_ms = offset_ms;
        self.put(key, &entry)
    }

    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::InvalidCacheKey);
        }
        Ok(self.dir.join(format!("{key}.json")))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_roundtrip_and_replace_atomically() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let cache = LyricsCache::new(dir.path().join("nested")).expect("cache");
        for lyrics in [
            CachedLyrics::Synced("[00:01]Hello".into()),
            CachedLyrics::Plain("Hello".into()),
            CachedLyrics::Instrumental,
            CachedLyrics::NotFound {
                checked_unix: unix_now(),
            },
        ] {
            let entry = CacheEntry {
                lyrics,
                offset_ms: -500,
                source: "lrclib".into(),
            };
            cache.put("song", &entry).expect("write entry");
            assert_eq!(cache.get("song"), Some(entry));
        }
        assert_eq!(fs::read_dir(&cache.dir).expect("read directory").count(), 1);
    }

    #[test]
    fn negative_entries_expire_but_lyrics_do_not() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let cache = LyricsCache::new(dir.path()).expect("cache");
        let entry = CacheEntry {
            lyrics: CachedLyrics::NotFound {
                checked_unix: unix_now() - NEGATIVE_TTL - 10,
            },
            offset_ms: 5,
            source: "lrclib".into(),
        };
        cache.put("song", &entry).expect("write entry");
        assert_eq!(cache.get("song"), None);
        cache
            .put(
                "song",
                &CacheEntry {
                    lyrics: CachedLyrics::Instrumental,
                    ..entry
                },
            )
            .expect("write entry");
        assert!(cache.get("song").is_some());
    }

    #[test]
    fn offset_update_preserves_content_and_creates_missing_entry() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let cache = LyricsCache::new(dir.path()).expect("cache");
        let mut entry = CacheEntry {
            lyrics: CachedLyrics::Plain("Keep me".into()),
            offset_ms: 0,
            source: "local".into(),
        };
        cache.put("song", &entry).expect("write entry");
        cache.set_offset("song", 250).expect("update offset");
        entry.offset_ms = 250;
        assert_eq!(cache.get("song"), Some(entry));
        cache.set_offset("missing", -500).expect("create offset");
        let missing = cache.get("missing").expect("new entry");
        assert_eq!(missing.offset_ms, -500);
        assert!(matches!(missing.lyrics, CachedLyrics::NotFound { .. }));
    }

    #[test]
    fn stable_key_matches_independent_fnv_and_normalizes_queries() {
        // Independent byte-by-byte reference for the documented wire representation.
        let mut reference: u64 = 14695981039346656037;
        for byte in b"song\0artist\x00200" {
            reference ^= u64::from(*byte);
            reference = reference.wrapping_mul(1099511628211);
        }
        let key = LyricsCache::key(
            " Song (Official Video) ",
            "Artist - Topic",
            Some(Duration::from_millis(200900)),
        );
        assert_eq!(key, format!("{reference:016x}"));
        assert_eq!(
            key,
            LyricsCache::key("SONG", "ARTIST", Some(Duration::from_secs(200)))
        );
        assert_eq!(
            key,
            LyricsCache::key("song", "artist", Some(Duration::from_secs(200)))
        );
        assert_ne!(key, LyricsCache::key("song", "artist", None));
        assert_ne!(
            key,
            LyricsCache::key("song", "artist", Some(Duration::from_secs(201)))
        );
        assert_eq!(
            LyricsCache::key("song", "artist", Some(Duration::from_secs(201))),
            LyricsCache::key("song", "artist", Some(Duration::from_secs(202)))
        );
        assert_eq!(LyricsCache::key("a\0b", "c", None).len(), 16);
    }

    #[test]
    fn corrupt_entries_and_unsafe_keys_are_misses() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let cache = LyricsCache::new(dir.path()).expect("cache");
        fs::write(dir.path().join("broken.json"), "{bad").expect("corrupt entry");
        assert_eq!(cache.get("broken"), None);
        for key in ["", "../escape", "a/b", "a\\b", "C:escape", "."] {
            assert_eq!(cache.get(key), None);
            assert!(matches!(
                cache.set_offset(key, 1),
                Err(Error::InvalidCacheKey)
            ));
        }
    }

    #[test]
    fn failed_replacement_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let cache = LyricsCache::new(dir.path()).expect("cache");
        fs::create_dir(dir.path().join("blocked.json")).expect("blocked destination");
        assert!(cache.set_offset("blocked", 1).is_err());
        assert_eq!(fs::read_dir(dir.path()).expect("read directory").count(), 1);
    }
}
