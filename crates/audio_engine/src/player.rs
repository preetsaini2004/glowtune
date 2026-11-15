//! The delay buffer and the song-to-song blend (UPDATE-PLAN §2 Step C).
//!
//! ```text
//!  captured frames ─► push ─► [ delay buffer ······ ] ─► pull ─► speakers
//!                               ▲ read position          (what you hear)
//!
//!  song change (automatic):
//!     … A A A A A│B B B B B …        the buffer already holds B's start
//!           └ fade ┘                 so A's tail and B's start are mixed,
//!     … A A (A+B) B B …              and the delay shrinks by the fade length
//!
//!  afterwards the delay is refilled by playing ~4 % slower for a while
//!  (same pitch, see `Stretch`), so the next change can blend too.
//! ```
//!
//! Manual skip / seek: `flush()` drops the buffer, so the new sound is heard
//! at once. Pause: `set_paused(true)` freezes the buffer where it is.
//!
//! Nothing here is ever written to disk; the buffer only lives in memory.

use std::collections::VecDeque;

pub type Frame = [f32; 2];
const SILENCE: Frame = [0.0, 0.0];

/// How one song hands over to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Blend {
    /// No delay: sound passes straight through.
    Off,
    /// Mix the end of one song with the start of the next.
    Crossfade { secs: f32 },
    /// Like a crossfade, but when both songs have a clear beat at nearly the
    /// compatible tempo the beats are lined up and the bass is swapped mid-way,
    /// the way a DJ would. Otherwise it is a plain crossfade.
    Automix { secs: f32 },
}

impl Blend {
    /// Fade length in seconds, if songs are blended at all.
    pub fn fade_secs(self) -> Option<f32> {
        match self {
            Blend::Off => None,
            Blend::Crossfade { secs } | Blend::Automix { secs } => Some(secs.clamp(1.0, 12.0)),
        }
    }
}

/// Bound tempo changes to a musically usable range (up to a 4:3 ratio).
/// The dedicated stretcher changes duration while preserving pitch.
const MAX_TEMPO_RATIO: f64 = 4.0 / 3.0;
/// Below this the beat is too unclear to trust.
const MIN_BEAT_CONFIDENCE: f32 = 0.35;
/// Automix looks at this much of the new song before blending it in.
const AUTOMIX_LOOKAHEAD_SECS: f32 = 5.0;
/// The least it needs to hear to find the new song's beat.
const AUTOMIX_MIN_LOOKAHEAD_SECS: f32 = 3.3;

/// Speed while refilling the delay, without changing pitch.
const REFILL_SPEED: f64 = 0.96;
/// Sound this much quieter than the song around it (about −34 dB) counts as
/// the silence between songs. Relative, because the service's own volume
/// slider can make the whole signal very quiet.
const QUIET_RATIO: f32 = 0.02;
/// Below this (about −80 dB) there is no sound at all, not a quiet intro.
const SILENT_LEVEL: f32 = 1e-4;

// ---- the DJ handover (Preet's #10/#11) -------------------------------------

/// Beats in a handover blend.
const HANDOVER_BEATS: usize = 16;
/// How much of the new song is studied before a handover (tempo and bar 1
/// were right from 15 s in 5/5 test songs; see experiments/stems-proof).
const HANDOVER_B_SECS: f32 = 15.0;
/// How much of the old song's end is studied.
const HANDOVER_A_SECS: f32 = 20.0;
/// Automix keeps this much delay: the longest blend (16 beats at ~70 BPM),
/// up to a bar to reach a downbeat, the new song's 15 s, and a cushion.
const AUTOMIX_DELAY_SECS: f32 = 14.0 + 3.5 + HANDOVER_B_SECS + 1.5;

/// A handover worked out from both songs, ready to start.
#[derive(Debug, Clone, Copy)]
struct Plan {
    /// Absolute index of the old song's downbeat where the blend starts.
    start: u64,
    /// Blend length (output frames) = 16 of the old song's beats.
    len: usize,
    /// One beat of the old song (frames).
    beat: f64,
    /// Absolute index of the new song's first downbeat (it enters there).
    b_down: f64,
    /// New song frames per output frame, so its beats land on the old ones.
    speed: f64,
    /// Blend progress where the old song's mids leave (a gap between lines).
    vocal_out: f64,
    a_bpm: f32,
    b_bpm: f32,
}

/// Studies the old song's end (`a`, starting at absolute index `a_at`, its
/// audible end at `a_end`) and the new song's start (`b`, from `b_at`), and
/// plans a bar-aligned handover: 16 beats, or 8 when 16 no longer fit before
/// the old song's end. `Err` says why there is none (for the log).
fn plan_handover(
    a: &[Frame],
    a_at: u64,
    a_end: u64,
    b: &[Frame],
    b_at: u64,
    sr: u32,
) -> Result<Plan, &'static str> {
    let usable = |t: &crate::tempo::Tempo| t.confidence >= MIN_BEAT_CONFIDENCE;
    let ta = crate::tempo::estimate(a, sr)
        .filter(usable)
        .ok_or("the old song has no clear beat at its end")?;
    let tb = crate::tempo::estimate(b, sr)
        .filter(usable)
        .ok_or("the new song has no clear beat yet")?;
    let (a_down, _) = crate::tempo::downbeat(a, sr, &ta).ok_or("no bar 1 in the old song")?;
    let (b_down, _) = crate::tempo::downbeat(b, sr, &tb).ok_or("no bar 1 in the new song")?;
    let speed = [0.5, 1.0, 2.0]
        .into_iter()
        .map(|k| tb.period * k / ta.period)
        .min_by(|x, y| x.ln().abs().total_cmp(&y.ln().abs()))
        .ok_or("no tempo")?;
    if speed.max(1.0 / speed) > MAX_TEMPO_RATIO {
        return Err("the tempos are too far apart");
    }
    // The last downbeat of the old song from which a whole blend still fits:
    // 16 beats, or 8 when the old song is nearly over.
    let bar = 4.0 * ta.period;
    let first = a_at as f64 + a_down;
    let (beats, len, start) = [HANDOVER_BEATS, HANDOVER_BEATS / 2]
        .into_iter()
        .find_map(|beats| {
            let len = (beats as f64 * ta.period) as usize;
            let bars = ((a_end as f64 - len as f64 - first) / bar).floor();
            (bars >= 0.0).then_some((beats, len, first + bars * bar))
        })
        .ok_or("not enough of the old song left, even for 8 beats")?;
    // Where the old song's mids (its vocals) pause in the blend's second half.
    let mut bands = crate::dj::Bands::new(sr);
    let mids: Vec<Frame> = a.iter().map(|f| bands.split(*f).1).collect();
    let level = |from: f64| {
        let s = (from - a_at as f64).max(0.0) as usize;
        let e = (s + ta.period as usize).min(mids.len());
        if s >= e {
            return 0.0;
        }
        let sum: f32 = mids[s..e]
            .iter()
            .map(|f| (f[0] * f[0] + f[1] * f[1]) * 0.5)
            .sum();
        (sum / (e - s) as f32).sqrt()
    };
    let mut levels: Vec<f32> = (0..((a.len() as f64 / ta.period) as usize))
        .map(|k| level(a_at as f64 + a_down.rem_euclid(ta.period) + k as f64 * ta.period))
        .filter(|v| *v > 1e-4)
        .collect();
    levels.sort_by(f32::total_cmp);
    let typical = levels.get(levels.len() / 2).copied().unwrap_or(0.0);
    let gap = (beats / 2..beats).find(|k| level(start + *k as f64 * ta.period) < typical * 0.25);
    let vocal_out = gap.map_or(0.9, |k| k as f64 / beats as f64);
    Ok(Plan {
        start: start as u64,
        len,
        beat: ta.period,
        b_down: b_at as f64 + b_down,
        speed,
        vocal_out,
        a_bpm: ta.bpm(sr),
        b_bpm: tb.bpm(sr),
    })
}

/// A planned blend, waiting for the read position to reach its start.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// Absolute index just after song A's last audible frame.
    a_end: u64,
    /// Absolute index where song B's audio begins.
    boundary: u64,
    /// Fade length in frames.
    len: usize,
    /// Automix: a beat of song A (absolute index) and its period, if A has a
    /// clear beat near its end.
    a_beat: Option<(f64, f64)>,
}

/// A blend in progress.
#[derive(Debug, Clone, Copy)]
struct Fade {
    /// Absolute (fractional) index of song B's next frame.
    b_pos: f64,
    pos: usize,
    len: usize,
    /// How fast B is read at first (1.0 = normal). Not 1.0 only when Automix
    /// matched the beats; eased back to 1.0 over the fade's second half.
    speed: f64,
    /// The part of the fade over which B comes in (0.3..1). Short when song A
    /// is already fading out by itself: B then fills the space at once
    /// instead of leaving a dip.
    b_rise: f32,
    /// Automix matched the beats: also swap the bass mid-way.
    matched: bool,
    /// Which move this blend uses (see `dj`).
    kind: crate::dj::Move,
    /// Low-pass memory for the bass swap (per song, per channel).
    a_low: [f32; 2],
    b_low: [f32; 2],
}

pub struct Player {
    sample_rate: usize,
    buf: VecDeque<Frame>,
    /// Absolute index of `buf[0]` (counts every frame ever pushed).
    base: u64,
    blend: Blend,
    paused: bool,
    /// Tempos (old song, new song, in BPM) of the last blend that was matched
    /// on the beat; `None` if the last blend was a plain crossfade.
    last_match: Option<(f32, f32)>,
    /// Usual loudness of what was played in the last few seconds (mean square).
    loudness: f32,
    /// Frames of sound still accepted as "the fade-out after pausing".
    pause_grace: usize,
    /// False until the small safety cushion has filled (after start or a flush).
    primed: bool,
    pending: Option<Pending>,
    fade: Option<Fade>,
    stretch: Option<crate::stretch::Source>,
    incoming: Option<crate::stretch::Source>,
    /// Echo-out / filter sweep state while such a blend plays.
    fx: Option<crate::dj::Fx>,
    refilling: bool,
    /// The service itself is playing faster to fill the delay (fast fill):
    /// no need to play slower here as well.
    upstream_fill: bool,
    recovery: Option<(f64, usize)>,
    /// 0..1 fade-in after a flush or resume, so cuts don't click.
    ramp: f32,
    /// Automix: studying both songs for a handover (off the audio thread).
    handover_job: Option<std::thread::JoinHandle<Result<Plan, &'static str>>>,
    /// Automix: the handover planned for the pending song change.
    plan: Option<Plan>,
    /// The handover playing now.
    handover: Option<crate::dj::Handover>,
    /// A finished handover whose echo still rings (and how far it got).
    tail: Option<(crate::dj::Handover, usize)>,
    /// The study for this song change was started (once per change).
    studied: bool,
    /// How far into the new song its study window starts (frames): 0, or a
    /// few seconds on the second look, past a beatless intro.
    b_skip: u64,
    /// The move of the last finished blend.
    last_kind: Option<crate::dj::Move>,
}

impl Player {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate as usize,
            buf: VecDeque::new(),
            base: 0,
            blend: Blend::Off,
            paused: false,
            last_match: None,
            loudness: 0.0,
            pause_grace: 0,
            primed: false,
            pending: None,
            fade: None,
            stretch: None,
            incoming: None,
            fx: None,
            refilling: false,
            upstream_fill: false,
            recovery: None,
            ramp: 1.0,
            handover_job: None,
            plan: None,
            handover: None,
            tail: None,
            studied: false,
            b_skip: 0,
            last_kind: None,
        }
    }

    /// Safety cushion against timing jitter between capture and playback:
    /// 30 ms. With a blend on it is 200 ms, because the stretcher that refills
    /// the delay needs some sound ahead of it to work with.
    fn cushion(&self) -> usize {
        match self.blend {
            Blend::Off => self.sample_rate * 30 / 1000,
            Blend::Crossfade { .. } | Blend::Automix { .. } => self.sample_rate / 5,
        }
    }

    /// The delay the current blend needs: fade length plus room to detect the
    /// song change and to stretch.
    fn target_delay(&self) -> usize {
        match self.blend {
            Blend::Off => self.sample_rate * 30 / 1000,
            Blend::Crossfade { secs } => {
                ((secs.clamp(1.0, 12.0) + 1.5) * self.sample_rate as f32) as usize
            }
            // Room for a handover: see AUTOMIX_DELAY_SECS.
            Blend::Automix { secs } => {
                let fallback = secs.clamp(1.0, 12.0) + AUTOMIX_LOOKAHEAD_SECS + 1.5;
                (fallback.max(AUTOMIX_DELAY_SECS) * self.sample_rate as f32) as usize
            }
        }
    }

    pub fn blend(&self) -> Blend {
        self.blend
    }

    pub fn set_blend(&mut self, blend: Blend) {
        if blend == self.blend {
            return;
        }
        self.blend = blend;
        if matches!(blend, Blend::Off) {
            self.pending = None;
            self.fade = None;
            self.fx = None;
            self.drop_stretch();
        }
        // A blend needs a bigger cushion than plain playback has: pause for a
        // moment (150 ms) to collect it, or the delay could never start filling.
        if self.buf.len() < self.cushion() {
            self.primed = false;
            self.ramp = 0.0;
        }
        // A shorter (or no) delay: jump forward instead of speeding up.
        // (With the blend off there is no reason to stay late at all.)
        let target = self.target_delay();
        let slack = match blend {
            Blend::Off => self.sample_rate / 20,
            _ => self.sample_rate,
        };
        if self.delay_frames() > target + slack {
            self.pending = None;
            self.fade = None;
            self.fx = None;
            self.drop_stretch();
            let extra = self.buf.len().saturating_sub(target);
            self.buf.drain(..extra);
            self.base += extra as u64;
            self.ramp = 0.0;
        }
    }

    /// See `upstream_fill`.
    pub fn set_upstream_fill(&mut self, on: bool) {
        self.upstream_fill = on;
    }

    /// The delay the current blend aims for, in ms (0 with the blend off).
    pub fn target_delay_ms(&self) -> u32 {
        match self.blend {
            Blend::Off => 0,
            _ => (self.target_delay() as u64 * 1000 / self.sample_rate as u64) as u32,
        }
    }

    pub fn delay_frames(&self) -> usize {
        self.buf.len()
    }

    pub fn delay_secs(&self) -> f32 {
        self.delay_frames() as f32 / self.sample_rate as f32
    }

    pub fn is_blending(&self) -> bool {
        self.fade.is_some()
    }

    /// For the blend running now (or the last one): the two tempos if it was
    /// matched on the beat, `None` for a plain crossfade.
    pub fn beat_match(&self) -> Option<(f32, f32)> {
        self.last_match
    }

    pub fn is_refilling(&self) -> bool {
        self.refilling
    }

    pub fn speed(&self) -> f64 {
        if let Some(fade) = self.fade {
            fade.speed
        } else {
            self.stretch.as_ref().map_or(1.0, |source| source.speed())
        }
    }

    /// How long until sound captured *now* is heard, in frames. Shorter than
    /// `delay_frames` while a blend is planned or running, because the blend
    /// removes its own length from the timeline.
    pub fn latency_frames(&self) -> usize {
        let cut = if let Some(f) = self.fade {
            (f.b_pos - self.base as f64).max(0.0) as usize
        } else if let Some(plan) = self.plan.filter(|_| self.pending.is_some()) {
            // Skipped by the handover: from its start to the new song's
            // downbeat, plus what the new song gains by playing faster.
            let skip = (plan.b_down - plan.start as f64).max(0.0);
            (skip + plan.len as f64 * (plan.speed - 1.0)).max(0.0) as usize
        } else if let Some(p) = self.pending {
            p.len + p.boundary.saturating_sub(p.a_end) as usize
        } else {
            0
        };
        self.delay_frames().saturating_sub(cut)
    }

    pub fn latency_secs(&self) -> f32 {
        self.latency_frames() as f32 / self.sample_rate as f32
    }

    /// Adds captured sound (interleaved stereo).
    pub fn push(&mut self, frames: &[f32]) {
        if self.paused {
            if frames.iter().all(|s| s.abs() < 1e-6) {
                return;
            }
            // Sound right after the pause notice is the player fading out (the
            // notice comes first): keep it, stay paused. Sound after that
            // means playback resumed, even if no notice says so.
            let n = frames.len() / 2;
            if self.pause_grace >= n {
                self.pause_grace -= n;
            } else {
                self.pause_grace = 0;
                self.set_paused(false);
            }
        }
        self.buf
            .extend(frames.chunks_exact(2).map(|f| [f[0], f[1]]));
        // Never hold much more than Automix needs, whatever happens upstream.
        let max = self.sample_rate * 45;
        if self.buf.len() > max {
            tracing::warn!("delay over 45 s: dropping it (something upstream went wrong)");
            self.flush();
        }
        self.start_handover_study();
    }

    /// Automix: once the new song's first 15 s are in, study both songs on a
    /// helper thread (a few tens of ms of work; the audio path never waits).
    fn start_handover_study(&mut self) {
        let Some(p) = self.pending else {
            return;
        };
        if !matches!(self.blend, Blend::Automix { .. }) || self.studied {
            return;
        }
        let sr = self.sample_rate;
        let b_from = self.first_audible_from(p.boundary) + self.b_skip;
        let b_len = (HANDOVER_B_SECS * sr as f32) as usize;
        let end = self.base + self.buf.len() as u64;
        if b_from + (b_len as u64) > end {
            return; // not enough of the new song yet
        }
        let a_from = p
            .a_end
            .saturating_sub((HANDOVER_A_SECS * sr as f32) as u64)
            .max(self.base);
        let slice = |from: u64, to: u64| -> Vec<Frame> {
            let (s, e) = ((from - self.base) as usize, (to - self.base) as usize);
            self.buf.range(s..e.min(self.buf.len())).copied().collect()
        };
        if p.a_end <= a_from {
            return;
        }
        let a = slice(a_from, p.a_end);
        let b = slice(b_from, b_from + b_len as u64);
        let a_end = p.a_end;
        self.studied = true;
        tracing::debug!(
            "automix: studying (heard {:.1} s before the old song ends, its last {:.1} s and the new song's first {:.1} s)",
            p.a_end.saturating_sub(self.base) as f32 / sr as f32,
            a.len() as f32 / sr as f32,
            b.len() as f32 / sr as f32
        );
        self.handover_job = std::thread::Builder::new()
            .name("glowtune-handover".into())
            .spawn(move || {
                let t = std::time::Instant::now();
                let plan = plan_handover(&a, a_from, a_end, &b, b_from, sr as u32);
                tracing::debug!("automix: study took {} ms", t.elapsed().as_millis());
                plan
            })
            .ok();
    }

    /// A handover study is running on its helper thread.
    pub fn is_studying(&self) -> bool {
        self.handover_job.as_ref().is_some_and(|j| !j.is_finished())
    }

    /// Forgets any handover being studied or planned.
    fn drop_plan(&mut self) {
        self.handover_job = None; // a running study finishes on its own; ignored
        self.plan = None;
        self.studied = false;
        self.b_skip = 0;
    }

    /// Freezes or continues playback of the buffer.
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        self.pause_grace = self.sample_rate * 7 / 10;
        if paused {
            // Sound captured between the real pause and its notice is silence.
            // Native lookahead has already consumed some future samples.
            // Never reuse those absolute positions for new audio on resume.
            let fed = self
                .stretch
                .as_ref()
                .map_or(self.base, |s| s.fed_until())
                .max(self.incoming.as_ref().map_or(self.base, |s| s.fed_until()));
            while self.base + self.buf.len() as u64 > fed
                && self.buf.back().is_some_and(|f| f[0] == 0.0 && f[1] == 0.0)
            {
                self.buf.pop_back();
            }
        } else {
            self.ramp = 0.0;
        }
    }

    /// Forgets everything buffered: the next sound pushed is heard right away.
    /// For manual skips and seeks.
    pub fn flush(&mut self) {
        self.base += self.buf.len() as u64;
        self.buf.clear();
        self.incoming = None;
        self.refilling = false;
        self.recovery = None;
        self.pending = None;
        self.fade = None;
        self.fx = None;
        self.stretch = None;
        self.handover = None;
        self.tail = None;
        self.drop_plan();
        self.primed = false;
        self.ramp = 0.0;
    }

    /// The service moved to another song. `automatic` = the old song ran out
    /// (blend it); otherwise the user skipped (cut at once).
    pub fn track_changed(&mut self, automatic: bool) {
        let Some(secs) = self.blend.fade_secs() else {
            return;
        };
        if !automatic {
            tracing::info!("skip: dropping the delay so the new song plays at once");
            self.flush();
            return;
        }
        if self.fade.is_some() || self.pending.is_some() {
            return;
        }
        let end = self.base + self.buf.len() as u64;
        let boundary = self.find_boundary(end);
        let a_end = self.last_audible_before(boundary);
        let read = self.read_position();
        let room = a_end.saturating_sub(read + (self.sample_rate / 5) as u64) as usize;
        let wanted = (secs * self.sample_rate as f32) as usize;
        let mut len = wanted.min(room);
        if len < self.sample_rate / 2 {
            // Not enough delay yet: a plain change (an audible cut).
            tracing::info!(
                "song change without a blend: delay {:.1} s, room {:.1} s, fade wanted {:.1} s",
                self.delay_secs(),
                room as f32 / self.sample_rate as f32,
                secs
            );
            return;
        }
        // Automix: where are song A's beats? (Up to 8 s before its end.)
        let mut a_beat = matches!(self.blend, Blend::Automix { .. })
            .then(|| {
                let to = (a_end - self.base) as usize;
                let from = to
                    .saturating_sub(self.sample_rate * 8)
                    .max((read - self.base) as usize);
                let tempo = self.tempo_of(from, to)?;
                Some((
                    self.base as f64 + from as f64 + tempo.first_beat,
                    tempo.period,
                ))
            })
            .flatten();
        if matches!(self.blend, Blend::Automix { .. }) {
            // Matching beats needs to hear the new song's first seconds before
            // the blend starts. While the delay is still short, a shorter blend
            // on the beat beats a longer one off it.
            let lookahead = (self.sample_rate as f32 * AUTOMIX_MIN_LOOKAHEAD_SECS) as usize;
            let matched_len = wanted.min(room.saturating_sub(lookahead));
            match a_beat {
                Some((_, period)) if matched_len >= self.sample_rate * 3 / 2 => {
                    len = matched_len;
                    tracing::info!(
                        "automix: old song at {:.1} BPM, blend of {:.1} s planned",
                        60.0 * self.sample_rate as f64 / period,
                        len as f32 / self.sample_rate as f32
                    );
                }
                Some(_) => {
                    a_beat = None;
                    tracing::info!("automix: not enough delay yet to match beats, crossfading");
                }
                None => tracing::info!("automix: the old song has no clear beat, crossfading"),
            }
        }
        self.drop_plan();
        self.pending = Some(Pending {
            a_end,
            boundary,
            len,
            a_beat,
        });
    }

    /// Fills `out` (interleaved stereo) with the next sound to play.
    pub fn pull(&mut self, out: &mut [f32]) {
        for frame in out.chunks_exact_mut(2) {
            let mut f = self.next_frame();
            // A finished handover's echo ringing on.
            if !self.paused
                && let Some((h, k)) = self.tail.as_mut()
            {
                match h.tail_frame(*k) {
                    Some(w) => {
                        f[0] += w[0];
                        f[1] += w[1];
                        *k += 1;
                    }
                    None => self.tail = None,
                }
            }
            if self.ramp < 1.0 {
                f[0] *= self.ramp;
                f[1] *= self.ramp;
                // 10 ms fade-in.
                self.ramp = (self.ramp + 100.0 / self.sample_rate as f32).min(1.0);
            }
            frame[0] = f[0];
            frame[1] = f[1];
            // About 4 s of memory; not updated during a blend or silence.
            let power = (f[0] * f[0] + f[1] * f[1]) * 0.5;
            if self.fade.is_none() && power > 1e-12 {
                self.loudness += (power - self.loudness) / (4.0 * self.sample_rate as f32);
            }
        }
    }

    fn next_frame(&mut self) -> Frame {
        if self.paused {
            return SILENCE;
        }
        if !self.primed {
            if self.buf.len() < self.cushion() {
                return SILENCE;
            }
            self.primed = true;
        }
        if let Some(fade) = self.fade {
            return self.fade_frame(fade);
        }
        // Automix: a planned handover starts exactly on its downbeat.
        if self.pending.is_some() {
            if self.plan.is_none()
                && self.handover_job.as_ref().is_some_and(|j| j.is_finished())
                && let Some(job) = self.handover_job.take()
            {
                match job.join() {
                    Ok(Ok(plan)) => {
                        tracing::info!(
                            "automix: handover planned ({} beats, {:.0} BPM into {:.0} BPM, starts in {:.1} s)",
                            (plan.len as f64 / plan.beat).round(),
                            plan.a_bpm,
                            plan.b_bpm,
                            plan.start.saturating_sub(self.base) as f32 / self.sample_rate as f32
                        );
                        self.plan = Some(plan);
                    }
                    // The new song may open with a beatless intro: look once
                    // more, at its 5..20 s (the handover then skips the intro).
                    Ok(Err(why)) if why.contains("new song") && self.b_skip == 0 => {
                        tracing::info!("automix: {why}; looking again past its intro");
                        self.b_skip = 5 * self.sample_rate as u64;
                        self.studied = false;
                    }
                    Ok(Err(why)) => tracing::info!("automix: no handover: {why}"),
                    Err(_) => tracing::warn!("automix: the handover study failed"),
                }
            }
            if let Some(plan) = self.plan
                && self.base >= plan.start
            {
                if self.base <= plan.start + (self.sample_rate / 20) as u64 {
                    return self.start_handover(plan);
                }
                tracing::info!("automix: handover was planned too late, using a fallback move");
                self.plan = None;
            }
        }
        if let Some(p) = self.pending
            && self.base + p.len as u64 >= p.a_end
        {
            self.pending = None;
            self.drop_plan();
            let b_start = self.first_audible_from(p.boundary);
            // Whatever is left of A before its end may be shorter than planned.
            let len = (p.a_end.saturating_sub(self.base) as usize).min(p.len);
            if len > 0 {
                let mut fade = Fade {
                    b_pos: b_start as f64,
                    pos: 0,
                    len,
                    speed: 1.0,
                    b_rise: self.rise_for_tail(len),
                    matched: false,
                    kind: crate::dj::Move::Crossfade,
                    a_low: [0.0; 2],
                    b_low: [0.0; 2],
                };
                self.last_match = None;
                self.fx = None;
                if let Some((b_pos, speed)) = self.match_beats(&p, b_start) {
                    fade.b_pos = b_pos;
                    fade.speed = speed;
                    fade.matched = true;
                    fade.kind = crate::dj::Move::BeatMatched;
                    let a_bpm = p
                        .a_beat
                        .map_or(0.0, |(_, period)| 60.0 * self.sample_rate as f64 / period);
                    self.last_match = Some((a_bpm as f32, (a_bpm / speed) as f32));
                } else if matches!(self.blend, Blend::Automix { .. }) {
                    // No beat match: a DJ move instead of a plain crossfade
                    // between two full-volume songs (which sounds like a jump).
                    let (kind, beat) = match p.a_beat {
                        Some((a_beat, period)) => {
                            // Cut on the old song's beat nearest 40 % into the fade.
                            let start = self.base as f64;
                            let aim = start + len as f64 * 0.4;
                            let k = ((aim - a_beat) / period).round();
                            let cut = (a_beat + k * period - start).max(period);
                            (
                                crate::dj::Move::EchoOut,
                                Some((period.round() as usize, cut.round() as usize)),
                            )
                        }
                        None => (crate::dj::Move::FilterSweep, None),
                    };
                    fade.kind = kind;
                    self.fx = Some(crate::dj::Fx::new(kind, self.sample_rate as u32, len, beat));
                }
                tracing::info!(
                    "blend move: {} over {:.1} s",
                    fade.kind.name(),
                    len as f32 / self.sample_rate as f32
                );
                self.incoming = Some(crate::stretch::Source::new(
                    self.sample_rate as u32,
                    fade.b_pos,
                ));
                self.fade = Some(fade);
                return self.fade_frame(fade);
            }
        }
        self.refilling = self.wants_refill();
        if self.blend.fade_secs().is_some() {
            return self.stretch_hop();
        }
        match self.buf.pop_front() {
            Some(f) => {
                self.base += 1;
                f
            }
            None => {
                self.primed = false;
                SILENCE
            }
        }
    }

    /// Starts the planned 16-beat handover (Preet's #11).
    fn start_handover(&mut self, plan: Plan) -> Frame {
        self.pending = None;
        self.drop_plan();
        self.fx = None;
        self.tail = None;
        let fade = Fade {
            b_pos: plan.b_down,
            pos: 0,
            len: plan.len,
            speed: plan.speed,
            b_rise: 1.0,
            // Matched: the new song keeps the old tempo through the blend,
            // then eases back to its own (`recovery`).
            matched: true,
            kind: crate::dj::Move::Handover,
            a_low: [0.0; 2],
            b_low: [0.0; 2],
        };
        self.last_match = Some((plan.a_bpm, plan.b_bpm));
        self.handover = Some(crate::dj::Handover::new(
            self.sample_rate as u32,
            plan.len,
            plan.beat.round() as usize,
            plan.vocal_out,
        ));
        tracing::info!(
            "blend move: {} over {:.1} s ({:.0} BPM into {:.0} BPM, new song at {:.2}x)",
            fade.kind.name(),
            plan.len as f32 / self.sample_rate as f32,
            plan.a_bpm,
            plan.b_bpm,
            plan.speed
        );
        self.incoming = Some(crate::stretch::Source::new(
            self.sample_rate as u32,
            plan.b_down,
        ));
        self.fade = Some(fade);
        self.fade_frame(fade)
    }

    /// The move of the blend playing now (or the last one).
    pub fn last_move(&self) -> Option<crate::dj::Move> {
        self.fade.map(|f| f.kind).or(self.last_kind)
    }

    /// One frame of the blend. Both sources keep continuous DSP phase.
    fn fade_frame(&mut self, mut fade: Fade) -> Frame {
        use std::f32::consts::FRAC_PI_2;
        self.refilling = false;
        let a = self.stretch_hop();
        let Some(source) = self.incoming.as_mut() else {
            return a;
        };
        let Some(b) = source.next(&self.buf, self.base, fade.speed) else {
            return a;
        };
        fade.b_pos = source.position;
        let x = (fade.pos as f32 + 0.5) / fade.len as f32;
        let ga = (x * FRAC_PI_2).cos();
        let gb = ((x / fade.b_rise).min(1.0) * FRAC_PI_2).sin();
        let out = if let Some(h) = self.handover.as_mut() {
            h.frame(a, b, fade.pos)
        } else if let Some(fx) = self.fx.as_mut() {
            fx.frame(a, b, fade.pos)
        } else if fade.matched {
            let k = 1.0 - (-std::f32::consts::TAU * 180.0 / self.sample_rate as f32).exp();
            let w = ((x - 0.35) / 0.3).clamp(0.0, 1.0) * FRAC_PI_2;
            let (la, lb) = (w.cos(), w.sin());
            let mut out = SILENCE;
            for ch in 0..2 {
                fade.a_low[ch] += (a[ch] - fade.a_low[ch]) * k;
                fade.b_low[ch] += (b[ch] - fade.b_low[ch]) * k;
                out[ch] = (a[ch] - fade.a_low[ch]) * ga
                    + fade.a_low[ch] * la
                    + (b[ch] - fade.b_low[ch]) * gb
                    + fade.b_low[ch] * lb;
            }
            out
        } else {
            [a[0] * ga + b[0] * gb, a[1] * ga + b[1] * gb]
        };
        fade.pos += 1;
        if fade.pos >= fade.len {
            let skip = ((fade.b_pos.floor() as u64).saturating_sub(self.base) as usize)
                .min(self.buf.len());
            self.buf.drain(..skip);
            self.base += skip as u64;
            // Keep B's processor and phase. Recover its original tempo only
            // after the overlap; changing tempo during the overlap loses sync.
            self.stretch = self.incoming.take();
            self.last_kind = Some(fade.kind);
            // The handover's echo may ring on for a few beats.
            if let Some(h) = self.handover.take() {
                self.tail = Some((h, 0));
            }
            self.recovery = fade.matched.then_some((fade.speed, self.sample_rate * 6));
            self.fade = None;
            self.fx = None;
        } else {
            self.fade = Some(fade);
        }
        out
    }

    /// How quickly song B should come in, given how loud the last `len`
    /// frames of song A still are compared with the music before them.
    ///
    /// ```text
    ///  A ends at full level  ─► 1.0: classic equal-power crossfade
    ///  A fades out by itself ─► 0.3: B is up within a third of the fade
    /// ```
    fn rise_for_tail(&self, len: usize) -> f32 {
        let usual = self.loudness.sqrt();
        if usual <= 1e-9 {
            return 1.0;
        }
        let tail = self.level(0, len) / usual;
        (0.3 + (tail - 0.15) / (0.7 - 0.15) * 0.7).clamp(0.3, 1.0)
    }

    /// Tempo of the buffer between two indexes, if it has a clear beat.
    fn tempo_of(&self, from: usize, to: usize) -> Option<crate::tempo::Tempo> {
        let to = to.min(self.buf.len());
        if from >= to {
            return None;
        }
        let frames: Vec<Frame> = self.buf.range(from..to).copied().collect();
        crate::tempo::estimate(&frames, self.sample_rate as u32)
            .filter(|t| t.confidence >= MIN_BEAT_CONFIDENCE)
    }

    /// Does the sound captured most recently have a clear beat? (Automix
    /// only cuts a song short while it still has one.)
    pub fn has_beat_now(&self) -> bool {
        let to = self.buf.len();
        self.tempo_of(to.saturating_sub(self.sample_rate * 6), to)
            .is_some()
    }

    /// Automix: where to start reading song B, and how fast, so that its beats
    /// land on song A's. `None` = no clear beat on one side or tempos too far
    /// apart: a plain crossfade then.
    fn match_beats(&self, p: &Pending, b_start: u64) -> Option<(f64, f64)> {
        let (a_beat, a_period) = p.a_beat?;
        let from = (b_start - self.base) as usize;
        let Some(b) = self.tempo_of(from, from + self.sample_rate * 6) else {
            tracing::info!("automix: the new song has no clear beat yet, crossfading");
            return None;
        };
        // Beat trackers can choose half/double time for the same rhythm.
        let speed = [0.5, 1.0, 2.0]
            .into_iter()
            .map(|factor| b.period * factor / a_period)
            .min_by(|a, b| a.ln().abs().total_cmp(&b.ln().abs()))?;
        if speed.max(1.0 / speed) > MAX_TEMPO_RATIO {
            tracing::info!(
                "automix: tempos too far apart ({:.1} and {:.1} BPM), crossfading",
                60.0 * self.sample_rate as f64 / a_period,
                60.0 * self.sample_rate as f64 / b.period
            );
            return None;
        }
        // Frames until A's next beat, counted from where A is read now.
        let now = self.base as f64;
        let until_beat = ((now - a_beat) / a_period).ceil() * a_period + a_beat - now;
        // B must reach its first beat at that moment.
        let mut b_pos = b_start as f64 + b.first_beat - until_beat * speed;
        while b_pos < b_start as f64 {
            b_pos += b.period;
        }
        Some((b_pos, speed))
    }

    fn read_position(&self) -> u64 {
        self.base
    }

    // ---- finding the song boundary -------------------------------------

    fn level(&self, from: usize, len: usize) -> f32 {
        let to = (from + len).min(self.buf.len());
        if from >= to {
            return 0.0;
        }
        let sum: f32 = self
            .buf
            .range(from..to)
            .map(|f| f[0] * f[0] + f[1] * f[1])
            .sum();
        (sum / (2 * (to - from)) as f32).sqrt()
    }

    /// The song change is reported a moment after (or before) the sound
    /// changes. Look for the gap between the songs in the last 2.5 s; without
    /// a clear gap, assume the report was 0.3 s late.
    fn find_boundary(&self, end: u64) -> u64 {
        let step = self.sample_rate * 30 / 1000;
        let span = (self.sample_rate * 5 / 2).min(self.buf.len());
        let start = self.buf.len() - span;
        let (mut quietest, mut quietest_at, mut total, mut count) = (f32::MAX, 0usize, 0.0, 0);
        let mut at = start;
        while at + step <= self.buf.len() {
            let level = self.level(at, step);
            total += level;
            count += 1;
            if level < quietest {
                quietest = level;
                quietest_at = at;
            }
            at += step;
        }
        let average = if count > 0 { total / count as f32 } else { 0.0 };
        if count > 0 && quietest < average * 0.1 {
            self.base + (quietest_at + step / 2) as u64
        } else {
            end.saturating_sub((self.sample_rate * 3 / 10) as u64)
                .max(self.base)
        }
    }

    /// What counts as silence around `boundary`: a small fraction of the
    /// loudness of the 6 s before it.
    fn quiet_level(&self, boundary: u64) -> f32 {
        let at = (boundary.saturating_sub(self.base) as usize).min(self.buf.len());
        let span = (self.sample_rate * 6).min(at);
        (self.level(at - span, span) * QUIET_RATIO).max(1e-9)
    }

    /// Skips back over the silence at the end of song A (at most 4 s).
    fn last_audible_before(&self, boundary: u64) -> u64 {
        let quiet = self.quiet_level(boundary);
        let step = self.sample_rate / 100;
        let mut at = (boundary.saturating_sub(self.base) as usize).min(self.buf.len());
        let stop = at.saturating_sub(self.sample_rate * 4);
        while at >= stop + step && self.level(at - step, step) < quiet {
            at -= step;
        }
        self.base + at as u64
    }

    /// Skips the silence at the start of song B: up to 3 s of quiet (a song
    /// fading in keeps its intro), and up to 20 s of nothing at all (the
    /// service loading the next song, or a muted ad: live, YouTube Music once
    /// took 10 s, heard as a pause in the middle of a blend).
    fn first_audible_from(&self, boundary: u64) -> u64 {
        let quiet = self.quiet_level(boundary);
        let step = self.sample_rate / 100;
        let from = boundary.saturating_sub(self.base) as usize;
        let mut at = from;
        let quiet_stop = (at + self.sample_rate * 3).min(self.buf.len());
        let silent_stop = (at + self.sample_rate * 20).min(self.buf.len());
        while at + step <= silent_stop {
            let level = self.level(at, step);
            let skip = level < SILENT_LEVEL || (at + step <= quiet_stop && level < quiet);
            if !skip {
                break;
            }
            at += step;
        }
        if at - from > self.sample_rate * 3 {
            tracing::info!(
                "automix: skipped {:.1} s of silence before the new song",
                (at - from) as f32 / self.sample_rate as f32
            );
        }
        self.base + at as u64
    }

    // ---- refilling the delay (time stretch) ----------------------------

    fn wants_refill(&self) -> bool {
        !self.upstream_fill
            && self.blend.fade_secs().is_some()
            && self.buf.len()
                + if self.refilling {
                    self.sample_rate / 50
                } else {
                    self.sample_rate / 4
                }
                < self.target_delay()
            && !self.fade_is_near(self.sample_rate / 3)
            && self.recovery.is_none()
    }

    fn fade_is_near(&self, within: usize) -> bool {
        self.pending
            .is_some_and(|p| self.read_position() + (p.len + within) as u64 >= p.a_end)
    }

    fn start_stretch(&mut self) {
        if self.stretch.is_none() {
            self.stretch = Some(crate::stretch::Source::new(
                self.sample_rate as u32,
                self.base as f64,
            ));
        }
    }

    fn drop_stretch(&mut self) {
        self.stretch = None;
        self.incoming = None;
        self.refilling = false;
        self.recovery = None;
        self.handover = None;
        self.tail = None;
        self.drop_plan();
    }

    fn stretch_hop(&mut self) -> Frame {
        self.start_stretch();
        let rate = if let Some((initial, remaining)) = self.recovery {
            let length = self.sample_rate * 6;
            let rate = 1.0 + (initial - 1.0) * remaining as f64 / length as f64;
            self.recovery = (remaining > 1).then_some((initial, remaining.saturating_sub(1)));
            rate
        } else if self.refilling {
            REFILL_SPEED
        } else {
            1.0
        };
        let source = self.stretch.as_mut().unwrap();
        let Some(frame) = source.next(&self.buf, self.base, rate) else {
            return SILENCE;
        };
        let consumed = (source.position.floor() as u64).saturating_sub(self.base) as usize;
        let consumed = consumed.min(self.buf.len());
        self.buf.drain(..consumed);
        self.base += consumed as u64;
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48000;

    fn tone(freq: f32, frames: usize, phase_from: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                // f64 phase: f32 runs out of precision after a minute of samples.
                let turns = freq as f64 * (n + phase_from) as f64 / SR as f64;
                let s = ((std::f64::consts::TAU * turns.fract()).sin() * 0.5) as f32;
                [s, s]
            })
            .collect()
    }

    fn pull(p: &mut Player, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        p.pull(&mut out);
        out
    }

    /// Feeds and drains in 10 ms steps, like the live engine.
    fn run_live(p: &mut Player, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for block in input.chunks(960) {
            p.push(block);
            out.extend(pull(p, block.len() / 2));
        }
        out
    }

    fn level(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn the_new_song_starts_after_a_long_loading_silence_but_keeps_a_soft_intro() {
        let mut p = Player::new(SR);
        // Song A for 6 s, then the service loads the next song: 10 s of
        // nothing at all, then song B.
        p.push(&tone(440.0, 6 * SR as usize, 0));
        let boundary = p.base + p.buf.len() as u64;
        p.push(&vec![0.0; 2 * 10 * SR as usize]);
        p.push(&tone(330.0, 4 * SR as usize, 0));
        let start = p.first_audible_from(boundary) - boundary;
        assert!(
            (start as i64 - 10 * SR as i64).abs() <= SR as i64 / 50,
            "starts {:.2} s in",
            start as f32 / SR as f32
        );

        // A soft intro (quiet, but sound) is part of the song: at most 3 s of
        // it may be taken for silence.
        let mut p = Player::new(SR);
        p.push(&tone(440.0, 6 * SR as usize, 0));
        let boundary = p.base + p.buf.len() as u64;
        let soft: Vec<f32> = tone(330.0, 8 * SR as usize, 0)
            .iter()
            .map(|s| s * 0.004)
            .collect();
        p.push(&soft);
        let start = p.first_audible_from(boundary) - boundary;
        assert!(start <= 3 * SR as u64, "skipped {start} frames of intro");
    }

    #[test]
    fn off_passes_sound_through_unchanged_after_the_cushion() {
        let mut p = Player::new(SR);
        let input = tone(440.0, 48000, 0);
        let out = run_live(&mut p, &input);
        // Find where the sound starts; from there it must be an exact copy,
        // apart from the 10 ms fade-in.
        let lead = out.iter().position(|s| *s != 0.0).unwrap() & !1;
        assert!(lead / 2 <= 1440 * 2, "cushion too long: {lead}");
        let skip = 1200; // past the fade-in
        let shift = (0..4000)
            .step_by(2)
            .find(|d| out[lead + skip..lead + skip + 200] == input[skip + d..skip + d + 200]);
        assert!(shift.is_some(), "output is not a copy of the input");
        assert!(p.delay_secs() < 0.08);
    }

    #[test]
    fn crossfade_mode_builds_its_delay_without_breaking_the_sound() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        // 3.5 s of delay wanted, at 4 % slower: about 90 s of music.
        let input = tone(330.0, 48000 * 100, 0);
        let out = run_live(&mut p, &input);
        assert!(
            (p.delay_secs() - 3.5).abs() < 0.3,
            "delay {}",
            p.delay_secs()
        );
        // A pure tone stays a smooth pure tone: no jumps between neighbours.
        let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
        let steps: Vec<f32> = left.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let biggest_step = steps.iter().skip(9600).copied().fold(0.0f32, f32::max);
        let natural_step = 0.5 * std::f32::consts::TAU * 330.0 / SR as f32;
        assert!(
            biggest_step < natural_step * 1.6,
            "step {biggest_step} vs {natural_step}"
        );
        // And keeps its loudness.
        let mid = &out[out.len() / 2..out.len() / 2 + 96000];
        assert!((level(mid) - 0.3535).abs() < 0.03, "level {}", level(mid));
    }

    #[test]
    fn refilling_preserves_out_of_phase_stereo_without_volume_flutter() {
        for blend in [Blend::Crossfade { secs: 2.0 }, Blend::Automix { secs: 2.0 }] {
            let mut p = Player::new(SR);
            p.set_blend(blend);
            let mut input = tone(330.0, SR as usize * 4, 0);
            for frame in input.chunks_exact_mut(2) {
                frame[1] = -frame[0];
            }
            let out = run_live(&mut p, &input);
            // Ignore startup; a steady stereo tone must not acquire volume pulses.
            let quietest = out[SR as usize / 2..]
                .chunks_exact(1920)
                .map(level)
                .fold(f32::MAX, f32::min);
            assert!(
                quietest > 0.32,
                "{blend:?}: 20 ms RMS dropped to {quietest}, expected about 0.354"
            );
            assert!(p.delay_secs() > 0.1, "the refill must still build delay");
        }
    }

    #[test]
    fn refill_does_not_erase_treble_over_a_bass_note() {
        for blend in [Blend::Crossfade { secs: 2.0 }, Blend::Automix { secs: 2.0 }] {
            let mut p = Player::new(SR);
            p.set_blend(blend);
            let mut input = tone(330.0, SR as usize * 4, 0);
            for (n, frame) in input.chunks_exact_mut(2).enumerate() {
                let treble = [0.0, 0.2, 0.0, -0.2][n % 4];
                frame[0] = frame[0] * 0.6 + treble;
                frame[1] = frame[0];
            }
            let out = run_live(&mut p, &input);
            // Measure the 12 kHz component separately: bass RMS hides lost treble.
            let quietest = out[SR as usize / 2..]
                .chunks_exact(1920)
                .map(|chunk| {
                    let (mut re, mut im) = (0.0f32, 0.0f32);
                    for (n, frame) in chunk.chunks_exact(2).enumerate() {
                        re += frame[0] * [1.0, 0.0, -1.0, 0.0][n % 4];
                        im += frame[0] * [0.0, 1.0, 0.0, -1.0][n % 4];
                    }
                    2.0 * re.hypot(im) / 960.0
                })
                .fold(f32::MAX, f32::min);
            eprintln!("{blend:?}: minimum treble amplitude {quietest:.6}");
            assert!(
                quietest > 0.17,
                "{blend:?}: treble amplitude {quietest}, expected 0.2"
            );
        }
    }

    #[test]
    fn automatic_change_is_blended_and_uses_up_the_fade_length() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        // Song A (left channel only would be overkill: use two tones), a short
        // gap, then the start of song B; the change is reported 0.2 s into B.
        let a = tone(440.0, 48000 * 6, 0);
        let gap = vec![0.0; 2 * 2400];
        let b = tone(1760.0, 48000 * 6, 0);
        p.push(&a);
        p.push(&gap);
        p.push(&b[..2 * 9600]);
        p.track_changed(true);
        p.push(&b[2 * 9600..]);
        assert!(p.pending.is_some());
        let before = p.delay_frames();
        let mut out = Vec::new();
        while p.delay_frames() > 4800 {
            out.extend(pull(&mut p, 480));
            if p.is_refilling() {
                break; // refilling starts once B is underway
            }
        }
        assert!(before - 2 * 48000 > 0);
        // A plays alone, then both, then B alone. With no gap in the result.
        let a_only = &out[..2 * 48000 * 3];
        assert!((level(a_only) - 0.3535).abs() < 0.02);
        let quietest = out
            .chunks(960)
            .take(out.len() / 960 - 2)
            .map(level)
            .fold(f32::MAX, f32::min);
        assert!(quietest > 0.2, "there is a hole in the blend: {quietest}");
        // The fade removed about 2 s (+ the 50 ms gap) from the timeline.
        let produced = out.len() / 2;
        let consumed = 48000 * 12 + 2400 - p.buf.len();
        let saved = consumed as i64 - produced as i64;
        assert!(
            (saved - (2 * 48000 + 2400)).abs() < 4800,
            "saved {saved} frames"
        );
    }

    #[test]
    fn very_quiet_songs_are_blended_too() {
        // The service's own volume slider turned almost all the way down.
        let quiet = |x: Vec<f32>| x.into_iter().map(|s| s * 0.002).collect::<Vec<f32>>();
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        p.push(&quiet(tone(440.0, 48000 * 6, 0)));
        p.push(&vec![0.0; 2 * 2400]);
        p.push(&quiet(tone(1760.0, 9600, 0)));
        p.track_changed(true);
        let planned = p.pending.expect("a quiet song still gets its blend");
        assert_eq!(planned.len, 2 * 48000);
    }

    /// A kick drum on every beat over a steady tone: music with a clear beat.
    fn beat_track(bpm: f32, tone_hz: f32, frames: usize, first_beat: usize) -> Vec<f32> {
        let period = (SR as f32 * 60.0 / bpm) as usize;
        (0..frames)
            .flat_map(|n| {
                let since = (n + period - first_beat % period) % period;
                let t = since as f32 / SR as f32;
                let kick = (std::f32::consts::TAU * 60.0 * t).sin() * (-t * 18.0).exp() * 0.5;
                let pad = (std::f32::consts::TAU * tone_hz * n as f32 / SR as f32).sin() * 0.05;
                [kick + pad, kick + pad]
            })
            .collect()
    }

    fn automix_fade(a_bpm: f32, b_bpm: f32) -> Option<Fade> {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Automix { secs: 4.0 });
        p.push(&beat_track(a_bpm, 440.0, 48000 * 10, 0));
        // Song B starts right away (the service cut A when "next" was pressed).
        let b = beat_track(b_bpm, 660.0, 48000 * 12, 7000);
        p.push(&b[..2 * 14400]);
        p.track_changed(true);
        p.push(&b[2 * 14400..]);
        assert!(p.pending.is_some());
        for _ in 0..48000 * 10 / 480 {
            pull(&mut p, 480);
            if let Some(fade) = p.fade {
                return Some(fade);
            }
        }
        None
    }

    #[test]
    fn automix_hands_over_like_a_dj_even_past_a_beatless_intro() {
        // Old song: kicks at 100 BPM, a new bass note every bar.
        // New song: 14 s of soft chords (no beat), then kicks at 110 BPM: the
        // first look (its 0..15 s) has almost no beat; the second look must.
        let bassline = |song: &mut Vec<Frame>, bpm: f32, from_s: f32| {
            let period = 60.0 / bpm;
            for (n, f) in song.iter_mut().enumerate() {
                let t = n as f32 / SR as f32;
                if t < from_s {
                    continue;
                }
                let bar = (((t - from_s) / period) as usize / 4) % 4;
                let s = (std::f32::consts::TAU * [55.0, 73.4, 61.7, 82.4][bar] * t).sin() * 0.2;
                f[0] += s;
                f[1] += s;
            }
        };
        let mut a = crate::tempo::tests::kicks(100.0, 60.0, 0.0);
        bassline(&mut a, 100.0, 0.0);
        let mut b = crate::tempo::tests::kicks(110.0, 40.0, 14.0);
        for (n, f) in b.iter_mut().enumerate().take(SR as usize * 14) {
            let t = n as f32 / SR as f32;
            let pad = [220.0f32, 277.2, 329.6]
                .iter()
                .map(|hz| (std::f32::consts::TAU * hz * t).sin())
                .sum::<f32>()
                * 0.08;
            f[0] += pad;
            f[1] += pad;
        }
        // Real intros aren't silent between notes: hits at random times (no
        // steady beat), like percussion noises or vocal chops.
        let mut seed = 99u32;
        let mut at = 0usize;
        while at < SR as usize * 14 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            at += SR as usize / 8 + (seed >> 8) as usize % (SR as usize / 2);
            for k in 0..2400.min(b.len().saturating_sub(at)) {
                let hit = (std::f32::consts::TAU * 180.0 * k as f32 / SR as f32).sin()
                    * (-(k as f32) / 300.0).exp()
                    * 0.5;
                b[at + k][0] += hit;
                b[at + k][1] += hit;
            }
        }
        bassline(&mut b, 110.0, 14.0);
        let flat = |x: &[Frame]| x.iter().flat_map(|f| [f[0], f[1]]).collect::<Vec<f32>>();
        let (a, b) = (flat(&a), flat(&b));

        let mut p = Player::new(SR);
        p.set_blend(Blend::Automix { secs: 6.0 });
        let delay = 34 * SR as usize;
        // A full delay already (as after fast fill), then live 10 ms blocks.
        p.push(&a[..delay * 2]);
        let input: Vec<f32> = a[delay * 2..].iter().chain(&b).copied().collect();
        let b_starts = (a.len() / 2) - delay;
        let mut out = vec![0.0; 960];
        let mut moves = Vec::new();
        for (k, block) in input.chunks(960).enumerate() {
            p.push(block);
            while p.is_studying() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if (k + 1) * 480 == b_starts + SR as usize * 3 / 10 {
                p.track_changed(true);
            }
            p.pull(&mut out);
            if p.is_blending() && moves.is_empty() {
                moves.push(p.last_move());
            }
        }
        assert_eq!(
            moves,
            vec![Some(crate::dj::Move::Handover)],
            "no DJ handover"
        );
        let (old_bpm, new_bpm) = p.beat_match().expect("tempos");
        assert!(
            (old_bpm - 100.0).abs() < 1.5 && (new_bpm - 110.0).abs() < 1.5,
            "{old_bpm} {new_bpm}"
        );
    }

    #[test]
    fn automix_lines_up_beats_of_songs_with_close_tempos() {
        let fade = automix_fade(120.0, 123.0).expect("a blend starts");
        assert!(fade.matched, "tempos 2.5 % apart should be beat-matched");
        // B (faster) is slowed to A's tempo at first.
        assert!(
            (fade.speed - 120.0 / 123.0).abs() < 0.01,
            "speed {}",
            fade.speed
        );
    }

    #[test]
    fn automix_matches_different_tempos_without_the_old_four_percent_limit() {
        let fade = automix_fade(120.0, 96.0).expect("a blend starts");
        assert!(fade.matched);
        assert!((fade.speed - 1.25).abs() < 0.02);
    }

    #[test]
    fn wider_tempo_mix_preserves_incoming_note_through_overlap_and_recovery() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Automix { secs: 4.0 });
        p.push(&beat_track(120.0, 440.0, 48000 * 10, 0));
        let b = beat_track(96.0, 660.0, 48000 * 15, 7000);
        p.push(&b[..28800]);
        p.track_changed(true);
        p.push(&b[28800..]);
        while p.fade.is_none() {
            pull(&mut p, 1);
        }
        assert!(p.fade.unwrap().matched);
        // Sample B while it is playing 25% faster, and later during recovery.
        let mut output = pull(&mut p, 48000 * 8);
        for start in [48000 * 2, 48000 * 5] {
            let segment = &mut output[start * 2..(start + 48000) * 2];
            let amplitude = |hz: f64| {
                let (mut re, mut im) = (0.0, 0.0);
                for (n, f) in segment.chunks_exact(2).enumerate() {
                    let phase = std::f64::consts::TAU * hz * n as f64 / 48000.0;
                    re += f[0] as f64 * phase.cos();
                    im += f[0] as f64 * phase.sin();
                }
                2.0 * re.hypot(im) / 48000.0
            };
            assert!(amplitude(660.0) > 0.025, "incoming note lost at {start}");
            assert!(
                amplitude(825.0) < 0.005,
                "incoming note was pitched up at {start}"
            );
        }
    }

    #[test]
    fn automix_with_a_short_delay_makes_a_shorter_blend_on_the_beat() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Automix { secs: 6.0 });
        // Only 6 s of delay so far (a full one would be 12.5 s).
        p.push(&beat_track(120.0, 440.0, 48000 * 6, 0));
        let b = beat_track(122.0, 660.0, 48000 * 12, 5000);
        p.push(&b[..2 * 9600]);
        p.track_changed(true);
        p.push(&b[2 * 9600..]);
        let planned = p.pending.expect("a blend is planned");
        assert!(planned.a_beat.is_some());
        let secs = planned.len as f32 / SR as f32;
        assert!((1.5..3.2).contains(&secs), "blend of {secs} s");
        while p.fade.is_none() {
            pull(&mut p, 1);
        }
        assert!(p.fade.unwrap().matched);
    }

    #[test]
    fn automix_falls_back_to_a_crossfade_when_tempos_differ() {
        let fade = automix_fade(120.0, 85.0).expect("a blend starts");
        assert!(!fade.matched);
        assert_eq!(fade.speed, 1.0);
    }

    #[test]
    fn automix_beats_coincide_during_the_blend() {
        // Two click tracks: after matching, B's clicks must land on A's.
        let mut p = Player::new(SR);
        p.set_blend(Blend::Automix { secs: 4.0 });
        p.push(&beat_track(120.0, 440.0, 48000 * 10, 0));
        let b = beat_track(122.0, 660.0, 48000 * 12, 9000);
        p.push(&b[..2 * 14400]);
        p.track_changed(true);
        p.push(&b[2 * 14400..]);
        let pending = p.pending.unwrap();
        let (a_beat, a_period) = pending.a_beat.expect("song A has a beat");
        // A's real beats are at multiples of 24000 frames.
        let off = a_beat.rem_euclid(24000.0);
        assert!(off.min(24000.0 - off) < 720.0, "A beat found at {a_beat}");
        assert!((a_period - 24000.0).abs() < 240.0);
        while p.fade.is_none() {
            pull(&mut p, 1);
        }
        let fade = p.fade.unwrap();
        assert!(fade.matched);
        // B's beats are at boundary + 9000 + k·period (in B's own frames).
        // When A (read at `base`) reaches its next beat, B must be on one too.
        let b_period = (SR as f64 * 60.0 / 122.0).floor();
        let b_zero = pending.boundary as f64 + 9000.0;
        let until =
            ((p.base as f64 - a_beat) / a_period).ceil() * a_period + a_beat - p.base as f64;
        let b_then = fade.b_pos + until * fade.speed;
        let miss = (b_then - b_zero).rem_euclid(b_period);
        assert!(
            miss.min(b_period - miss) < 1500.0,
            "beats miss each other by {miss} frames"
        );
    }

    #[test]
    fn a_song_that_fades_out_lets_the_next_one_in_quickly() {
        let run = |fading: bool| -> f32 {
            let mut p = Player::new(SR);
            p.set_blend(Blend::Crossfade { secs: 4.0 });
            // Play some of A first so its usual loudness is known.
            let mut a = tone(440.0, 48000 * 12, 0);
            if fading {
                // The last 5 s die away, like most recorded endings.
                let n = a.len();
                for (i, s) in a.iter_mut().enumerate().skip(n - 2 * 48000 * 5) {
                    let left = (n - i) as f32 / (2.0 * 48000.0 * 5.0);
                    *s *= left * left * left;
                }
            }
            p.push(&a);
            pull(&mut p, 48000 * 6);
            let b = tone(1760.0, 48000 * 8, 0);
            p.push(&b[..2 * 9600]);
            p.track_changed(true);
            p.push(&b[2 * 9600..]);
            while p.fade.is_none() {
                pull(&mut p, 1);
            }
            p.fade.unwrap().b_rise
        };
        assert!(run(false) > 0.95, "full-level ending: classic crossfade");
        assert!(
            run(true) < 0.8,
            "fading ending: the new song comes in faster"
        );
    }

    #[test]
    fn manual_skip_cuts_at_once() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 4.0 });
        p.push(&tone(440.0, 48000 * 5, 0));
        assert!(p.delay_secs() > 4.9);
        p.track_changed(false);
        assert_eq!(p.delay_frames(), 0);
        let out = run_live(&mut p, &tone(880.0, 24000, 0));
        assert!(level(&out) > 0.1, "new song is not heard right away");
    }

    #[test]
    fn pause_freezes_the_buffer() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        p.push(&tone(440.0, 48000, 0));
        p.push(&vec![0.0; 2000]); // silence captured before the pause notice
        p.set_paused(true);
        let held = p.delay_frames();
        assert_eq!(held, 48000);
        assert!(pull(&mut p, 4800).iter().all(|s| *s == 0.0));
        p.push(&vec![0.0; 960]);
        assert_eq!(p.delay_frames(), held);
        // The player's short fade-out arrives after the notice: still paused.
        p.push(&tone(440.0, 9600, 0));
        assert!(pull(&mut p, 4800).iter().all(|s| *s == 0.0));
        // Sound that keeps coming means playback resumed, even without a notice.
        p.push(&tone(440.0, 48000, 0));
        assert!(level(&pull(&mut p, 4800)) > 0.1);
    }

    #[test]
    fn pause_does_not_reuse_samples_already_fed_to_native_lookahead() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        p.push(&tone(440.0, 2000, 0));
        p.push(&vec![0.0; 40000]);
        pull(&mut p, 1000);
        let fed = p.stretch.as_ref().unwrap().fed_until();
        assert!(fed > 2000, "fixture must have fed some trailing silence");
        p.set_paused(true);
        assert!(p.base + p.buf.len() as u64 >= fed);
        let before = p.base;
        pull(&mut p, 1000);
        assert_eq!(p.base, before);
        p.set_paused(false);
        p.push(&tone(660.0, 48000, 0));
        let out = pull(&mut p, 30000);
        assert!(level(&out[out.len() - 10000..]) > 0.3);
    }

    #[test]
    fn turning_the_blend_off_returns_to_live() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 6.0 });
        p.push(&tone(440.0, 48000 * 7, 0));
        p.set_blend(Blend::Off);
        assert!(p.delay_secs() < 0.05, "delay {}", p.delay_secs());

        // Even a small leftover delay goes: without a blend, sound is live.
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 6.0 });
        p.push(&tone(440.0, 48000 * 7 / 10, 0));
        p.set_blend(Blend::Off);
        assert!(p.delay_secs() < 0.09, "delay {}", p.delay_secs());
    }

    #[test]
    fn turning_blend_off_stops_the_processor_even_with_a_short_buffer() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 6.0 });
        let input = tone(440.0, SR as usize, 0);
        run_live(&mut p, &input);
        assert!(p.stretch.is_some());
        // Deliberately test below the delay-trimming threshold.
        p.buf.truncate(2000);
        p.set_blend(Blend::Off);
        assert!(p.stretch.is_none());
        assert_eq!(p.speed(), 1.0);
    }

    #[test]
    fn turning_the_blend_on_while_playing_starts_filling_the_delay() {
        let mut p = Player::new(SR);
        let input = tone(330.0, 48000 * 30, 0);
        run_live(&mut p, &input[..2 * 48000 * 2]);
        assert!(p.delay_secs() < 0.08);
        p.set_blend(Blend::Crossfade { secs: 2.0 });
        run_live(&mut p, &input[2 * 48000 * 2..]);
        assert!(p.delay_secs() > 0.8, "delay {}", p.delay_secs());
    }

    #[test]
    fn too_little_delay_means_a_plain_change() {
        let mut p = Player::new(SR);
        p.set_blend(Blend::Crossfade { secs: 6.0 });
        p.push(&tone(440.0, 48000 / 2, 0));
        p.track_changed(true);
        assert!(p.pending.is_none());
    }
}
