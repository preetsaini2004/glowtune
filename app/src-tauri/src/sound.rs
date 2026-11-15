//! Sound effects for the service window: equalizer and song-to-song blend.
//!
//! ```text
//!  settings (Sound) ──┐
//!  service window ────┼─► Sound ──► audio_engine::Engine (only while both say "on")
//!  hub (song change,  │              │
//!       pause, seek) ─┘              └─► timing (delay, speed) ─► hub shifts the
//!                                        position so lyrics match what is heard
//! ```
//! The equalizer and the blend (Crossfade / Automix) are switched separately.
//! The engine runs only while a service window is open and at least one of
//! them is on, so nothing runs in the background otherwise.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use audio_engine::{Blend, EqSettings};
use tauri::{AppHandle, Manager};

use crate::settings::Settings;

/// How the timing of what is heard differs from what the service reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// Sound is heard this much later than the service plays it.
    pub delay_ms: f64,
    /// Playback speed (1.0, or slightly less while the delay refills).
    pub speed: f64,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundStatus {
    /// Sound is flowing through the engine.
    pub running: bool,
    /// How late the sound is heard, in ms.
    pub delay_ms: u32,
    /// The delay is still filling up (playing a touch slower).
    pub refilling: bool,
    /// Two songs are being blended right now.
    pub blending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Config {
    on: bool,
    eq: EqSettings,
    blend: Blend,
}

impl Config {
    fn from(s: &Settings) -> Self {
        let mut gains_db = [0.0; 10];
        for (g, v) in gains_db.iter_mut().zip(&s.eq_gains) {
            *g = *v;
        }
        let blend = match s.sound_blend.as_str() {
            "crossfade" => Blend::Crossfade {
                secs: s.crossfade_secs,
            },
            "automix" => Blend::Automix {
                secs: s.crossfade_secs,
            },
            _ => Blend::Off,
        };
        Self {
            // The equalizer and the blend each have their own switch; the
            // engine runs when at least one is on.
            on: s.eq_enabled || blend != Blend::Off,
            eq: EqSettings {
                enabled: s.eq_enabled,
                preamp_db: s.eq_preamp,
                gains_db,
            },
            blend,
        }
    }
}

/// Delay Automix needs before it may cut a song short: the fade itself, the
/// first seconds of the next song (to find its beat) and a small cushion.
///
/// ```text
///  heard ◄─────────── delay ───────────► captured
///        [ fade of A's tail ][ B's first 3.3 s + 0.5 s ]
/// ```
/// Pressing Next with less than that skips the song's last seconds and gives
/// a short or no blend: an abrupt cut. Then the song is left to end by itself
/// and blends like Crossfade.
fn needed_delay_ms(fade_secs: f64) -> f64 {
    (fade_secs.clamp(1.0, 12.0) + 3.8) * 1000.0
}

fn delay_ready(delay_ms: f64, fade_secs: f64) -> bool {
    delay_ms >= needed_delay_ms(fade_secs)
}

/// What the Automix watcher should do after one look.
enum Tick {
    Wait,
    PressNext,
    Stop,
}

#[cfg(windows)]
type Engine = audio_engine::Engine;
/// Other systems have no audio route yet: nothing to run.
#[cfg(not(windows))]
type Engine = ();

struct Inner {
    config: Config,
    /// The service window's WebView2 browser process, while one is open.
    window_pid: Option<u32>,
    engine: Option<Engine>,
    /// Last play / next / previous / seek sent from Glowtune itself.
    last_command: Option<Instant>,
    /// Automix: when it last pressed Next itself, and for which song.
    automix_at: Option<Instant>,
    automix_song: String,
    /// Automix: the last song it decided not to cut short (logged once).
    declined_song: String,
    /// The song the watcher saw last, and since when.
    seen_song: String,
    seen_since: Option<Instant>,
    /// The Automix watcher thread is alive.
    watching: bool,
    /// The fast-fill thread is alive.
    filling: bool,
    /// Glowtune's own volume on top of the page's, while the engine runs
    /// (1 = as the page plays). See `set_volume`.
    gain: f32,
    /// Counts volume changes, so only the last one of a drag is saved.
    gain_edits: u64,
}

/// Fast fill: the service plays up to 1.4× (pitch raised) while the blend's
/// delay is short, and the engine reads it back at normal speed, so the delay
/// fills in seconds instead of minutes. (`media_rate.js` owns the page side.)
///
/// ```text
///  delay short, song has > 30 s left ─► ask 1.02, 1.04 … 1.4 (every 200 ms)
///  page reports its real speed       ─► the engine undoes exactly that speed
///  delay nearly full, song ending, page reset the speed ─► back to 1.0
/// ```
/// Small steps keep the moment the page and the engine switch speed (a few
/// tens of ms apart) inaudible. 1.4×, not more: everything above ~17 kHz is
/// lost while filling (the browser drops it when it raises the pitch).
const FILL_SPEED: f64 = 1.4;
const FILL_STEP: f64 = 0.02;
/// Stop filling this close to the end of a song: the next song resets the
/// speed by itself, and that must never happen mid-fill.
const FILL_MIN_SONG_LEFT_MS: f64 = 20_000.0;
/// Ramping down from full speed still adds about this much delay.
const FILL_RAMP_DOWN_GAIN_MS: f64 = 1100.0;

#[derive(Default)]
struct FillState {
    /// Speed last asked of the page (1.0 = not filling).
    asked: f64,
    /// Speed the page really plays at, as the engine was told.
    real: f64,
    /// `(wall ms, real speed)` at each change, oldest first: lets positions
    /// reported by the service be read correctly while it ran faster.
    history: Vec<(f64, f64)>,
    /// Last moment the service ran faster than normal.
    last_fast: Option<Instant>,
    /// The page has no speed hook: don't try again for this song.
    refused_song: String,
    /// When the current fill started (for the log).
    started: Option<(Instant, u32)>,
    /// Since when the page hasn't answered (see `PageAnswer::Silent`).
    silent_since: Option<Instant>,
}

/// The page's own speed hook drops back to 1× this long after the last ask
/// (`fill(speed, 1500)` in `media_rate.js`).
const PAGE_FILL_EXPIRY: Duration = Duration::from_millis(1500);

impl FillState {
    /// Extra service progress (ms) since `since_wall_ms`: what positions the
    /// service reported then need adding, since it played faster for a while.
    fn extra_ms_since(&self, since_wall_ms: f64, now_wall_ms: f64) -> f64 {
        let mut extra = 0.0;
        for (i, &(at, speed)) in self.history.iter().enumerate() {
            let end = self.history.get(i + 1).map_or(now_wall_ms, |n| n.0);
            let from = at.max(since_wall_ms);
            if end > from {
                extra += (end - from) * (speed - 1.0);
            }
        }
        extra
    }

    fn record(&mut self, now_wall_ms: f64, speed: f64) {
        self.history.push((now_wall_ms, speed));
        // Positions are re-reported far more often than this; keep 30 minutes.
        let old = now_wall_ms - 30.0 * 60_000.0;
        let keep_from = self
            .history
            .iter()
            .rposition(|&(at, _)| at <= old)
            .unwrap_or(0);
        self.history.drain(..keep_from);
    }
}

/// What the page's speed hook reports (see `media_rate.js`).
#[derive(Debug, serde::Deserialize)]
struct PageRate {
    speed: f64,
}

/// What the page said when asked for a speed.
enum PageAnswer {
    /// The speed it really plays at.
    Rate(PageRate),
    /// The page answered: it has no speed hook.
    NoHook,
    /// No window, or no answer in time (the page was busy, e.g. loading the
    /// next song). Says nothing about the hook: ask again.
    Silent,
}

/// Asks the service page for a speed.
fn ask_page(app: &AppHandle, speed: f64) -> PageAnswer {
    ask_page_raw(app, speed).unwrap_or(PageAnswer::Silent)
}

fn ask_page_raw(app: &AppHandle, speed: f64) -> Option<PageAnswer> {
    let window = app.get_webview_window(crate::services::SERVICE)?;
    let js = format!(
        "(() => {{ const r = window.__glowtuneRate; return r ? r.fill({speed}, 1500) : null; }})()"
    );
    let (tx, rx) = std::sync::mpsc::channel();
    crate::services::run_script(&window, js, tx).ok()?;
    let out = rx.recv_timeout(Duration::from_secs(1)).ok()?;
    match serde_json::from_str::<Option<PageRate>>(&out).ok()? {
        Some(rate) => Some(PageAnswer::Rate(rate)),
        None => Some(PageAnswer::NoHook),
    }
}

enum FillPlan {
    /// Nothing to do: look again in a second.
    Idle,
    /// Ask the page for this speed (and read back what it really does).
    Ask(f64),
    /// The engine or the blend is off: end the thread.
    Stop,
}

pub struct Sound {
    app: AppHandle,
    inner: Mutex<Inner>,
    fill: Mutex<FillState>,
    align: Mutex<AlignState>,
}

/// Automatic lyrics alignment (see `align_tick`).
#[derive(Default)]
struct AlignState {
    last: Option<Instant>,
    /// The song (lyrics key) already decided on.
    decided: String,
}

impl Sound {
    pub fn new(app: AppHandle, settings: &Settings) -> Self {
        Self {
            app,
            inner: Mutex::new(Inner {
                config: Config::from(settings),
                window_pid: None,
                engine: None,
                last_command: None,
                automix_at: None,
                automix_song: String::new(),
                declined_song: String::new(),
                seen_song: String::new(),
                seen_since: None,
                watching: false,
                filling: false,
                gain: settings.service_volume,
                gain_edits: 0,
            }),
            fill: Mutex::new(FillState {
                asked: 1.0,
                real: 1.0,
                ..Default::default()
            }),
            align: Mutex::new(AlignState::default()),
        }
    }

    /// Applies the Sound settings (live: no restart of the music).
    pub fn configure(&self, settings: &Settings) {
        self.change(|inner| inner.config = Config::from(settings));
    }

    pub fn window_opened(&self, browser_pid: u32) {
        let mut previous = None;
        self.change(|inner| {
            if inner.window_pid != Some(browser_pid) {
                previous = inner.engine.take();
                inner.window_pid = Some(browser_pid);
            }
        });
        drop(previous);
    }

    pub fn window_closed(&self) {
        self.change(|inner| inner.window_pid = None);
    }

    /// Applies a change, then starts, stops or updates the engine to match.
    ///
    /// An engine that is no longer needed is dropped **after** the lock is
    /// released: stopping it waits for its thread, and that thread's last act
    /// (`notify`) reads this state again. Holding the lock would deadlock.
    fn change(&self, f: impl FnOnce(&mut Inner)) {
        let (retired, watch, fill, hand_back) = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            f(&mut inner);
            let retired = self.sync(&mut inner);
            // Sound effects turned off with the window still open: from now on
            // the page plays directly, at its own volume. Give it Glowtune's.
            let hand_back = (retired.is_some()
                && inner.window_pid.is_some()
                && (inner.gain - 1.0).abs() > 0.001)
                .then(|| std::mem::replace(&mut inner.gain, 1.0));
            let watch = !inner.watching
                && inner.engine.is_some()
                && matches!(inner.config.blend, Blend::Automix { .. });
            inner.watching |= watch;
            let fill = !inner.filling && inner.engine.is_some() && inner.config.blend != Blend::Off;
            inner.filling |= fill;
            (retired, watch, fill, hand_back)
        };
        match hand_back {
            Some(gain) => self.hand_back_volume(gain, retired),
            None => drop(retired),
        }
        if watch {
            Self::watch_automix(self.app.clone());
        }
        if fill {
            Self::watch_fill(self.app.clone());
        }
    }

    // ---- Glowtune's own volume --------------------------------------------
    //
    //   the page's volume ─► baked into the captured sound ─► heard after the delay (34 s)
    //   Glowtune's volume ─► a gain after the delay        ─► heard at once
    //
    // So while the engine runs, the player's slider moves this gain and leaves
    // the page alone. The slider shows page volume × gain.

    /// Glowtune's volume on top of the page's, while the engine runs.
    pub fn gain(&self) -> Option<f32> {
        let inner = self.inner.lock().ok()?;
        inner.engine.as_ref()?;
        Some(inner.gain)
    }

    /// Sets Glowtune's volume (heard at once) and remembers it.
    pub fn set_gain(&self, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, audio_engine::Volume::MAX)
        } else {
            1.0
        };
        let edit = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.gain = gain;
            #[cfg(windows)]
            if let Some(engine) = &inner.engine {
                engine.send(audio_engine::Command::Volume(gain));
            }
            inner.gain_edits += 1;
            inner.gain_edits
        };
        // Saved once the slider rests, so a restart keeps the level.
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let latest = app
                .try_state::<Sound>()
                .and_then(|s| s.inner.lock().ok().map(|i| i.gain_edits));
            if latest == Some(edit) {
                crate::update_settings(&app, |s| s.service_volume = gain);
            }
        });
    }

    /// The engine stops while the page keeps playing: set the page to the
    /// level the listener hears now *before* the page is heard directly again
    /// (otherwise turning Sound effects off could be suddenly loud).
    fn hand_back_volume(&self, gain: f32, retired: Option<Engine>) {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let page = crate::services::web_player(&app, "state", None)
                .await
                .and_then(|s| s.volume);
            if let Some(page) = page {
                let level = (page * gain as f64).clamp(0.0, 1.0);
                crate::services::web_player(&app, "volume", Some(level)).await;
                tracing::info!("volume: handed {level:.2} back to the page");
            }
            crate::update_settings(&app, |s| s.service_volume = 1.0);
            let _ = tauri::async_runtime::spawn_blocking(move || drop(retired)).await;
        });
    }

    // ---- automatic lyrics alignment ---------------------------------------
    //
    //   the delay holds the next ~34 s of the song before it is heard
    //   ──► where does the voice first come in? (voice::first_entry)
    //   ──► the lyrics' first sung line is more than 1 s away from it?
    //       shift every line by the difference (this play only, not saved)
    //
    // Only a clear entry counts (a quiet intro, then a voice); a busy intro
    // leaves the lyrics as they are. A nudge saved for the song: hands off.

    /// How much clearer the voice must be after its entry than before.
    const ALIGN_MIN_STEP: f32 = 1.5;
    /// Closer than this, the lyrics are left alone.
    const ALIGN_MIN_SHIFT_MS: f64 = 1000.0;
    /// How far from the first line the voice may come in.
    const ALIGN_SEARCH_MS: f64 = 12_000.0;
    /// An entry this early is the blend with the song before, not the voice
    /// (live: a DJ handover read as "voice at 0.0 s", lyrics moved 9.7 s).
    const ALIGN_NOT_BEFORE_MS: f64 = 2000.0;
    /// Shifts bigger than this need a much clearer entry.
    const ALIGN_BIG_SHIFT_MS: f64 = 5000.0;
    const ALIGN_BIG_STEP: f32 = 2.5;

    #[cfg(windows)]
    fn align_tick(&self) {
        use audio_engine::voice;
        {
            let Ok(mut state) = self.align.lock() else {
                return;
            };
            if state
                .last
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
            {
                return;
            }
            state.last = Some(Instant::now());
        }
        let Some(lyrics) = self
            .app
            .try_state::<std::sync::Arc<crate::lyrics_service::LyricsService>>()
        else {
            return;
        };
        let Some((key, lines)) = lyrics.alignable() else {
            return;
        };
        let decide = |why: &str| {
            if let Ok(mut state) = self.align.lock() {
                state.decided = key.clone();
            }
            // (The lyrics key is a hash: name the song from what's shown.)
            let title = self
                .app
                .try_state::<std::sync::Arc<crate::hub::Shared>>()
                .and_then(|s| {
                    let snapshot = s.snapshot.lock().ok()?;
                    snapshot.track.as_ref().map(|t| t.title.clone())
                })
                .unwrap_or_default();
            tracing::info!("lyrics align: {title}: {why}");
        };
        if self.align.lock().is_ok_and(|s| s.decided == key) {
            return;
        }
        let Some(first) = lines.iter().find(|l| l.sung).map(|l| l.time_ms) else {
            return decide("no sung line");
        };
        let status = {
            let Ok(inner) = self.inner.lock() else {
                return;
            };
            match &inner.engine {
                Some(engine) => engine.status(),
                None => return,
            }
        };
        // Mid-blend, two songs share the sound: wait until it's one again.
        if !status.running() || status.blending() {
            return;
        }
        let Some(heard_ms) = crate::hub::heard_position_ms(&self.app) else {
            return;
        };
        let (ring, heard_frame) = status.voice();
        let per_ms = audio_engine::engine::SAMPLE_RATE as f64 / 1000.0;
        let start_ms = heard_ms + (ring.first_frame as f64 - heard_frame as f64) / per_ms;
        let end_ms = start_ms + ring.hops.len() as f64 * voice::HOP_MS;
        let from = (first - Self::ALIGN_SEARCH_MS)
            .max(start_ms + 3000.0)
            .max(Self::ALIGN_NOT_BEFORE_MS);
        let to = first + Self::ALIGN_SEARCH_MS;
        if from > first - 2000.0 {
            // No 3 s of sound before the first line to compare with: the
            // singing starts right away, or the sound captured starts later
            // (a skip or a seek into the song).
            return decide(&format!(
                "no sound before the first line ({:.1} s) to compare with, leaving it",
                first / 1000.0
            ));
        }
        if end_ms < to + 3500.0 {
            if heard_ms > first + 30_000.0 {
                decide("not enough sound ahead (short delay), leaving it");
            }
            return;
        }
        let trace: Vec<f32> = ring.hops.into_iter().collect();
        match voice::first_entry(&trace, start_ms, from, to, Self::ALIGN_MIN_STEP) {
            Some(entry) => {
                let lag = entry.at_ms - first;
                if lag.abs() < Self::ALIGN_MIN_SHIFT_MS {
                    decide(&format!(
                        "in time (voice at {:.1} s, first line {:.1} s)",
                        entry.at_ms / 1000.0,
                        first / 1000.0
                    ));
                } else if lag.abs() > Self::ALIGN_BIG_SHIFT_MS && entry.step < Self::ALIGN_BIG_STEP
                {
                    decide(&format!(
                        "voice maybe at {:.1} s, first line {:.1} s, but not clear enough (step {:.1}) for a {:.1} s shift, leaving it",
                        entry.at_ms / 1000.0,
                        first / 1000.0,
                        entry.step,
                        lag.abs() / 1000.0
                    ));
                } else {
                    // Positive offsets show lyrics earlier.
                    let offset = -((lag / 50.0).round() * 50.0) as i64;
                    decide(&format!(
                        "voice comes in at {:.1} s, first line says {:.1} s (step {:.1}): shifting the lyrics {:+.1} s",
                        entry.at_ms / 1000.0,
                        first / 1000.0,
                        entry.step,
                        lag / 1000.0
                    ));
                    lyrics.align(&self.app, &key, offset);
                }
            }
            None => decide(&format!(
                "no clear voice entry near the first line ({:.1} s), leaving it",
                first / 1000.0
            )),
        }
    }

    // ---- fast fill ------------------------------------------------------

    fn watch_fill(app: AppHandle) {
        let spawned = std::thread::Builder::new()
            .name("glowtune-fill".into())
            .spawn(move || {
                loop {
                    let Some(sound) = app.try_state::<Sound>() else {
                        return;
                    };
                    // Runs as long as the delay does, like the fill.
                    #[cfg(windows)]
                    sound.align_tick();
                    match sound.fill_plan() {
                        FillPlan::Stop => {
                            // Never leave the service running fast.
                            if sound.fill.lock().is_ok_and(|f| f.real > 1.0) {
                                let state = ask_page(&app, 1.0);
                                sound.fill_report(state, 1.0);
                            }
                            return;
                        }
                        FillPlan::Idle => std::thread::sleep(Duration::from_secs(1)),
                        FillPlan::Ask(speed) => {
                            let state = ask_page(&app, speed);
                            sound.fill_report(state, speed);
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("fast fill: {e}");
        }
    }

    /// One look: should the service play faster, and how fast exactly?
    fn fill_plan(&self) -> FillPlan {
        #[cfg(windows)]
        let (latency, target, running) = {
            let Ok(mut inner) = self.inner.lock() else {
                return FillPlan::Stop;
            };
            match &inner.engine {
                Some(engine) if inner.config.blend != Blend::Off => {
                    let s = engine.status();
                    (s.latency_ms() as f64, s.target_ms() as f64, s.running())
                }
                _ => {
                    inner.filling = false;
                    return FillPlan::Stop;
                }
            }
        };
        #[cfg(not(windows))]
        let (latency, target, running) = (0.0, 0.0, false);
        // `fill` is a leaf lock: never take another lock while holding it.
        let song = self.song_now();
        let Ok(fill) = self.fill.lock() else {
            return FillPlan::Stop;
        };
        let eligible = running
            && target > 0.0
            && song.as_ref().is_some_and(|(name, left)| {
                *left > FILL_MIN_SONG_LEFT_MS && *name != fill.refused_song
            });
        let goal = if eligible && latency + FILL_RAMP_DOWN_GAIN_MS < target {
            FILL_SPEED
        } else {
            1.0
        };
        let asked = fill.asked.max(1.0);
        if goal <= 1.0 && asked <= 1.0 && fill.real <= 1.0 {
            return FillPlan::Idle;
        }
        // Not playing a service song at all (another app, nothing): straight
        // back to normal. Otherwise ramp in small steps.
        if song.is_none() {
            return FillPlan::Ask(1.0);
        }
        let next = if goal > asked {
            (asked + FILL_STEP).min(goal)
        } else {
            (asked - FILL_STEP).max(1.0)
        };
        FillPlan::Ask((next * 100.0).round() / 100.0)
    }

    /// The service window's song now playing, with how much of it is left (ms).
    fn song_now(&self) -> Option<(String, f64)> {
        let (name, duration, p) = {
            let shared = self.app.try_state::<std::sync::Arc<crate::hub::Shared>>()?;
            let snapshot = shared.snapshot.lock().ok()?;
            let track = snapshot.track.as_ref()?;
            let p = snapshot.playback.clone();
            if track.source != "Web player" || p.status != "playing" {
                return None;
            }
            let duration = track.duration_ms.filter(|d| *d > 0.0)?;
            (format!("{}\n{}", track.title, track.artist), duration, p)
        };
        let now = crate::hub::wall_ms(Instant::now());
        let position = p.position_ms
            + (now - p.reported_at_ms).max(0.0) * p.rate
            + self.extra_ms_since(p.reported_at_ms);
        Some((name, duration - position))
    }

    /// The page answered: tell the engine the speed it really plays at.
    fn fill_report(&self, state: PageAnswer, asked: f64) {
        let song = self.song_now();
        let latency = self.status().delay_ms;
        let Ok(mut fill) = self.fill.lock() else {
            return;
        };
        let real = match state {
            PageAnswer::Rate(s) if s.speed.is_finite() => {
                fill.silent_since = None;
                s.speed.clamp(1.0, 1.5)
            }
            // Busy page: it keeps its speed until its own hook expires, so
            // keep the engine's too, and ask again. Past the expiry it is
            // surely back at 1×.
            PageAnswer::Silent => {
                let since = *fill.silent_since.get_or_insert_with(Instant::now);
                if since.elapsed() < PAGE_FILL_EXPIRY || fill.real <= 1.0 {
                    // Don't ramp on while unheard: one step at a time.
                    fill.asked = fill.real.max(1.0);
                    return;
                }
                tracing::info!("fast fill: no answer from the page, so it is back at 1x");
                1.0
            }
            _ => {
                fill.silent_since = None;
                if asked > 1.0 && fill.refused_song.is_empty() {
                    tracing::info!("fast fill: this page has no speed hook");
                }
                if let Some((name, _)) = &song {
                    fill.refused_song = name.clone();
                }
                1.0
            }
        };
        // The page went back to normal by itself (a new song, an ad): start
        // over from there instead of jumping back up.
        if real + 0.005 < asked && real <= 1.0 && fill.real > 1.0 {
            tracing::info!(
                "fast fill: the page went back to 1x by itself (asked {asked:.2}x; a new song or a reloaded stream)"
            );
        }
        fill.asked = if real + 0.005 < asked { real } else { asked };
        if (real - fill.real).abs() < 0.0005 {
            return;
        }
        let now = Instant::now();
        if fill.real <= 1.0 && real > 1.0 {
            fill.started = Some((now, latency));
            tracing::info!(
                "fast fill: started (delay {:.1} s)",
                latency as f32 / 1000.0
            );
        }
        if real <= 1.0
            && let Some((at, from)) = fill.started.take()
        {
            tracing::info!(
                "fast fill: ended after {:.1} s, delay {:.1} s → {:.1} s",
                at.elapsed().as_secs_f32(),
                from as f32 / 1000.0,
                latency as f32 / 1000.0
            );
        }
        fill.real = real;
        fill.record(crate::hub::wall_ms(now), real);
        if real > 1.0 {
            fill.last_fast = Some(now);
        }
        drop(fill);
        #[cfg(windows)]
        self.send(audio_engine::Command::ServiceSpeed(real));
    }

    /// Extra progress (ms) the service made since `since_wall_ms` by playing
    /// faster than normal. Add it to positions it reported at that time.
    pub fn extra_ms_since(&self, since_wall_ms: f64) -> f64 {
        self.fill.lock().map_or(0.0, |f| {
            f.extra_ms_since(since_wall_ms, crate::hub::wall_ms(Instant::now()))
        })
    }

    /// The service ran faster than normal within `window` (its position
    /// reports can't be compared with the usual clock then).
    pub fn fast_within(&self, window: Duration) -> bool {
        self.fill
            .lock()
            .is_ok_and(|f| f.real > 1.0 || f.last_fast.is_some_and(|t| t.elapsed() < window))
    }

    /// Returns the engine to stop, if it should no longer run.
    #[cfg(windows)]
    fn sync(&self, inner: &mut Inner) -> Option<Engine> {
        use audio_engine::Command;
        let config = inner.config;
        match (config.on, inner.window_pid) {
            (true, Some(pid)) => {
                match &inner.engine {
                    Some(engine) => {
                        engine.send(Command::Eq(config.eq));
                        engine.send(Command::Blend(config.blend));
                    }
                    None => {
                        let app = self.app.clone();
                        let notify = Box::new(move || crate::hub::reemit_playback(&app));
                        match Engine::start(pid, config.eq, config.blend, inner.gain, notify) {
                            Ok(engine) => inner.engine = Some(engine),
                            Err(e) => tracing::error!("sound effects could not start: {e}"),
                        }
                    }
                }
                None
            }
            // Dropping the engine stops it and restores the window's volume.
            _ => inner.engine.take(),
        }
    }

    #[cfg(not(windows))]
    fn sync(&self, inner: &mut Inner) -> Option<Engine> {
        let _ = &self.app;
        inner.engine.take()
    }

    /// The delay to apply to positions, when the engine is delaying the sound.
    pub fn timing(&self) -> Option<Timing> {
        #[cfg(windows)]
        {
            let inner = self.inner.lock().ok()?;
            let status = inner.engine.as_ref()?.status();
            let delay_ms = status.latency_ms() as f64;
            // The plain route adds ~40 ms: not worth shifting anything for.
            // But from the moment the delay starts filling, positions must
            // follow it, however small it still is.
            let late = delay_ms >= 60.0 || status.speed() < 0.999;
            (status.running() && late).then(|| Timing {
                delay_ms,
                speed: status.speed(),
            })
        }
        #[cfg(not(windows))]
        None
    }

    /// For the interface: is the engine processing sound, and how late is it?
    pub fn status(&self) -> SoundStatus {
        #[cfg(windows)]
        if let Ok(inner) = self.inner.lock()
            && let Some(engine) = &inner.engine
        {
            let status = engine.status();
            return SoundStatus {
                running: status.running(),
                delay_ms: status.latency_ms(),
                refilling: status.speed() < 0.999,
                blending: status.blending(),
            };
        }
        SoundStatus::default()
    }

    fn blend_secs(&self) -> Option<f32> {
        self.inner.lock().ok()?.config.blend.fade_secs()
    }

    /// Automix pressed Next itself a moment ago: that song change is a blend,
    /// not a skip by the user.
    pub fn automix_pressed_within(&self, window: Duration) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|i| i.automix_at)
            .is_some_and(|t| t.elapsed() < window)
    }

    /// Automix (UPDATE-PLAN §2 Step D): near the end of a song that still has
    /// a beat, press Next early. The engine's delay buffer holds the song's
    /// last seconds, so they blend, on the beat, into the next song's start.
    ///
    /// ```text
    ///  once a second, while Automix is on and a service window plays:
    ///     song ends in (fade + 2.5 s) or less, it has a beat,
    ///     and the delay holds a whole blend (`delay_ready`)?        ─► press Next
    /// ```
    /// Otherwise the song is left to end by itself (a plain crossfade).
    fn watch_automix(app: AppHandle) {
        let spawned = std::thread::Builder::new()
            .name("glowtune-automix".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let Some(sound) = app.try_state::<Sound>() else {
                        return;
                    };
                    match sound.automix_tick() {
                        Tick::Stop => return,
                        Tick::Wait => {}
                        Tick::PressNext => {
                            let app = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if !crate::services::web_transport(&app, "next").await {
                                    tracing::debug!("automix: the page had no Next button");
                                }
                            });
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("automix watcher: {e}");
        }
    }

    fn automix_tick(&self) -> Tick {
        let Ok(mut inner) = self.inner.lock() else {
            return Tick::Stop;
        };
        let secs = match inner.config.blend {
            Blend::Automix { secs } if inner.engine.is_some() => secs as f64,
            _ => {
                inner.watching = false;
                return Tick::Stop;
            }
        };
        #[cfg(windows)]
        let has_beat = inner
            .engine
            .as_ref()
            .is_some_and(|e| e.status().has_beat() && e.status().running());
        #[cfg(not(windows))]
        let has_beat = false;
        #[cfg(windows)]
        let delay_ms = inner
            .engine
            .as_ref()
            .map_or(0.0, |e| e.status().latency_ms() as f64);
        #[cfg(not(windows))]
        let delay_ms = 0.0;
        let Some(shared) = self.app.try_state::<std::sync::Arc<crate::hub::Shared>>() else {
            return Tick::Wait;
        };
        let Ok(snapshot) = shared.snapshot.lock() else {
            return Tick::Wait;
        };
        let (Some(track), playback) = (&snapshot.track, &snapshot.playback) else {
            return Tick::Wait;
        };
        let Some(duration) = track.duration_ms.filter(|d| *d > 60_000.0) else {
            return Tick::Wait;
        };
        if track.source != "Web player" || playback.status != "playing" {
            return Tick::Wait;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        let position = playback.position_ms
            + (now_ms - playback.reported_at_ms).max(0.0) * playback.rate
            + self.extra_ms_since(playback.reported_at_ms);
        let left = duration - position;
        let song = format!("{}\n{}", track.title, track.artist);
        // Right after a change, the position can still be the old song's:
        // only trust it once this song has been on for a while.
        if inner.seen_song != song {
            inner.seen_song = song.clone();
            inner.seen_since = Some(Instant::now());
        }
        let settled = inner
            .seen_since
            .is_some_and(|t| t.elapsed() > Duration::from_secs(20));
        let in_window = settled
            && position > duration * 0.5
            && left > 3500.0
            && left <= secs * 1000.0 + 2500.0
            && inner.automix_song != song;
        if !in_window {
            return Tick::Wait;
        }
        // Not ready: check again next second, but say why only once per song.
        let first_look = inner.declined_song != song;
        if !has_beat {
            if first_look {
                tracing::info!(
                    "automix: {} has no clear beat now, letting it end by itself",
                    track.title
                );
            }
            inner.declined_song = song;
            return Tick::Wait;
        }
        if !delay_ready(delay_ms, secs) {
            if first_look {
                tracing::info!(
                    "automix: the delay is only {:.1} s (a blend needs {:.1} s), letting {} end by itself",
                    delay_ms / 1000.0,
                    needed_delay_ms(secs) / 1000.0,
                    track.title
                );
            }
            inner.declined_song = song;
            return Tick::Wait;
        }
        inner.automix_song = song;
        inner.automix_at = Some(Instant::now());
        tracing::info!(
            "automix: blending out of {} early ({:.1} s left, delay {:.1} s)",
            track.title,
            left / 1000.0,
            delay_ms / 1000.0
        );
        Tick::PressNext
    }

    #[cfg(windows)]
    fn send(&self, command: audio_engine::Command) {
        if let Ok(inner) = self.inner.lock()
            && let Some(engine) = &inner.engine
        {
            engine.send(command);
        }
    }

    pub fn paused(&self, paused: bool) {
        #[cfg(windows)]
        self.send(audio_engine::Command::Paused(paused));
        #[cfg(not(windows))]
        let _ = paused;
    }

    /// A seek: what is buffered no longer follows what plays.
    pub fn flush(&self) {
        #[cfg(windows)]
        self.send(audio_engine::Command::Flush);
    }

    /// The service moved to another song. Returns how long the rest of the app
    /// should keep showing the old song: with a blend, the new one is only
    /// heard once the delayed sound gets there.
    pub fn track_changed(&self, automatic: bool) -> Option<Duration> {
        let before = self.timing();
        #[cfg(windows)]
        self.send(audio_engine::Command::TrackChanged { automatic });
        let (timing, secs) = (before?, self.blend_secs()?);
        if !automatic {
            return None;
        }
        // The fade uses up to its own length of the delay; the new song is
        // "current" from about the middle of the fade. Automix's DJ handover
        // is 16 beats (~7–14 s) and starts up to a bar before the old song's
        // end: its middle, the drop, is about 6.5 s before that end.
        let automix = self
            .inner
            .lock()
            .is_ok_and(|i| matches!(i.config.blend, Blend::Automix { .. }));
        let secs = if automix { 13.0 } else { secs };
        let fade_ms = (secs as f64 * 1000.0).min(timing.delay_ms - 200.0);
        let fade_ms = if fade_ms < 500.0 { 0.0 } else { fade_ms };
        Some(Duration::from_secs_f64(
            (timing.delay_ms - fade_ms / 2.0).max(0.0) / 1000.0,
        ))
    }

    /// Glowtune itself just sent play / next / previous / seek.
    pub fn note_command(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_command = Some(Instant::now());
        }
    }

    pub fn commanded_within(&self, window: Duration) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|i| i.last_command)
            .is_some_and(|t| t.elapsed() < window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automix_waits_for_a_delay_that_holds_the_whole_blend() {
        // Right after start, a skip or a seek the delay is short: cutting the
        // song early then would be an abrupt cut.
        assert!(!delay_ready(0.0, 6.0));
        assert!(!delay_ready(3000.0, 6.0));
        assert!(!delay_ready(9700.0, 6.0));
        // Fade (6 s) + the next song's first seconds (3.8 s).
        assert!(delay_ready(9800.0, 6.0));
        assert!(delay_ready(12_500.0, 6.0));
        // Fade lengths are clamped like the engine clamps them.
        assert_eq!(needed_delay_ms(0.2), needed_delay_ms(1.0));
        assert_eq!(needed_delay_ms(30.0), needed_delay_ms(12.0));
    }

    #[test]
    fn fast_fill_extra_progress_is_counted_from_the_report_time() {
        let mut f = FillState {
            asked: 1.0,
            real: 1.0,
            ..Default::default()
        };
        // Normal until 10 s, 1.25× from 10 s to 30 s, normal again after.
        f.record(10_000.0, 1.25);
        f.record(30_000.0, 1.0);
        // 20 s at +0.25 = 5 s ahead, for a report from before the fill.
        assert_eq!(f.extra_ms_since(0.0, 40_000.0), 5_000.0);
        // A report from mid-fill only misses the rest of it.
        assert_eq!(f.extra_ms_since(20_000.0, 40_000.0), 2_500.0);
        // A report after the fill needs nothing.
        assert_eq!(f.extra_ms_since(35_000.0, 40_000.0), 0.0);
        // Still filling now: counted up to now.
        f.record(50_000.0, 1.2);
        assert!((f.extra_ms_since(45_000.0, 60_000.0) - 2_000.0).abs() < 1e-6);
    }

    #[test]
    fn fast_fill_history_keeps_what_positions_still_need() {
        let mut f = FillState::default();
        f.record(0.0, 1.25);
        f.record(10_000.0, 1.0);
        // 40 minutes later: the old fill is forgotten, the last speed kept.
        f.record(40.0 * 60_000.0, 1.25);
        assert!(f.history.len() <= 2, "{:?}", f.history);
        assert_eq!(f.history.last().unwrap().1, 1.25);
    }

    #[test]
    fn automix_target_delay_is_enough_for_its_own_check() {
        // The engine fills the delay up to fade + 5 s lookahead + 1.5 s;
        // the check must be reachable well before that.
        for secs in [1.0, 4.0, 6.0, 12.0] {
            let target_ms = (secs + 5.0 + 1.5) * 1000.0;
            assert!(needed_delay_ms(secs) < target_ms - 1000.0, "{secs}");
        }
    }
}
