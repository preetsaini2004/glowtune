//! Prints raw tempo estimates (no confidence filter) for windows of songs.
use audio_engine::tempo;
use stems_proof::{SR, decode};
fn main() {
    for path in std::env::args().skip(1) {
        let x = decode(&path);
        let loud = x
            .iter()
            .position(|f| f[0].abs().max(f[1].abs()) > 0.01)
            .unwrap_or(0);
        let x = &x[loud..];
        let name: String = std::path::Path::new(&path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .chars()
            .take(28)
            .collect();
        let mut line = format!("{name:<28}");
        for (from, len) in [(0, 6), (0, 10), (0, 15), (0, 20), (0, 60), (60, 15)] {
            let s = (from * SR as usize).min(x.len());
            let e = ((from + len) * SR as usize).min(x.len());
            match tempo::estimate(&x[s..e], SR) {
                Some(t) => {
                    line += &format!(" | {from}+{len}s {:.0} BPM c{:.2}", t.bpm(SR), t.confidence)
                }
                None => line += &format!(" | {from}+{len}s -"),
            }
        }
        println!("{line}");
    }
}
