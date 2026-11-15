//! The live audio route (UPDATE-PLAN §2 Step A), Windows only.
//!
//! ```text
//!  service window (WebView2 process tree)
//!     │ plays as usual, but its Windows volume is a whisper (−80 dB)
//!     ▼
//!  process loopback capture ─► ×10 000 ─► Player ─► Equalizer ─► Limiter ─► default output
//!                                          ▲
//!                 commands (EQ, blend, song change, pause, flush)
//! ```
//! Capture, processing and analysis feed a bounded queue. A separate output
//! thread meets the device deadlines without running analysis or COM upkeep.
//! Dropping the [`Engine`] stops it and gives the window its volume back.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasapi::{
    AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta,
};

use crate::eq::{EqSettings, Equalizer, Limiter, Volume};
use crate::player::{Blend, Player};
use crate::sessions::{self, WHISPER};

pub const SAMPLE_RATE: u32 = 48000;

/// A captured block louder than this cannot be whispered sound (which peaks
/// near `WHISPER`): the window's volume was changed under us.
const NOT_WHISPERED: f32 = WHISPER * 10.0;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Eq(EqSettings),
    Blend(Blend),
    /// The service moved to another song (`automatic` = the old one ran out).
    TrackChanged {
        automatic: bool,
    },
    Paused(bool),
    /// Manual seek: drop the buffer.
    Flush,
    /// How fast the service window plays right now: 1.0, or up to 1.25 during
    /// a fast fill. The capture is read back at the song's own speed and pitch.
    ServiceSpeed(f64),
    /// Glowtune's own volume on top of the page's (0..`Volume::MAX`, 1 = as
    /// the page plays). Heard at once, unlike the page's, which is delayed.
    Volume(f32),
}

/// What the rest of the app may want to know, updated by the engine thread.
#[derive(Debug, Default)]
pub struct Status {
    /// How long until sound captured now is heard, in ms.
    latency_ms: AtomicU32,
    /// Playback speed × 1000 (1000, or 960 while the delay refills).
    speed_milli: AtomicU32,
    blending: AtomicBool,
    running: AtomicBool,
    /// Automix: the sound captured in the last seconds has a clear beat.
    beat: AtomicBool,
    /// The delay the blend aims for, in ms (0 with the blend off).
    target_ms: AtomicU32,
    /// Song-time frames captured so far (after the fast fill is undone).
    song_frames: AtomicU64,
    /// The frame of that count being heard right now.
    heard_frames: AtomicU64,
    /// Where the voice is in the recently captured sound (lyrics alignment).
    voice: Mutex<VoiceRing>,
}

/// The voice trace of the last minute of captured sound.
#[derive(Debug, Default, Clone)]
pub struct VoiceRing {
    /// Song-time frame where `hops[0]` starts.
    pub first_frame: u64,
    /// `voice::Hop::voice` every `voice::HOP_MS`.
    pub hops: std::collections::VecDeque<f32>,
}

impl VoiceRing {
    const MAX_HOPS: usize = 3000;

    fn push(&mut self, value: f32, hop_frames: u64) {
        self.hops.push_back(value);
        if self.hops.len() > Self::MAX_HOPS {
            self.hops.pop_front();
            self.first_frame += hop_frames;
        }
    }

    /// Forget everything: what was captured no longer plays (seek, skip).
    fn restart(&mut self, at_frame: u64) {
        self.hops.clear();
        self.first_frame = at_frame;
    }
}

impl Status {
    pub fn target_ms(&self) -> u32 {
        self.target_ms.load(Ordering::Relaxed)
    }

    pub fn latency_ms(&self) -> u32 {
        self.latency_ms.load(Ordering::Relaxed)
    }

    pub fn speed(&self) -> f64 {
        match self.speed_milli.load(Ordering::Relaxed) {
            0 => 1.0,
            s => s as f64 / 1000.0,
        }
    }

    pub fn blending(&self) -> bool {
        self.blending.load(Ordering::Relaxed)
    }

    /// Automix only: does the music playing right now have a clear beat?
    pub fn has_beat(&self) -> bool {
        self.beat.load(Ordering::Relaxed)
    }

    /// True once sound is flowing through the engine.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// The voice trace, and which song-time frame of it is being heard now.
    pub fn voice(&self) -> (VoiceRing, u64) {
        let ring = self.voice.lock().map(|r| r.clone()).unwrap_or_default();
        (ring, self.heard_frames.load(Ordering::Relaxed))
    }
}

/// Called (from the engine thread) when the latency or speed changed in a
/// step: a blend finished, the delay started or stopped refilling, a flush.
pub type Notify = Box<dyn Fn() + Send>;

pub struct Engine {
    tx: Sender<Command>,
    stop: Arc<AtomicBool>,
    status: Arc<Status>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// Starts processing the sound of `browser_pid`'s process tree.
    pub fn start(
        browser_pid: u32,
        eq: EqSettings,
        blend: Blend,
        volume: f32,
        notify: Notify,
    ) -> std::io::Result<Self> {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Status::default());
        let settings = Settings { eq, blend, volume };
        let thread = std::thread::Builder::new()
            .name("glowtune-audio".into())
            .spawn({
                let (stop, status) = (stop.clone(), status.clone());
                move || run(browser_pid, rx, stop, status, settings, notify)
            })?;
        Ok(Self {
            tx,
            stop,
            status,
            thread: Some(thread),
        })
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    pub fn status(&self) -> Arc<Status> {
        self.status.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Kept across output-device restarts (a restart at full volume would be a
/// loud jump after the listener turned Glowtune down).
struct Settings {
    eq: EqSettings,
    blend: Blend,
    volume: f32,
}

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run(
    pid: u32,
    rx: Receiver<Command>,
    stop: Arc<AtomicBool>,
    status: Arc<Status>,
    mut settings: Settings,
    notify: Notify,
) {
    audio_priority("sound effects");
    if initialize_mta().is_err() {
        tracing::error!("sound effects off: COM could not start");
        return;
    }
    // Volumes the window's sessions had before we touched them.
    let remembered = Mutex::new(Vec::new());
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = session(
            pid,
            &rx,
            &stop,
            &status,
            &mut settings,
            &remembered,
            &notify,
        ) {
            tracing::warn!("sound effects restarting: {e}");
        }
        status.running.store(false, Ordering::Relaxed);
        give_back(pid, &remembered);
        // Output device changed or vanished: wait a moment, then try again.
        let wait = Instant::now();
        while !stop.load(Ordering::Relaxed) && wait.elapsed() < Duration::from_millis(700) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    give_back(pid, &remembered);
    status.latency_ms.store(0, Ordering::Relaxed);
    status.speed_milli.store(1000, Ordering::Relaxed);
    notify();
    tracing::info!("sound effects stopped");
}

fn give_back(pid: u32, remembered: &Mutex<Vec<(u32, f32)>>) {
    let tree = sessions::process_tree(pid);
    let remembered = remembered.lock().map(|r| r.clone()).unwrap_or_default();
    if let Err(e) = sessions::restore(&tree, &remembered) {
        tracing::warn!("could not restore the service window's volume: {e}");
    }
}

/// One run on one output device. Returns `Ok` when asked to stop.
fn session(
    pid: u32,
    rx: &Receiver<Command>,
    stop: &AtomicBool,
    status: &Status,
    settings: &mut Settings,
    remembered: &Mutex<Vec<(u32, f32)>>,
    notify: &Notify,
) -> AnyResult<()> {
    let format = WaveFormat::new(32, 32, SampleType::Float, SAMPLE_RATE as usize, 2, None);

    // Capture: only what the service window's processes play.
    let mut capture_client = AudioClient::new_application_loopback_client(pid, true)?;
    capture_client.initialize_client(
        &format,
        Direction::Capture,
        StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        },
        None,
    )?;
    let capture = capture_client.get_audiocaptureclient()?;

    let output = crate::output::Output::start()?;
    let device_id = output.device_id.clone();

    let mut player = Player::new(SAMPLE_RATE);
    player.set_blend(settings.blend);
    let mut eq = Equalizer::new(SAMPLE_RATE);
    eq.set(settings.eq);
    let mut limiter = Limiter::new(SAMPLE_RATE);
    let mut volume = Volume::new(SAMPLE_RATE, settings.volume);
    let mut voice = crate::voice::VoiceMeter::new(SAMPLE_RATE);
    let hop_frames = (SAMPLE_RATE as f64 * crate::voice::HOP_MS / 1000.0) as u64;
    let mut song_frames = status.song_frames.load(Ordering::Relaxed);
    let restart_voice = |at: u64| {
        if let Ok(mut ring) = status.voice.lock() {
            ring.restart(at);
        }
    };
    restart_voice(song_frames);

    capture_client.start_stream()?;
    tracing::info!("sound effects running (window pid {pid})");

    let mut captured = std::collections::VecDeque::<u8>::new();
    let mut block: Vec<f32> = Vec::new();
    let mut unsped: Vec<f32> = Vec::new();
    let mut source_rate = crate::resample::SourceRate::new();
    let mut out: Vec<f32> = Vec::new();
    // The window's volume has been a whisper since this moment.
    let mut whispered_at: Option<Instant> = None;
    let mut last_upkeep = Instant::now() - Duration::from_secs(1);
    let mut last_device_check = Instant::now();
    let mut last_beat_check = Instant::now();
    let mut last = (false, false, 0u32); // (refilling, blending, latency ms)
    let mut last_at = Instant::now();

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = capture_client.stop_stream();
            return Ok(());
        }
        if output.failed() {
            return Err("audio output failed".into());
        }
        if output.space() == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                Command::Eq(e) => {
                    settings.eq = e;
                    eq.set(e);
                }
                Command::Blend(b) => {
                    settings.blend = b;
                    player.set_blend(b);
                }
                Command::TrackChanged { automatic } => {
                    player.track_changed(automatic);
                    if !automatic {
                        output.clear();
                        source_rate.reset();
                        voice = crate::voice::VoiceMeter::new(SAMPLE_RATE);
                        restart_voice(song_frames);
                    }
                }
                Command::Paused(p) => player.set_paused(p),
                Command::Flush => {
                    player.flush();
                    output.clear();
                    source_rate.reset();
                    voice = crate::voice::VoiceMeter::new(SAMPLE_RATE);
                    restart_voice(song_frames);
                }
                Command::ServiceSpeed(speed) => {
                    source_rate.set_speed(speed);
                    player.set_upstream_fill(speed > 1.0);
                }
                Command::Volume(v) => {
                    settings.volume = v;
                    volume.set(v);
                }
            }
        }

        // Keep the window whispered. Check often until it is, then twice a second.
        let every = if whispered_at.is_some() { 500 } else { 100 };
        if last_upkeep.elapsed() >= Duration::from_millis(every) {
            last_upkeep = Instant::now();
            whispered_at = upkeep(pid, remembered, whispered_at);
        }
        if last_device_check.elapsed() >= Duration::from_secs(2) {
            last_device_check = Instant::now();
            let now = DeviceEnumerator::new()?.get_default_device(Direction::Render)?;
            if now.get_id()? != device_id {
                return Err("the default output device changed".into());
            }
        }

        // Capture → Player.
        while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
            capture.read_from_device_to_deque(&mut captured)?;
        }
        if !captured.is_empty() {
            block.clear();
            let mut sample = [0u8; 4];
            for (i, byte) in captured.drain(..).enumerate() {
                sample[i % 4] = byte;
                if i % 4 == 3 {
                    block.push(f32::from_le_bytes(sample));
                }
            }
            let peak = block.iter().fold(0f32, |m, s| m.max(s.abs()));
            let settled = whispered_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(150));
            if peak > NOT_WHISPERED {
                // Full-volume sound: it is already audible directly, so don't
                // play it again (and never amplify it). Re-check the volume now.
                whispered_at = None;
                last_upkeep = Instant::now() - Duration::from_secs(1);
            } else if settled {
                for s in &mut block {
                    *s /= WHISPER;
                }
                unsped.clear();
                source_rate.process(&block, &mut unsped);
                player.push(&unsped);
                if let Ok(mut ring) = status.voice.lock() {
                    voice.process(&unsped, |h| ring.push(h.voice(), hop_frames));
                }
                song_frames += (unsped.len() / 2) as u64;
                status.song_frames.store(song_frames, Ordering::Relaxed);
                status.running.store(true, Ordering::Relaxed);
            }
        }

        // Player → volume → EQ → limiter → output.
        let space = output.space();
        if space > 0 {
            out.clear();
            out.resize(space * 2, 0.0);
            player.pull(&mut out);
            volume.process(&mut out);
            eq.process(&mut out);
            limiter.process(&mut out);
            output.push(&out);
        }

        // Automix: keep an eye on whether the music has a beat (every 2 s).
        if last_beat_check.elapsed() >= Duration::from_secs(2) {
            last_beat_check = Instant::now();
            let beat = matches!(player.blend(), Blend::Automix { .. }) && player.has_beat_now();
            status.beat.store(beat, Ordering::Relaxed);
        }

        // Which captured frame is being heard now (lyrics alignment).
        let ahead = player.latency_frames() as f64 + output.latency() as f64 * player.speed();
        status
            .heard_frames
            .store(song_frames.saturating_sub(ahead as u64), Ordering::Relaxed);

        // Tell the app when the timing changed in a step.
        let latency = ((player.latency_frames() as f64 + output.latency() as f64 * player.speed())
            * 1000.0
            / SAMPLE_RATE as f64) as u32;
        let now = (player.is_refilling(), player.is_blending(), latency);
        // The delay grows steadily while playing slower (refill) or while the
        // service plays faster (fast fill). That is expected, not a step: the
        // app's clock already follows it, so no need to tell it (each notice
        // redraws the windows).
        let drift = last_at.elapsed().as_secs_f64()
            * 1000.0
            * ((1.0 - player.speed()) + (source_rate.speed() - 1.0));
        let jumped = (latency as f64 - (last.2 as f64 + drift)).abs() > 150.0;
        status.latency_ms.store(latency, Ordering::Relaxed);
        status
            .target_ms
            .store(player.target_delay_ms(), Ordering::Relaxed);
        status
            .speed_milli
            .store((player.speed() * 1000.0) as u32, Ordering::Relaxed);
        status.blending.store(now.1, Ordering::Relaxed);
        if now.1 && !last.1 {
            match player.beat_match() {
                Some((a, b)) => {
                    tracing::info!("blending songs on the beat ({a:.1} BPM into {b:.1} BPM)")
                }
                None => tracing::info!("blending songs (crossfade)"),
            }
        }
        if now.0 != last.0 || now.1 != last.1 || jumped {
            last = now;
            last_at = Instant::now();
            notify();
        } else if last_at.elapsed() > Duration::from_secs(2) {
            last = now;
            last_at = Instant::now();
            // While the sound is late, remind the app every 2 s. One missed
            // or mistimed step would otherwise leave lyrics ahead of the
            // sound for the rest of the song.
            if now.0 || latency >= 60 {
                notify();
            }
        }
    }
}

/// Re-applies the whisper. Returns since when it has been in place.
fn upkeep(
    pid: u32,
    remembered: &Mutex<Vec<(u32, f32)>>,
    whispered_at: Option<Instant>,
) -> Option<Instant> {
    let tree = sessions::process_tree(pid);
    let Ok(mut remembered) = remembered.lock() else {
        return None;
    };
    match sessions::whisper(&tree, &mut remembered) {
        // No session yet: nothing plays, nothing to capture.
        Ok((0, _)) => None,
        Ok((_, 0)) => whispered_at.or_else(|| Some(Instant::now())),
        Ok(_) => Some(Instant::now()),
        Err(e) => {
            tracing::warn!("could not set the service window's volume: {e}");
            None
        }
    }
}

/// Asks Windows to schedule the calling thread as audio (MMCSS "Pro Audio",
/// as audio software does), so a game or a build hogging the CPU can no
/// longer starve it into clicks. Lasts for the thread's life.
pub(crate) fn audio_priority(what: &str) {
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    let mut task = 0u32;
    // SAFETY: a constant task name and a valid out pointer.
    match unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task) } {
        Ok(_) => tracing::debug!("{what}: audio priority on"),
        Err(e) => tracing::warn!("{what}: no audio priority ({e}); may click under load"),
    }
}
