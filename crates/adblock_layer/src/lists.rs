//! Filter list downloads and a disk cache with per-source JSON timestamps.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use adblock::lists::{ParseOptions, parse_filter};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const USER_AGENT: &str = "Glowtune/0.1 (filter list updater)";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// A named filter list download source.
#[derive(Debug, Clone, Copy)]
pub struct ListSource {
    /// Cache filename stem, limited to 1–64 ASCII letters, digits, underscores, or hyphens.
    pub id: &'static str,
    /// Filter list URL.
    pub url: &'static str,
}

/// Default network and cosmetic lists for service webviews.
pub const DEFAULT_LISTS: &[ListSource] = &[
    ListSource {
        id: "easylist",
        url: "https://easylist.to/easylist/easylist.txt",
    },
    ListSource {
        id: "easyprivacy",
        url: "https://easylist.to/easylist/easyprivacy.txt",
    },
    ListSource {
        id: "ublock-filters",
        url: "https://ublockorigin.github.io/uAssets/filters/filters.txt",
    },
    ListSource {
        id: "ublock-quick-fixes",
        url: "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
    },
];

/// A directory of filter texts (`id.txt`) and freshness metadata (`id.json`).
#[derive(Debug)]
pub struct ListCache {
    dir: PathBuf,
}

/// Per-source outcomes of a refresh; failures do not abort remaining sources.
#[derive(Debug, Default)]
pub struct RefreshReport {
    /// IDs whose filter text and metadata were both updated.
    pub updated: Vec<String>,
    /// Failed source IDs and their error messages.
    pub failed: Vec<(String, String)>,
    /// IDs with fresh cached texts that required no download.
    pub skipped: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    fetched_unix: u64,
}

impl ListCache {
    /// Creates the cache directory and any missing parents.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Loads readable UTF-8 cached texts in source order, regardless of age.
    /// Missing or unreadable files and unsafe source IDs are skipped.
    pub fn load_cached(&self, sources: &[ListSource]) -> Vec<String> {
        sources
            .iter()
            .filter_map(|source| fs::read_to_string(self.path(source, "txt").ok()?).ok())
            .collect()
    }

    /// Reports missing texts, bad metadata, future timestamps, or age at least `max_age` as stale.
    /// Freshness uses `fetched_unix` in JSON, never the text file's modification time.
    pub fn is_stale(&self, source: &ListSource, max_age: Duration) -> bool {
        let fresh = (|| -> Option<bool> {
            if !self.path(source, "txt").ok()?.is_file() {
                return None;
            }
            let metadata: Metadata =
                serde_json::from_slice(&fs::read(self.path(source, "json").ok()?).ok()?).ok()?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
            Some(now.checked_sub(Duration::from_secs(metadata.fetched_unix))? < max_age)
        })();
        fresh != Some(true)
    }

    /// Sequentially downloads stale lists with a 20-second timeout and an 8 MiB body limit each.
    /// Responses must be 2xx, UTF-8, non-HTML, and contain at least 100 parseable rules.
    /// Each text and metadata file is replaced by temporary-file rename; failed downloads retain old texts.
    /// Use a seven-day `max_age` for weekly updates.
    pub async fn refresh(&self, sources: &[ListSource], max_age: Duration) -> RefreshReport {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build();
        let mut report = RefreshReport::default();
        for source in sources {
            if !self.is_stale(source, max_age) {
                report.skipped.push(source.id.to_owned());
                continue;
            }
            let result = match &client {
                Ok(client) => self
                    .refresh_one(client, source)
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            match result {
                Ok(()) => report.updated.push(source.id.to_owned()),
                Err(error) => {
                    tracing::warn!(list_id = source.id, "filter list refresh failed");
                    report.failed.push((source.id.to_owned(), error));
                }
            }
        }
        report
    }

    async fn refresh_one(&self, client: &reqwest::Client, source: &ListSource) -> Result<()> {
        let text_path = self.path(source, "txt")?;
        let metadata_path = self.path(source, "json")?;
        let mut response = client.get(source.url).send().await?;
        if !response.status().is_success() {
            return Err(Error::HttpStatus(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err(Error::InvalidList("body exceeds 8 MiB"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > MAX_BYTES - bytes.len() {
                return Err(Error::InvalidList("body exceeds 8 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes).map_err(|_| Error::InvalidList("body is not UTF-8"))?;
        validate_list(&text)?;
        let fetched_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidList("system clock precedes Unix epoch"))?
            .as_secs();
        let metadata = serde_json::to_vec(&Metadata { fetched_unix })?;
        atomic_write(&text_path, text.as_bytes())?;
        atomic_write(&metadata_path, &metadata)?;
        Ok(())
    }

    fn path(&self, source: &ListSource, extension: &str) -> Result<PathBuf> {
        if source.id.is_empty()
            || source.id.len() > 64
            || !source
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(Error::InvalidSourceId);
        }
        Ok(self.dir.join(format!("{}.{}", source.id, extension)))
    }
}

fn validate_list(text: &str) -> Result<()> {
    if text.len() > MAX_BYTES {
        return Err(Error::InvalidList("body exceeds 8 MiB"));
    }
    // Bare markup can parse as network substring rules, so reject it before counting rules.
    if text.lines().any(|line| line.trim_start().starts_with('<')) {
        return Err(Error::InvalidList("body is HTML"));
    }
    let rules = text
        .lines()
        .filter(|line| parse_filter(line, false, ParseOptions::default()).is_ok())
        .take(100)
        .count();
    if rules < 100 {
        return Err(Error::InvalidList("fewer than 100 parseable rules"));
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let (temp, mut file) = loop {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: ListSource = ListSource {
        id: "fixture",
        url: "https://example.invalid/list.txt",
    };
    const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

    fn fixture() -> String {
        (0..100)
            .map(|n| format!("||ad{n}.example.com^\n"))
            .collect()
    }

    fn seed(cache: &ListCache, source: &ListSource, fetched_unix: u64) {
        atomic_write(&cache.path(source, "txt").unwrap(), fixture().as_bytes()).unwrap();
        atomic_write(
            &cache.path(source, "json").unwrap(),
            &serde_json::to_vec(&Metadata { fetched_unix }).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn creates_directory_and_loads_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path().join("nested/cache")).unwrap();
        assert!(cache.dir.is_dir());
        assert!(cache.load_cached(&[SOURCE]).is_empty());
        assert!(cache.is_stale(&SOURCE, WEEK));
    }

    #[test]
    fn metadata_controls_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        seed(&cache, &SOURCE, now);
        assert!(!cache.is_stale(&SOURCE, WEEK));
        seed(&cache, &SOURCE, now - WEEK.as_secs() - 1);
        assert!(cache.is_stale(&SOURCE, WEEK));
        assert_eq!(cache.load_cached(&[SOURCE]), [fixture()]);
    }

    #[test]
    fn bad_metadata_and_future_timestamps_are_stale() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        seed(&cache, &SOURCE, u64::MAX);
        assert!(cache.is_stale(&SOURCE, WEEK));
        fs::write(cache.path(&SOURCE, "json").unwrap(), "not JSON").unwrap();
        assert!(cache.is_stale(&SOURCE, WEEK));
        fs::remove_file(cache.path(&SOURCE, "json").unwrap()).unwrap();
        assert!(cache.is_stale(&SOURCE, WEEK));
    }

    #[test]
    fn missing_text_is_stale_even_with_fresh_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        seed(&cache, &SOURCE, now);
        fs::remove_file(cache.path(&SOURCE, "txt").unwrap()).unwrap();
        assert!(cache.is_stale(&SOURCE, WEEK));
    }

    #[test]
    fn load_cached_skips_missing_and_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        seed(&cache, &SOURCE, 0);
        fs::write(dir.path().join("bad.txt"), [0xff]).unwrap();
        let bad = ListSource {
            id: "bad",
            ..SOURCE
        };
        assert_eq!(
            cache.load_cached(&[bad, SOURCE, DEFAULT_LISTS[0]]),
            [fixture()]
        );
    }

    #[test]
    fn rejects_tiny_empty_and_comment_only_lists() {
        for text in [
            String::new(),
            "||ads.example.com^".into(),
            "! comment\n".repeat(101),
            "##.sponsored\n".repeat(99),
        ] {
            assert!(validate_list(&text).is_err());
        }
    }

    #[test]
    fn rejects_html_even_with_enough_filter_lines() {
        assert!(validate_list(&format!("<!DOCTYPE HTML>\n<html>\n{}", fixture())).is_err());
        assert!(validate_list(&"<div>error</div>\n".repeat(101)).is_err());
    }

    #[test]
    fn accepts_filter_text_and_generic_cosmetics() {
        assert!(validate_list(&format!("[Adblock Plus 2.0]\n! fixture\n{}", fixture())).is_ok());
        assert!(validate_list(&"##.sponsored\n".repeat(100)).is_ok());
    }

    #[test]
    fn rejects_oversized_list() {
        assert!(validate_list(&"x".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn atomic_write_replaces_file_without_temporary_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.txt");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_atomic_rename_cleans_up_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("directory.txt");
        fs::create_dir(&path).unwrap();
        assert!(atomic_write(&path, b"new").is_err());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn unsafe_ids_cannot_escape_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        for id in ["", "../outside", "a/b", "a\\b", "C:outside", ".", ".."] {
            let source = ListSource { id, ..SOURCE };
            assert!(cache.path(&source, "txt").is_err());
            assert!(cache.is_stale(&source, WEEK));
            assert!(cache.load_cached(&[source]).is_empty());
        }
    }

    #[tokio::test]
    async fn refresh_skips_fresh_sources_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        seed(&cache, &SOURCE, now);
        let report = cache.refresh(&[SOURCE], WEEK).await;
        assert_eq!(report.skipped, [SOURCE.id]);
        assert!(report.failed.is_empty());
        assert!(report.updated.is_empty());
    }

    #[tokio::test]
    async fn refresh_continues_after_failures_and_retains_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        let bad = ListSource {
            url: "not a URL",
            ..SOURCE
        };
        let second = ListSource {
            id: "second",
            url: "not a URL",
        };
        seed(&cache, &bad, 0);
        let report = cache.refresh(&[bad, second], WEEK).await;
        assert_eq!(
            report
                .failed
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["fixture", "second"]
        );
        assert_eq!(cache.load_cached(&[bad]), [fixture()]);
        assert!(report.updated.is_empty());
    }

    #[tokio::test]
    #[ignore = "downloads EasyList from the network"]
    async fn refresh_easylist_from_network() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ListCache::new(dir.path()).unwrap();
        let report = cache.refresh(&DEFAULT_LISTS[..1], WEEK).await;
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.updated, ["easylist"]);
        assert!(!cache.is_stale(&DEFAULT_LISTS[0], WEEK));
        assert_eq!(cache.load_cached(&DEFAULT_LISTS[..1]).len(), 1);
    }
}
