//! Prints the voice trace (mid vs side, voice band) every 250 ms.
//! `cargo run --release --bin voiceplot -- <song> <from s> <to s>`
use audio_engine::voice::VoiceMeter;
use stems_proof::{SR, decode};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (from, to): (f64, f64) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let frames = decode(&a[0]);
    let flat: Vec<f32> = frames.iter().flat_map(|f| [f[0], f[1]]).collect();
    let mut m = VoiceMeter::new(SR);
    let mut hops = Vec::new();
    m.process(&flat, |h| hops.push(h));
    for k in ((from * 50.0) as usize..(to * 50.0) as usize).step_by(12) {
        let w = &hops[k..(k + 12).min(hops.len())];
        let v = w.iter().map(|h| h.voice()).sum::<f32>() / w.len() as f32;
        let mid = w.iter().map(|h| h.mid).sum::<f32>() / w.len() as f32;
        let side = w.iter().map(|h| h.side).sum::<f32>() / w.len() as f32;
        let bar = "#".repeat(((v + 1.0) * 6.0).clamp(0.0, 60.0) as usize);
        println!(
            "{:6.2}s voice {:+5.2} mid {:7.4} side {:7.4} {bar}",
            k as f64 / 50.0,
            v,
            mid * 1000.0,
            side * 1000.0
        );
    }
}
