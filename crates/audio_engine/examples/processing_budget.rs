//! Offline processing budget probe; decodes only a user-supplied local clip.
//! No device access, telemetry, playback changes or audio written to disk.
use audio_engine::{Blend, Player};
fn main() {
    let path = std::env::args().nth(1).expect("local audio/video file");
    let seconds: usize = std::env::args()
        .nth(2)
        .map(|s| s.parse().unwrap())
        .unwrap_or(1800);
    let decoded = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &path,
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
            "48000",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg");
    assert!(decoded.status.success());
    let input: Vec<f32> = decoded
        .stdout
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    assert!(input.len() > 960);
    for blend in [Blend::Crossfade { secs: 6.0 }, Blend::Automix { secs: 6.0 }] {
        let mut player = Player::new(48000);
        player.set_blend(blend);
        let (mut at, mut max, mut over, mut max_delay, mut blends) = (0, 0.0f64, 0, 0.0f32, 0);
        let mut block = [0.0; 960];
        let mut out = [0.0; 960];
        let started = std::time::Instant::now();
        let mut was_blending = false;
        for tick in 0..seconds * 100 {
            for s in &mut block {
                *s = input[at];
                at = (at + 1) % input.len();
            }
            let cycle = std::time::Instant::now();
            player.push(&block);
            if tick > 0 && tick % 18000 == 0 {
                player.track_changed(true);
            }
            player.pull(&mut out);
            if matches!(blend, Blend::Automix { .. }) && tick % 200 == 0 {
                let _ = player.has_beat_now();
            }
            let ms = cycle.elapsed().as_secs_f64() * 1000.0;
            max = max.max(ms);
            if ms > 10.0 {
                over += 1;
            }
            assert!(out.iter().all(|s| s.is_finite()));
            max_delay = max_delay.max(player.delay_secs());
            if player.is_blending() && !was_blending {
                blends += 1;
            }
            was_blending = player.is_blending();
        }
        println!(
            "{blend:?}: {seconds}s simulated in {:.2}s; worst 10ms producer cycle {max:.2}ms; >10ms cycles {over}; max delay {max_delay:.3}s; blends {blends}",
            started.elapsed().as_secs_f64()
        );
        assert!(max_delay < 30.0);
    }
}
