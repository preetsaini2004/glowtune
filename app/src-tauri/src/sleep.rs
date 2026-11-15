//! Sleep timer: pauses the music after a while, or when the current song ends.
//!
//! ```text
//!  set(15 min) ─► state = At(now + 15 min), round += 1 ─► waiter thread (one per set)
//!                                                       wakes → same round? → pause
//!  set(end of song) ─► state = EndOfSong ─► hub: next song starts → pause
//! ```
//! Nothing runs while it's off. Pausing goes through the same path as the
//! taskbar buttons, and only when something is actually playing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Off,
    /// Wall-clock deadline (ms since 1970), so the page can count down.
    At(u64),
    EndOfSong,
}

#[derive(Default)]
pub struct SleepTimer {
    mode: Mutex<Option<Mode>>,
    round: AtomicU64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SleepView {
    /// When the music pauses (ms since 1970), for a minutes timer.
    pub ends_at_ms: Option<u64>,
    pub end_of_song: bool,
}

/// The choices, in the order the hotkey steps through them (None = off).
pub const STEPS: [Option<u32>; 6] = [Some(15), Some(30), Some(45), Some(60), Some(0), None];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl SleepTimer {
    fn mode(&self) -> Mode {
        self.mode.lock().ok().and_then(|m| *m).unwrap_or(Mode::Off)
    }

    pub fn view(&self) -> SleepView {
        match self.mode() {
            Mode::Off => SleepView {
                ends_at_ms: None,
                end_of_song: false,
            },
            Mode::At(t) => SleepView {
                ends_at_ms: Some(t),
                end_of_song: false,
            },
            Mode::EndOfSong => SleepView {
                ends_at_ms: None,
                end_of_song: true,
            },
        }
    }

    /// The step after the current one (for the hotkey).
    pub fn next_step(&self) -> Option<u32> {
        let at = match self.mode() {
            Mode::Off => return STEPS[0],
            Mode::EndOfSong => STEPS.iter().position(|s| *s == Some(0)),
            Mode::At(t) => {
                // Whichever choice is nearest to what's left.
                let left = t.saturating_sub(now_ms()) / 60_000;
                STEPS
                    .iter()
                    .position(|s| s.is_some_and(|m| m > 0 && u64::from(m) >= left))
            }
        };
        at.and_then(|i| STEPS[(i + 1) % STEPS.len()])
    }
}

/// Sets the timer: `Some(minutes)`, `Some(0)` = at the end of this song, None = off.
pub fn set(app: &AppHandle, minutes: Option<u32>) -> SleepView {
    let Some(timer) = app.try_state::<SleepTimer>() else {
        return SleepView {
            ends_at_ms: None,
            end_of_song: false,
        };
    };
    let round = timer.round.fetch_add(1, Ordering::SeqCst) + 1;
    let mode = match minutes {
        None => Mode::Off,
        Some(0) => Mode::EndOfSong,
        Some(m) => Mode::At(now_ms() + u64::from(m.min(24 * 60)) * 60_000),
    };
    if let Ok(mut slot) = timer.mode.lock() {
        *slot = Some(mode);
    }
    if let Mode::At(t) = mode {
        let app = app.clone();
        std::thread::Builder::new()
            .name("glowtune-sleep".into())
            .spawn(move || {
                // Short naps so a later set() or a clock change is noticed.
                loop {
                    let Some(timer) = app.try_state::<SleepTimer>() else {
                        return;
                    };
                    if timer.round.load(Ordering::SeqCst) != round {
                        return;
                    }
                    let left = t.saturating_sub(now_ms());
                    if left == 0 {
                        fire(&app);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(left.min(5_000)));
                }
            })
            .ok();
    }
    let view = timer.view();
    let _ = app.emit("sleep-timer", &view);
    view
}

/// A new song started (from the hub).
pub fn song_changed(app: &AppHandle) {
    if app
        .try_state::<SleepTimer>()
        .is_some_and(|t| t.mode() == Mode::EndOfSong)
    {
        fire(app);
    }
}

fn fire(app: &AppHandle) {
    set(app, None);
    let playing = app
        .state::<std::sync::Arc<crate::hub::Shared>>()
        .snapshot
        .lock()
        .ok()
        .is_some_and(|s| s.playback.status == "playing");
    tracing::info!("sleep timer: time's up (playing: {playing})");
    if playing {
        crate::transport(app.clone(), "playPause");
    }
    crate::osd::show(
        app,
        crate::osd::Toast::new("sleep", "Good night. Music paused"),
    );
}

/// Human words for a timer, for toasts.
pub fn describe(view: &SleepView) -> String {
    if view.end_of_song {
        return "Sleep after this song".into();
    }
    match view.ends_at_ms {
        Some(t) => {
            let mins = t.saturating_sub(now_ms()).div_ceil(60_000);
            format!("Sleep in {mins} min")
        }
        None => "Sleep timer off".into(),
    }
}

#[tauri::command]
pub fn set_sleep_timer(minutes: Option<u32>, app: AppHandle) -> SleepView {
    set(&app, minutes)
}

#[tauri::command]
pub fn get_sleep_timer(timer: tauri::State<'_, SleepTimer>) -> SleepView {
    timer.view()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hotkey_steps_through_every_choice_and_back_to_off() {
        let t = SleepTimer::default();
        assert_eq!(t.next_step(), Some(15));
        *t.mode.lock().unwrap() = Some(Mode::At(now_ms() + 15 * 60_000));
        assert_eq!(t.next_step(), Some(30));
        *t.mode.lock().unwrap() = Some(Mode::At(now_ms() + 59 * 60_000));
        assert_eq!(t.next_step(), Some(0), "after 60 comes end of song");
        *t.mode.lock().unwrap() = Some(Mode::EndOfSong);
        assert_eq!(t.next_step(), None, "then off");
    }

    #[test]
    fn toasts_read_naturally() {
        let at = SleepView {
            ends_at_ms: Some(now_ms() + 30 * 60_000 - 10),
            end_of_song: false,
        };
        assert_eq!(describe(&at), "Sleep in 30 min");
        let song = SleepView {
            ends_at_ms: None,
            end_of_song: true,
        };
        assert_eq!(describe(&song), "Sleep after this song");
    }
}
