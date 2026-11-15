//! How much of a song must the live engine hear to find its beat and
//! downbeat as well as from 60 s? (No AI: full mix, like the live engine.)
//! Usage: lookahead <song>...
//!
//! For each song: the reference grid from its first 60 s, then the grid
//! from only its first 6 / 10 / 15 / 20 s (what the engine has of a new
//! song), and from only its last 8 / 15 / 20 s (what it has of the old one).
use audio_engine::tempo::{self, Tempo};
use stems_proof::{Frame, SR, decode};

fn grid(x: &[Frame]) -> Option<(Tempo, f64)> {
    let t = tempo::estimate(x, SR).filter(|t| t.confidence >= 0.35)?;
    // Downbeat: kick strength + bass/harmony change per beat (tempo::downbeat);
    // LOOKAHEAD_OLD=1 uses the old strongest-kick heuristic for comparison.
    if std::env::var_os("LOOKAHEAD_OLD").is_none() {
        let (down, _) = tempo::downbeat(x, SR, &t)?;
        return Some((t, down));
    }
    // Downbeat: the phase of 4 with the most low-end punch.
    let k = 1.0 - (-std::f32::consts::TAU * 150.0 / SR as f32).exp();
    let mut y = 0.0;
    let low: Vec<f32> = x
        .iter()
        .map(|f| {
            y += ((f[0] + f[1]) * 0.5 - y) * k;
            y
        })
        .collect();
    let around = (SR as f64 * 0.04) as usize;
    let mut punch = [0f64; 4];
    let mut n = 0;
    loop {
        let at = (t.first_beat + n as f64 * t.period) as usize;
        if at + around >= low.len() {
            break;
        }
        punch[n % 4] += low[at.saturating_sub(around)..at + around]
            .iter()
            .map(|v| (v * v) as f64)
            .sum::<f64>();
        n += 1;
    }
    let phase = (0..4).max_by(|a, b| punch[*a].total_cmp(&punch[*b]))?;
    let down = t.first_beat + phase as f64 * t.period;
    Some((t, down))
}

/// Same tempo (or half/double) within 2 %, and the same downbeat position
/// (within 1/8 beat), measured in the reference's frame of reference.
fn agrees(r: &(Tempo, f64), g: &(Tempo, f64), offset: f64) -> (bool, bool) {
    let ratio = g.0.period / r.0.period;
    let tempo_ok = [0.5, 1.0, 2.0]
        .iter()
        .any(|k| (ratio / k - 1.0).abs() < 0.02);
    let bar = 4.0 * r.0.period;
    let diff = ((g.1 + offset) - r.1).rem_euclid(bar);
    let down_ok = diff.min(bar - diff) < r.0.period / 8.0;
    (tempo_ok, tempo_ok && down_ok)
}

fn main() {
    let songs: Vec<String> = std::env::args().skip(1).collect();
    println!(
        "{:<34} {:>7} | new song: first 6/10/15/20 s | old song: last 8/15/20 s",
        "song", "BPM"
    );
    let mut tally = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for path in &songs {
        let name: String = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().chars().take(34).collect())
            .unwrap_or_default();
        let x = decode(path);
        let loud = x
            .iter()
            .position(|f| f[0].abs().max(f[1].abs()) > 0.01)
            .unwrap_or(0);
        let x = &x[loud..];
        let sec = SR as usize;
        let mut line = String::new();
        // New-song role: the start.
        let Some(r_head) = grid(&x[..(60 * sec).min(x.len())]) else {
            println!("{name:<34}    none | no clear beat in the first 60 s");
            continue;
        };
        for s in [6, 10, 15, 20] {
            let g = grid(&x[..(s * sec).min(x.len())]);
            let mark = match g.map(|g| agrees(&r_head, &g, 0.0)) {
                Some((_, true)) => "beat+bar",
                Some((true, false)) => "beat   ",
                Some(_) => "WRONG  ",
                None => "none   ",
            };
            let e = tally.entry(format!("new {s:>2} s")).or_default();
            e.1 += 1;
            if mark == "beat+bar" {
                e.0 += 1;
            }
            line += &format!(" {mark}");
        }
        line += " |";
        // Old-song role: the end.
        let end = x
            .iter()
            .rposition(|f| f[0].abs().max(f[1].abs()) > 0.01)
            .unwrap_or(x.len());
        let x = &x[..end];
        let tail60 = x.len().saturating_sub(60 * sec);
        if let Some(r_tail) = grid(&x[tail60..]) {
            for s in [8, 15, 20] {
                let from = x.len().saturating_sub(s * sec);
                let g = grid(&x[from..]);
                let mark = match g.map(|g| agrees(&r_tail, &g, (from - tail60) as f64)) {
                    Some((_, true)) => "beat+bar",
                    Some((true, false)) => "beat   ",
                    Some(_) => "WRONG  ",
                    None => "none   ",
                };
                let e = tally.entry(format!("old {s:>2} s")).or_default();
                e.1 += 1;
                if mark == "beat+bar" {
                    e.0 += 1;
                }
                line += &format!(" {mark}");
            }
        } else {
            line += " (no clear beat at the end)";
        }
        println!("{name:<34} {:>7.1} |{line}", r_head.0.bpm(SR));
    }
    println!("\nbeat and downbeat agree with the 60 s reference:");
    for (k, (ok, n)) in tally {
        println!("  {k}: {ok}/{n}");
    }
}
