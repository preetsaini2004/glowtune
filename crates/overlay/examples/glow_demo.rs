//! Shows the edge glow without any music: fades in, cycles through a few
//! palettes, simulates beats, then fades out and quits.
//!
//! `cargo run -p overlay --example glow_demo [seconds] [idle_seconds]`

use std::time::{Duration, Instant};

use overlay::{Overlay, OverlayConfig};

const PALETTES: &[&[[u8; 3]]] = &[
    &[[255, 96, 160], [140, 110, 255], [70, 190, 255]], // neon pink / violet / cyan
    &[[240, 129, 100], [239, 190, 104]],                // sunset
    &[[0, 191, 10], [163, 111, 122]],                   // green + rose
    &[[86, 135, 255], [192, 67, 249], [199, 143, 249]], // blue / purple
];

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("overlay=debug")
        .init();
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let overlay = Overlay::spawn(OverlayConfig::default()).expect("overlay");
    println!("monitors: {:#?}", overlay.monitors());
    overlay.set_palette(PALETTES[0]);
    overlay.set_playing(true);

    let start = Instant::now();
    let per_palette = Duration::from_secs(secs.max(4) / PALETTES.len() as u64);
    let mut idx = 0;
    let mut last_switch = start;
    while start.elapsed() < Duration::from_secs(secs) {
        if last_switch.elapsed() >= per_palette {
            idx = (idx + 1) % PALETTES.len();
            overlay.set_palette(PALETTES[idx]);
            last_switch = Instant::now();
            println!("palette {idx}");
        }
        // Fake 120 BPM kick: a sharp hit decaying over the beat.
        let beat = (start.elapsed().as_secs_f32() * 2.0).fract();
        overlay.set_energy((1.0 - beat * 3.0).max(0.0));
        std::thread::sleep(Duration::from_millis(16));
    }

    println!("fading out");
    overlay.set_energy(0.0);
    overlay.set_playing(false);
    std::thread::sleep(Duration::from_secs(3));

    // Optional idle phase (second arg, seconds) to measure the paused state.
    let idle: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if idle > 0 {
        println!("idle for {idle}s");
        std::thread::sleep(Duration::from_secs(idle));
    }
}
