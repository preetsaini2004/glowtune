//! First voice entry vs the lyrics' first sung line, on real songs.
//! `cargo run --release --bin entry -- <song> <lrc> [<lrc> ...]`
use audio_engine::voice::{self, VoiceMeter};
use stems_proof::{SR, decode};

fn first_sung(path: &str) -> f64 {
    let text = std::fs::read_to_string(path).expect("lrc");
    text.lines()
        .filter_map(|raw| {
            let rest = raw.trim_start_matches('\u{feff}').strip_prefix('[')?;
            let (stamp, words) = rest.split_once(']')?;
            let (m, s) = stamp.split_once(':')?;
            let t = (m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?) * 1000.0;
            (!words.trim().is_empty()).then_some(t)
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let frames = decode(&a[0]);
    let flat: Vec<f32> = frames.iter().flat_map(|f| [f[0], f[1]]).collect();
    let mut m = VoiceMeter::new(SR);
    let mut trace = Vec::new();
    m.process(&flat, |h| trace.push(h.voice()));
    for lrc in &a[1..] {
        let line = first_sung(lrc);
        for shift in [0.0, 3000.0, -3000.0, 8000.0] {
            let said = line + shift;
            let e = voice::first_entry(
                &trace,
                0.0,
                (said - 15_000.0).max(0.0),
                said + 15_000.0,
                1.5,
            );
            match e {
                Some(e) => println!(
                    "{lrc}: lyrics say {:.2} s (shifted {:+.0}); voice enters {:.2} s, step {:.2} → lag {:+.2} s",
                    said / 1000.0,
                    shift / 1000.0,
                    e.at_ms / 1000.0,
                    e.step,
                    (e.at_ms - said) / 1000.0
                ),
                None => println!("{lrc}: no entry found"),
            }
        }
    }
}
