//! DJ moves for Automix when the two songs' beats can't be lined up.
//!
//! ```text
//!  echo-out (old song has a beat):
//!     old  ████████████▌~ ~ ~ ~  ~   ~     its last beat repeats, thinner each time
//!     new          ▁▃▆█████████████  comes in under the echo, at full by the cut
//!                       ▲ cut, on the old song's beat
//!
//!  filter sweep (no clear beat):
//!     old  bass leaves first, then the rest  (high-pass rising, then fade)
//!     new  muffled at first, opens up        (low-pass opening, fade in)
//! ```
//! Both keep the two songs' bass and beats from clashing, which is what made
//! a plain crossfade between two full-volume songs sound like a jump.

use std::f32::consts::FRAC_PI_2;

use crate::player::Frame;

/// Which move a blend uses (logged by name, so a listener can say which one
/// sounded good).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Crossfade mode: equal-power fade.
    Crossfade,
    /// Beats lined up, bass swapped mid-way.
    BeatMatched,
    /// The old song's last beat echoes out as the new song comes in.
    EchoOut,
    /// High-pass the old song away, open a low-pass on the new one.
    FilterSweep,
    /// Preet's #10: a 16-beat, bar-aligned EQ handover (see `Handover`).
    Handover,
}

impl Move {
    pub fn name(self) -> &'static str {
        match self {
            Move::Crossfade => "crossfade",
            Move::BeatMatched => "beat-matched",
            Move::EchoOut => "echo-out",
            Move::FilterSweep => "filter sweep",
            Move::Handover => "DJ handover",
        }
    }
}

/// Splits a song into three EQ bands, like a DJ mixer's bass / mid / treble:
/// low < 150 Hz, high > 4 kHz, mid in between. Linkwitz–Riley crossovers
/// (two Butterworth stages each side), the kind DJ mixers and speakers use:
/// each band really drops away outside its range, and the three together
/// keep the song's level (they sum to an all-pass, not a copy).
///
/// (An earlier version subtracted a gentle low-pass from the song; its phase
/// shift left ~70 % of a 60 Hz bass in the "mid" band.)
#[derive(Debug, Clone, Copy)]
pub struct Bands {
    low: [Biquad; 2],
    rest: [Biquad; 2],
    mid: [Biquad; 2],
    high: [Biquad; 2],
}

impl Bands {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let make = |high: bool, hz: f32| {
            let mut b = Biquad::new();
            b.set(high, hz, sr);
            [b, b]
        };
        Self {
            low: make(false, 150.0),
            rest: make(true, 150.0),
            mid: make(false, 4000.0),
            high: make(true, 4000.0),
        }
    }

    /// `(low, mid, high)`.
    pub fn split(&mut self, x: Frame) -> (Frame, Frame, Frame) {
        let twice = |stages: &mut [Biquad; 2], x: Frame| {
            let once = stages[0].process(x);
            stages[1].process(once)
        };
        let low = twice(&mut self.low, x);
        let rest = twice(&mut self.rest, x);
        let mid = twice(&mut self.mid, rest);
        let high = twice(&mut self.high, rest);
        (low, mid, high)
    }
}

/// Preet's chosen transition (#10 in `new update plan/automix-proof`), over
/// `len` frames = 16 beats of the old song, bar-aligned, beats locked:
///
/// ```text
///  u (0..1)      0      .25            .5−1 beat .5          vocal_out   1
///  new treble    ↗ to half ──────────── 0 (breath) │ full ───────────────
///  new bass      ────────────── 0 ──────────────── │ slam in ────────────
///  new mids      ─────────────────── 0 ───────────────────────── ↗ in ───
///  old treble    full ──────────────────── 0 (breath) ────────────────────
///  old bass      full ──────────────────── 0 (breath) ────────────────────
///  old mids      full ─────────────────────────────────── ↘ + ¾-beat echo
/// ```
/// The new song's treble opens from a 400 Hz low-pass over the first half.
/// The echo keeps ringing (thinner each repeat) for 8 beats after the blend.
pub struct Handover {
    len: usize,
    one: f64,
    vocal_out: f64,
    sample_rate: f32,
    a: Bands,
    b: Bands,
    b_lp: Biquad,
    echo: Vec<Frame>,
    echo_at: usize,
    echo_hp: Biquad,
    /// Frames the echo rings on after the blend.
    tail: usize,
}

impl Handover {
    /// `len`: 16 beats of the old song; `beat`: one beat (frames);
    /// `vocal_out`: blend progress (0.5..0.9) where the old song's mids leave,
    /// at a gap between vocal lines if one was found.
    pub fn new(sample_rate: u32, len: usize, beat: usize, vocal_out: f64) -> Self {
        let one = beat as f64 / len.max(1) as f64;
        Self {
            len: len.max(1),
            one,
            // With the echo, the dry line stops a beat earlier; the echo fills in.
            vocal_out: (vocal_out.clamp(0.5, 0.9) - one).max(0.4),
            sample_rate: sample_rate as f32,
            a: Bands::new(sample_rate),
            b: Bands::new(sample_rate),
            b_lp: Biquad::new(),
            echo: vec![[0.0; 2]; (beat * 3 / 4).max(1)],
            echo_at: 0,
            echo_hp: Biquad::new(),
            tail: beat * 8,
        }
    }

    fn ramp(u: f64, from: f64, to: f64) -> f32 {
        ((u - from) / (to - from)).clamp(0.0, 1.0) as f32
    }

    fn fade_in(x: f32) -> f32 {
        (x * FRAC_PI_2).sin()
    }

    fn fade_out(x: f32) -> f32 {
        ((1.0 - x) * FRAC_PI_2).sin()
    }

    /// One frame of the blend: `a` the old song, `b` the new one (already
    /// time-stretched onto the old song's beats), `pos` 0..len.
    pub fn frame(&mut self, a: Frame, b: Frame, pos: usize) -> Frame {
        let u = (pos as f64 + 0.5) / self.len as f64;
        let (one, vo) = (self.one, self.vocal_out);
        let breath = (0.5 - one..0.5).contains(&u);
        let (a_low, a_mid, a_high) = self.a.split(a);
        let (b_low, b_mid, mut b_high) = self.b.split(b);
        // The new song's treble opens up from muffled over the first half.
        if u < 0.5 {
            if pos.is_multiple_of(32) {
                let hz = 400.0 * (18000.0f32 / 400.0).powf((u / 0.5) as f32);
                self.b_lp.set(false, hz, self.sample_rate);
            }
            b_high = self.b_lp.process(b_high);
        }
        let gb_high = if breath {
            0.0
        } else if u >= 0.5 {
            1.0
        } else {
            Self::fade_in(Self::ramp(u, 0.0, 0.25)) * 0.5
        };
        let gb_low = Self::fade_in(Self::ramp(u, 0.5, 0.5 + one * 0.25));
        let gb_mid = Self::fade_in(Self::ramp(u, vo, vo + 2.0 * one));
        let gone = u >= 0.5 - one; // the breath and after: old drums and bass out
        let ga_high = if gone { 0.0 } else { 1.0 };
        let ga_low = if gone { 0.0 } else { 1.0 };
        let ga_mid = Self::fade_out(Self::ramp(u, vo, vo + 0.25 * one));
        let send = if (vo - one..vo + 0.25 * one).contains(&u) {
            1.0
        } else {
            0.0
        };
        let wet = self.echo_step(a_mid, send, u);
        // The bands sum to an all-pass, not a copy: slide from the plain song
        // into the bands (and out to the plain new song) over 30 ms, so the
        // start and end of the blend can't click.
        let edge = (self.sample_rate * 0.03) as usize;
        let w = (pos.min(self.len - pos) as f32 / edge.max(1) as f32).min(1.0);
        let dry = if pos < self.len / 2 { a } else { b };
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let banded = a_low[ch] * ga_low
                + a_mid[ch] * ga_mid
                + a_high[ch] * ga_high
                + b_low[ch] * gb_low
                + b_mid[ch] * gb_mid
                + b_high[ch] * gb_high;
            out[ch] = banded * w + dry[ch] * (1.0 - w) + wet[ch];
        }
        out
    }

    /// The echo after the blend has ended: `Some` while it still rings.
    pub fn tail_frame(&mut self, after: usize) -> Option<Frame> {
        if after >= self.tail {
            return None;
        }
        let u = (self.len + after) as f64 / self.len as f64;
        Some(self.echo_step([0.0; 2], 0.0, u))
    }

    fn echo_step(&mut self, input: Frame, send: f32, u: f64) -> Frame {
        let vo = self.vocal_out;
        let held = self.echo[self.echo_at];
        if u >= vo && ((u * self.len as f64) as usize).is_multiple_of(32) {
            // Thinner each repeat: high-pass 200 Hz → 2 kHz to the tail's end.
            let span = 1.0 - vo + self.tail as f64 / self.len as f64;
            let t = ((u - vo) / span).min(1.0) as f32;
            self.echo_hp
                .set(true, 200.0 * 10f32.powf(t), self.sample_rate);
        }
        let wet = self.echo_hp.process(held);
        for ch in 0..2 {
            self.echo[self.echo_at][ch] = input[ch] * send + held[ch] * 0.55;
        }
        self.echo_at = (self.echo_at + 1) % self.echo.len();
        [wet[0] * 0.9, wet[1] * 0.9]
    }
}

/// A plain biquad (RBJ cookbook), stereo.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

impl Biquad {
    pub(crate) fn new() -> Self {
        Self {
            b: [1.0, 0.0, 0.0],
            a: [0.0, 0.0],
            z: [[0.0; 2]; 2],
        }
    }

    pub(crate) fn set(&mut self, high_pass: bool, freq: f32, sample_rate: f32) {
        let f = freq.clamp(10.0, sample_rate * 0.45);
        let w = std::f32::consts::TAU * f / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high_pass {
            ((1.0 + cos) / 2.0, -(1.0 + cos))
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos)
        };
        self.b = [b0 / a0, b1 / a0, b0 / a0];
        self.a = [-2.0 * cos / a0, (1.0 - alpha) / a0];
    }

    pub(crate) fn process(&mut self, x: Frame) -> Frame {
        let mut y = [0.0; 2];
        for ch in 0..2 {
            // Transposed direct form II.
            let out = self.b[0] * x[ch] + self.z[0][ch];
            self.z[0][ch] = self.b[1] * x[ch] - self.a[0] * out + self.z[1][ch];
            self.z[1][ch] = self.b[2] * x[ch] - self.a[1] * out;
            y[ch] = out;
        }
        y
    }
}

/// Filters are retuned every this many frames during a sweep.
const RETUNE: usize = 32;

/// The state of one DJ move while it plays.
pub struct Fx {
    kind: Move,
    sample_rate: f32,
    len: usize,
    // echo-out
    /// Fade position of the cut (frames), on the old song's beat.
    cut: usize,
    /// One beat of the old song, in frames.
    beat: usize,
    delay: Vec<Frame>,
    at: usize,
    echo_hp: Biquad,
    // filter sweep
    a_hp: Biquad,
    b_lp: Biquad,
}

/// Echoes repeat at this fraction of the level before.
const ECHO_FEEDBACK: f32 = 0.55;

impl Fx {
    /// `len`: fade length; `beat`: the old song's beat period and where its
    /// beat falls closest to the planned cut (both in fade frames), if known.
    pub fn new(kind: Move, sample_rate: u32, len: usize, beat: Option<(usize, usize)>) -> Self {
        let sr = sample_rate as f32;
        let (period, cut) = beat.unwrap_or(((sr * 0.5) as usize, len * 2 / 5));
        let period = period.clamp((sr * 0.25) as usize, (sr * 1.2) as usize);
        Self {
            kind,
            sample_rate: sr,
            len: len.max(1),
            cut: cut.clamp(period, len.saturating_sub(1).max(period)),
            beat: period,
            delay: vec![[0.0; 2]; period],
            at: 0,
            echo_hp: Biquad::new(),
            a_hp: Biquad::new(),
            b_lp: Biquad::new(),
        }
    }

    pub fn kind(&self) -> Move {
        self.kind
    }

    /// Mixes one frame of the old song (`a`) and the new one (`b`) at fade
    /// position `pos`.
    pub fn frame(&mut self, a: Frame, b: Frame, pos: usize) -> Frame {
        match self.kind {
            Move::EchoOut => self.echo_out(a, b, pos),
            Move::FilterSweep => self.sweep(a, b, pos),
            // The other moves are mixed by the player itself.
            Move::Crossfade | Move::BeatMatched | Move::Handover => {
                let x = (pos as f32 + 0.5) / self.len as f32;
                let (ga, gb) = ((x * FRAC_PI_2).cos(), (x * FRAC_PI_2).sin());
                [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb]
            }
        }
    }

    fn echo_out(&mut self, a: Frame, b: Frame, pos: usize) -> Frame {
        let (cut, beat) = (self.cut, self.beat);
        // The old song plays dry up to the cut, then stops within 20 ms.
        let release = (self.sample_rate * 0.02) as usize;
        let dry = if pos < cut {
            1.0
        } else {
            1.0 - ((pos - cut) as f32 / release as f32).min(1.0)
        };
        // Its last beat before the cut goes into the echo.
        let send = if pos + beat >= cut && pos < cut {
            1.0
        } else {
            0.0
        };
        if pos >= cut && (pos - cut).is_multiple_of(RETUNE) {
            // The echo gets thinner: 150 Hz → 1.5 kHz high-pass over the rest.
            let t = (pos - cut) as f32 / (self.len - cut).max(1) as f32;
            self.echo_hp
                .set(true, 150.0 * 10f32.powf(t), self.sample_rate);
        }
        let echoed = self.delay[self.at];
        let wet = if pos >= cut {
            self.echo_hp.process(echoed)
        } else {
            echoed
        };
        for ch in 0..2 {
            self.delay[self.at][ch] = a[ch] * send + echoed[ch] * ECHO_FEEDBACK;
        }
        self.at = (self.at + 1) % self.delay.len();
        // The echo itself fades out by the end.
        let x = pos as f32 / self.len as f32;
        let tail = (1.0 - x).clamp(0.0, 1.0);
        // The new song comes in over the last beat before the cut and is at
        // full level half a beat after it.
        let from = cut.saturating_sub(beat);
        let rise = ((pos as f32 - from as f32) / (beat + beat / 2) as f32).clamp(0.0, 1.0);
        let gb = (rise * FRAC_PI_2).sin();
        let mut out = [0.0; 2];
        for ch in 0..2 {
            out[ch] = a[ch] * dry + wet[ch] * tail + b[ch] * gb;
        }
        out
    }

    fn sweep(&mut self, a: Frame, b: Frame, pos: usize) -> Frame {
        let x = (pos as f32 + 0.5) / self.len as f32;
        if pos.is_multiple_of(RETUNE) {
            // Old song: high-pass 30 Hz → 2 kHz, so its bass leaves first.
            let hp = 30.0 * (2000.0f32 / 30.0).powf((x / 0.85).min(1.0));
            self.a_hp.set(true, hp, self.sample_rate);
            // New song: low-pass 250 Hz → wide open over the first 60 %.
            let lp = 250.0 * (20000.0f32 / 250.0).powf((x / 0.6).min(1.0));
            self.b_lp.set(false, lp, self.sample_rate);
        }
        let a = self.a_hp.process(a);
        let b_open = x >= 0.6;
        let b = if b_open { b } else { self.b_lp.process(b) };
        let ga = (((x - 0.3) / 0.7).clamp(0.0, 1.0) * FRAC_PI_2).cos();
        let gb = ((x / 0.5).clamp(0.0, 1.0) * FRAC_PI_2).sin();
        [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48000;

    fn sine(freq: f32, n: usize, amp: f32) -> Frame {
        let s = (std::f32::consts::TAU * freq * n as f32 / SR as f32).sin() * amp;
        [s, s]
    }

    fn rms(x: &[Frame]) -> f32 {
        (x.iter().map(|f| f[0] * f[0]).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Amplitude of one frequency in the left channel.
    fn amplitude(x: &[Frame], freq: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, f) in x.iter().enumerate() {
            let ph = std::f64::consts::TAU * freq as f64 * n as f64 / SR as f64;
            re += f[0] as f64 * ph.cos();
            im += f[0] as f64 * ph.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
    }

    #[test]
    fn echo_out_repeats_the_last_beat_and_dies_away() {
        let len = SR as usize * 6;
        let beat = SR as usize / 2; // 120 BPM
        let cut = beat * 5; // 2.5 s in
        let mut fx = Fx::new(Move::EchoOut, SR, len, Some((beat, cut)));
        // Old song: a steady tone; new song: silence, to hear the echo alone.
        let out: Vec<Frame> = (0..len)
            .map(|n| fx.frame(sine(440.0, n, 0.5), [0.0; 2], n))
            .collect();
        // Before the cut: the old song, dry.
        assert!((rms(&out[cut - beat..cut - beat / 2]) - 0.354).abs() < 0.03);
        // After the cut: echoes, each beat quieter than the one before.
        let per_beat: Vec<f32> = (0..5)
            .map(|k| rms(&out[cut + 2000 + k * beat..cut + (k + 1) * beat]))
            .collect();
        assert!(per_beat[0] > 0.05, "{per_beat:?}");
        assert!(per_beat.windows(2).all(|w| w[1] < w[0]), "{per_beat:?}");
        // Gone by the end, and never louder than the song was.
        assert!(rms(&out[len - beat / 2..]) < 0.01);
        assert!(out.iter().all(|f| f[0].abs() < 0.9));
    }

    #[test]
    fn echo_out_brings_the_new_song_in_around_the_cut() {
        let len = SR as usize * 6;
        let beat = SR as usize / 2;
        let cut = beat * 5;
        let mut fx = Fx::new(Move::EchoOut, SR, len, Some((beat, cut)));
        let out: Vec<Frame> = (0..len)
            .map(|n| fx.frame([0.0; 2], sine(330.0, n, 0.5), n))
            .collect();
        assert!(rms(&out[..cut - beat - 100]) < 0.001, "new song too early");
        let full = 0.5 / 2f32.sqrt();
        assert!(
            (rms(&out[cut + beat..]) - full).abs() < 0.01,
            "not at full level after the cut"
        );
    }

    #[test]
    fn filter_sweep_takes_the_old_bass_out_first_and_opens_the_new_song() {
        let len = SR as usize * 6;
        let mut fx = Fx::new(Move::FilterSweep, SR, len, None);
        let mix = |n: usize| {
            let lo = sine(60.0, n, 0.4);
            let hi = sine(3000.0, n, 0.2);
            [lo[0] + hi[0], lo[1] + hi[1]]
        };
        // Old song only: in the middle its bass is gone but its top remains.
        let out: Vec<Frame> = (0..len).map(|n| fx.frame(mix(n), [0.0; 2], n)).collect();
        let mid = &out[len / 2..len / 2 + SR as usize / 2];
        assert!(amplitude(mid, 60.0) < 0.02, "old bass still there");
        assert!(amplitude(mid, 3000.0) > 0.1, "old top gone too early");
        // New song only: muffled at first, full at the end.
        let mut fx = Fx::new(Move::FilterSweep, SR, len, None);
        let out: Vec<Frame> = (0..len).map(|n| fx.frame([0.0; 2], mix(n), n)).collect();
        let early = &out[SR as usize / 2..SR as usize];
        assert!(
            amplitude(early, 3000.0) < 0.02,
            "new song not muffled at first"
        );
        let end = &out[len - SR as usize / 2..];
        assert!(amplitude(end, 60.0) > 0.38 && amplitude(end, 3000.0) > 0.19);
    }

    #[test]
    fn bands_keep_the_level_and_really_separate() {
        for (freq, band) in [(60.0, 0), (1000.0, 1), (9000.0, 2)] {
            let mut bands = Bands::new(SR);
            let (mut sum, mut own, mut others) = (Vec::new(), Vec::new(), Vec::new());
            for n in 0..SR as usize {
                let (l, m, h) = bands.split(sine(freq, n, 0.5));
                let parts = [l, m, h];
                if n > SR as usize / 4 {
                    sum.push([l[0] + m[0] + h[0], 0.0]);
                    own.push(parts[band]);
                    let o: Vec<Frame> = (0..3).filter(|i| *i != band).map(|i| parts[i]).collect();
                    others.push([o[0][0] + o[1][0], 0.0]);
                }
            }
            let full = 0.5 / 2f32.sqrt();
            assert!(
                (rms(&sum) - full).abs() < 0.01,
                "{freq} Hz level {}",
                rms(&sum)
            );
            assert!(rms(&own) > 0.9 * full, "{freq} Hz mostly in its own band");
            assert!(
                rms(&others) < 0.2 * full,
                "{freq} Hz leaks: {}",
                rms(&others)
            );
        }
    }

    #[test]
    fn handover_goes_from_the_old_song_to_the_new_one_with_a_breath() {
        let beat = SR as usize / 2; // 120 BPM
        let len = 16 * beat;
        let mut h = Handover::new(SR, len, beat, 0.75);
        // Old song: bass at 60 Hz; new song: bass at 90 Hz. Both also have mids.
        let a = |n| {
            let (b, m) = (sine(60.0, n, 0.4), sine(700.0, n, 0.2));
            [b[0] + m[0], b[1] + m[1]]
        };
        let b = |n| {
            let (bs, m) = (sine(90.0, n, 0.4), sine(500.0, n, 0.2));
            [bs[0] + m[0], bs[1] + m[1]]
        };
        let out: Vec<Frame> = (0..len).map(|n| h.frame(a(n), b(n), n)).collect();
        let at =
            |from_beat: usize, beats: usize| &out[from_beat * beat..(from_beat + beats) * beat];
        // Start: the old song's bass, none of the new one's.
        assert!(amplitude(at(1, 2), 60.0) > 0.3);
        assert!(amplitude(at(1, 2), 90.0) < 0.02);
        // The breath (beat 7): no bass from either song.
        let breath = at(7, 1);
        // Mostly gone: the bass / mid crossover is gentle (as on a DJ mixer),
        // so a little of a 60 Hz tone stays in the (still playing) mids.
        let (old_bass, new_bass) = (amplitude(breath, 60.0), amplitude(breath, 90.0));
        assert!(
            old_bass < 0.4 * 0.3 && new_bass < 0.4 * 0.3,
            "bass in the breath: {old_bass} {new_bass}"
        );
        // After the drop: the new song's bass at full, the old one's gone.
        assert!(amplitude(at(9, 3), 90.0) > 0.35);
        assert!(amplitude(at(9, 3), 60.0) < 0.02);
        // End: the new song's mids are in.
        assert!(amplitude(at(15, 1), 500.0) > 0.15);
    }

    #[test]
    fn handover_echo_carries_the_old_vocal_out() {
        let beat = SR as usize / 2;
        let len = 16 * beat;
        let mut h = Handover::new(SR, len, beat, 0.75);
        // Old song: a held note in the mids; new song silent.
        let out: Vec<Frame> = (0..len)
            .map(|n| h.frame(sine(1000.0, n, 0.3), [0.0; 2], n))
            .collect();
        // The dry note leaves a beat before 0.75 (beat 11) within a quarter
        // beat; after that only the echo sounds, thinner each repeat.
        let echo_1 = rms(&out[12 * beat..13 * beat]);
        let echo_3 = rms(&out[14 * beat..15 * beat]);
        assert!(echo_1 > 0.02, "no echo after the vocal left: {echo_1}");
        assert!(echo_3 < echo_1, "echo not dying away: {echo_1} -> {echo_3}");
        // It may ring on past the blend, never longer than 8 beats.
        let tail = (0..).map_while(|k| h.tail_frame(k)).count();
        assert_eq!(tail, 8 * beat);
    }

    #[test]
    fn moves_never_click() {
        for kind in [Move::EchoOut, Move::FilterSweep] {
            let len = SR as usize * 4;
            let mut fx = Fx::new(kind, SR, len, Some((SR as usize / 2, SR as usize * 2)));
            let out: Vec<Frame> = (0..len)
                .map(|n| fx.frame(sine(110.0, n, 0.5), sine(165.0, n, 0.5), n))
                .collect();
            let jump = out
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).abs())
                .fold(0.0f32, f32::max);
            // Two low notes at 0.5 change by well under 0.03 per frame.
            assert!(jump < 0.03, "{} jump {jump}", kind.name());
        }
    }
}
