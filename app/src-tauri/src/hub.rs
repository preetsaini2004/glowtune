//! The hub: one thread that receives every media event and fans it out.
//!
//! ```text
//!                      ┌─► Glow     (palette, fade, beat capture)
//!  MediaWatcher ─► hub ┼─► UI       (Tauri events "np-track" / "np-playback")
//!                      └─► Lyrics   (fetch on track change, Phase 4)
//! ```
//!
//! The hub also keeps the latest snapshot, so a window that opens later can
//! ask for the full state (`get_state`) instead of waiting for the next event.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use analysis::GlowPalette;
use media_source::{AlbumArt, MediaEvent, MediaWatcher, PlaybackStatus, Timeline, TrackInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::glow::Glow;
use crate::lyrics_service::LyricsService;

/// Players often send "no art" right before the real art (Spotify: ~5 ms).
const NO_ART_GRACE: Duration = Duration::from_millis(600);

/// Track info as the UI sees it.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackView {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<f64>,
    /// Friendly player name ("Spotify", "Brave").
    pub source: String,
    /// URL of the current album art (served by the `glowart` scheme), if any.
    pub art_url: Option<String>,
    /// Album palette as CSS hex colors, most important first.
    pub palette: Vec<String>,
}

/// Playback state as the UI sees it. `position_ms` was valid at the wall-clock
/// time `reported_at_ms`; the UI interpolates while playing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackView {
    pub status: &'static str,
    pub position_ms: f64,
    pub rate: f64,
    pub reported_at_ms: f64,
    /// None = the player doesn't support it (hide the button).
    pub shuffle: Option<bool>,
    /// "off" | "track" | "list"; None = unsupported.
    pub repeat: Option<&'static str>,
}

impl Default for PlaybackView {
    fn default() -> Self {
        Self {
            status: "stopped",
            position_ms: 0.0,
            rate: 1.0,
            reported_at_ms: wall_ms(Instant::now()),
            shuffle: None,
            repeat: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub track: Option<TrackView>,
    pub playback: PlaybackView,
}

/// Current album art bytes, served to the webviews.
/// Recent album art by key, so the current track and the "recently played"
/// list can both be served by `glowart:`.
#[derive(Default)]
pub struct ArtStore {
    items: VecDeque<(u64, Arc<[u8]>, String)>,
}

impl ArtStore {
    const KEEP: usize = 24;

    pub fn insert(&mut self, key: u64, bytes: Arc<[u8]>, mime: String) {
        self.items.retain(|(k, ..)| *k != key);
        self.items.push_front((key, bytes, mime));
        self.items.truncate(Self::KEEP);
    }

    pub fn get(&self, key: u64) -> Option<(Arc<[u8]>, String)> {
        self.items
            .iter()
            .find(|(k, ..)| *k == key)
            .map(|(_, b, m)| (b.clone(), m.clone()))
    }
}

/// One entry of the "recently played" list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub title: String,
    pub artist: String,
    pub source: String,
    pub art_url: Option<String>,
}

/// State shared with Tauri commands.
pub struct Shared {
    pub snapshot: Mutex<Snapshot>,
    pub art: Mutex<ArtStore>,
    /// Recently played, newest first (this session).
    pub history: Mutex<VecDeque<HistoryItem>>,
    pub media: MediaWatcher,
    /// `None` if the glow couldn't start (no GPU, unsupported OS).
    pub overlay: Option<Arc<overlay::Overlay>>,
    /// Glow colors pinned by the theme (static/gradient); `None` = album colors.
    pub fixed_palette: Mutex<Option<Vec<[u8; 3]>>>,
    /// Latest album palette, to restore when switching back to an adaptive theme.
    pub album_palette: Mutex<Vec<[u8; 3]>>,
    /// The user's own colors (Settings → "Use artwork colors" off): beat everything else.
    pub user_palette: Mutex<Option<Vec<[u8; 3]>>>,
}

impl Shared {
    /// Shows the right glow colors: the theme's if pinned, else the album's.
    pub fn refresh_glow_palette(&self) {
        let Some(overlay) = &self.overlay else { return };
        let fixed = self
            .user_palette
            .lock()
            .ok()
            .and_then(|u| u.clone())
            .or_else(|| self.fixed_palette.lock().ok().and_then(|f| f.clone()));
        let colors = fixed.unwrap_or_else(|| {
            self.album_palette
                .lock()
                .map(|p| p.clone())
                .unwrap_or_default()
        });
        if !colors.is_empty() {
            overlay.set_palette(&colors);
        }
    }
}

pub fn start(
    app: AppHandle,
    glow_config: overlay::OverlayConfig,
    fixed_palette: Option<Vec<[u8; 3]>>,
    lyrics: Arc<LyricsService>,
) -> Result<Arc<Shared>, String> {
    let (watcher, events) = MediaWatcher::start().map_err(|e| format!("media watcher: {e}"))?;
    let glow = match Glow::start(glow_config) {
        Ok(g) => Some(g),
        Err(e) => {
            tracing::error!("edge glow disabled: {e}");
            None
        }
    };
    let shared = Arc::new(Shared {
        snapshot: Mutex::new(Snapshot::default()),
        art: Mutex::new(ArtStore::default()),
        history: Mutex::new(VecDeque::new()),
        media: watcher,
        overlay: glow.as_ref().map(Glow::overlay),
        fixed_palette: Mutex::new(fixed_palette),
        album_palette: Mutex::new(GlowPalette::fallback().colors),
        user_palette: Mutex::new(None),
    });
    shared.refresh_glow_palette();
    let hub_shared = shared.clone();
    std::thread::Builder::new()
        .name("glowtune-hub".into())
        .spawn(move || {
            Hub {
                app,
                shared: hub_shared,
                glow,
                lyrics,
                no_art_since: None,
            }
            .run(events)
        })
        .map_err(|e| e.to_string())?;
    Ok(shared)
}

struct Hub {
    app: AppHandle,
    shared: Arc<Shared>,
    glow: Option<Glow>,
    lyrics: Arc<LyricsService>,
    no_art_since: Option<Instant>,
}

impl Hub {
    fn run(mut self, events: Receiver<MediaEvent>) {
        loop {
            // Block until the next event; only wake early for a pending "no art" fallback.
            let event = match self.no_art_since {
                Some(t) => match events.recv_timeout(NO_ART_GRACE.saturating_sub(t.elapsed())) {
                    Ok(e) => Some(e),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match events.recv() {
                    Ok(e) => Some(e),
                    Err(_) => return,
                },
            };
            match event {
                Some(e) => self.handle(e),
                None => {
                    self.no_art_since = None;
                    self.set_art(None, GlowPalette::fallback());
                }
            }
        }
    }

    fn handle(&mut self, event: MediaEvent) {
        match event {
            MediaEvent::SessionChanged(None) => {
                self.update_track(|t| *t = None);
                self.lyrics.clear(&self.app);
            }
            MediaEvent::SessionChanged(Some(_)) => {}
            MediaEvent::TrackChanged(info) => {
                tracing::info!("now playing: {} — {}", info.title, info.artist);
                self.lyrics.on_track(
                    &self.app,
                    &info.title,
                    &info.artist,
                    &info.album,
                    info.duration,
                );
                self.update_track(|t| {
                    let old = t.take().unwrap_or_default();
                    *t = Some(track_view(&info, old));
                });
            }
            MediaEvent::ArtChanged(Some(AlbumArt::Bytes { data, mime })) => {
                self.no_art_since = None;
                let key = hash(&data);
                let palette = self
                    .glow
                    .as_mut()
                    .and_then(|g| g.palette_for(key, &data))
                    .or_else(|| analysis::extract_palette(&data).ok())
                    .unwrap_or_else(GlowPalette::fallback);
                if let Ok(mut art) = self.shared.art.lock() {
                    art.insert(key, data, mime);
                }
                self.set_art(Some(key), palette);
            }
            // Linux gives art URLs; fetching them comes with the Linux overlay.
            MediaEvent::ArtChanged(Some(AlbumArt::Url(_))) | MediaEvent::ArtChanged(None) => {
                self.no_art_since.get_or_insert_with(Instant::now);
            }
            MediaEvent::PlaybackChanged(status) => {
                let playing = status == PlaybackStatus::Playing;
                #[cfg(windows)]
                crate::thumbbar::set_playing(&self.app, playing);
                if let Some(gate) = self.app.try_state::<crate::lyrics_gate::LyricsGate>() {
                    gate.playing(playing);
                }
                if let Some(gate) = self.app.try_state::<crate::stage::StageGate>() {
                    gate.playing(playing);
                }
                if let Some(g) = &self.glow {
                    g.on_playing(playing);
                }
                self.update_playback(|p| p.status = status_name(status));
            }
            MediaEvent::ModesChanged(m) => {
                self.update_playback(|p| {
                    p.shuffle = m.shuffle;
                    p.repeat = m.repeat.map(|r| match r {
                        media_source::RepeatMode::Off => "off",
                        media_source::RepeatMode::Track => "track",
                        media_source::RepeatMode::List => "list",
                    });
                });
            }
            MediaEvent::TimelineChanged(tl) => {
                self.update_track(|t| {
                    if let Some(t) = t
                        && let Some(d) = tl.duration
                    {
                        t.duration_ms = Some(d.as_secs_f64() * 1000.0);
                    }
                });
                self.update_playback(|p| apply_timeline(p, &tl));
            }
        }
    }

    fn set_art(&mut self, key: Option<u64>, palette: GlowPalette) {
        if let Ok(mut p) = self.shared.album_palette.lock() {
            p.clone_from(&palette.colors);
        }
        self.shared.refresh_glow_palette();
        self.update_track(|t| {
            if let Some(t) = t {
                t.art_url = key.map(|k| format!("{}/{k:x}", crate::ART_ORIGIN));
                t.palette = palette
                    .colors
                    .iter()
                    .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
                    .collect();
            }
        });
    }

    fn update_track(&self, f: impl FnOnce(&mut Option<TrackView>)) {
        let track = {
            let Ok(mut snap) = self.shared.snapshot.lock() else {
                return;
            };
            f(&mut snap.track);
            snap.track.clone()
        };
        if let Some(t) = &track {
            self.remember(t);
        }
        let _ = self.app.emit("np-track", track);
    }

    /// Keeps the "recently played" list: newest first, one entry per song.
    fn remember(&self, t: &TrackView) {
        if t.title.is_empty() {
            return;
        }
        let item = HistoryItem {
            title: t.title.clone(),
            artist: t.artist.clone(),
            source: t.source.clone(),
            art_url: t.art_url.clone(),
        };
        let list = {
            let Ok(mut h) = self.shared.history.lock() else {
                return;
            };
            if h.front() == Some(&item) {
                return;
            }
            h.retain(|i| !(i.title == item.title && i.artist == item.artist));
            h.push_front(item);
            h.truncate(12);
            h.iter().cloned().collect::<Vec<_>>()
        };
        let _ = self.app.emit("np-history", list);
    }

    fn update_playback(&self, f: impl FnOnce(&mut PlaybackView)) {
        let playback = {
            let Ok(mut snap) = self.shared.snapshot.lock() else {
                return;
            };
            f(&mut snap.playback);
            snap.playback.clone()
        };
        let _ = self.app.emit("np-playback", playback);
    }
}

fn track_view(info: &TrackInfo, old: TrackView) -> TrackView {
    TrackView {
        title: info.title.clone(),
        artist: info.artist.clone(),
        album: info.album.clone(),
        duration_ms: info
            .duration
            .map(|d| d.as_secs_f64() * 1000.0)
            .or(old.duration_ms),
        source: friendly_source(&info.source_app),
        // Art and palette arrive separately; keep the old ones until then.
        art_url: old.art_url,
        palette: old.palette,
    }
}

fn apply_timeline(p: &mut PlaybackView, tl: &Timeline) {
    p.position_ms = tl.position.as_secs_f64() * 1000.0;
    p.rate = tl.rate;
    p.reported_at_ms = wall_ms(tl.reported_at);
}

fn status_name(s: PlaybackStatus) -> &'static str {
    match s {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        PlaybackStatus::Stopped => "stopped",
    }
}

/// Converts a monotonic `Instant` to wall-clock milliseconds (what JS uses).
fn wall_ms(at: Instant) -> f64 {
    let now_wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let age = Instant::now().saturating_duration_since(at);
    now_wall.saturating_sub(age).as_secs_f64() * 1000.0
}

/// "Spotify.exe" → "Spotify", "308046B0AF4A39CB" (Firefox) → "Firefox", etc.
pub(crate) fn friendly_source(app_id: &str) -> String {
    let id = app_id.rsplit(['\\', '/']).next().unwrap_or(app_id);
    let id = id.strip_suffix(".exe").unwrap_or(id);
    let known = [
        // Glowtune's own service windows (Spotify / YouTube Music web players).
        ("msedgewebview2", "Web player"),
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("brave", "Brave"),
        ("firefox", "Firefox"),
        ("308046b0af4a39cb", "Firefox"),
        ("applemusic", "Apple Music"),
        ("zunemusic", "Media Player"),
        ("opera", "Opera"),
        ("vivaldi", "Vivaldi"),
        ("org.mpris.mediaplayer2.", ""),
    ];
    let lower = id.to_lowercase();
    for (needle, name) in known {
        if lower.contains(needle) {
            if name.is_empty() {
                // MPRIS bus name: use the player part, capitalized.
                let rest = &id[needle.len()..];
                let rest = rest.split('.').next().unwrap_or(rest);
                let mut c = rest.chars();
                return c
                    .next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default();
            }
            return name.to_string();
        }
    }
    id.to_string()
}

fn hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names() {
        assert_eq!(friendly_source("Spotify.exe"), "Spotify");
        assert_eq!(friendly_source("Brave"), "Brave");
        assert_eq!(friendly_source("308046B0AF4A39CB"), "Firefox");
        assert_eq!(friendly_source("MSEdge"), "Edge");
        assert_eq!(friendly_source("org.mpris.MediaPlayer2.spotify"), "Spotify");
        assert_eq!(friendly_source("Something"), "Something");
    }
}
