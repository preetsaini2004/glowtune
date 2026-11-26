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

/// The song whose lyrics are showing, and the LRCLIB versions tried for it.
struct Song {
    key: String,
    title: String,
    artist: String,
    duration: Option<Duration>,
    /// LRCLIB ids already shown by "Try other lyrics".
    tried: Vec<i64>,
    /// The synced lyrics text on screen now (to skip identical versions).
    shown: Option<String>,
}

/// Furthest the lyrics timing can be moved either way.
const MAX_OFFSET_MS: i64 = 10_000;

pub struct LyricsService {
    client: Option<LrclibClient>,
    cache: Option<LyricsCache>,
    generation: AtomicU64,
    /// Cache key of the current song, and what's on screen.
    current: Mutex<(Option<String>, LyricsView)>,
    /// The current song, for "Try other lyrics".
    song: Mutex<Option<Song>>,
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
            song: Mutex::new(None),
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
        if let Ok(mut song) = self.song.lock()
            && song.as_ref().is_none_or(|s| s.key != key)
        {
            *song = Some(Song {
                key: key.clone(),
                title: title.to_owned(),
                artist: artist.to_owned(),
                duration,
                tried: Vec::new(),
                shown: None,
            });
        }
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
            self.showing(&key, &entry);
            self.publish(app, Some(key), LyricsView::from_entry(&entry));
            return;
        }
        self.publish(app, Some(key.clone()), LyricsView::empty("loading"));

        // Ads (Spotify free) and blank titles have no lyrics to look up.
        let ad = title.trim().is_empty()
            || title.trim().eq_ignore_ascii_case("advertisement")
            || artist.trim().is_empty() && title.trim().eq_ignore_ascii_case("spotify");
        let Some(client) = self.client.clone().filter(|_| !ad) else {
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
                this.showing(&key, &entry);
                this.publish(&app, Some(key), view);
            }
        });
    }

    /// The current song's synced lines, for automatic alignment: only while
    /// its timing is untouched (no nudge saved, not aligned already).
    pub fn alignable(&self) -> Option<(String, Vec<audio_engine::voice::Line>)> {
        let current = self.current.lock().ok()?;
        let (Some(key), view) = &*current else {
            return None;
        };
        if view.status != "synced" || view.offset_ms != 0 {
            return None;
        }
        let lines = view
            .lines
            .iter()
            .map(|l| audio_engine::voice::Line {
                time_ms: l.time_ms,
                sung: !l.text.trim().is_empty(),
            })
            .collect();
        Some((key.clone(), lines))
    }

    /// Automatic alignment found the lyrics off by `offset_ms` (positive:
    /// show them earlier). For this session only, not saved: the next play
    /// is measured again. A nudge on top saves the sum.
    pub fn align(&self, app: &AppHandle, key: &str, offset_ms: i64) {
        let Ok((current, mut view)) = self.current.lock().map(|c| c.clone()) else {
            return;
        };
        if current.as_deref() != Some(key) {
            return;
        }
        view.offset_ms += offset_ms;
        self.publish(app, current, view);
    }

    /// Shifts the current song's lyrics timing and remembers it.
    pub fn nudge(&self, app: &AppHandle, delta_ms: i64) -> Result<i64, String> {
        let now = self.view().offset_ms;
        self.set_offset(app, now + delta_ms)
    }

    /// Sets the current song's lyrics timing (positive: earlier) and
    /// remembers it: the number box, tap to sync, and Sync's result.
    pub fn set_offset(&self, app: &AppHandle, offset_ms: i64) -> Result<i64, String> {
        let (key, mut view) = self
            .current
            .lock()
            .map(|c| c.clone())
            .map_err(|e| e.to_string())?;
        let key = key.ok_or("Nothing is playing.")?;
        view.offset_ms = offset_ms.clamp(-MAX_OFFSET_MS, MAX_OFFSET_MS);
        if let Some(cache) = &self.cache {
            cache
                .set_offset(&key, view.offset_ms)
                .map_err(|e| e.to_string())?;
        }
        let offset = view.offset_ms;
        self.publish(app, Some(key), view);
        Ok(offset)
    }

    /// Back to the lyrics file's own timing for now (not saved), so Sync can
    /// measure from it. Returns the timing it had.
    pub fn unshift(&self, app: &AppHandle) -> Option<(String, i64)> {
        let (key, mut view) = self.current.lock().ok()?.clone();
        let key = key?;
        let had = view.offset_ms;
        view.offset_ms = 0;
        self.publish(app, Some(key.clone()), view);
        Some((key, had))
    }

    /// Remembers which lyrics are on screen for the song `key`.
    fn showing(&self, key: &str, entry: &CacheEntry) {
        if let Ok(mut song) = self.song.lock()
            && let Some(song) = song.as_mut().filter(|s| s.key == key)
        {
            song.shown = match &entry.lyrics {
                CachedLyrics::Synced(text) => Some(text.clone()),
                _ => None,
            };
        }
    }

    /// "Try other lyrics": another synced version of this song from LRCLIB,
    /// the one whose length is closest to the song's, skipping versions
    /// already shown. It replaces the cached lyrics, with no timing shift.
    /// Returns a sentence for the user.
    pub async fn other_version(self: Arc<Self>, app: AppHandle) -> Result<String, String> {
        let client = self
            .client
            .clone()
            .ok_or("Lyrics lookups are unavailable.")?;
        let (key, title, artist, duration, tried, shown) = {
            let song = self.song.lock().map_err(|e| e.to_string())?;
            let s = song.as_ref().ok_or("Nothing is playing.")?;
            (
                s.key.clone(),
                s.title.clone(),
                s.artist.clone(),
                s.duration,
                s.tried.clone(),
                s.shown.clone(),
            )
        };
        let query = lyrics::clean_query(&title, &artist);
        let mut records = client
            .search(&query.title, &query.artist)
            .await
            .map_err(|_| "Couldn't reach LRCLIB. Check your internet connection.")?;
        if !query.artist.is_empty() {
            // The artist field may not name the singer: also by title alone.
            if let Ok(more) = client.search(&query.title, "").await {
                records.extend(more.into_iter().filter(|r| r.duration.is_some()));
            }
        }
        let pick = pick_version(&records, duration, &tried, shown.as_deref())
            .ok_or("No other synced version of these lyrics on LRCLIB.")?;
        let (id, text, length) = (
            pick.id,
            pick.synced_lyrics.clone().unwrap_or_default(),
            pick.duration,
        );
        let entry = CacheEntry {
            lyrics: CachedLyrics::Synced(text),
            offset_ms: 0,
            source: "lrclib".into(),
        };
        if let Some(cache) = &self.cache
            && let Err(e) = cache.put(&key, &entry)
        {
            tracing::warn!("lyrics cache write: {e}");
        }
        if let Ok(mut song) = self.song.lock()
            && let Some(s) = song.as_mut().filter(|s| s.key == key)
        {
            s.tried.push(id);
        }
        // Still the same song?
        if !self
            .current
            .lock()
            .is_ok_and(|c| c.0.as_deref() == Some(key.as_str()))
        {
            return Err("The song changed.".into());
        }
        self.showing(&key, &entry);
        self.publish(&app, Some(key), LyricsView::from_entry(&entry));
        tracing::info!("lyrics: switched {title} to LRCLIB version {id}");
        Ok(match length {
            Some(s) => format!(
                "Switched to another version ({}:{:02} long).",
                (s as u64) / 60,
                (s as u64) % 60
            ),
            None => "Switched to another version.".into(),
        })
    }
}

/// The best other synced version: not tried yet, not the text on screen,
/// closest in length to the song (versions with no length last).
fn pick_version<'a>(
    records: &'a [lyrics::lrclib::LrclibRecord],
    duration: Option<Duration>,
    tried: &[i64],
    shown: Option<&str>,
) -> Option<&'a lyrics::lrclib::LrclibRecord> {
    let song = duration.map(|d| d.as_secs_f64());
    records
        .iter()
        .filter(|r| !tried.contains(&r.id))
        .filter(|r| {
            r.synced_lyrics
                .as_deref()
                .is_some_and(|t| !t.trim().is_empty() && Some(t.trim()) != shown.map(str::trim))
        })
        .min_by(|a, b| {
            let off = |r: &lyrics::lrclib::LrclibRecord| match (r.duration, song) {
                (Some(d), Some(s)) => (d - s).abs(),
                (Some(_), None) => 0.0,
                (None, _) => f64::MAX,
            };
            off(a).total_cmp(&off(b))
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use lyrics::lrclib::LrclibRecord;

    fn record(id: i64, secs: Option<f64>, synced: Option<&str>) -> LrclibRecord {
        LrclibRecord {
            id,
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            album_name: None,
            duration: secs,
            instrumental: false,
            plain_lyrics: None,
            synced_lyrics: synced.map(String::from),
        }
    }

    #[test]
    fn other_lyrics_picks_the_closest_length_not_already_shown() {
        let song = Some(Duration::from_secs(206));
        let records = [
            record(1, Some(284.0), Some("[00:01.00]album")),
            record(2, Some(207.0), Some("[00:01.00]shown now")),
            record(3, Some(205.0), Some("[00:01.00]radio edit")),
            record(4, Some(206.0), None), // plain only
            record(5, None, Some("[00:01.00]no length")),
        ];
        let shown = Some("[00:01.00]shown now");
        assert_eq!(
            pick_version(&records, song, &[], shown).map(|r| r.id),
            Some(3)
        );
        // Pressed again: the next best.
        assert_eq!(
            pick_version(&records, song, &[3], shown).map(|r| r.id),
            Some(1)
        );
        assert_eq!(
            pick_version(&records, song, &[3, 1], shown).map(|r| r.id),
            Some(5)
        );
        assert_eq!(
            pick_version(&records, song, &[3, 1, 5], shown).map(|r| r.id),
            None
        );
    }
}
