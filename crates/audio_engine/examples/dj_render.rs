//! Renders each Automix move on real music to WAV files, to listen to them
//! without waiting for songs to end. Uses a local file you supply (decoded
//! with ffmpeg); nothing is bundled or sent anywhere.
//!
//!   cargo run --release -p audio_engine --example dj_render -- <file> [a_end_s] [b_start_s] [out_dir]
//!
//! Song A is the 12 s before `a_end_s`, song B starts at `b_start_s`.
//! Each WAV: 6 s of A, the 6 s move, 6 s of B.
use audio_engine::dj::{Fx, Move};
use audio_engine::player::Frame;
use audio_engine::tempo;

const SR: usize = 48000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a local audio or video file");
    let a_end: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let b_start: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(29.0);
    let out_dir = args.get(4).map_or(".", |s| s.as_str());
    let decoded = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", path])
        .args(["-f", "f32le", "-ac", "2", "-ar", "48000", "pipe:1"])
        .output()
        .expect("ffmpeg");
    assert!(decoded.status.success(), "ffmpeg could not decode the file");
    let all: Vec<Frame> = decoded
        .stdout
        .chunks_exact(8)
        .map(|b| {
            [
                f32::from_le_bytes(b[0..4].try_into().unwrap()),
                f32::from_le_bytes(b[4..8].try_into().unwrap()),
            ]
        })
        .collect();
    let at = |s: f64| ((s * SR as f64) as usize).min(all.len());
    let lead = 6 * SR;
    let len = 6 * SR;
    let a = &all[at(a_end - 12.0)..at(a_end)];
    let b = &all[at(b_start)..at(b_start + 12.0)];
    assert!(
        a.len() == lead + len && b.len() >= len + lead,
        "file too short for these times"
    );

    // Song A's beat over its last 8 s, as the player would find it.
    let tail = &a[a.len() - 8 * SR..];
    let beat = tempo::estimate(tail, SR as u32).filter(|t| t.confidence >= 0.35);
    match &beat {
        Some(t) => println!(
            "song A: {:.1} BPM (confidence {:.2})",
            t.bpm(SR as u32),
            t.confidence
        ),
        None => println!("song A: no clear beat (echo-out uses 120 BPM)"),
    }
    // Where the fade starts in A, and A's beat nearest 40 % into it.
    let echo_beat = beat.as_ref().map(|t| {
        let a_beat = (a.len() - 8 * SR) as f64 + t.first_beat;
        let start = lead as f64;
        let aim = start + len as f64 * 0.4;
        let k = ((aim - a_beat) / t.period).round();
        let cut = (a_beat + k * t.period - start).max(t.period);
        (t.period.round() as usize, cut.round() as usize)
    });

    for (kind, file) in [
        (Move::Crossfade, "1-crossfade.wav"),
        (Move::EchoOut, "2-echo-out.wav"),
        (Move::FilterSweep, "3-filter-sweep.wav"),
    ] {
        let mut fx = Fx::new(kind, SR as u32, len, echo_beat);
        let mut out: Vec<Frame> = a[..lead].to_vec();
        for pos in 0..len {
            out.push(fx.frame(a[lead + pos], b[pos], pos));
        }
        out.extend_from_slice(&b[len..len + lead]);
        let path = std::path::Path::new(out_dir).join(file);
        write_wav(&path, &out);
        println!("wrote {} ({})", path.display(), kind.name());
    }
}

/// 16-bit stereo WAV.
fn write_wav(path: &std::path::Path, frames: &[Frame]) {
    let data_len = (frames.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
    bytes.extend_from_slice(&(SR as u32).to_le_bytes());
    bytes.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for f in frames {
        for s in f {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, bytes).expect("write wav");
}
