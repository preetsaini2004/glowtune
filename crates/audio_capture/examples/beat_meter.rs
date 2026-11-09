//! Live beat meter for whatever the PC is playing.
//!
//! `cargo run -p audio_capture --example beat_meter [seconds]`
//! Prints an energy bar ~15 times per second; `*` marks detected kicks.

use std::time::{Duration, Instant};

use analysis::BeatDetector;
use audio_capture::LoopbackCapture;

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    let mut cap = LoopbackCapture::start().expect("loopback capture");
    let mut det = BeatDetector::new(cap.sample_rate());
    let mut buf = Vec::with_capacity(8192);
    let start = Instant::now();
    let (mut onsets, mut frames, mut shown_max, mut kick_seen) = (0u32, 0u32, 0.0f32, false);
    let mut last_print = Instant::now();

    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(16));
        cap.read(&mut buf);
        // Run every analysis in this chunk so no onset is missed.
        for chunk in buf.chunks(256) {
            if let Some(f) = det.push(chunk) {
                frames += 1;
                onsets += f.onset as u32;
                kick_seen |= f.onset;
                shown_max = shown_max.max(f.energy);
            }
        }
        if last_print.elapsed() >= Duration::from_millis(66) {
            let bar = "█".repeat((shown_max * 40.0) as usize);
            println!(
                "{:6.2}s {} {:<40} {:.2}",
                start.elapsed().as_secs_f32(),
                if kick_seen { '*' } else { ' ' },
                bar,
                shown_max
            );
            shown_max = 0.0;
            kick_seen = false;
            last_print = Instant::now();
        }
    }
    let secs = start.elapsed().as_secs_f32();
    println!(
        "\n{frames} analyses ({:.0}/s), {onsets} onsets ({:.1}/s)",
        frames as f32 / secs,
        onsets as f32 / secs
    );
}
