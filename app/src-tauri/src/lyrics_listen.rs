//! Automatic lyrics timing for music Glowtune doesn't play itself: Spotify
//! desktop, a browser, or a service window with no sound effects on.
//!
//! ```text
//!  a song starts (with its intro still ahead)
//!   ──► listen to that app's sound (audio_engine::listen; nothing played,
//!       no volume touched) ──► where does the voice first come in?
//!   ──► the lyrics' first sung line is more than 1 s away from it?
//!       shift every line by the difference (this play only, not saved)
//!   ──► stop listening (a few seconds after the first line)
//! ```
//! The same check the sound engine makes (`Sound::align_tick`), but the
//! engine hears the song ~34 s ahead through its delay, so it fixes the
//! lyrics before the first line. Here the voice is heard as it plays: the
//! fix lands a few seconds after the first line. The search is narrower
//! (±8 s) to land sooner. Browsers report the position coarsely, so only a
//! clear entry and a shift of 1 s or more count, as with the engine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::lyrics_service::LyricsService;
use crate::sound::Sound;

/// How often the watcher looks.
const TICK: Duration = Duration::from_millis(500);
/// How far from the first line the voice may come in (the engine: 12 s).
const SEARCH_MS: f64 = 8000.0;
/// Sound needed after the search window to judge an entry there.
const AFTER_MS: f64 = 3500.0;
/// A position this far from where the heard sound puts it: a seek.
const SEEK_MS: f64 = 1500.0;

/// The program that plays a source, by its friendly name (`hub`).
fn program(source: &str) -> Option<&'static str> {
    Some(match source {
        "Spotify" => "spotify.exe",
        "Chrome" => "chrome.exe",
        "Edge" => "msedge.exe",
        "Brave" => "brave.exe",
        "Firefox" => "firefox.exe",
        "Opera" => "opera.exe",
        "Vivaldi" => "vivaldi.exe",
        _ => return None,
    })
}

/// What's playing, as far as this watcher cares.
struct Now {
    source: String,
    playing: bool,
    position_ms: f64,
}

fn now_playing(app: &AppHandle) -> Option<Now> {
    let shared = app.try_state::<Arc<crate::hub::Shared>>()?;
    let (source, playing) = {
        let s = shared.snapshot.lock().ok()?;
        (
            s.track.as_ref()?.source.clone(),
            s.playback.status == "playing",
        )
    };
    Some(Now {
        source,
        playing,
        position_ms: crate::hub::lyrics_position_ms(app)?,
    })
}

/// The process to listen to for `source`, if this watcher should.
fn target(app: &AppHandle, source: &str) -> Option<u32> {
    if source == "Web player" {
        // With sound effects on, the engine does this (and better).
        let (pid, engine) = app.try_state::<Sound>()?.window();
        return pid.filter(|_| !engine);
    }
    audio_engine::sessions::roots_named(program(source)?)
        .into_iter()
        .next()
}

/// One song being listened to.
struct Listening {
    key: String,
    listener: audio_engine::listen::Listener,
    /// Song time where the heard sound starts, first worked out.
    start_ms: Option<f64>,
}

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("glowtune-lyrics-listen".into())
        .spawn(move || run(app));
}

fn run(app: AppHandle) {
    // The song (lyrics key) already decided on.
    let mut decided = String::new();
    let mut on: Option<Listening> = None;
    loop {
        std::thread::sleep(TICK);
        let Some(lyrics) = app.try_state::<Arc<LyricsService>>() else {
            continue;
        };
        // Synced lyrics whose timing nobody touched (no saved nudge, not
        // aligned already), and something playing.
        let Some((key, lines)) = lyrics.alignable() else {
            on = None;
            continue;
        };
        let key = key.as_str();
        if key == decided {
            on = None;
            continue;
        }
        let Some(now) = now_playing(&app) else {
            on = None;
            continue;
        };
        let Some(first) = lines.iter().find(|l| l.sung).map(|l| l.time_ms) else {
            decided = key.to_string();
            continue;
        };
        let decide = |why: String, decided: &mut String, on: &mut Option<Listening>| {
            tracing::info!("lyrics listen: {why}");
            *decided = key.to_string();
            *on = None;
        };
        if !now.playing {
            if on.is_some() {
                decide("paused, leaving it".into(), &mut decided, &mut on);
            }
            continue;
        }
        if on.as_ref().is_some_and(|l| l.key != key) {
            on = None;
        }
        if on.is_none() {
            // 3 s of sound to compare with before the voice may come in,
            // which is at least 2 s before the first line.
            if now.position_ms > first - 5000.0 || first < 4000.0 {
                decide(
                    format!(
                        "joined at {:.1} s, too close to the first line ({:.1} s), leaving it",
                        now.position_ms / 1000.0,
                        first / 1000.0
                    ),
                    &mut decided,
                    &mut on,
                );
                continue;
            }
            let Some(pid) = target(&app, &now.source) else {
                decided = key.to_string();
                continue;
            };
            match audio_engine::listen::Listener::start(pid) {
                Ok(listener) => {
                    tracing::info!("lyrics listen: {} (pid {pid})", now.source);
                    on = Some(Listening {
                        key: key.to_string(),
                        listener,
                        start_ms: None,
                    });
                }
                Err(e) => decide(format!("can't listen: {e}"), &mut decided, &mut on),
            }
            continue;
        }
        let Some((heard, known_start)) = on.as_ref().map(|l| (l.listener.heard(), l.start_ms))
        else {
            continue;
        };
        let (Some(first_at), Some(last_at)) = (heard.hop_at(0), heard.last_at) else {
            if now.position_ms > first + 5000.0 {
                decide(
                    "heard nothing from the app, leaving it".into(),
                    &mut decided,
                    &mut on,
                );
            }
            continue;
        };
        let wall = Instant::now();
        let ms = |at: Instant| wall.saturating_duration_since(at).as_secs_f64() * 1000.0;
        let start_ms = now.position_ms - ms(first_at);
        let end_ms = now.position_ms - ms(last_at);
        match known_start {
            None => {
                if let Some(l) = on.as_mut() {
                    l.start_ms = Some(start_ms);
                }
            }
            Some(s) if (s - start_ms).abs() > SEEK_MS => {
                decide(
                    "the song jumped (a seek), leaving it".into(),
                    &mut decided,
                    &mut on,
                );
                continue;
            }
            Some(_) => {}
        }
        let to = first + SEARCH_MS;
        if end_ms < to + AFTER_MS {
            continue;
        }
        // Judge on the first start worked out: the position reports of the
        // app jitter, the sound itself doesn't.
        let start_ms = known_start.unwrap_or(start_ms);
        let from = (first - SEARCH_MS)
            .max(start_ms + 3000.0)
            .max(Sound::ALIGN_NOT_BEFORE_MS);
        let trace: Vec<f32> = heard.hops.into_iter().collect();
        let verdict = match audio_engine::voice::first_entry(
            &trace,
            start_ms,
            from,
            to,
            Sound::ALIGN_MIN_STEP,
        ) {
            None => format!(
                "no clear voice entry near the first line ({:.1} s), leaving it",
                first / 1000.0
            ),
            Some(entry) => {
                let lag = entry.at_ms - first;
                if lag.abs() < Sound::ALIGN_MIN_SHIFT_MS {
                    format!(
                        "in time (voice at {:.1} s, first line {:.1} s)",
                        entry.at_ms / 1000.0,
                        first / 1000.0
                    )
                } else if lag.abs() > Sound::ALIGN_BIG_SHIFT_MS
                    && entry.step < Sound::ALIGN_BIG_STEP
                {
                    format!(
                        "voice maybe at {:.1} s, first line {:.1} s, not clear enough (step {:.1}), leaving it",
                        entry.at_ms / 1000.0,
                        first / 1000.0,
                        entry.step
                    )
                } else {
                    // Positive offsets show lyrics earlier.
                    let offset = -((lag / 50.0).round() * 50.0) as i64;
                    lyrics.align(&app, key, offset);
                    format!(
                        "voice comes in at {:.1} s, first line says {:.1} s (step {:.1}): shifting the lyrics {:+.1} s",
                        entry.at_ms / 1000.0,
                        first / 1000.0,
                        entry.step,
                        lag / 1000.0
                    )
                }
            }
        };
        let title = app
            .try_state::<Arc<crate::hub::Shared>>()
            .and_then(|s| {
                s.snapshot
                    .lock()
                    .ok()?
                    .track
                    .as_ref()
                    .map(|t| t.title.clone())
            })
            .unwrap_or_default();
        decide(
            format!("{title} ({}): {verdict}", now.source),
            &mut decided,
            &mut on,
        );
    }
}
