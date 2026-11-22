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
//!
//! With Sound effects blending songs, what you hear runs a few seconds behind
//! what the service plays (see `sound`). The hub hides that:
//!
//! ```text
//!  position sent to the UI = service position − delay      (lyrics match the sound)
//!  song change (automatic) = held back until the delayed sound reaches it
//!  song change (skip)      = shown at once, and the sound buffer is dropped
//! ```

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
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
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
    /// The playback state last sent to the windows (see `send_playback`).
    pub sent_playback: Mutex<Option<PlaybackView>>,
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
    // Kicks go to the desktop scene's windows only (its wallpapers pulse).
    let kick_app = app.clone();
    let on_kick: crate::glow::OnKick = Box::new(move |strength| {
        let _ = kick_app.emit_filter("kick", strength, |t| match t {
            tauri::EventTarget::Window { label }
            | tauri::EventTarget::Webview { label }
            | tauri::EventTarget::WebviewWindow { label }
            | tauri::EventTarget::AnyLabel { label } => label.starts_with("stage"),
            _ => false,
        });
    });
    let glow = match Glow::start(glow_config, on_kick) {
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
        sent_playback: Mutex::new(None),
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
                held: VecDeque::new(),
                release_at: None,
                paused_at: None,
                automix_hold: false,
                ended_at: None,
                web_song: None,
                resync_until: None,
                lyrics_need_length: false,
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
    /// Events of a song that isn't audible yet (the sound is delayed).
    held: VecDeque<MediaEvent>,
    /// When the delayed sound reaches the held song.
    release_at: Option<Instant>,
    /// Paused while holding: the delayed sound stands still too.
    paused_at: Option<Instant>,
    /// The held change was started by Automix pressing Next.
    automix_hold: bool,
    /// When a song was last seen running out (its position wrapped to the start).
    ended_at: Option<Instant>,
    /// The last song seen from a service window (title, artist).
    web_song: Option<(String, String)>,
    /// Until then, position reports follow an app switch, not a seek.
    resync_until: Option<Instant>,
    /// The lyrics were looked up before the song's length was known (web
    /// players tell it a moment later): look again once it is.
    lyrics_need_length: bool,
}

/// How long position reports may lag behind an app switch.
const SESSION_RESYNC: Duration = Duration::from_secs(3);

impl Hub {
    fn run(mut self, events: Receiver<MediaEvent>) {
        loop {
            // Block until the next event; only wake early for a pending
            // "no art" fallback or to release a held song change.
            let mut wait = self
                .no_art_since
                .map(|t| NO_ART_GRACE.saturating_sub(t.elapsed()));
            if let Some(at) = self.release_at
                && self.paused_at.is_none()
            {
                let left = at.saturating_duration_since(Instant::now());
                wait = Some(wait.map_or(left, |w| w.min(left)));
            }
            let event = match wait {
                Some(w) => match events.recv_timeout(w) {
                    Ok(e) => Some(e),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match events.recv() {
                    Ok(e) => Some(e),
                    Err(_) => return,
                },
            };
            if let Some(e) = event {
                self.incoming(e);
            }
            if self.paused_at.is_none() && self.release_at.is_some_and(|at| at <= Instant::now()) {
                self.release_held();
            }
            if self
                .no_art_since
                .is_some_and(|t| t.elapsed() >= NO_ART_GRACE)
            {
                self.no_art_since = None;
                self.set_art(None, GlowPalette::fallback());
            }
        }
    }

    /// Sorts an event: tell the sound engine what happened, then show it now
    /// or hold it until the delayed sound gets there.
    fn incoming(&mut self, event: MediaEvent) {
        let sound = self.app.try_state::<crate::sound::Sound>();
        match &event {
            // Pause and play act at once, on the sound and on the screen.
            // Except between two songs: players often drop their media session
            // for a moment there ("stopped", then "playing" again). That is
            // part of the song change and waits with it.
            MediaEvent::PlaybackChanged(status) => {
                let playing = *status == PlaybackStatus::Playing;
                let holding = self.release_at.is_some();
                // Spotify can report "paused" for a moment as Automix's Next
                // switches songs. Shown at once, it froze the scene (lyrics,
                // video, dock) while the old song was still heard.
                // Seen live: "stopped" ~90 ms after the press, before the song
                // change even arrived, then "paused" 9 s later that lasted
                // until the new song was heard. Both froze the scene and the
                // sound. From the press until the change is released, they're
                // part of the switch (Glowtune's own pause button still acts).
                let switching = sound.as_ref().is_some_and(|s| {
                    !s.commanded_within(Duration::from_secs(2))
                        && (s.automix_pressed_within(Duration::from_secs(4))
                            || (holding && self.automix_hold))
                });
                if switching && *status != PlaybackStatus::Playing {
                    tracing::info!("automix: {status:?} while switching songs, not shown");
                    if holding {
                        self.held.push_back(event);
                    }
                    return;
                }
                let real = match status {
                    PlaybackStatus::Paused => true,
                    PlaybackStatus::Playing => !holding || self.paused_at.is_some(),
                    PlaybackStatus::Stopped => !holding,
                };
                if real {
                    if let Some(sound) = &sound
                        && self.web_player()
                    {
                        sound.paused(!playing);
                    }
                    if playing {
                        if let (Some(paused), Some(at)) =
                            (self.paused_at.take(), &mut self.release_at)
                        {
                            *at += paused.elapsed();
                        }
                    } else if holding && self.paused_at.is_none() {
                        self.paused_at = Some(Instant::now());
                    }
                    return self.handle(event);
                }
            }
            // "Nothing is playing": with delayed sound, the old song is still
            // audible for a while, so this waits like a song change does. (It
            // is usually the gap between two songs, see above.)
            MediaEvent::SessionChanged(None) => {
                if let Some(timing) = sound.as_ref().and_then(|s| s.timing())
                    && self.web_player()
                {
                    self.release_at.get_or_insert(
                        Instant::now() + Duration::from_secs_f64(timing.delay_ms / 1000.0),
                    );
                }
            }
            // Windows switched "now playing" to another app (or back). The
            // positions that follow belong to a different player for a moment.
            MediaEvent::SessionChanged(Some(_)) => {
                self.resync_until = Some(Instant::now() + SESSION_RESYNC);
            }
            MediaEvent::TrackChanged(info) if self.is_new_song(info) => {
                let web = friendly_source(&info.source_app) == "Web player";
                let was_web = self.web_player();
                let song = (info.title.clone(), info.artist.clone());
                if web != was_web {
                    self.resync_until = Some(Instant::now() + SESSION_RESYNC);
                }
                // Back to the service window's song after another app (a video
                // in a browser, say) was "now playing" for a while: the service
                // never changed songs, so the blend's delay must stay.
                let returning = web && !was_web && self.web_song.as_ref() == Some(&song);
                if web {
                    self.web_song = Some(song);
                }
                let hold = match &sound {
                    Some(_) if returning => {
                        tracing::info!(
                            "back to {} from another app: not a song change",
                            info.title
                        );
                        None
                    }
                    Some(sound) if web => {
                        let automatic = self.changed_by_itself(sound);
                        self.automix_hold = sound.automix_pressed_within(Duration::from_secs(4));
                        tracing::info!("song change, automatic = {automatic}");
                        sound.track_changed(automatic)
                    }
                    _ => None,
                };
                match hold {
                    // A second change while one is waiting keeps the first one's time.
                    Some(_)
                        if self
                            .held
                            .iter()
                            .any(|e| matches!(e, MediaEvent::TrackChanged(_))) => {}
                    Some(hold) => self.release_at = Some(Instant::now() + hold),
                    None => self.release_held(),
                }
            }
            // A position report that wasn't predicted is a seek (or a repeat).
            MediaEvent::TimelineChanged(tl) if self.release_at.is_none() => {
                // Only the service window's own positions can be seeks, and
                // not right after an app switch (the last known position then
                // is the other app's).
                // Nor during / just after a fast fill (the service's own clock
                // ran ahead then; its next report re-anchors everything).
                let resyncing = self.resync_until.is_some_and(|t| Instant::now() < t)
                    || sound
                        .as_ref()
                        .is_some_and(|s| s.fast_within(Duration::from_secs(3)));
                if let Some(sound) = &sound
                    && self.web_player()
                    && !resyncing
                    && sound.timing().is_some()
                    && let Some((expected, duration)) = self.service_position()
                {
                    let now = tl.position.as_secs_f64() * 1000.0;
                    // (Automix ends songs a few seconds early by pressing Next
                    // itself: the same thing as running out.)
                    let ran_out = now < 3000.0
                        && (duration.is_some_and(|d| expected >= d - 5000.0)
                            || sound.automix_pressed_within(Duration::from_secs(4)));
                    if ran_out {
                        // The song ended and the position started over. Either
                        // the next song's name follows in a moment, or the same
                        // song repeats: blend it like a change in both cases.
                        self.ended_at = Some(Instant::now());
                        if let Some(hold) = sound.track_changed(true) {
                            self.release_at = Some(Instant::now() + hold);
                        }
                    } else if now > expected + 1500.0
                        && sound.automix_pressed_within(Duration::from_secs(4))
                    {
                        // Automix pressed Next and Spotify first reports the
                        // old song at its end. Showing that made the lyrics
                        // jump ~6 s ahead while the old song was still heard.
                        tracing::info!(
                            "automix: the old song jumped to its end ({:.1} s), not shown",
                            now / 1000.0
                        );
                        return;
                    } else if (now - expected).abs() > 1500.0
                        // Right after Automix pressed Next, Spotify may first
                        // report the old song jumping to its end: that is the
                        // song change itself, not a seek. (Flushing there threw
                        // away a full 34 s delay: a hard cut instead of a blend.)
                        && !sound.automix_pressed_within(Duration::from_secs(4))
                    {
                        tracing::info!(
                            "seek: the service is at {:.1} s, {:.1} s from where it was expected, dropping the delay",
                            now / 1000.0,
                            (now - expected) / 1000.0
                        );
                        sound.flush();
                    }
                } else if let Some(sound) = &sound
                    && self.web_player()
                    && resyncing
                    && sound.timing().is_some()
                    && let Some((expected, _)) = self.service_position()
                {
                    // During a fast fill the expected position can be off by a
                    // little (the fill's speed steps), never by seconds. A big
                    // jump is a real seek, or the page reloading its stream and
                    // resuming elsewhere (seen live: −29 s): the delay then holds
                    // sound that no longer matches the position, so drop it.
                    // Right after an app switch, though, the last position was
                    // the other app's: only log.
                    let now = tl.position.as_secs_f64() * 1000.0;
                    let off = now - expected;
                    let app_switch = self.resync_until.is_some_and(|t| Instant::now() < t);
                    if off.abs() > 5000.0
                        && !app_switch
                        && now >= 3000.0
                        && !sound.automix_pressed_within(Duration::from_secs(4))
                    {
                        tracing::info!(
                            "seek during a fast fill: the service is at {:.1} s, {:.1} s from where it was expected, dropping the delay",
                            now / 1000.0,
                            off / 1000.0
                        );
                        sound.flush();
                    } else if off.abs() > 1500.0 {
                        tracing::warn!(
                            "seek during a fast fill (not acted on): the service is at {:.1} s, {:.1} s from where it was expected",
                            now / 1000.0,
                            off / 1000.0
                        );
                    }
                }
            }
            _ => {}
        }
        if self.release_at.is_some() {
            self.held.push_back(event);
        } else {
            self.handle(event);
        }
    }

    fn release_held(&mut self) {
        self.release_at = None;
        self.paused_at = None;
        self.automix_hold = false;
        let events: Vec<_> = self.held.drain(..).collect();
        for event in settle(events) {
            self.handle(event);
        }
    }

    /// False for the same song's metadata arriving again (e.g. its duration).
    fn is_new_song(&self, info: &TrackInfo) -> bool {
        let latest = self.held.iter().rev().find_map(|e| match e {
            MediaEvent::TrackChanged(i) => Some((i.title.clone(), i.artist.clone())),
            _ => None,
        });
        let current = latest.or_else(|| {
            let snap = self.shared.snapshot.lock().ok()?;
            let t = snap.track.as_ref()?;
            Some((t.title.clone(), t.artist.clone()))
        });
        current.is_none_or(|(title, artist)| title != info.title || artist != info.artist)
    }

    /// Where the service is in the current song right now (ms), and the
    /// song's length, as last reported. `None` without a position report.
    fn service_position(&self) -> Option<(f64, Option<f64>)> {
        let (p, duration) = {
            let snap = self.shared.snapshot.lock().ok()?;
            let duration = snap.track.as_ref()?.duration_ms.filter(|d| *d > 0.0);
            (snap.playback.clone(), duration)
        };
        if p.position_ms <= 0.0 && duration.is_none() {
            return None;
        }
        let mut position = p.position_ms;
        if p.status == "playing" {
            position += (wall_ms(Instant::now()) - p.reported_at_ms).max(0.0) * p.rate;
        }
        // Fast fill: the service ran ahead of its own clock for a while.
        // (Asked with the snapshot unlocked: the fill lock is taken last.)
        if let Some(sound) = self.app.try_state::<crate::sound::Sound>() {
            position += sound.extra_ms_since(p.reported_at_ms);
        }
        Some((position, duration))
    }

    /// True while the music comes from one of Glowtune's service windows.
    fn web_player(&self) -> bool {
        self.shared
            .snapshot
            .lock()
            .ok()
            .and_then(|s| s.track.as_ref().map(|t| t.source == "Web player"))
            .unwrap_or(false)
    }

    /// Did the song end on its own (blend it) or did the user skip (cut)?
    fn changed_by_itself(&self, sound: &crate::sound::Sound) -> bool {
        // Automix pressed Next itself, to blend out of the song early.
        if sound.automix_pressed_within(Duration::from_secs(4)) {
            return true;
        }
        // A button in Glowtune was just pressed.
        if sound.commanded_within(Duration::from_secs(2)) {
            return false;
        }
        // The position report of the new song came first and showed the old
        // one running out.
        if self
            .ended_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        {
            return true;
        }
        // The service said where it was: near the end means it ran out.
        if let Some((position, Some(duration))) = self.service_position() {
            return position >= duration - 5000.0;
        }
        // No position (some players don't report one): a click or key press
        // in the service page just before the change means a skip.
        crate::services::since_page_input(&self.app).is_none_or(|ms| ms > 2500.0)
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
                self.lyrics_need_length = info.duration.is_none();
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
                crate::stats::playing(&self.app, playing);
                self.update_playback(|p| apply_playback_status(p, status, wall_ms(Instant::now())));
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
                // Web players tell the song's length only now, after the song
                // change: the lyrics were looked up without it, and could be
                // for another version (live: a 4:44 album lyrics file on a
                // 3:26 music video, every line early). Look up again with it.
                let first_length = (self.lyrics_need_length && tl.duration.is_some())
                    .then(|| {
                        let s = self.shared.snapshot.lock().ok()?;
                        let t = s.track.as_ref()?;
                        Some((t.title.clone(), t.artist.clone(), t.album.clone()))
                    })
                    .flatten();
                if let Some((title, artist, album)) = first_length {
                    self.lyrics_need_length = false;
                    self.lyrics
                        .on_track(&self.app, &title, &artist, &album, tl.duration);
                }
                self.update_track(|t| {
                    if let Some(t) = t
                        && let Some(d) = tl.duration
                    {
                        t.duration_ms = Some(d.as_secs_f64() * 1000.0);
                    }
                });
                let filling = self
                    .app
                    .try_state::<crate::sound::Sound>()
                    .is_some_and(|s| s.fast_within(Duration::from_secs(3)));
                self.update_playback(|p| apply_timeline(p, &tl, filling));
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
            let before = snap.track.clone();
            f(&mut snap.track);
            // Unchanged (YouTube Music re-reports its timeline, and with it the
            // length, every 200 ms during a fast fill): nothing to redraw.
            if snap.track == before {
                return;
            }
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
        let (list, new_song) = {
            let Ok(mut h) = self.shared.history.lock() else {
                return;
            };
            if h.front() == Some(&item) {
                return;
            }
            // Not just the same song's cover arriving late.
            let new_song = h
                .front()
                .is_none_or(|f| !(f.title == item.title && f.artist == item.artist));
            h.retain(|i| !(i.title == item.title && i.artist == item.artist));
            h.push_front(item);
            h.truncate(12);
            (h.iter().cloned().collect::<Vec<_>>(), new_song)
        };
        if new_song {
            // "Sleep after this song" fires now; the song before is counted.
            crate::sleep::song_changed(&self.app);
            let playing = self
                .shared
                .snapshot
                .lock()
                .is_ok_and(|s| s.playback.status == "playing");
            crate::stats::song_started(&self.app, &t.title, &t.artist, &t.source, playing);
        }
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
        send_playback(&self.app, playback);
    }
}

/// Sends the playback state as heard, unless it says nothing new: same
/// status, rate and modes, and the position where the last one sent
/// predicted it. (A web player reports every 200 ms during a fast fill;
/// each report re-planned the lyrics, restarting every word's animation and
/// stacking two copies of a karaoke line.)
fn send_playback(app: &AppHandle, playback: PlaybackView) {
    let heard = heard_playback(app, playback);
    if let Some(shared) = app.try_state::<Arc<Shared>>()
        && let Ok(mut last) = shared.sent_playback.lock()
    {
        if last.as_ref().is_some_and(|l| same_course(l, &heard)) {
            return;
        }
        *last = Some(heard.clone());
    }
    let _ = app.emit("np-playback", heard);
}

/// What a held song change still has to show once it's heard. The switch's
/// in-between steps (no session, the old song's last positions) are dropped
/// when a new song follows them, and of the play / pause / stop reports only
/// the last counts. Replayed one by one, they blanked the scene for a moment:
/// no song, stopped, the old position, then the new song.
///
/// Spotify can report the new song's start (position ~0) a moment *before*
/// its name. That report is the new song's, not the old one's: it moves to
/// just after the song change. Dropped, the new song kept the old song's end
/// position, so the bar sat full and the lyrics froze until the next seek.
fn settle(events: Vec<MediaEvent>) -> Vec<MediaEvent> {
    let is_start = |e: &MediaEvent| matches!(e, MediaEvent::TimelineChanged(tl) if tl.position < Duration::from_secs(5));
    let last_track = events
        .iter()
        .rposition(|e| matches!(e, MediaEvent::TrackChanged(_)));
    let last_status = events
        .iter()
        .rposition(|e| matches!(e, MediaEvent::PlaybackChanged(_)));
    let early_start = last_track.and_then(|t| {
        let later = events[t + 1..]
            .iter()
            .any(|e| matches!(e, MediaEvent::TimelineChanged(_)));
        (!later)
            .then(|| events[..t].iter().rposition(is_start))
            .flatten()
    });
    let mut start = None;
    let mut kept = Vec::with_capacity(events.len());
    for (i, e) in events.into_iter().enumerate() {
        if Some(i) == early_start {
            start = Some(e);
            continue;
        }
        let keep = match &e {
            MediaEvent::SessionChanged(None) | MediaEvent::TimelineChanged(_) => {
                last_track.is_none_or(|t| i > t)
            }
            MediaEvent::PlaybackChanged(_) => Some(i) == last_status,
            _ => true,
        };
        if keep {
            let track = Some(i) == last_track;
            kept.push(e);
            if track && let Some(s) = start.take() {
                kept.push(s);
            }
        }
    }
    kept
}

/// Most a resent position may differ from where the last one predicted it.
const SAME_COURSE_MS: f64 = 120.0;

/// True when `b` is just `a` carried forward in time.
fn same_course(a: &PlaybackView, b: &PlaybackView) -> bool {
    if a.status != b.status
        || (a.rate - b.rate).abs() > 0.002
        || a.shuffle != b.shuffle
        || a.repeat != b.repeat
    {
        return false;
    }
    let predicted = if a.status == "playing" {
        a.position_ms + (b.reported_at_ms - a.reported_at_ms).max(0.0) * a.rate
    } else {
        a.position_ms
    };
    (b.position_ms - predicted).abs() < SAME_COURSE_MS
}

/// The playback state as it is *heard*: with Sound effects delaying the
/// sound, the position is that much earlier (and moves a touch slower while
/// the delay refills).
/// Where in the song shown now the listener is (ms, the lyrics' own clock),
/// while a service window's song plays. `None` otherwise.
pub(crate) fn heard_position_ms(app: &AppHandle) -> Option<f64> {
    let playback = {
        let shared = app.try_state::<Arc<Shared>>()?;
        let snapshot = shared.snapshot.lock().ok()?;
        let track = snapshot.track.as_ref()?;
        if track.source != "Web player" || snapshot.playback.status != "playing" {
            return None;
        }
        snapshot.playback.clone()
    };
    let p = heard_playback(app, playback);
    let now = wall_ms(Instant::now());
    Some(p.position_ms + (now - p.reported_at_ms).max(0.0) * p.rate)
}

pub(crate) fn heard_playback(app: &AppHandle, mut p: PlaybackView) -> PlaybackView {
    let web = app.try_state::<Arc<Shared>>().is_some_and(|shared| {
        shared
            .snapshot
            .lock()
            .ok()
            .and_then(|s| s.track.as_ref().map(|t| t.source == "Web player"))
            .unwrap_or(false)
    });
    let Some(sound) = app.try_state::<crate::sound::Sound>().filter(|_| web) else {
        return p;
    };
    let Some(timing) = sound.timing() else {
        return p;
    };
    let now = wall_ms(Instant::now());
    if p.status == "playing" {
        p.position_ms += (now - p.reported_at_ms).max(0.0) * p.rate;
    }
    // Fast fill: the service ran ahead of its own clock for a while.
    p.position_ms += sound.extra_ms_since(p.reported_at_ms);
    p.position_ms = (p.position_ms - timing.delay_ms).max(0.0);
    p.reported_at_ms = now;
    p.rate *= timing.speed;
    p
}

/// Sends the playback state again: the sound's delay or speed just changed.
pub(crate) fn reemit_playback(app: &AppHandle) {
    let Some(shared) = app.try_state::<Arc<Shared>>() else {
        return;
    };
    let Ok(playback) = shared.snapshot.lock().map(|s| s.playback.clone()) else {
        return;
    };
    send_playback(app, playback);
}

fn track_view(info: &TrackInfo, old: TrackView) -> TrackView {
    // A web player tells the new song's length only with its first position
    // report. Until then keep the old length only for the same song: another
    // song's (or a 30 s ad's) length made the bar fill early and stay full.
    let same_song = old.title == info.title && old.artist == info.artist;
    TrackView {
        title: info.title.clone(),
        artist: info.artist.clone(),
        album: info.album.clone(),
        duration_ms: info
            .duration
            .map(|d| d.as_secs_f64() * 1000.0)
            .or(old.duration_ms.filter(|_| same_song)),
        source: friendly_source(&info.source_app),
        // Art and palette arrive separately; keep the old ones until then.
        art_url: old.art_url,
        palette: old.palette,
    }
}

fn apply_timeline(p: &mut PlaybackView, tl: &Timeline, filling: bool) {
    p.position_ms = tl.position.as_secs_f64() * 1000.0;
    p.rate = service_rate(tl.rate, filling);
    p.reported_at_ms = wall_ms(tl.reported_at);
}

/// The speed the service's song itself plays at, from the rate it reports.
///
/// During a fast fill, some services report the page's real speed (YouTube
/// Music: 1.02 … 1.4) and some the speed the page asked for (Spotify: 1.0).
/// The fill's own extra progress is counted separately (`extra_ms_since`),
/// so the fill must be taken out here, or it is counted twice and the lyrics
/// run ahead of the sound. A fill only runs while the page wants 1×, so any
/// faster rate reported then is the fill's.
fn service_rate(reported: f64, filling: bool) -> f64 {
    if filling && reported > 1.0 {
        1.0
    } else {
        reported
    }
}

fn status_name(s: PlaybackStatus) -> &'static str {
    match s {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        PlaybackStatus::Stopped => "stopped",
    }
}

/// Anchor the running clock before freezing it, then start a new anchor on resume.
fn apply_playback_status(p: &mut PlaybackView, status: PlaybackStatus, now_ms: f64) {
    if p.status == "playing" {
        p.position_ms += (now_ms - p.reported_at_ms).max(0.0) * p.rate;
    }
    p.reported_at_ms = now_ms;
    p.status = status_name(status);
}

/// Converts a monotonic `Instant` to wall-clock milliseconds (what JS uses).
pub(crate) fn wall_ms(at: Instant) -> f64 {
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
    fn a_new_song_does_not_keep_the_old_length() {
        let info = |title: &str| TrackInfo {
            title: title.into(),
            artist: "A".into(),
            album: String::new(),
            album_artist: String::new(),
            duration: None,
            source_app: "Glowtune".into(),
        };
        let ad = TrackView {
            title: "Advertisement".into(),
            artist: "A".into(),
            duration_ms: Some(30_000.0),
            ..Default::default()
        };
        assert_eq!(track_view(&info("Destiny"), ad.clone()).duration_ms, None);
        // The same song again (its art arriving, say) keeps it.
        assert_eq!(
            track_view(&info("Advertisement"), ad).duration_ms,
            Some(30_000.0)
        );
    }

    fn playing_at(position_ms: f64, at_ms: f64) -> PlaybackView {
        PlaybackView {
            status: "playing",
            position_ms,
            rate: 1.0,
            reported_at_ms: at_ms,
            shuffle: None,
            repeat: None,
        }
    }

    #[test]
    fn a_report_that_only_carries_the_song_forward_is_not_sent_again() {
        let a = playing_at(10_000.0, 1_000.0);
        // 200 ms later, 200 ms further: the same course.
        assert!(same_course(&a, &playing_at(10_200.0, 1_200.0)));
        assert!(same_course(&a, &playing_at(10_280.0, 1_200.0)));
        // A real jump, a pause or a new speed is news.
        assert!(!same_course(&a, &playing_at(16_000.0, 1_200.0)));
        assert!(!same_course(
            &a,
            &PlaybackView {
                status: "paused",
                ..playing_at(10_200.0, 1_200.0)
            }
        ));
        assert!(!same_course(
            &a,
            &PlaybackView {
                rate: 0.96,
                ..playing_at(10_200.0, 1_200.0)
            }
        ));
    }

    #[test]
    fn a_released_song_change_skips_the_switch_in_between() {
        let song = |title: &str| {
            MediaEvent::TrackChanged(TrackInfo {
                title: title.into(),
                artist: "A".into(),
                album: String::new(),
                album_artist: String::new(),
                duration: None,
                source_app: "Glowtune".into(),
            })
        };
        let at = |s: u64| {
            MediaEvent::TimelineChanged(media_source::Timeline {
                position: Duration::from_secs(s),
                duration: None,
                rate: 1.0,
                reported_at: Instant::now(),
            })
        };
        let held = vec![
            MediaEvent::SessionChanged(None),
            MediaEvent::PlaybackChanged(PlaybackStatus::Stopped),
            at(203),
            song("next"),
            MediaEvent::PlaybackChanged(PlaybackStatus::Playing),
            at(2),
        ];
        let kept: Vec<String> = settle(held).iter().map(|e| format!("{e:?}")).collect();
        assert_eq!(kept.len(), 3, "{kept:#?}");
        assert!(kept[0].contains("next"));
        assert!(kept[1].contains("Playing"));
        assert!(kept[2].contains("2s"));

        // The new song's start reported before its name moves after it; the
        // old song's end position is still dropped.
        let held = vec![at(203), at(1), song("next")];
        let kept: Vec<String> = settle(held).iter().map(|e| format!("{e:?}")).collect();
        assert_eq!(kept.len(), 2, "{kept:#?}");
        assert!(kept[0].contains("next"));
        assert!(kept[1].contains("1s"));
    }

    #[test]
    fn source_names() {
        assert_eq!(friendly_source("Spotify.exe"), "Spotify");
        assert_eq!(friendly_source("Brave"), "Brave");
        assert_eq!(friendly_source("308046B0AF4A39CB"), "Firefox");
        assert_eq!(friendly_source("MSEdge"), "Edge");
        assert_eq!(friendly_source("org.mpris.MediaPlayer2.spotify"), "Spotify");
        assert_eq!(friendly_source("Something"), "Something");
    }

    #[test]
    fn a_fast_fill_reported_as_the_rate_is_not_counted_twice() {
        // YouTube Music reports 1.4 while filling: the fill's extra is added
        // elsewhere, so the song's own rate is 1.
        assert_eq!(service_rate(1.4, true), 1.0);
        assert_eq!(service_rate(1.02, true), 1.0);
        // Spotify reports 1.0; a slower speed is the page's own choice.
        assert_eq!(service_rate(1.0, true), 1.0);
        assert_eq!(service_rate(0.75, true), 0.75);
        // Without a fill, a faster speed is the listener's choice.
        assert_eq!(service_rate(1.25, false), 1.25);
    }

    #[test]
    fn stopped_event_keeps_the_song_end_for_natural_crossfade() {
        let mut p = PlaybackView {
            status: "playing",
            position_ms: 170_000.0,
            reported_at_ms: 1_000.0,
            ..PlaybackView::default()
        };
        apply_playback_status(&mut p, PlaybackStatus::Stopped, 11_000.0);
        assert_eq!(p.status, "stopped");
        assert_eq!(p.position_ms, 180_000.0);
        assert!(
            p.position_ms >= 180_000.0 - 5_000.0,
            "the next song must count as a natural ending"
        );
    }

    #[test]
    fn pause_and_resume_do_not_add_paused_time_to_the_media_clock() {
        let mut p = PlaybackView {
            status: "playing",
            position_ms: 20_000.0,
            reported_at_ms: 1_000.0,
            rate: 0.96,
            ..PlaybackView::default()
        };
        apply_playback_status(&mut p, PlaybackStatus::Paused, 6_000.0);
        assert_eq!(p.position_ms, 24_800.0);
        apply_playback_status(&mut p, PlaybackStatus::Playing, 66_000.0);
        assert_eq!(p.position_ms, 24_800.0);
        assert_eq!(p.reported_at_ms, 66_000.0);
        apply_playback_status(&mut p, PlaybackStatus::Stopped, 71_000.0);
        assert_eq!(p.position_ms, 29_600.0);
    }
}
