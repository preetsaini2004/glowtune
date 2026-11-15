use audio_engine::tempo;

fn main() {
    let path = std::env::args().nth(1).expect("pass the public demo clip");
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
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(decoded.stdout.len() % 8, 0);
    let frames: Vec<audio_engine::player::Frame> = decoded
        .stdout
        .chunks_exact(8)
        .map(|f| {
            [
                f32::from_le_bytes(f[..4].try_into().unwrap()),
                f32::from_le_bytes(f[4..].try_into().unwrap()),
            ]
        })
        .collect();
    for secs in [0, 4, 8, 12, 16, 20, 24, 28, 32, 35] {
        let from = secs * 48_000;
        let to = ((secs + 6) * 48_000).min(frames.len());
        if from >= to {
            continue;
        }
        let started = std::time::Instant::now();
        let result = tempo::estimate(&frames[from..to], 48_000);
        eprint!(
            "analysis {:.2} ms: ",
            started.elapsed().as_secs_f64() * 1000.0
        );
        match result {
            Some(t) => println!(
                "{secs:2}..{:2}s: {:6.2} BPM, confidence {:.3}",
                to / 48_000,
                t.bpm(48_000),
                t.confidence
            ),
            None => println!("{secs:2}..{:2}s: no beat", to / 48_000),
        }
    }
}
