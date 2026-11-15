//! Offline real-music transition probe for a supplied two-song demo.
//! Default excerpts are from the public Numb -> In the End reference clip.
use audio_engine::{Blend, Player, tempo};
fn main() {
    let path = std::env::args().nth(1).expect("local reference video");
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
        .unwrap();
    assert!(decoded.status.success());
    let samples: Vec<f32> = decoded
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let a = &samples[..12 * 48000 * 2];
    let b = &samples[29 * 48000 * 2..41 * 48000 * 2];
    let mut player = Player::new(48000);
    player.set_blend(Blend::Automix { secs: 6.0 });
    player.push(a);
    player.push(&b[..28800]);
    player.track_changed(true);
    player.push(&b[28800..]);
    let mut block = [0.0; 960];
    let mut mixed = Vec::new();
    let mut matched = None;
    for _ in 0..2400 {
        player.pull(&mut block);
        if player.is_blending() {
            matched = player.beat_match();
            mixed.extend(block.chunks_exact(2).map(|f| [f[0], f[1]]));
        } else if !mixed.is_empty() {
            break;
        }
    }
    let pair = matched.expect("real excerpts must beat-match rather than fall back");
    let grid = tempo::estimate(&mixed, 48000).expect("mixed music has a beat");
    println!(
        "source tempos {:.2} -> {:.2} BPM; mixed {:.2} BPM, confidence {:.3}; overlap {:.2}s",
        pair.0,
        pair.1,
        grid.bpm(48000),
        grid.confidence,
        mixed.len() as f64 / 48000.0
    );
    assert!((pair.0 - 110.0).abs() < 2.0 && (pair.1 - 105.0).abs() < 2.0);
    assert!(
        (grid.bpm(48000) - pair.0).abs() < 2.0,
        "mixed beat grid drifted"
    );
    assert!(
        grid.confidence >= 0.35,
        "overlap did not retain a reliable beat grid"
    );
}
