//! Edge glow: album palette → overlay, and beat energy while playing.
//!
//! ```text
//!  hub ──art bytes──► Glow::palette_for ──► extract palette (cached)
//!  hub ──colors─────► Shared::refresh_glow_palette ──► Overlay::set_palette
//!  hub ──playing────► Glow::on_playing ─┬─► Overlay::set_playing (fade in / out)
//!                                       └─► beat thread: start / stop capture
//!
//!  beat thread (only while playing):
//!   LoopbackCapture ─samples─► BeatDetector ─energy (60/s)─► Overlay::set_energy
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use analysis::{BeatDetector, GlowPalette, extract_palette};
use audio_capture::LoopbackCapture;
use overlay::{Overlay, OverlayConfig};

const PALETTE_CACHE: usize = 32;
/// How often the beat thread drains captured audio while playing.
const BEAT_POLL: Duration = Duration::from_millis(16);

pub struct Glow {
    overlay: Arc<Overlay>,
    beat: Sender<bool>,
    cache: HashMap<u64, GlowPalette>,
    _beat_thread: std::thread::JoinHandle<()>,
}

impl Glow {
    pub fn start(config: OverlayConfig) -> Result<Self, String> {
        let overlay = Arc::new(Overlay::spawn(config).map_err(|e| format!("glow overlay: {e}"))?);
        let (beat, rx) = mpsc::channel();
        let beat_overlay = overlay.clone();
        let beat_thread = std::thread::Builder::new()
            .name("glowtune-beat".into())
            .spawn(move || beat_loop(rx, beat_overlay))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            overlay,
            beat,
            cache: HashMap::new(),
            _beat_thread: beat_thread,
        })
    }

    /// Computes (or recalls) the palette for this art. Doesn't show it: the
    /// hub decides (a theme may pin its own colors).
    pub fn palette_for(&mut self, key: u64, bytes: &[u8]) -> Option<GlowPalette> {
        if !self.cache.contains_key(&key) {
            let started = Instant::now();
            match extract_palette(bytes) {
                Ok(p) => {
                    tracing::info!("palette {:?} in {:?}", p.colors, started.elapsed());
                    if self.cache.len() >= PALETTE_CACHE {
                        self.cache.clear();
                    }
                    self.cache.insert(key, p);
                }
                Err(e) => {
                    tracing::warn!("album art: {e}");
                    return None;
                }
            }
        }
        Some(self.cache[&key].clone())
    }

    pub fn on_playing(&self, playing: bool) {
        self.overlay.set_playing(playing);
        let _ = self.beat.send(playing);
    }

    /// The overlay handle, for applying settings from other threads.
    pub fn overlay(&self) -> Arc<Overlay> {
        self.overlay.clone()
    }
}

/// Captures system audio only while something plays, and feeds beat energy
/// to the glow. While paused it blocks on the channel: no capture, no CPU.
fn beat_loop(control: Receiver<bool>, overlay: Arc<Overlay>) {
    let mut playing = false;
    loop {
        if !playing {
            match control.recv() {
                Ok(p) => playing = p,
                Err(_) => return,
            }
            continue;
        }

        let mut capture = match LoopbackCapture::start() {
            Ok(c) => c,
            Err(e) => {
                // The glow still works without audio, just without pulsing.
                tracing::warn!("beat detection off: {e}");
                playing = false;
                continue;
            }
        };
        let mut detector = BeatDetector::new(capture.sample_rate());
        let mut buf = Vec::with_capacity(16_384);

        loop {
            match control.try_recv() {
                Ok(p) => playing = p,
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return,
            }
            if !playing {
                overlay.set_energy(0.0);
                break; // drops `capture` → WASAPI stream stops
            }
            std::thread::sleep(BEAT_POLL);
            capture.read(&mut buf);
            // Run every analysis in the chunk and keep the strongest, so a
            // kick that falls between two polls is never lost.
            let peak = buf
                .chunks(256)
                .filter_map(|c| detector.push(c))
                .map(|f| f.energy)
                .reduce(f32::max);
            if let Some(energy) = peak {
                overlay.set_energy(energy);
            }
        }
    }
}
