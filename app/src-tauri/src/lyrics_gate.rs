//! Decides when the desktop-lyrics *window* is shown at all.
//!
//! ```text
//!  hub: playing on/off ─┐
//!  lyrics: new status ──┼─► gate thread ─► show / hide the lyrics window
//!  every 500 ms (only ──┘     shown = playing && synced lyrics && no fullscreen app on the main screen
//!   while playing)
//! ```
//! Hiding the window (not just making it transparent) matters over games: a
//! visible topmost window over a borderless game can cost frames and latency.
//! While nothing plays, the thread just waits: no polling.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::LYRICS;
use crate::lyrics_service::LyricsService;

const RECHECK: Duration = Duration::from_millis(500);

enum Msg {
    Playing(bool),
    Changed,
}

pub struct LyricsGate {
    tx: Sender<Msg>,
}

impl LyricsGate {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("glowtune-lyrics-gate".into())
            .spawn(move || {
                let mut playing = false;
                loop {
                    let msg = if playing {
                        match rx.recv_timeout(RECHECK) {
                            Ok(m) => Some(m),
                            Err(RecvTimeoutError::Timeout) => None,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    } else {
                        match rx.recv() {
                            Ok(m) => Some(m),
                            Err(_) => return,
                        }
                    };
                    if let Some(Msg::Playing(p)) = msg {
                        playing = p;
                    }
                    apply(&app, playing);
                }
            })
            .expect("spawn lyrics gate");
        Self { tx }
    }

    pub fn playing(&self, playing: bool) {
        let _ = self.tx.send(Msg::Playing(playing));
    }

    /// Lyrics status or the lyrics window changed; re-evaluate now.
    pub fn changed(&self) {
        let _ = self.tx.send(Msg::Changed);
    }
}

/// "Move lyrics" mode: the window stays visible (with a sample line) until Done.
pub static MOVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn apply(app: &AppHandle, playing: bool) {
    let Some(window) = app.get_webview_window(LYRICS) else {
        // Desktop lyrics are off: nothing is hidden, nothing to explain.
        set_hint(app, None);
        return;
    };
    if MOVING.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = window.show();
        return;
    }
    let synced = app
        .try_state::<std::sync::Arc<LyricsService>>()
        .is_some_and(|l| l.view().status == "synced");
    // The desktop scene shows its own lyrics; don't double them on the same screen.
    let scene_has_lyrics = app
        .try_state::<crate::stage::StageGate>()
        .is_some_and(|g| g.on_main_screen())
        && app
            .try_state::<crate::AppSettings>()
            .and_then(|s| s.current.lock().ok().map(|s| s.stage_lyrics))
            .unwrap_or(false);
    // Over a game only when asked to (Settings → Lyrics). Works for games in
    // borderless / windowed full screen; nothing can draw over exclusive
    // full screen.
    let over_fullscreen = app
        .try_state::<crate::AppSettings>()
        .and_then(|s| s.current.lock().ok().map(|s| s.lyrics_over_fullscreen))
        .unwrap_or(false);
    let covered = !over_fullscreen && main_screen_covered(app);
    let player_full = !covered && player_fills_main_screen(app);
    let want = playing && synced && !scene_has_lyrics && !covered && !player_full;
    // Why they're hidden, so it can be said (Settings, a one-time toast).
    let status = app
        .try_state::<std::sync::Arc<LyricsService>>()
        .map(|l| l.view().status)
        .unwrap_or("none");
    set_hint(
        app,
        match () {
            _ if !playing || scene_has_lyrics => None,
            _ if !synced => match status {
                "plain" => Some(Hint::new(
                    "plain",
                    "This song has only unsynced lyrics, so they can't float on the desktop.",
                )),
                "instrumental" => Some(Hint::new("instrumental", "Instrumental: no lyrics.")),
                "none" => Some(Hint::new("none", "No synced lyrics found for this song.")),
                _ => None, // still looking
            },
            _ if covered => Some(Hint::new(
                "fullscreen_app",
                "Lyrics are hidden during full-screen games and videos. Turn on Show over full-screen games to keep them.",
            )),
            _ if player_full => Some(Hint::new(
                "music_app_fullscreen",
                format!(
                    "Lyrics are hidden while {} fills the screen. Make it smaller to see them.",
                    playing_source(app).unwrap_or_else(|| "the music app".into())
                ),
            )),
            _ => None,
        },
    );
    let shown = window.is_visible().unwrap_or(false);
    if want != shown {
        let result = if want { window.show() } else { window.hide() };
        if let Err(e) = result {
            tracing::warn!("lyrics window: {e}");
        }
    }
}

/// Why the floating lyrics aren't showing, in words.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Hint {
    pub reason: &'static str,
    pub text: String,
}

impl Hint {
    fn new(reason: &'static str, text: impl Into<String>) -> Self {
        Self {
            reason,
            text: text.into(),
        }
    }
}

static HINT: std::sync::Mutex<Option<Hint>> = std::sync::Mutex::new(None);

/// Reasons explained once with a toast. Not the game one: a toast must not
/// pop up over a game uninvited.
const TOAST_ONCE: [&str; 2] = ["music_app_fullscreen", "plain"];

/// Stores the current hint; on a change, tells the windows ("lyrics-hint")
/// and, the first time ever for some reasons, shows a toast.
fn set_hint(app: &AppHandle, hint: Option<Hint>) {
    let changed = match HINT.lock() {
        Ok(mut h) if *h != hint => {
            *h = hint.clone();
            true
        }
        _ => false,
    };
    if !changed {
        return;
    }
    let _ = app.emit("lyrics-hint", &hint);
    let Some(hint) = hint.filter(|h| TOAST_ONCE.contains(&h.reason)) else {
        return;
    };
    let seen = crate::current_settings(app)
        .hints_seen
        .iter()
        .any(|r| r == hint.reason);
    if seen {
        return;
    }
    tracing::info!("lyrics hint (first time): {}", hint.text);
    crate::osd::show(app, crate::osd::Toast::new("lyrics", hint.text.clone()));
    let reason = hint.reason.to_string();
    crate::update_settings(app, |s| {
        if !s.hints_seen.contains(&reason) {
            s.hints_seen.push(reason);
        }
    });
}

/// Why lyrics are hidden right now, for a window that just opened.
#[tauri::command]
pub fn lyrics_hint() -> Option<Hint> {
    HINT.lock().ok().and_then(|h| h.clone())
}

/// The playing app's name ("Spotify").
fn playing_source(app: &AppHandle) -> Option<String> {
    app.try_state::<std::sync::Arc<crate::hub::Shared>>()
        .and_then(|s| {
            s.snapshot
                .lock()
                .ok()
                .and_then(|s| s.track.as_ref().map(|t| t.source.clone()))
        })
        .filter(|s| !s.is_empty())
}

/// The music app itself (e.g. Spotify) is in front and fills the main screen:
/// it's showing its own player or lyrics view, so ours would only double them.
fn player_fills_main_screen(app: &AppHandle) -> bool {
    let Some(source) = app
        .try_state::<std::sync::Arc<crate::hub::Shared>>()
        .and_then(|s| {
            s.snapshot
                .lock()
                .ok()
                .and_then(|s| s.track.as_ref().map(|t| t.source.clone()))
        })
    else {
        return false;
    };
    let Some((exe, [l, t, r, b])) = crate::foreground::window() else {
        return false;
    };
    if source.is_empty() || crate::hub::friendly_source(&exe) != source {
        return false;
    }
    let Ok(Some(m)) = app.primary_monitor() else {
        return false;
    };
    let area = m.work_area();
    let (ax, ay) = (area.position.x, area.position.y);
    let (aw, ah) = (area.size.width as i32, area.size.height as i32);
    let w = (r.min(ax + aw) - l.max(ax)).max(0) as f64;
    let h = (b.min(ay + ah) - t.max(ay)).max(0) as f64;
    w * h >= 0.85 * (aw as f64 * ah as f64)
}

/// True while a fullscreen app or game is in front on the primary screen.
fn main_screen_covered(app: &AppHandle) -> bool {
    let Some(origin) = overlay::fullscreen_screen() else {
        return false;
    };
    app.primary_monitor()
        .ok()
        .flatten()
        .is_some_and(|m| (m.position().x, m.position().y) == origin)
}
