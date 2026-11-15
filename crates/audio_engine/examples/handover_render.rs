//! Acceptance render: two local songs through the real Player, fed the way
//! the live engine feeds it (10 ms in, 10 ms out), with Automix on, a full
//! delay (as after fast fill) and Automix leaving song A 8 s before its end.
//! The new song only arrives as it would live. Nothing is bundled or sent.
//!
//!   cargo run --release -p audio_engine --example handover_render -- <song A> <song B> <out.wav>
use audio_engine::{Blend, Player};

const SR: usize = 48000;

fn decode(path: &str) -> Vec<f32> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", path])
        .args(["-f", "f32le", "-ac", "2", "-ar", "48000", "pipe:1"])
        .output()
        .expect("ffmpeg");
    assert!(out.status.success(), "ffmpeg could not decode {path}");
    out.stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

/// Drops leading and trailing near-silence (interleaved stereo).
fn trim(x: Vec<f32>) -> Vec<f32> {
    let loud = |f: &[f32]| f[0].abs().max(f[1].abs()) > 0.01;
    let frames: Vec<&[f32]> = x.chunks_exact(2).collect();
    let s = frames.iter().position(|f| loud(f)).unwrap_or(0);
    let e = frames
        .iter()
        .rposition(|f| loud(f))
        .map_or(frames.len(), |e| e + 1);
    x[s * 2..e * 2].to_vec()
}

fn main() {
    tracing_subscriber::fmt()
        .without_time()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    let a_path = std::env::args().nth(1).expect("song A");
    let b_path = std::env::args().nth(2).expect("song B");
    let out_path = std::env::args().nth(3).expect("out.wav");
    let a = trim(decode(&a_path));
    let b = trim(decode(&b_path));
    // Automix presses Next ~8 s before A's end: A is cut there.
    let a_cut = a.len() / 2 - 8 * SR;
    let delay = 34 * SR; // a full Automix delay
    let lead = 10 * SR;
    let from = a_cut - delay - lead;
    // Everything the service will play, in order: A up to the cut, then B.
    let mut input: Vec<f32> = a[from * 2..a_cut * 2].to_vec();
    let b_starts_at = input.len() / 2;
    input.extend_from_slice(&b);

    let mut p = Player::new(SR as u32);
    p.set_blend(Blend::Automix { secs: 6.0 });
    // The delay is already full (fast fill did that while A played).
    p.push(&input[..delay * 2]);
    let mut fed = delay;
    let mut out: Vec<f32> = Vec::new();
    let mut announced = false;
    let mut blend_at = None;
    let block = SR / 100;
    let mut pulled = vec![0.0; block * 2];
    while fed + block <= input.len() / 2 {
        p.push(&input[fed * 2..(fed + block) * 2]);
        fed += block;
        // This loop runs far faster than real time; live, the study's few
        // tens of ms pass in real time. Wait for it as real time would.
        while p.is_studying() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // The service reports the new song ~0.3 s after it starts.
        if !announced && fed >= b_starts_at + SR * 3 / 10 {
            p.track_changed(true);
            announced = true;
        }
        p.pull(&mut pulled);
        if p.is_blending() && blend_at.is_none() {
            blend_at = Some(out.len() / 2);
            println!(
                "blend starts {:.1} s into the output: {:?}, tempos {:?}",
                out.len() as f32 / 2.0 / SR as f32,
                p.last_move(),
                p.beat_match()
            );
        }
        out.extend_from_slice(&pulled);
        if let Some(at) = blend_at
            && out.len() / 2 > at + 40 * SR
        {
            break;
        }
    }
    let at = blend_at.expect("no blend happened");
    println!("after the blend: {:?}", p.last_move());
    // Write from 15 s before the blend to 35 s after its start.
    let s = at.saturating_sub(15 * SR);
    let e = (at + 35 * SR).min(out.len() / 2);
    write_wav(&out_path, &out[s * 2..e * 2]);
    println!("wrote {out_path} ({:.1} s)", (e - s) as f32 / SR as f32);
}

fn write_wav(path: &str, samples: &[f32]) {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&(SR as u32).to_le_bytes());
    b.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).expect("write wav");
}
