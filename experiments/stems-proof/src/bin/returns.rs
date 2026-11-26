//! Mid-song check proof: where the singer comes back after a pause, on real
//! songs, against lyrics shifted by known amounts.
//! `cargo run --release --bin returns -- <song> <lrc>`
use audio_engine::voice::{Line, VoiceMeter};
use mid::{Drift, Verdict};
use stems_proof::{SR, decode};

fn lines(path: &str) -> Vec<Line> {
    let text = std::fs::read_to_string(path).expect("lrc");
    let mut out: Vec<Line> = text
        .lines()
        .filter_map(|raw| {
            let rest = raw.trim_start_matches('\u{feff}').strip_prefix('[')?;
            let (stamp, words) = rest.split_once(']')?;
            let (m, s) = stamp.split_once(':')?;
            let t = (m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?) * 1000.0;
            Some(Line {
                time_ms: t,
                sung: !words.trim().is_empty(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
    out
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let frames = decode(&a[0]);
    let flat: Vec<f32> = frames.iter().flat_map(|f| [f[0], f[1]]).collect();
    let mut m = VoiceMeter::new(SR);
    let mut trace = Vec::new();
    m.process(&flat, |h| trace.push(h.voice()));
    let lyr = lines(&a[1]);
    let rets = mid::returns(&lyr);
    if std::env::var("GLOWTUNE_STEPS").is_ok() {
        steps(&trace, &rets);
        return;
    }
    println!(
        "{} lines, {} sung, {} returns: {:?}",
        lyr.len(),
        lyr.iter().filter(|l| l.sung).count(),
        rets.len(),
        rets.iter().map(|r| (r.time_ms / 1000.0 * 10.0).round() / 10.0).collect::<Vec<_>>()
    );
    // The lyrics claim each line `shift` ms later than the file: the timing
    // in use is off by -shift, so a correct check finds miss = -shift.
    for shift in [0.0, 1000.0, -1000.0, 2000.0] {
        let mut drift = Drift::default();
        let mut found = 0;
        let mut errs = Vec::new();
        let mut verdicts = Vec::new();
        let mut extra = 0.0; // shifts the check applied so far
        for r in &rets {
            if r.time_ms < 1000.0 {
                continue;
            }
            let expected = r.time_ms + shift - extra;
            match mid::return_entry(&trace, 0.0, *r, expected, 1.5) {
                Some(e) => {
                    found += 1;
                    let miss = e.at_ms - expected;
                    // Truth: the voice is at r.time_ms (the file is right).
                    errs.push(e.at_ms - r.time_ms);
                    let v = drift.add(miss);
                    if let Verdict::Shift(s) = v {
                        extra -= s;
                    }
                    verdicts.push(format!("{:.1}s:{:+.2}→{v:?}", r.time_ms / 1000.0, miss / 1000.0));
                }
                None => verdicts.push(format!("{:.1}s:-", r.time_ms / 1000.0)),
            }
        }
        println!(
            "shift {:+.1}s: found {found}/{}, entry error vs file (s): {:?}\n   final timing off by {:+.2}s\n   {}",
            shift / 1000.0,
            rets.len(),
            errs.iter().map(|e| (e / 10.0).round() / 100.0).collect::<Vec<_>>(),
            (shift - extra) / 1000.0,
            verdicts.join("  ")
        );
    }
}

/// Diagnostic: the step at each return with a low bar (`GLOWTUNE_STEPS=1`).
#[allow(dead_code)]
pub fn steps(trace: &[f32], rets: &[mid::Return]) {
    for r in rets {
        let e = mid::return_entry(trace, 0.0, *r, r.time_ms, 0.2);
        match e {
            Some(e) => println!(
                "  return {:.1}s: entry {:.2}s (err {:+.2}), step {:.2}",
                r.time_ms / 1000.0,
                e.at_ms / 1000.0,
                (e.at_ms - r.time_ms) / 1000.0,
                e.step
            ),
            None => println!("  return {:.1}s: nothing", r.time_ms / 1000.0),
        }
    }
}

/// The mid-song check that failed this proof (2026-11-26): moved out of
/// the app, kept here for a better voice detector.
mod mid {
    use audio_engine::voice::{self, Entry, Line};
/// Longest stretch one line counts as sung (as in voice.rs).
const LINE_MAX_MS: f64 = 5000.0;
    // ---- the singer coming back, mid-song ---------------------------------
    //
    //   lyrics: … line ··· (no line sung for 3 s or more) ··· line …
    //                                                         ▲ a "return"
    //   sound:  the voice should rise right there (shifted by the timing in use)
    //   ──► first_entry in a small window around it ──► how far off it is
    //
    // Only returns count: in the middle of a verse the voice never goes quiet
    // long enough to hear where a line starts (that's why `align` failed).

    /// How long the lyrics must have nobody singing before a line, for the
    /// voice coming back to be heard: `first_entry` compares 3 s each side.
    pub const RETURN_GAP_MS: f64 = 3000.0;

    /// A sung line the singer comes back to after a pause.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Return {
        /// The line's time in the lyrics.
        pub time_ms: f64,
        /// When the singing before it stops, by the lyrics (each line until the
        /// next, at most 5 s; a blank line ends it). 0 for the first sung line.
        pub quiet_from_ms: f64,
    }

    /// Every sung line with at least `RETURN_GAP_MS` of nobody singing before
    /// it, the first sung line included. `lines` sorted by time.
    pub fn returns(lines: &[Line]) -> Vec<Return> {
        let mut out = Vec::new();
        // When the last sung line stops, by the lyrics.
        let mut quiet_from: Option<f64> = None;
        for (i, line) in lines.iter().enumerate() {
            if line.sung {
                let from = quiet_from.unwrap_or(0.0);
                if line.time_ms - from >= RETURN_GAP_MS {
                    out.push(Return {
                        time_ms: line.time_ms,
                        quiet_from_ms: from,
                    });
                }
                let next = lines.get(i + 1).map_or(f64::INFINITY, |n| n.time_ms);
                quiet_from = Some(next.min(line.time_ms + LINE_MAX_MS));
            }
        }
        out
    }

    /// How far a return is searched either side of where the timing in use
    /// puts it. Wider would reach into the verse before it.
    pub const RETURN_SEARCH_MS: f64 = 2500.0;

    /// Where the voice comes back for `ret`, which the timing in use puts at
    /// sound time `expected_ms` (its lyrics time minus the offset), in `trace`
    /// (song time `start_ms + k·HOP_MS`). `None`: not clear, or not captured.
    pub fn return_entry(
        trace: &[f32],
        start_ms: f64,
        ret: Return,
        expected_ms: f64,
        min_step: f32,
    ) -> Option<Entry> {
        let shift = expected_ms - ret.time_ms;
        // Never before the singing before it stops (by the same timing), plus
        // a little slack for a held last word.
        let from = (expected_ms - RETURN_SEARCH_MS).max(ret.quiet_from_ms + shift + 500.0);
        voice::first_entry(
            trace,
            start_ms,
            from,
            expected_ms + RETURN_SEARCH_MS,
            min_step,
        )
    }

    /// What the mid-song check makes of one more measured return.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Verdict {
        /// Not enough to go on yet.
        Wait,
        /// The lyrics are on time.
        InTime,
        /// Two returns in a row agree the lyrics are off: show every line this
        /// much later (negative: earlier), on top of the timing in use.
        Shift(f64),
        /// Two returns in a row disagree by a lot: these lyrics aren't timed
        /// to this recording (another version, or timed by hand badly).
        Mismatch,
    }

    /// Misses smaller than this are on time (eyes don't see it).
    pub const IN_TIME_MS: f64 = 350.0;
    /// Two misses this close together agree.
    pub const AGREE_MS: f64 = 500.0;
    /// Two misses this far apart: the lyrics don't fit this recording.
    pub const MISMATCH_MS: f64 = 1500.0;

    /// The mid-song check's memory: one decision per pair of returns, so one
    /// bad measurement never moves the lyrics.
    #[derive(Debug, Default, Clone)]
    pub struct Drift {
        /// The last miss measured (ms; positive: the voice came later than the
        /// lyrics said), against the timing in use now.
        last: Option<f64>,
    }

    impl Drift {
        /// One more return measured, `miss_ms` off (positive: the voice came
        /// later than the timing in use said).
        pub fn add(&mut self, miss_ms: f64) -> Verdict {
            let Some(last) = self.last.replace(miss_ms) else {
                return if miss_ms.abs() < IN_TIME_MS {
                    Verdict::InTime
                } else {
                    Verdict::Wait
                };
            };
            if miss_ms.abs() < IN_TIME_MS && last.abs() < IN_TIME_MS {
                return Verdict::InTime;
            }
            if (miss_ms - last).abs() <= AGREE_MS {
                let mean = (miss_ms + last) / 2.0;
                if mean.abs() < IN_TIME_MS {
                    return Verdict::InTime;
                }
                // Measured against the old timing: start over against the new.
                self.last = None;
                return Verdict::Shift(mean);
            }
            if (miss_ms - last).abs() >= MISMATCH_MS {
                return Verdict::Mismatch;
            }
            Verdict::Wait
        }
    }


}
