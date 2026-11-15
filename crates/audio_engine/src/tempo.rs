//! Tempo and beat positions of a few seconds of music (for Automix).
//!
//! Stereo spectral onset strength with local background removal and harmonic
//! autocorrelation. Run on the processing thread, never the device output thread.

use crate::player::Frame;

/// One analysis step: 5 ms at 48 kHz.
const HOP: usize = 240;
const MIN_BPM: f32 = 70.0;
const MAX_BPM: f32 = 170.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    /// Frames from one beat to the next.
    pub period: f64,
    /// Frames from the start of the analysed sound to its first beat.
    pub first_beat: f64,
    /// 0..1: how clearly the beat repeats. Below ~0.3 there is no usable beat.
    pub confidence: f32,
}

impl Tempo {
    pub fn bpm(&self, sample_rate: u32) -> f32 {
        (60.0 * sample_rate as f64 / self.period) as f32
    }
}

/// Estimates the tempo of `frames`. `None` for less than 3 s or silence.
pub fn estimate(frames: &[Frame], sample_rate: u32) -> Option<Tempo> {
    let sr = sample_rate as f32;
    if frames.len() < sample_rate as usize * 3 {
        return None;
    }
    const N: usize = 2048;
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let window: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut previous = vec![0.0f32; N / 2 + 1];
    let mut magnitudes = previous.clone();
    let mut onsets = Vec::with_capacity(frames.len() / HOP);
    for start in (0..frames.len().saturating_sub(N)).step_by(HOP) {
        magnitudes.fill(0.0);
        for ch in [0, 1] {
            for i in 0..N {
                input[i] = frames[start + i][ch] * window[i];
            }
            fft.process_with_scratch(&mut input, &mut spectrum, &mut scratch)
                .ok()?;
            for (m, c) in magnitudes.iter_mut().zip(&spectrum) {
                *m += c.norm();
            }
        }
        let mut onset = 0.0;
        for bin in 2..N / 2 {
            let magnitude = (1.0 + 10.0 * magnitudes[bin]).ln();
            let old = previous[bin - 1..=bin + 1]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
            onset += (magnitude - old).max(0.0);
        }
        for (p, m) in previous.iter_mut().zip(&magnitudes) {
            *p = (1.0 + 10.0 * m).ln();
        }
        onsets.push(onset);
    }
    if let Some(first) = onsets.first_mut() {
        *first = 0.0;
    }
    // Local background removal suppresses sustained vocals and noise.
    let raw = onsets.clone();
    for (i, o) in onsets.iter_mut().enumerate() {
        let from = i.saturating_sub(20);
        let to = (i + 21).min(raw.len());
        let background = raw[from..to].iter().sum::<f32>() / (to - from) as f32;
        *o = (*o - background).max(0.0);
    }
    let mean = onsets.iter().sum::<f32>() / onsets.len() as f32;
    for o in &mut onsets {
        *o -= mean;
    }

    // Which spacing (in hops) repeats best?
    let hops_per_sec = sr / HOP as f32;
    let min_lag = (hops_per_sec * 60.0 / MAX_BPM) as usize;
    let max_lag = ((hops_per_sec * 60.0 / MIN_BPM) as usize).min(onsets.len() / 2);
    let zero: f32 = onsets.iter().map(|o| o * o).sum();
    if zero <= 1e-9 || max_lag <= min_lag {
        return None;
    }
    let corr = |lag: usize| -> f32 {
        let n = onsets.len() - lag;
        let sum: f32 = onsets[..n]
            .iter()
            .zip(&onsets[lag..])
            .map(|(a, b)| a * b)
            .sum();
        // Unbiased: longer lags have fewer terms.
        sum / n as f32 * onsets.len() as f32 / zero
    };
    // A real beat repeats at 1, 2, 3 and 4 beats; noise and one-off
    // accents don't. Averaging those (a comb) is far steadier on real songs
    // than any single spacing.
    let comb = |lag: usize| -> f32 {
        let (mut sum, mut weight) = (0.0, 0.0);
        for k in 1..=4usize {
            if k * lag < onsets.len() * 3 / 4 {
                let w = 1.0 / k as f32;
                sum += corr(k * lag) * w;
                weight += w;
            }
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    };
    // Gently prefer usual tempos (log-normal around 120 BPM, about an octave
    // wide), so half / double time isn't picked for a plain 4/4 beat.
    let prior = |lag: usize| -> f32 {
        let bpm = 60.0 * hops_per_sec / lag as f32;
        let octaves = (bpm / 120.0).log2();
        (-0.5 * (octaves / 0.9).powi(2)).exp()
    };
    let score = |lag: usize| comb(lag) * (0.5 + 0.5 * prior(lag));
    let (mut best_lag, mut best) = (min_lag, f32::MIN);
    for lag in min_lag..=max_lag {
        let s = score(lag);
        if s > best {
            best = s;
            best_lag = lag;
        }
    }
    // Finer than one hop: fit a parabola through the peak and its neighbours.
    let fine = if best_lag > min_lag && best_lag < max_lag {
        let (a, b, c) = (score(best_lag - 1), score(best_lag), score(best_lag + 1));
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-9 {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    let period_hops = best_lag as f32 + fine;
    // Confidence: how far the winning tempo stands out from all the others
    // (in standard deviations). Busy or sparse songs repeat only weakly
    // overall, yet one tempo still clearly wins; noise has no winner.
    let scores: Vec<f32> = (min_lag..=max_lag).map(score).collect();
    let mean_score = scores.iter().sum::<f32>() / scores.len() as f32;
    let spread = (scores.iter().map(|s| (s - mean_score).powi(2)).sum::<f32>()
        / scores.len() as f32)
        .sqrt()
        .max(1e-9);
    let standout = (best - mean_score) / spread;
    // Calibrated on real songs and noise: noise stands out ~2, a beatless
    // intro or a wrong reading ~3-3.7, a real beat 4 and up (0.35 = usable).
    let confidence = ((standout - 2.6) / 4.0).clamp(0.0, 1.0);

    // Which offset collects the most onsets?
    let steps = period_hops.ceil() as usize;
    let (mut best_phase, mut best_sum) = (0usize, f32::MIN);
    for phase in 0..steps {
        let mut sum = 0.0;
        let mut at = phase as f32;
        while (at as usize) < onsets.len() {
            sum += onsets[at as usize];
            at += period_hops;
        }
        if sum > best_sum {
            best_sum = sum;
            best_phase = phase;
        }
    }
    Some(Tempo {
        period: (period_hops * HOP as f32) as f64,
        // `onsets[i]` is the rise into hop i + 1.
        first_beat: (best_phase * HOP + N / 2) as f64,
        confidence,
    })
}

/// Where bar 1 falls, given a beat grid (assumes 4 beats to the bar).
/// Returns the first downbeat (frames into `frames`) and how clearly that
/// beat of the four stood out (0..1).
///
/// ```text
///  each beat ─► kick: low-end rise around the beat
///            ─► change: how different the bass + harmony of this beat is
///                       from the beat before (chords change on bar 1)
///  score of beat position 0..3 = sum of both over every bar ─► the winner
/// ```
pub fn downbeat(frames: &[Frame], sample_rate: u32, tempo: &Tempo) -> Option<(f64, f32)> {
    const N: usize = 4096;
    let sr = sample_rate as f64;
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    // Bins from ~40 Hz to ~1.5 kHz: bass notes and chords, not cymbals.
    let lo = (40.0 * N as f64 / sr) as usize;
    let hi = ((1500.0 * N as f64 / sr) as usize).min(N / 2);
    let kick_hi = ((150.0 * N as f64 / sr) as usize).max(lo + 1);
    let mut kicks = Vec::new();
    let mut changes = Vec::new();
    let mut previous: Option<Vec<f32>> = None;
    let mut at = tempo.first_beat;
    while (at as usize) + N <= frames.len() {
        let start = at as usize;
        for (i, x) in input.iter_mut().enumerate() {
            let f = frames[start + i];
            // Hann window over the beat's first N frames.
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
            *x = (f[0] + f[1]) * 0.5 * w;
        }
        fft.process_with_scratch(&mut input, &mut spectrum, &mut scratch)
            .ok()?;
        let mags: Vec<f32> = spectrum[lo..hi]
            .iter()
            .map(|c| (1.0 + c.norm()).ln())
            .collect();
        kicks.push(mags[..kick_hi - lo].iter().sum::<f32>());
        let change = previous.as_ref().map_or(0.0, |p| {
            // 1 − cosine similarity with the beat before.
            let dot: f32 = p.iter().zip(&mags).map(|(a, b)| a * b).sum();
            let na = p.iter().map(|a| a * a).sum::<f32>().sqrt();
            let nb = mags.iter().map(|b| b * b).sum::<f32>().sqrt();
            1.0 - dot / (na * nb).max(1e-9)
        });
        changes.push(change);
        previous = Some(mags);
        at += tempo.period;
    }
    if kicks.len() < 8 {
        return None;
    }
    // Standardise each cue so neither dominates by scale.
    let z = |v: &[f32]| -> Vec<f32> {
        let m = v.iter().sum::<f32>() / v.len() as f32;
        let s = (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32)
            .sqrt()
            .max(1e-9);
        v.iter().map(|x| (x - m) / s).collect()
    };
    let (kz, cz) = (z(&kicks), z(&changes));
    let mut score = [0f32; 4];
    let mut count = [0f32; 4];
    for k in 1..kz.len() {
        score[k % 4] += kz[k] + cz[k];
        count[k % 4] += 1.0;
    }
    for p in 0..4 {
        score[p] /= count[p].max(1.0);
    }
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|a, b| score[*b].total_cmp(&score[*a]));
    let (best, second) = (score[order[0]], score[order[1]]);
    let clarity = ((best - second) / (best.abs() + second.abs()).max(1e-6)).clamp(0.0, 1.0);
    Some((tempo.first_beat + order[0] as f64 * tempo.period, clarity))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A kick drum (decaying 60 Hz thump) every beat, over quiet noise.
    pub(crate) fn kicks(bpm: f32, secs: f32, first_beat_secs: f32) -> Vec<Frame> {
        let sr = 48000.0;
        let period = 60.0 / bpm;
        let mut seed = 12345u32;
        (0..(sr * secs) as usize)
            .map(|n| {
                let t = n as f32 / sr;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((seed >> 8) as f32 / 8388608.0 - 1.0) * 0.01;
                let since = (t - first_beat_secs).rem_euclid(period);
                let kick = if t >= first_beat_secs {
                    (std::f32::consts::TAU * 60.0 * since).sin() * (-since * 18.0).exp() * 0.6
                } else {
                    0.0
                };
                [kick + noise, kick + noise]
            })
            .collect()
    }

    #[test]
    fn finds_the_tempo_and_first_beat_of_a_kick_pattern() {
        for (bpm, first) in [(120.0, 0.1), (96.0, 0.33), (128.0, 0.0), (140.0, 0.2)] {
            let t = estimate(&kicks(bpm, 6.0, first), 48000).unwrap();
            assert!(
                (t.bpm(48000) - bpm).abs() < 1.0,
                "{bpm} BPM came out as {}",
                t.bpm(48000)
            );
            assert!(t.confidence > 0.5, "confidence {}", t.confidence);
            // The first beat, give or take 15 ms (and whole beats).
            let period = 48000.0 * 60.0 / bpm as f64;
            let error = (t.first_beat - first as f64 * 48000.0).rem_euclid(period);
            let error = error.min(period - error);
            assert!(error < 720.0, "{bpm} BPM: first beat off by {error} frames");
        }
    }

    #[test]
    fn tracks_drums_under_chords_vocal_harmonics_and_syncopation() {
        let sr = 48000;
        let mut song = kicks(110.0, 8.0, 0.17);
        for (n, f) in song.iter_mut().enumerate() {
            let t = n as f32 / sr as f32;
            let chord = [220.0, 277.18, 329.63]
                .into_iter()
                .map(|hz| (std::f32::consts::TAU * hz * t + 0.03 * (t * 9.0).sin()).sin())
                .sum::<f32>()
                * 0.1;
            let vocal = (std::f32::consts::TAU * 410.0 * t + 1.3 * (t * 4.7).sin()).sin()
                * (0.12 + 0.08 * (t * 7.1).sin());
            let since = (t - 0.17 - 60.0 / 110.0 / 2.0).rem_euclid(60.0 / 110.0);
            let hat = (std::f32::consts::TAU * 7300.0 * t).sin() * (-since * 90.0).exp() * 0.12;
            f[0] += chord + vocal + hat;
            f[1] += chord - vocal - hat;
        }
        let tempo = estimate(&song, sr).expect("a beat");
        assert!((tempo.bpm(sr) - 110.0).abs() < 1.0, "{tempo:?}");
        assert!(tempo.confidence >= 0.35, "{tempo:?}");
    }

    #[test]
    fn finds_bar_one_where_the_bass_note_changes() {
        // A kick on every beat (no hint there), but the bass note changes
        // every 4 beats, starting on beat index 2: that is bar 1.
        let (sr, bpm) = (48000u32, 100.0f32);
        let period = 60.0 / bpm;
        let mut song = kicks(bpm, 16.0, 0.0);
        let notes = [55.0f32, 73.4, 61.7, 82.4];
        for (n, f) in song.iter_mut().enumerate() {
            let t = n as f32 / sr as f32;
            let beat = (t / period).floor() as i64;
            let bar = (beat - 2).div_euclid(4).rem_euclid(4) as usize;
            let s = (std::f32::consts::TAU * notes[bar] * t).sin() * 0.25;
            f[0] += s;
            f[1] += s;
        }
        let t = estimate(&song, sr).expect("a beat");
        let (down, clarity) = downbeat(&song, sr, &t).expect("a downbeat");
        let p = t.period;
        let beat_index = ((down / p).round() as i64).rem_euclid(4);
        assert_eq!(
            beat_index, 2,
            "downbeat at {down} frames, clarity {clarity}"
        );
        assert!(clarity > 0.2, "clarity {clarity}");
    }

    #[test]
    fn wide_stereo_drums_are_not_mistaken_for_silence() {
        let mut frames = kicks(110.0, 6.0, 0.1);
        for frame in &mut frames {
            frame[1] = -frame[0];
        }
        let t = estimate(&frames, 48000).expect("stereo percussion must retain its beat");
        assert!((t.bpm(48000) - 110.0).abs() < 1.0);
        assert!(t.confidence > 0.5);
    }

    #[test]
    fn noise_and_silence_have_no_beat() {
        let mut seed = 7u32;
        let noise: Vec<Frame> = (0..48000 * 6)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let s = (seed >> 8) as f32 / 8388608.0 - 1.0;
                [s * 0.3, s * 0.3]
            })
            .collect();
        assert!(estimate(&noise, 48000).is_none_or(|t| t.confidence < 0.3));
        assert!(estimate(&vec![[0.0, 0.0]; 48000 * 6], 48000).is_none());
        assert!(estimate(&vec![[0.1, 0.1]; 48000], 48000).is_none());
    }
}
