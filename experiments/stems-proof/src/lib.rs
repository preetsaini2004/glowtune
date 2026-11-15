//! Stem-aware Automix proof: shared pieces (decode, separate, write WAV).
//!
//! ```text
//!  song window (44.1 kHz stereo)
//!     │ normalise (mono mean / std, as Demucs does)
//!     ▼
//!  7.8 s chunks, 25 % overlap ──► HTDemucs (ONNX, CPU) ──► 4 stems per chunk
//!     │ triangle-weighted overlap-add
//!     ▼
//!  drums, bass, other, vocals  (same length as the window)
//! ```

use ort::{
    ep,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

pub type Frame = [f32; 2];
pub const SR: u32 = 44_100;
/// The model's fixed input length (7.8 s at 44.1 kHz).
pub const SEGMENT: usize = 343_980;
pub const STEMS: [&str; 4] = ["drums", "bass", "other", "vocals"];

/// Decodes any audio/video file to 44.1 kHz stereo with ffmpeg.
pub fn decode(path: &str) -> Vec<Frame> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", path])
        .args(["-f", "f32le", "-ac", "2", "-ar", "44100", "pipe:1"])
        .output()
        .expect("ffmpeg");
    assert!(out.status.success(), "ffmpeg could not decode {path}");
    out.stdout
        .chunks_exact(8)
        .map(|b| {
            [
                f32::from_le_bytes(b[0..4].try_into().unwrap()),
                f32::from_le_bytes(b[4..8].try_into().unwrap()),
            ]
        })
        .collect()
}

pub struct Separator {
    session: Session,
    input: String,
}

impl Separator {
    /// Lean settings measured in the proof: no graph optimisation (avoids a
    /// 4.5 GB load spike), no memory arena (avoids unbounded growth), 2
    /// threads (so playback elsewhere never starves).
    pub fn new(model: &str, threads: usize) -> ort::Result<Self> {
        let env = ort::init().with_name("stems-proof").build()?;
        let session = Session::builder(&env)?
            .with_optimization_level(GraphOptimizationLevel::Disable)?
            .with_memory_pattern(false)?
            .with_intra_threads(threads)?
            .with_execution_providers([ep::CPU::default().with_arena_allocator(false).build()])?
            .commit_from_file(model)?;
        let input = session.inputs()[0].name().to_string();
        Ok(Self { session, input })
    }

    /// Splits `audio` into drums, bass, other, vocals.
    pub fn separate(&mut self, audio: &[Frame]) -> ort::Result<[Vec<Frame>; 4]> {
        let n = audio.len();
        // Demucs normalises by the mono mix's mean and spread.
        let mono: Vec<f32> = audio.iter().map(|f| (f[0] + f[1]) * 0.5).collect();
        let mean = mono.iter().sum::<f32>() / n.max(1) as f32;
        let std = (mono.iter().map(|m| (m - mean).powi(2)).sum::<f32>() / n.max(1) as f32)
            .sqrt()
            .max(1e-5);
        let weight: Vec<f32> = (0..SEGMENT)
            .map(|t| (t + 1).min(SEGMENT - t) as f32)
            .collect();
        let mut out: [Vec<Frame>; 4] = std::array::from_fn(|_| vec![[0.0; 2]; n]);
        let mut total = vec![0f32; n];
        let stride = SEGMENT * 3 / 4;
        let mut start = 0;
        loop {
            // Planar [1, 2, SEGMENT], zero-padded past the end.
            let mut data = vec![0f32; 2 * SEGMENT];
            for t in 0..SEGMENT.min(n.saturating_sub(start)) {
                let f = audio[start + t];
                data[t] = (f[0] - mean) / std;
                data[SEGMENT + t] = (f[1] - mean) / std;
            }
            let input = Tensor::from_array(([1usize, 2, SEGMENT], data))?;
            let result = self
                .session
                .run(ort::inputs![self.input.as_str() => input])?;
            let (_, stems) = result[0].try_extract_tensor::<f32>()?;
            // stems: [1, 4, 2, SEGMENT]
            for t in 0..SEGMENT.min(n.saturating_sub(start)) {
                let w = weight[t];
                for (s, stem) in out.iter_mut().enumerate() {
                    let base = s * 2 * SEGMENT;
                    stem[start + t][0] += w * (stems[base + t] * std + mean / 4.0);
                    stem[start + t][1] += w * (stems[base + SEGMENT + t] * std + mean / 4.0);
                }
                total[start + t] += w;
            }
            if start + SEGMENT >= n {
                break;
            }
            start += stride;
        }
        for stem in &mut out {
            for (f, w) in stem.iter_mut().zip(&total) {
                f[0] /= w;
                f[1] /= w;
            }
        }
        Ok(out)
    }
}

/// 16-bit stereo WAV.
pub fn write_wav(path: &std::path::Path, frames: &[Frame], sample_rate: u32) {
    let data_len = (frames.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for f in frames {
        for s in f {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, b).expect("write wav");
}

pub fn rms(x: &[Frame]) -> f32 {
    (x.iter()
        .map(|f| (f[0] * f[0] + f[1] * f[1]) * 0.5)
        .sum::<f32>()
        / x.len().max(1) as f32)
        .sqrt()
}
