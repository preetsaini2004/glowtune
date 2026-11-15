//! Undoes the service window's fast fill (UPDATE-PLAN §2 Step C).
//!
//! ```text
//!  service plays 1.25× with the pitch raised (preservesPitch = false)
//!        │ captured: 1.25 s of song per second, everything 1.25× too high
//!        ▼
//!  SourceRate (speed 1.25): read 0.8 captured frames per frame written
//!        │ = the original song, at its own speed and pitch
//!        ▼
//!  Player's delay buffer grows by 0.25 s every second
//! ```
//! Reading slower only moves frequencies down (everything captured lands below
//! 19.2 kHz at 1.25×), so a short windowed-sinc (Lanczos) interpolator is
//! enough. At speed 1 with no fractional offset the sound passes through
//! untouched.

use std::collections::VecDeque;

use crate::player::Frame;

/// Kernel half-width in frames (Lanczos `a`).
const HALF: usize = 8;
/// Kernel table resolution between two frames.
const PHASES: usize = 512;

pub struct SourceRate {
    /// Captured frames read per frame written (1 / service speed).
    step: f64,
    /// Read position (fractional) into `hist`.
    pos: f64,
    hist: VecDeque<Frame>,
    /// `PHASES + 1` rows of `2 * HALF` weights.
    table: Vec<f32>,
    /// No fill since the start or the last flush: copy straight through.
    /// (After a fill the sound sits a fraction of a frame off; it is then
    /// interpolated at speed 1 until the next flush, which costs little.)
    pristine: bool,
}

impl Default for SourceRate {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRate {
    pub fn new() -> Self {
        let taps = 2 * HALF;
        let mut table = Vec::with_capacity((PHASES + 1) * taps);
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            let row: Vec<f64> = (0..taps)
                .map(|k| lanczos(k as f64 - (HALF as f64 - 1.0) - frac))
                .collect();
            // Normalise each row so a constant passes at exactly its level.
            let sum: f64 = row.iter().sum();
            table.extend(row.iter().map(|w| (w / sum) as f32));
        }
        let mut r = Self {
            step: 1.0,
            pos: 0.0,
            hist: VecDeque::new(),
            table,
            pristine: true,
        };
        r.reset();
        r
    }

    /// How fast the service plays: 1.0 normally, 1.25 during a fast fill.
    pub fn set_speed(&mut self, speed: f64) {
        self.step = 1.0 / speed.clamp(1.0, 2.0);
    }

    pub fn speed(&self) -> f64 {
        1.0 / self.step
    }

    /// Forgets what is buffered (a flush: the next sound starts fresh). Back
    /// at speed 1 this also returns to the exact pass-through.
    pub fn reset(&mut self) {
        self.hist.clear();
        self.hist.extend(std::iter::repeat_n([0.0; 2], HALF));
        self.pos = (HALF - 1) as f64;
        self.pristine = true;
    }

    /// Converts interleaved stereo `input` and appends the result to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.pristine && self.step == 1.0 {
            // No fill since the start: pass straight through (no delay, bit
            // exact), keeping the last frames so a fill can start without a click.
            out.extend_from_slice(input);
            self.hist
                .extend(input.chunks_exact(2).map(|f| [f[0], f[1]]));
            let extra = self.hist.len() - HALF;
            self.hist.drain(..extra);
            return;
        }
        if self.pristine {
            // The last frame in `hist` was already written: continue after it.
            self.pristine = false;
            self.pos = (HALF - 1) as f64 + self.step;
        }
        self.hist
            .extend(input.chunks_exact(2).map(|f| [f[0], f[1]]));
        let taps = 2 * HALF;
        // Kernel tap `i` reads hist[floor(pos) + 1 - HALF + i].
        while self.pos.floor() as usize + HALF < self.hist.len() {
            let base = self.pos.floor() as usize + 1 - HALF;
            let frac = self.pos.fract();
            let x = frac * PHASES as f64;
            let p = (x as usize).min(PHASES - 1);
            let t = (x - p as f64) as f32;
            let r0 = &self.table[p * taps..][..taps];
            let r1 = &self.table[(p + 1) * taps..][..taps];
            let mut acc = [0f32; 2];
            for i in 0..taps {
                let w = r0[i] + (r1[i] - r0[i]) * t;
                let f = self.hist[base + i];
                acc[0] += f[0] * w;
                acc[1] += f[1] * w;
            }
            out.extend_from_slice(&acc);
            self.pos += self.step;
        }
        // Drop frames no kernel will touch again.
        let keep_from = (self.pos.floor() as usize + 1)
            .saturating_sub(HALF)
            .min(self.hist.len());
        self.hist.drain(..keep_from);
        self.pos -= keep_from as f64;
    }
}

fn lanczos(x: f64) -> f64 {
    let a = HALF as f64;
    if x.abs() < 1e-12 {
        1.0
    } else if x.abs() >= a {
        0.0
    } else {
        let px = std::f64::consts::PI * x;
        a * px.sin() * (px / a).sin() / (px * px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn tone(freq: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let s = (std::f64::consts::TAU * freq * n as f64 / SR).sin() as f32 * 0.5;
                [s, -s]
            })
            .collect()
    }

    fn run(r: &mut SourceRate, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for block in input.chunks(960) {
            r.process(block, &mut out);
        }
        out
    }

    /// Amplitude of `freq` in the left channel (single-bin DFT).
    fn amplitude(x: &[f32], freq: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        let n = x.len() / 2;
        for i in 0..n {
            let ph = std::f64::consts::TAU * freq * i as f64 / SR;
            re += x[2 * i] as f64 * ph.cos();
            im += x[2 * i] as f64 * ph.sin();
        }
        2.0 * (re * re + im * im).sqrt() / n as f64
    }

    #[test]
    fn speed_one_is_an_exact_copy() {
        let mut r = SourceRate::new();
        let input = tone(440.0, 48000);
        assert_eq!(run(&mut r, &input), input);
    }

    #[test]
    fn fast_fill_is_undone_to_the_original_pitch_and_length() {
        // The service played a 1 kHz note 1.25× fast: it was captured at 1250 Hz.
        // One second of capture holds 1.25 s of song: the buffer grows by 0.25 s.
        let mut r = SourceRate::new();
        r.set_speed(1.25);
        let out = run(&mut r, &tone(1250.0, 48000));
        let frames = out.len() / 2;
        assert!((frames as i64 - 60000).abs() < 20, "{frames} frames");
        let mid = &out[4800..out.len() - 4800];
        assert!(
            amplitude(mid, 1000.0) > 0.49,
            "level {}",
            amplitude(mid, 1000.0)
        );
        assert!(amplitude(mid, 1250.0) < 0.002, "pitch not undone");
        // Clean: almost all the energy is the one note.
        let rms = (mid.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt();
        let pure = 0.5 / 2f64.sqrt();
        assert!((rms - pure).abs() < 0.002, "rms {rms}");
        // Stereo stays exact (right = −left).
        assert!(out.chunks_exact(2).all(|f| (f[0] + f[1]).abs() < 1e-5));
    }

    #[test]
    fn treble_survives_the_conversion() {
        // 12 kHz played 1.25× fast = 15 kHz captured.
        let mut r = SourceRate::new();
        r.set_speed(1.25);
        let out = run(&mut r, &tone(15000.0, 48000));
        let mid = &out[4800..out.len() - 4800];
        assert!(
            amplitude(mid, 12000.0) > 0.48,
            "{}",
            amplitude(mid, 12000.0)
        );
    }

    #[test]
    fn ramping_the_speed_has_no_click_and_returns_to_pass_through() {
        let mut r = SourceRate::new();
        let mut out = Vec::new();
        // A low note so a 1.25× capture still changes slowly per frame.
        let input = tone(220.0, 96000);
        for (i, block) in input.chunks(960).enumerate() {
            // 1.0 → 1.25 → 1.0 in 0.01 steps, like the page ramp.
            let step = (i as f64 - 10.0).clamp(0.0, 25.0) - (i as f64 - 60.0).clamp(0.0, 25.0);
            r.set_speed(1.0 + step * 0.01);
            r.process(block, &mut out);
        }
        let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
        let jump = left
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        // A 220 Hz sine at 0.5 changes at most 2π·220/48000·0.5 ≈ 0.0144 per frame.
        assert!(jump < 0.016, "jump {jump}");
        // After the ramp: the plain note at its level, nothing else.
        let tail = &out[out.len() - 19200..];
        assert!(amplitude(tail, 220.0) > 0.495, "{}", amplitude(tail, 220.0));
        // A flush returns to the exact pass-through.
        r.reset();
        let again = tone(440.0, 4800);
        assert_eq!(run(&mut r, &again), again);
    }
}
