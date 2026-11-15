//! Lyrics alignment proof on real songs (no AI): does `voice::align` find a
//! shift we put into the lyrics, and does it stay quiet on the wrong lyrics?
//!
//! `cargo run --release --bin align -- <song.mp3> <lyrics.lrc> [<other.lrc>]`
//!
//! ```text
//!  per window (0–45 s, 30–75 s, …) and per shift (0, ±2, ±5, ±10 s):
//!    found lag  (should be: the song's own offset L0 − shift)
//!    peak / runner-up / at zero   (how clearly it stands out)
//! ```

use audio_engine::voice::{self, Line, VoiceMeter};
use stems_proof::{SR, decode};

fn parse_lrc(path: &str) -> Vec<Line> {
    let text = std::fs::read_to_string(path).expect("lrc");
    let mut lines = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_start_matches('\u{feff}');
        let Some(rest) = raw.strip_prefix('[') else {
            continue;
        };
        let Some((stamp, words)) = rest.split_once(']') else {
            continue;
        };
        let Some((m, s)) = stamp.split_once(':') else {
            continue;
        };
        let (Ok(m), Ok(s)) = (m.parse::<f64>(), s.parse::<f64>()) else {
            continue;
        };
        lines.push(Line {
            time_ms: (m * 60.0 + s) * 1000.0,
            sung: !words.trim().is_empty(),
        });
    }
    lines.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
    lines
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (song, lrc) = (&args[0], &args[1]);
    let frames = decode(song);
    let flat: Vec<f32> = frames.iter().flat_map(|f| [f[0], f[1]]).collect();
    let mut meter = VoiceMeter::new(SR);
    let mut trace = Vec::new();
    meter.process(&flat, |h| trace.push(h.voice()));
    let secs = frames.len() as f64 / SR as f64;
    println!("{song}: {secs:.1} s, {} hops", trace.len());

    let lines = parse_lrc(lrc);
    println!(
        "{lrc}: {} lines, first sung at {:.1} s",
        lines.len(),
        lines
            .iter()
            .find(|l| l.sung)
            .map_or(0.0, |l| l.time_ms / 1000.0)
    );
    let hops = |s: f64| (s * 1000.0 / voice::HOP_MS) as usize;
    let windows: Vec<(f64, f64)> = (0..)
        .map(|i| (i as f64 * 30.0, i as f64 * 30.0 + 45.0))
        .take_while(|w| w.1 <= secs)
        .collect();
    println!("window      shift   found   L0=found+shift   peak  runner  zero");
    for &(a, b) in &windows {
        for shift in [0.0, 2000.0, -2000.0, 5000.0, -5000.0, 10_000.0, -10_000.0] {
            // The lyrics say every line comes `shift` later than they really do.
            let shifted: Vec<Line> = lines
                .iter()
                .map(|l| Line {
                    time_ms: l.time_ms + shift,
                    ..*l
                })
                .collect();
            let w = &trace[hops(a)..hops(b).min(trace.len())];
            match voice::align(w, a * 1000.0, &shifted, 12_000.0) {
                Some(f) => println!(
                    "{a:>4.0}-{b:<4.0}  {:>+6.1}  {:>+6.2}   {:>+6.2}          {:.3}  {:.3}  {:.3}",
                    shift / 1000.0,
                    f.lag_ms / 1000.0,
                    (f.lag_ms + shift) / 1000.0,
                    f.peak,
                    f.runner_up,
                    f.at_zero
                ),
                None => println!("{a:>4.0}-{b:<4.0}  {:>+6.1}  (no answer)", shift / 1000.0),
            }
        }
    }
    if let Some(other) = args.get(2) {
        println!("\nwrong lyrics ({other}): must not stand out");
        let wrong = parse_lrc(other);
        for &(a, b) in &windows {
            let w = &trace[hops(a)..hops(b).min(trace.len())];
            match voice::align(w, a * 1000.0, &wrong, 12_000.0) {
                Some(f) => println!(
                    "{a:>4.0}-{b:<4.0}  found {:>+6.2}  peak {:.3}  runner {:.3}  zero {:.3}",
                    f.lag_ms / 1000.0,
                    f.peak,
                    f.runner_up,
                    f.at_zero
                ),
                None => println!("{a:>4.0}-{b:<4.0}  (no answer)"),
            }
        }
    }
}
