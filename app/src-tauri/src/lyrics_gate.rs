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

use tauri::{AppHandle, Manager};

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
    let want = playing
        && synced
        && !scene_has_lyrics
        && !main_screen_covered(app)
        && !player_fills_main_screen(app);
    let shown = window.is_visible().unwrap_or(false);
    if want != shown {
        let result = if want { window.show() } else { window.hide() };
        if let Err(e) = result {
            tracing::warn!("lyrics window: {e}");
        }
    }
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
