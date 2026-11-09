//! Lyrics for the current track (PLAN.md section 8).
//!
//! ```text
//!  hub: TrackChanged ─► on_track ─► disk cache? ──yes──► emit "lyrics"
//!                                      │ no
//!                                      ▼
//!                                   LRCLIB (async) ─► cache ─► emit "lyrics"
//! ```
//! Each request gets a generation number; a slow answer for a song you've
//! already skipped is dropped instead of overwriting the new song's lyrics.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lyrics::{CacheEntry, CachedLyrics, LrclibClient, LyricsCache};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub time_ms: f64,
    pub text: String,
    /// Start time of each word, when the lyrics time words (enhanced LRC).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<WordView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordView {
    pub time_ms: f64,
    pub text: String,
}

/// What the lyrics window shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsView {
    /// "loading" | "synced" | "plain" | "instrumental" | "none"
    pub status: &'static str,
    pub lines: Vec<LineView>,
    /// Per-song timing nudge; positive shows lyrics earlier.
    pub offset_ms: i64,
}

impl LyricsView {
    fn empty(status: &'static str) -> Self {
        Self {
            status,
            lines: Vec::new(),
            offset_ms: 0,
        }
    }

    fn from_entry(entry: &CacheEntry) -> Self {
        let (status, parsed) = match &entry.lyrics {
            CachedLyrics::Synced(text) => {
                let l = lyrics::Lyrics::parse_lrc(text);
                (if l.synced { "synced" } else { "plain" }, l)
            }
            CachedLyrics::Plain(text) => ("plain", lyrics::Lyrics::from_plain(text)),
            CachedLyrics::Instrumental => {
                return Self {
                    offset_ms: entry.offset_ms,
                    ..Self::empty("instrumental")
                };
            }
            CachedLyrics::NotFound { .. } => {
                return Self {
                    offset_ms: entry.offset_ms,
                    ..Self::empty("none")
                };
            }
        };
        Self {
            status,
            lines: parsed
                .lines
                .into_iter()
                .map(|l| LineView {
                    time_ms: l.time.as_secs_f64() * 1000.0,
                    text: l.text,
                    words: l
                        .words
                        .into_iter()
                        .map(|w| WordView {
                            time_ms: w.time.as_secs_f64() * 1000.0,
                            text: w.text,
                        })
                        .collect(),
                })
                .collect(),
            offset_ms: entry.offset_ms,
        }
    }
}

pub struct LyricsService {
    client: Option<LrclibClient>,
    cache: Option<LyricsCache>,
    generation: AtomicU64,
    /// Cache key of the current song, and what's on screen.
    current: Mutex<(Option<String>, LyricsView)>,
}

impl LyricsService {
    pub fn new(cache_dir: std::path::PathBuf) -> Self {
        let client = LrclibClient::new()
            .map_err(|e| tracing::error!("lyrics client: {e}"))
            .ok();
        let cache = LyricsCache::new(cache_dir)
            .map_err(|e| tracing::warn!("lyrics cache off: {e}"))
            .ok();
        Self {
            client,
            cache,
            generation: AtomicU64::new(0),
            current: Mutex::new((None, LyricsView::empty("none"))),
        }
    }

    pub fn view(&self) -> LyricsView {
        self.current
            .lock()
            .map(|c| c.1.clone())
            .unwrap_or_else(|_| LyricsView::empty("none"))
    }

    fn publish(&self, app: &AppHandle, key: Option<String>, view: LyricsView) {
        if let Ok(mut cur) = self.current.lock() {
            *cur = (key, view.clone());
        }
        let _ = app.emit("lyrics", view);
        if let Some(gate) = app.try_state::<crate::lyrics_gate::LyricsGate>() {
            gate.changed();
        }
    }

    /// Nothing is playing anymore.
    pub fn clear(&self, app: &AppHandle) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.publish(app, None, LyricsView::empty("none"));
    }

    /// A new track started: look up its lyrics.
    pub fn on_track(
        self: &Arc<Self>,
        app: &AppHandle,
        title: &str,
        artist: &str,
        album: &str,
        duration: Option<Duration>,
    ) {
        let key = LyricsCache::key(title, artist, duration);
        let same_song = self
            .current
            .lock()
            .map(|c| c.0.as_deref() == Some(key.as_str()))
            .unwrap_or(false);
        if same_song {
            return; // e.g. metadata corrected (duration arrived) for the same song
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        if let Some(entry) = self.cache.as_ref().and_then(|c| c.get(&key)) {
            self.publish(app, Some(key), LyricsView::from_entry(&entry));
            return;
        }
        self.publish(app, Some(key.clone()), LyricsView::empty("loading"));

        let Some(client) = self.client.clone() else {
            self.publish(app, Some(key), LyricsView::empty("none"));
            return;
        };
        let (this, app) = (self.clone(), app.clone());
        let (title, artist) = (title.to_owned(), artist.to_owned());
        let album = (!album.is_empty()).then(|| album.to_owned());
        tauri::async_runtime::spawn(async move {
            let result = client
                .find(&title, &artist, album.as_deref(), duration)
                .await;
            let lyrics = match result {
                Ok(Some(r)) if r.instrumental => CachedLyrics::Instrumental,
                Ok(Some(r)) => match (r.synced_lyrics, r.plain_lyrics) {
                    (Some(s), _) if !s.trim().is_empty() => CachedLyrics::Synced(s),
                    (_, Some(p)) if !p.trim().is_empty() => CachedLyrics::Plain(p),
                    _ => CachedLyrics::NotFound {
                        checked_unix: unix_now(),
                    },
                },
                Ok(None) => CachedLyrics::NotFound {
                    checked_unix: unix_now(),
                },
                Err(e) => {
                    // Network trouble: show nothing, don't cache, retry next time.
                    tracing::warn!("lyrics lookup failed for {title}: {e}");
                    if this.generation.load(Ordering::SeqCst) == generation {
                        this.publish(&app, Some(key), LyricsView::empty("none"));
                    }
                    return;
                }
            };
            let entry = CacheEntry {
                lyrics,
                offset_ms: 0,
                source: "lrclib".into(),
            };
            if let Some(cache) = &this.cache
                && let Err(e) = cache.put(&key, &entry)
            {
                tracing::warn!("lyrics cache write: {e}");
            }
            if this.generation.load(Ordering::SeqCst) == generation {
                let view = LyricsView::from_entry(&entry);
                tracing::info!(
                    "lyrics for {title}: {} ({} lines)",
                    view.status,
                    view.lines.len()
                );
                this.publish(&app, Some(key), view);
            }
        });
    }

    /// Shifts the current song's lyrics timing and remembers it.
    pub fn nudge(&self, app: &AppHandle, delta_ms: i64) -> Result<i64, String> {
        let (key, mut view) = self
            .current
            .lock()
            .map(|c| c.clone())
            .map_err(|e| e.to_string())?;
        let key = key.ok_or("no song")?;
        view.offset_ms = (view.offset_ms + delta_ms).clamp(-10_000, 10_000);
        if let Some(cache) = &self.cache {
            cache
                .set_offset(&key, view.offset_ms)
                .map_err(|e| e.to_string())?;
        }
        let offset = view.offset_ms;
        self.publish(app, Some(key), view);
        Ok(offset)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(debug_assertions)]
impl LyricsService {
    /// Dev aid: shows sample synced lines (one every 2.5 s) so the lyrics
    /// window can be checked with any audio playing. `GLOWTUNE_SHOW=lyrics-demo`.
    pub fn show_demo(&self, app: &AppHandle) {
        let words = [
            "Turn the lights down low",
            "Let the colors find the walls",
            "Every kick a little sunrise",
            "",
            "Hold the moment while it falls",
        ];
        let lines = (0..400)
            .map(|i| LineView {
                time_ms: i as f64 * 2500.0,
                text: words[i % words.len()].to_string(),
                words: Vec::new(),
            })
            .collect();
        self.publish(
            app,
            Some("demo".into()),
            LyricsView {
                status: "synced",
                lines,
                offset_ms: 0,
            },
        );
    }
}
