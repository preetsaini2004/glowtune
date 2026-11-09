//! Beat / energy detection for the glow.
//!
//! ```text
//!  mono samples ─► 1024-pt Hann FFT (every ~1/60 s) ─┬─► kick band 40–130 Hz power ─► adaptive level 0..1
//!                                                     └─► spectral flux 30–200 Hz ──► onset ("kick!")
//!                                                                                        │
//!                              energy = 0.55·level^1.4  +  kick impulse  ◄───────────────┘
//! ```
//!
//! "Adaptive" = levels are judged against the last few seconds, so a quiet
//! verse after a loud chorus drifts low and the next drop hits hard, at any
//! system volume. Hot loop does no allocations.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const FFT_SIZE: usize = 1024;
/// Analyses per second (PLAN.md section 9: ~60/s max).
const RATE_HZ: f32 = 60.0;

/// One analysis result.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BeatFrame {
    /// What the glow should show, 0..1: bass level plus kick impulses.
    pub energy: f32,
    /// Adaptive bass level 0..1 (without kick impulses).
    pub level: f32,
    /// True on the frame a kick/onset was detected.
    pub onset: bool,
}

pub struct BeatDetector {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Last FFT_SIZE samples (ring), plus write position.
    history: Vec<f32>,
    pos: usize,
    /// Samples since the last analysis.
    since_hop: usize,
    hop: usize,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Log magnitudes of the onset band from the previous frame.
    prev_mags: Vec<f32>,
    kick_bins: (usize, usize),
    flux_bins: (usize, usize),

    // Adaptive statistics (per-frame exponential averages).
    peak: f32,
    floor: f32,
    flux_mean: f32,
    flux_var: f32,
    frames_since_onset: u32,
    frames: u64,
    last: BeatFrame,
}

impl BeatDetector {
    pub fn new(sample_rate: u32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let sr = sample_rate.max(8000) as f32;
        let bin = |hz: f32| ((hz * FFT_SIZE as f32 / sr).round() as usize).clamp(1, FFT_SIZE / 2);
        let kick_bins = (bin(40.0), bin(130.0).max(bin(40.0) + 1));
        let flux_bins = (bin(30.0), bin(200.0).max(bin(30.0) + 1));
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        Self {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            history: vec![0.0; FFT_SIZE],
            pos: 0,
            since_hop: 0,
            hop: ((sr / RATE_HZ) as usize).max(64),
            prev_mags: vec![0.0; flux_bins.1 - flux_bins.0],
            kick_bins,
            flux_bins,
            peak: 0.0,
            floor: 0.0,
            flux_mean: 0.0,
            flux_var: 0.0,
            frames_since_onset: u32::MAX / 2,
            frames: 0,
            last: BeatFrame::default(),
        }
    }

    /// Feeds mono samples (any count). Returns the newest analysis if at least
    /// one was due, else `None`.
    pub fn push(&mut self, samples: &[f32]) -> Option<BeatFrame> {
        let mut out = None;
        for &s in samples {
            // Real audio is within ±1; clamp so garbage can't overflow into inf/NaN.
            self.history[self.pos] = if s.is_finite() {
                s.clamp(-4.0, 4.0)
            } else {
                0.0
            };
            self.pos = (self.pos + 1) % FFT_SIZE;
            self.since_hop += 1;
            if self.since_hop >= self.hop {
                self.since_hop = 0;
                out = Some(self.analyze());
            }
        }
        out
    }

    /// Resets all adaptive state (e.g. after a track change or a long pause).
    pub fn reset(&mut self) {
        let sr_hop = self.hop;
        self.history.fill(0.0);
        self.prev_mags.fill(0.0);
        self.peak = 0.0;
        self.floor = 0.0;
        self.flux_mean = 0.0;
        self.flux_var = 0.0;
        self.frames_since_onset = u32::MAX / 2;
        self.frames = 0;
        self.since_hop = 0;
        self.hop = sr_hop;
        self.last = BeatFrame::default();
    }

    fn analyze(&mut self) -> BeatFrame {
        self.frames += 1;
        self.frames_since_onset = self.frames_since_onset.saturating_add(1);

        // Unroll the ring (oldest first) into the windowed FFT input.
        let mut sum_sq = 0.0;
        for i in 0..FFT_SIZE {
            let s = self.history[(self.pos + i) % FFT_SIZE];
            sum_sq += s * s;
            self.input[i] = s * self.window[i];
        }
        let rms = (sum_sq / FFT_SIZE as f32).sqrt();
        if rms < 1e-4 {
            // Silence: no light pulse, and don't let silence drag the stats down.
            self.prev_mags.fill(0.0);
            self.last = BeatFrame::default();
            return self.last;
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return self.last;
        }

        // ── Bass level ─────────────────────────────────────────────
        let (k0, k1) = self.kick_bins;
        let power: f32 = self.spectrum[k0..k1]
            .iter()
            .map(|c| c.norm_sqr())
            .sum::<f32>()
            / (k1 - k0) as f32;
        let bass = (1.0 + power * 50.0).ln();

        // Peak decays slowly (~4 s half-life), floor follows the quiet parts.
        let half_life = |secs: f32| 0.5f32.powf(1.0 / (secs * RATE_HZ));
        self.peak = bass.max(self.peak * half_life(4.0));
        let floor_rate = if bass < self.floor {
            0.15
        } else {
            1.0 - half_life(3.0)
        };
        self.floor += (bass - self.floor) * floor_rate;
        let range = (self.peak - self.floor * 0.7).max(1e-3);
        let level = ((bass - self.floor * 0.7) / range).clamp(0.0, 1.0);

        // ── Onsets: spectral flux in the low band ──────────────────────
        let (f0, f1) = self.flux_bins;
        let mut flux = 0.0;
        for (prev, c) in self.prev_mags.iter_mut().zip(&self.spectrum[f0..f1]) {
            let m = (1.0 + c.norm() * 10.0).ln();
            flux += (m - *prev).max(0.0);
            *prev = m;
        }
        let a = 1.0 - half_life(1.5);
        let diff = flux - self.flux_mean;
        self.flux_mean += a * diff;
        self.flux_var = (1.0 - a) * (self.flux_var + a * diff * diff);
        let std = self.flux_var.sqrt();
        let threshold = self.flux_mean + 1.6 * std + 0.05;
        // Need ~0.5 s of history before trusting stats; then max ~8 kicks/s.
        let onset = self.frames > 30 && flux > threshold && self.frames_since_onset >= 7;
        let mut kick = 0.0;
        if onset {
            self.frames_since_onset = 0;
            kick = ((flux - threshold) / (std + 0.05)).clamp(0.3, 1.0) * (0.5 + 0.5 * level);
        }

        let energy = (0.55 * level.powf(1.4) + kick).clamp(0.0, 1.0);
        if !energy.is_finite() {
            self.reset();
            return self.last;
        }
        self.last = BeatFrame {
            energy,
            level,
            onset,
        };
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// 120 BPM four-on-the-floor kick (decaying 55 Hz thump) + quiet hiss.
    fn kick_track(seconds: f32, gain: f32) -> Vec<f32> {
        let n = (seconds * SR as f32) as usize;
        let mut noise = 12345u32;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let tb = t % 0.5; // beat every 0.5 s
                let kick = (std::f32::consts::TAU * 55.0 * tb).sin() * (-tb * 18.0).exp();
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                gain * (0.8 * kick + 0.02 * hiss)
            })
            .collect()
    }

    fn run(det: &mut BeatDetector, samples: &[f32]) -> Vec<BeatFrame> {
        samples.chunks(480).filter_map(|c| det.push(c)).collect()
    }

    #[test]
    fn detects_kicks_at_120_bpm() {
        let mut det = BeatDetector::new(SR);
        let frames = run(&mut det, &kick_track(8.0, 0.5));
        // Ignore the warm-up second; expect ~2 kicks/s over the remaining 7 s.
        let onsets = frames[60..].iter().filter(|f| f.onset).count();
        assert!((12..=16).contains(&onsets), "onsets: {onsets}");
    }

    #[test]
    fn volume_independent() {
        for gain in [0.05, 0.9] {
            let mut det = BeatDetector::new(SR);
            let frames = run(&mut det, &kick_track(6.0, gain));
            let peak = frames[60..].iter().map(|f| f.energy).fold(0.0, f32::max);
            assert!(peak > 0.6, "gain {gain}: peak energy {peak}");
        }
    }

    #[test]
    fn energy_dips_between_kicks() {
        let mut det = BeatDetector::new(SR);
        let frames = run(&mut det, &kick_track(6.0, 0.5));
        let tail = &frames[120..];
        let max = tail.iter().map(|f| f.energy).fold(0.0, f32::max);
        let min = tail.iter().map(|f| f.energy).fold(1.0, f32::min);
        assert!(max - min > 0.5, "not pulsing: {min}..{max}");
    }

    #[test]
    fn silence_is_dark() {
        let mut det = BeatDetector::new(SR);
        let frames = run(&mut det, &vec![0.0; SR as usize * 2]);
        assert!(frames.iter().all(|f| f.energy == 0.0 && !f.onset));
    }

    #[test]
    fn steady_tone_does_not_retrigger() {
        let mut det = BeatDetector::new(SR);
        let tone: Vec<f32> = (0..SR as usize * 4)
            .map(|i| 0.4 * (std::f32::consts::TAU * 80.0 * i as f32 / SR as f32).sin())
            .collect();
        let frames = run(&mut det, &tone);
        let onsets = frames[60..].iter().filter(|f| f.onset).count();
        assert!(onsets <= 1, "steady tone produced {onsets} onsets");
    }

    #[test]
    fn analysis_rate_is_about_60_hz() {
        let mut det = BeatDetector::new(SR);
        let frames = run(&mut det, &kick_track(2.0, 0.5));
        assert!(
            (115..=125).contains(&frames.len()),
            "{} frames in 2 s",
            frames.len()
        );
    }

    #[test]
    fn garbage_input_is_safe() {
        let mut det = BeatDetector::new(SR);
        let junk = [f32::NAN, f32::INFINITY, -f32::INFINITY, 1e30, -1e30];
        for _ in 0..2000 {
            if let Some(f) = det.push(&junk) {
                assert!(f.energy.is_finite() && (0.0..=1.0).contains(&f.energy));
            }
        }
    }
}
