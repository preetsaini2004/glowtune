//! Where the singing is: lines the lyrics up with the sound (no AI).
//!
//! ```text
//!  song sound ──► mid (L+R)/2 and side (L−R)/2, each 250 Hz … 3.5 kHz
//!             ──► every 20 ms: how much louder the middle is than the sides
//!                 (a voice is mixed in the middle, most instruments are wide)
//!             ──► VoiceTrace
//!
//!  lyrics lines ──► "someone sings here" template (each line until the next,
//!                   at most 5 s; a blank line ends it)
//!
//!  slide the template over the trace (± 12 s) ──► the best match is the
//!  version difference, if it stands out clearly; otherwise: no answer
//! ```
//! The lyrics come from LRCLIB and are timed against one version of a song;
//! the service may play another (a music video with a longer intro). Then
//! every line is off by the same amount, and that amount is what `align` finds.

use crate::dj::Biquad;

/// One value per this much sound.
pub const HOP_MS: f64 = 20.0;

/// Measures the voice-band energy in the middle and at the sides of the mix.
pub struct VoiceMeter {
    hop: usize,
    high: Biquad,
    low: Biquad,
    at: usize,
    mid: f64,
    side: f64,
}

/// Energy (mean square) of the voice band in one hop.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hop {
    pub mid: f32,
    pub side: f32,
}

impl Hop {
    /// How much the middle stands out (log ratio, with a floor so silence
    /// reads as "no voice", not as noise).
    pub fn voice(self) -> f32 {
        const FLOOR: f32 = 1e-6;
        ((self.mid + FLOOR) / (self.side + FLOOR)).ln()
            * (self.mid / (self.mid + 4.0 * FLOOR)).min(1.0)
    }
}

impl VoiceMeter {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let mut high = Biquad::new();
        high.set(true, 250.0, sr);
        let mut low = Biquad::new();
        low.set(false, 3500.0, sr);
        Self {
            hop: (sample_rate as f64 * HOP_MS / 1000.0) as usize,
            high,
            low,
            at: 0,
            mid: 0.0,
            side: 0.0,
        }
    }

    /// Feeds interleaved stereo sound; calls `out` once per finished hop.
    pub fn process(&mut self, frames: &[f32], mut out: impl FnMut(Hop)) {
        for f in frames.chunks_exact(2) {
            let ms = [(f[0] + f[1]) * 0.5, (f[0] - f[1]) * 0.5];
            let [m, s] = self.low.process(self.high.process(ms));
            self.mid += (m * m) as f64;
            self.side += (s * s) as f64;
            self.at += 1;
            if self.at == self.hop {
                let n = self.hop as f64;
                out(Hop {
                    mid: (self.mid / n) as f32,
                    side: (self.side / n) as f32,
                });
                self.at = 0;
                self.mid = 0.0;
                self.side = 0.0;
            }
        }
    }
}

/// A lyrics line as `align` needs it.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub time_ms: f64,
    /// Blank lines mark instrumental parts: nobody sings from there.
    pub sung: bool,
}

/// What `align` found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// The singing is this much later in the sound than in the lyrics
    /// (show every line this much later).
    pub lag_ms: f64,
    /// Correlation at the best lag.
    pub peak: f32,
    /// Best correlation more than 1.5 s away from it.
    pub runner_up: f32,
    /// Correlation with no shift at all.
    pub at_zero: f32,
}

/// Longest stretch one line counts as sung.
const LINE_MAX_MS: f64 = 5000.0;

/// "Sung" (1) or not (0) at song time `t` by the lyrics.
fn sung_at(lines: &[Line], t: f64) -> f32 {
    let i = lines.partition_point(|l| l.time_ms <= t);
    if i == 0 {
        return 0.0;
    }
    let line = lines[i - 1];
    let end = lines
        .get(i)
        .map_or(line.time_ms + LINE_MAX_MS, |n| n.time_ms)
        .min(line.time_ms + LINE_MAX_MS);
    (line.sung && t < end) as u8 as f32
}

/// A line start counts as "here" this close to its time.
const ONSET_MS: f64 = 150.0;

/// 1 near a sung line's start, else 0.
fn onset_at(lines: &[Line], t: f64) -> f32 {
    let i = lines.partition_point(|l| l.time_ms < t - ONSET_MS);
    lines
        .get(i)
        .is_some_and(|l| l.sung && (l.time_ms - t).abs() <= ONSET_MS) as u8 as f32
}

/// Pearson correlation of `z` (already zero mean, unit spread) with `template`.
fn correlation(z: &[f32], template: &[f32]) -> Option<f32> {
    let (tm, tsd) = mean_sd(template);
    if tsd <= 1e-6 {
        return None;
    }
    let r = z
        .iter()
        .zip(template)
        .map(|(a, t)| a * (t - tm) / tsd)
        .sum::<f32>()
        / z.len() as f32;
    Some(r)
}

fn standardise(x: &[f32]) -> Option<Vec<f32>> {
    let (mean, sd) = mean_sd(x);
    (sd > 1e-6).then(|| x.iter().map(|v| (v - mean) / sd).collect())
}

/// Slides the lyrics over the voice trace.
///
/// `trace[k]` is the sound at song time `start_ms + k·HOP_MS`; `lines` are
/// sorted by time. Lags from −`max_lag_ms` to +`max_lag_ms` in 50 ms steps.
///
/// Two views are scored together: *is someone singing* (each line until the
/// next) and *does a line start here* (the voice rising). The first finds the
/// rough place, the second pins it down: a lyrics file marks when each line
/// starts, not how long it is held.
///
/// `None` when the window holds too little singing to say anything.
pub fn align(trace: &[f32], start_ms: f64, lines: &[Line], max_lag_ms: f64) -> Option<Fit> {
    if trace.len() < 200 {
        return None;
    }
    // Smooth over ~300 ms: syllables blur into "singing now".
    let smooth = moving_average(trace, 15);
    let level = standardise(&smooth)?;
    // The voice rising over ~200 ms.
    let rise: Vec<f32> = (0..smooth.len())
        .map(|k| {
            let a = smooth[k.saturating_sub(5)];
            let b = smooth[(k + 5).min(smooth.len() - 1)];
            (b - a).max(0.0)
        })
        .collect();
    let rise = standardise(&rise)?;
    let time = |k: usize, lag: f64| start_ms + k as f64 * HOP_MS - lag;
    let score = |lag: f64| -> Option<f32> {
        let sung: Vec<f32> = (0..level.len())
            .map(|k| sung_at(lines, time(k, lag)))
            .collect();
        // Mostly sung or mostly silent: nothing to line up against.
        let share = sung.iter().sum::<f32>() / sung.len() as f32;
        if !(0.1..=0.9).contains(&share) {
            return None;
        }
        let starts = lines
            .iter()
            .filter(|l| l.sung && (time(0, lag)..time(level.len() - 1, lag)).contains(&l.time_ms))
            .count();
        if starts < 3 {
            return None;
        }
        let onsets: Vec<f32> = (0..rise.len())
            .map(|k| onset_at(lines, time(k, lag)))
            .collect();
        Some((correlation(&level, &sung)? + correlation(&rise, &onsets)?) / 2.0)
    };
    let steps = (max_lag_ms / 50.0).round() as i64;
    let scores: Vec<(f64, f32)> = (-steps..=steps)
        .filter_map(|s| {
            let lag = s as f64 * 50.0;
            score(lag).map(|r| (lag, r))
        })
        .collect();
    let &(lag_ms, peak) = scores.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let runner_up = scores
        .iter()
        .filter(|(l, _)| (l - lag_ms).abs() > 1500.0)
        .map(|s| s.1)
        .fold(f32::NEG_INFINITY, f32::max);
    let at_zero = score(0.0).unwrap_or(0.0);
    Some(Fit {
        lag_ms,
        peak,
        runner_up,
        at_zero,
    })
}

fn moving_average(x: &[f32], half: usize) -> Vec<f32> {
    let mut prefix = Vec::with_capacity(x.len() + 1);
    prefix.push(0.0f64);
    for v in x {
        prefix.push(prefix.last().unwrap() + *v as f64);
    }
    (0..x.len())
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(x.len());
            ((prefix[b] - prefix[a]) / (b - a) as f64) as f32
        })
        .collect()
}

fn mean_sd(x: &[f32]) -> (f32, f32) {
    let n = x.len().max(1) as f32;
    let mean = x.iter().sum::<f32>() / n;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
    (mean, var.sqrt())
}

/// Where the voice first comes in, found in a trace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    /// Song time of the entry (ms).
    pub at_ms: f64,
    /// How much clearer the voice is in the 3 s after than in the 3 s before
    /// (in `Hop::voice` units). A real entry over a quiet intro is 2 or more;
    /// a busy intro with the band in the middle is well under 1.
    pub step: f32,
}

/// Seconds compared on each side of an entry.
const ENTRY_SIDE_HOPS: usize = 150;

/// The first place in `trace` (song time `start_ms + k·HOP_MS`) between
/// `from_ms` and `to_ms` where the voice comes in and stays: the 3 s after are
/// clearer than the 3 s before by at least `min_step`. The *first* such rise,
/// not the biggest (a loud chorus must not win over a quieter first verse).
pub fn first_entry(
    trace: &[f32],
    start_ms: f64,
    from_ms: f64,
    to_ms: f64,
    min_step: f32,
) -> Option<Entry> {
    let smooth = moving_average(trace, 5);
    let hop_at = |ms: f64| ((ms - start_ms) / HOP_MS).round();
    let lo = hop_at(from_ms).max(ENTRY_SIDE_HOPS as f64) as usize;
    let hi = (hop_at(to_ms).max(0.0) as usize).min(smooth.len().saturating_sub(ENTRY_SIDE_HOPS));
    if lo >= hi {
        return None;
    }
    let mut prefix = Vec::with_capacity(smooth.len() + 1);
    prefix.push(0.0f64);
    for v in &smooth {
        prefix.push(prefix.last().unwrap() + *v as f64);
    }
    let mean = |a: usize, b: usize| ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
    let step_at = |k: usize| mean(k, k + ENTRY_SIDE_HOPS) - mean(k - ENTRY_SIDE_HOPS, k);
    let first = (lo..hi).find(|&k| step_at(k) >= min_step)?;
    // The step keeps growing until the window edge sits on the rise: up to
    // one window (3 s) after it first counts. Its peak is the entry.
    let (k, step) = (first..(first + ENTRY_SIDE_HOPS).min(hi))
        .map(|k| (k, step_at(k)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    // Sharpen: the rise itself is where the voice crosses halfway.
    let before = mean(k - ENTRY_SIDE_HOPS, k);
    let after = mean(k, k + ENTRY_SIDE_HOPS);
    let half = (before + after) / 2.0;
    let from = k.saturating_sub(25);
    let at = (from..(k + 25).min(smooth.len()))
        .find(|&i| smooth[i] >= half)
        .unwrap_or(k);
    Some(Entry {
        at_ms: start_ms + at as f64 * HOP_MS,
        step,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A song where a centred "voice" (a tone) sings each line for 2 s over a
    /// wide "band" (different noise left and right).
    fn song(lines: &[f64], secs: f64, sr: u32) -> Vec<f32> {
        let mut seed = 7u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let n = (secs * sr as f64) as usize;
        let mut out = Vec::with_capacity(2 * n);
        for i in 0..n {
            let t = i as f64 / sr as f64 * 1000.0;
            // Sung until just before the next line (at most 4 s), as songs go.
            let singing = lines.iter().enumerate().any(|(k, &l)| {
                let end = lines
                    .get(k + 1)
                    .map_or(l + 4000.0, |n| n - 300.0)
                    .min(l + 4000.0);
                t >= l && t < end
            });
            let v = if singing {
                (std::f64::consts::TAU * 440.0 * i as f64 / sr as f64).sin() as f32 * 0.2
            } else {
                0.0
            };
            out.push(v + noise() * 0.1);
            out.push(v + noise() * 0.1);
        }
        out
    }

    fn trace(sound: &[f32], sr: u32) -> Vec<f32> {
        let mut meter = VoiceMeter::new(sr);
        let mut t = Vec::new();
        meter.process(sound, |h| t.push(h.voice()));
        t
    }

    #[test]
    fn finds_how_much_later_the_singing_is_than_the_lyrics() {
        let sr = 48000;
        let sung = [8000.0, 11000.0, 13500.0, 19000.0, 22000.0, 27500.0, 30000.0];
        let t = trace(&song(&sung, 40.0, sr), sr);
        for shift in [-5000.0, 0.0, 3000.0] {
            // The lyrics say each line comes `shift` earlier than it does.
            let lines: Vec<Line> = sung
                .iter()
                .map(|&s| Line {
                    time_ms: s - shift,
                    sung: true,
                })
                .collect();
            let fit = align(&t, 0.0, &lines, 12_000.0).unwrap();
            assert!((fit.lag_ms - shift).abs() <= 100.0, "{shift}: {fit:?}");
            assert!(fit.peak > fit.runner_up + 0.1, "{shift}: {fit:?}");
        }
    }

    /// A wide "band" throughout, a centred voice from `from_s` on (louder
    /// from `loud_s`), as the trace `first_entry` reads.
    fn entering(from_s: f64, loud_s: f64, secs: f64, sr: u32) -> Vec<f32> {
        let mut seed = 11u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let n = (secs * sr as f64) as usize;
        let mut out = Vec::with_capacity(2 * n);
        for i in 0..n {
            let t = i as f64 / sr as f64;
            let level = if t >= loud_s {
                0.3
            } else if t >= from_s {
                0.12
            } else {
                0.0
            };
            let v = (std::f64::consts::TAU * 330.0 * t).sin() as f32 * level;
            out.push(v + noise() * 0.1);
            out.push(v + noise() * 0.1);
        }
        trace(&out, sr)
    }

    #[test]
    fn finds_where_the_voice_first_comes_in_not_the_loudest_part() {
        let t = entering(9.0, 20.0, 40.0, 48000);
        let e = first_entry(&t, 0.0, 3000.0, 30_000.0, 1.5).unwrap();
        assert!((e.at_ms - 9000.0).abs() <= 150.0, "{e:?}");
        assert!(e.step >= 1.5, "{e:?}");
    }

    #[test]
    fn a_band_without_a_voice_has_no_entry() {
        let t = entering(f64::INFINITY, f64::INFINITY, 40.0, 48000);
        assert_eq!(first_entry(&t, 0.0, 3000.0, 30_000.0, 1.5), None);
    }
}
