//! 10-band equalizer, preamp and limiter (interleaved stereo, 32-bit float).
//!
//! ```text
//!  in ─► preamp ─► 10 peaking filters (31 Hz … 16 kHz) ─► limiter ─► out
//! ```
//! Filters are RBJ "cookbook" biquads: a few multiplies per sample, so the
//! whole chain costs well under 1 % of one core.

/// Center frequencies of the ten bands, in Hz.
pub const BANDS: [f32; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Largest boost or cut per band, in dB.
pub const MAX_GAIN_DB: f32 = 12.0;

/// One octave per band.
const Q: f64 = 1.41;

/// Band gains and preamp in dB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqSettings {
    pub enabled: bool,
    pub preamp_db: f32,
    pub gains_db: [f32; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            gains_db: [0.0; 10],
        }
    }
}

/// Built-in presets: `(id, name, preamp dB, band gains dB)`.
pub const PRESETS: [(&str, &str, f32, [f32; 10]); 6] = [
    ("flat", "Flat", 0.0, [0.0; 10]),
    (
        "bass-boost",
        "Bass boost",
        -3.0,
        [6.0, 5.5, 4.5, 2.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "vocal",
        "Vocal",
        -2.0,
        [-2.0, -2.0, -1.0, 0.5, 2.5, 4.0, 4.0, 2.5, 0.5, -0.5],
    ),
    (
        "treble",
        "Treble",
        -3.0,
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 4.0, 5.5, 6.0],
    ),
    (
        "loudness",
        "Loudness",
        -4.0,
        [6.0, 4.5, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 4.0, 5.0],
    ),
    (
        "late-night",
        "Late night",
        0.0,
        [-6.0, -4.5, -2.5, -0.5, 1.0, 2.0, 2.0, 0.5, -2.0, -4.0],
    ),
];

#[derive(Debug, Clone, Copy)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefs {
    const UNITY: Coefs = Coefs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// RBJ peaking filter. Bands at or above Nyquist are left out.
    fn peaking(freq: f64, gain_db: f64, sample_rate: f64) -> Coefs {
        if gain_db.abs() < 0.01 || freq >= sample_rate * 0.49 {
            return Coefs::UNITY;
        }
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = std::f64::consts::TAU * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * Q);
        let a0 = 1.0 + alpha / a;
        Coefs {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * w0.cos() / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// One filter for both channels (transposed direct form II, f64 state so the
/// 31 Hz band stays exact).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    c: Coefs,
    z: [[f64; 2]; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, ch: usize, x: f64) -> f64 {
        let y = self.c.b0 * x + self.z[ch][0];
        self.z[ch][0] = self.c.b1 * x - self.c.a1 * y + self.z[ch][1];
        self.z[ch][1] = self.c.b2 * x - self.c.a2 * y;
        y
    }
}

pub struct Equalizer {
    sample_rate: f64,
    settings: EqSettings,
    filters: [Biquad; 10],
    /// Preamp as a linear factor, moved smoothly toward `preamp_target`.
    preamp: f32,
    preamp_target: f32,
}

impl Equalizer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate as f64,
            settings: EqSettings::default(),
            filters: [Biquad {
                c: Coefs::UNITY,
                z: [[0.0; 2]; 2],
            }; 10],
            preamp: 1.0,
            preamp_target: 1.0,
        }
    }

    pub fn settings(&self) -> EqSettings {
        self.settings
    }

    /// Applies new settings. Filter memory is kept, so changes don't click.
    pub fn set(&mut self, mut settings: EqSettings) {
        for g in &mut settings.gains_db {
            *g = if g.is_finite() {
                g.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
            } else {
                0.0
            };
        }
        settings.preamp_db = if settings.preamp_db.is_finite() {
            settings.preamp_db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
        } else {
            0.0
        };
        for (i, f) in self.filters.iter_mut().enumerate() {
            let gain = if settings.enabled {
                settings.gains_db[i]
            } else {
                0.0
            };
            f.c = Coefs::peaking(BANDS[i] as f64, gain as f64, self.sample_rate);
        }
        self.preamp_target = if settings.enabled {
            10f32.powf(settings.preamp_db / 20.0)
        } else {
            1.0
        };
        self.settings = settings;
    }

    /// Filters interleaved stereo frames in place.
    pub fn process(&mut self, frames: &mut [f32]) {
        let idle = !self.settings.enabled && (self.preamp - 1.0).abs() < 1e-4;
        if idle {
            return;
        }
        for frame in frames.chunks_exact_mut(2) {
            // ~5 ms glide at 48 kHz.
            self.preamp += (self.preamp_target - self.preamp) * 0.004;
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = (*sample * self.preamp) as f64;
                for f in &mut self.filters {
                    x = f.run(ch, x);
                }
                *sample = x as f32;
            }
        }
    }
}

/// Keeps peaks under the ceiling so boosts never clip: instant attack, smooth
/// release, both channels moved together.
pub struct Limiter {
    gain: f32,
    release: f32,
}

impl Limiter {
    const CEILING: f32 = 0.98;

    pub fn new(sample_rate: u32) -> Self {
        Self {
            gain: 1.0,
            // Back to full level in about 150 ms.
            release: 1.0 - (-1.0 / (0.15 * sample_rate as f32)).exp(),
        }
    }

    pub fn process(&mut self, frames: &mut [f32]) {
        for frame in frames.chunks_exact_mut(2) {
            let peak = frame[0].abs().max(frame[1].abs());
            let wanted = if peak > Self::CEILING {
                Self::CEILING / peak
            } else {
                1.0
            };
            if wanted < self.gain {
                self.gain = wanted;
            } else {
                self.gain += (wanted - self.gain) * self.release;
            }
            frame[0] = (frame[0] * self.gain).clamp(-1.0, 1.0);
            frame[1] = (frame[1] * self.gain).clamp(-1.0, 1.0);
        }
    }
}

/// Glowtune's own volume for the service window's sound, applied after the
/// delay so a change is heard at once (the page's own volume is baked into
/// the captured sound, and is only heard once the delay has passed).
///
/// Changes glide over about 25 ms: a jump would click.
pub struct Volume {
    gain: f32,
    target: f32,
    glide: f32,
}

impl Volume {
    /// Up to this much louder than the page plays: Glowtune's slider shows
    /// page volume × this gain, so turning it up from a quiet page needs more than 1.
    pub const MAX: f32 = 20.0;

    pub fn new(sample_rate: u32, gain: f32) -> Self {
        let gain = Self::clean(gain);
        Self {
            gain,
            target: gain,
            glide: 1.0 - (-1.0 / (0.025 * sample_rate as f32 / 4.0)).exp(),
        }
    }

    fn clean(gain: f32) -> f32 {
        if gain.is_finite() {
            gain.clamp(0.0, Self::MAX)
        } else {
            1.0
        }
    }

    pub fn set(&mut self, gain: f32) {
        self.target = Self::clean(gain);
    }

    pub fn process(&mut self, frames: &mut [f32]) {
        if self.gain == self.target && self.gain == 1.0 {
            return;
        }
        for frame in frames.chunks_exact_mut(2) {
            if self.gain != self.target {
                self.gain += (self.target - self.gain) * self.glide;
                if (self.target - self.gain).abs() < 1e-4 {
                    self.gain = self.target;
                }
            }
            frame[0] *= self.gain;
            frame[1] *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_changes_at_once_without_a_click() {
        let mut volume = Volume::new(48000, 1.0);
        let mut steady = vec![0.5f32; 2 * 4800];
        volume.process(&mut steady);
        assert!(
            steady.iter().all(|s| *s == 0.5),
            "full volume leaves the sound alone"
        );

        volume.set(0.2);
        let mut out = vec![0.5f32; 2 * 4800];
        volume.process(&mut out);
        // Quiet within 50 ms (heard at once) ...
        assert!((out[2 * 2400] - 0.1).abs() < 0.002, "{}", out[2 * 2400]);
        // ... but never in one jump: the largest step between samples is tiny.
        let step = out
            .chunks_exact(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.01, "step {step}");

        volume.set(f32::NAN);
        let mut back = vec![0.5f32; 2 * 4800];
        volume.process(&mut back);
        assert!((back[back.len() - 1] - 0.5).abs() < 1e-3);
    }

    fn sine(freq: f32, secs: f32) -> Vec<f32> {
        (0..(48000.0 * secs) as usize)
            .flat_map(|n| {
                let s = (std::f32::consts::TAU * freq * n as f32 / 48000.0).sin() * 0.25;
                [s, s]
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn gain_db_at(eq: &mut Equalizer, freq: f32) -> f32 {
        let input = sine(freq, 1.0);
        let mut out = input.clone();
        eq.process(&mut out);
        // Skip the start, where the filter settles.
        20.0 * (rms(&out[48000..]) / rms(&input[48000..])).log10()
    }

    #[test]
    fn disabled_eq_changes_nothing() {
        let mut eq = Equalizer::new(48000);
        let input = sine(1000.0, 0.1);
        let mut out = input.clone();
        eq.process(&mut out);
        assert_eq!(input, out);
    }

    #[test]
    fn flat_but_enabled_is_transparent() {
        let mut eq = Equalizer::new(48000);
        eq.set(EqSettings {
            enabled: true,
            ..Default::default()
        });
        assert!(gain_db_at(&mut eq, 1000.0).abs() < 0.01);
    }

    #[test]
    fn a_band_boosts_its_own_frequency_only() {
        let mut eq = Equalizer::new(48000);
        let mut gains = [0.0; 10];
        gains[5] = 6.0; // 1 kHz
        eq.set(EqSettings {
            enabled: true,
            preamp_db: 0.0,
            gains_db: gains,
        });
        assert!((gain_db_at(&mut eq, 1000.0) - 6.0).abs() < 0.2);
        assert!(gain_db_at(&mut eq, 62.5).abs() < 0.3);
        assert!(gain_db_at(&mut eq, 16000.0).abs() < 0.3);
    }

    #[test]
    fn preamp_and_cut_work() {
        let mut eq = Equalizer::new(48000);
        let mut gains = [0.0; 10];
        gains[1] = -12.0;
        eq.set(EqSettings {
            enabled: true,
            preamp_db: -6.0,
            gains_db: gains,
        });
        assert!((gain_db_at(&mut eq, 62.5) + 18.0).abs() < 0.3);
        assert!((gain_db_at(&mut eq, 4000.0) + 6.0).abs() < 0.2);
    }

    #[test]
    fn wild_settings_are_clamped() {
        let mut eq = Equalizer::new(48000);
        eq.set(EqSettings {
            enabled: true,
            preamp_db: f32::NAN,
            gains_db: [99.0; 10],
        });
        assert_eq!(eq.settings().preamp_db, 0.0);
        assert_eq!(eq.settings().gains_db, [MAX_GAIN_DB; 10]);
    }

    #[test]
    fn limiter_holds_the_ceiling_and_leaves_quiet_sound_alone() {
        let mut limiter = Limiter::new(48000);
        let mut loud: Vec<f32> = sine(200.0, 0.2).iter().map(|s| s * 8.0).collect();
        limiter.process(&mut loud);
        assert!(loud.iter().all(|s| s.abs() <= 0.981));

        let mut limiter = Limiter::new(48000);
        let quiet = sine(200.0, 0.2);
        let mut out = quiet.clone();
        limiter.process(&mut out);
        assert_eq!(quiet, out);
    }

    #[test]
    fn presets_are_within_range() {
        for (id, _, preamp, gains) in PRESETS {
            assert!(preamp.abs() <= MAX_GAIN_DB, "{id}");
            assert!(gains.iter().all(|g| g.abs() <= MAX_GAIN_DB), "{id}");
        }
    }
}
