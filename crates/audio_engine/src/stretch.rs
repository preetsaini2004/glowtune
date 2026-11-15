//! Continuous, stereo-linked time stretching. Source and output cursors are
//! separate: feeding lookahead into the DSP must never advance the media clock.
use super::player::Frame;
use std::collections::VecDeque;
const BLOCK: usize = 256;

pub(crate) struct Source {
    dsp: signalsmith_stretch::Stretch,
    pub position: f64,
    feed: u64,
    initialized: bool,
    ready: VecDeque<Frame>,
    input: Vec<f32>,
    output: Vec<f32>,
    fraction: f64,
    queued_rate: f64,
}
impl Source {
    pub fn new(sample_rate: u32, position: f64) -> Self {
        Self {
            dsp: signalsmith_stretch::Stretch::preset_default(2, sample_rate),
            position,
            feed: 0,
            initialized: false,
            ready: VecDeque::new(),
            input: Vec::with_capacity(20000),
            output: Vec::with_capacity(20000),
            fraction: 0.0,
            queued_rate: 1.0,
        }
    }

    pub fn speed(&self) -> f64 {
        self.queued_rate
    }

    pub fn fed_until(&self) -> u64 {
        self.feed
    }
    fn copy(&mut self, buf: &VecDeque<Frame>, base: u64, from: i64, n: usize) {
        self.input.clear();
        for i in 0..n {
            let index = from + i as i64 - base as i64;
            let frame = if index >= 0 {
                buf.get(index as usize).copied()
            } else {
                None
            };
            self.input.extend_from_slice(&frame.unwrap_or([0.0; 2]));
        }
    }
    pub fn next(&mut self, buf: &VecDeque<Frame>, base: u64, rate: f64) -> Option<Frame> {
        if !self.initialized {
            let il = self.dsp.input_latency();
            let ol = self.dsp.output_latency();
            let start = self.position.floor() as u64;
            let prime = (ol as f64 * rate).round() as usize;
            if start + il as u64 + prime as u64 + BLOCK as u64 * 2 > base + buf.len() as u64 {
                return None;
            }
            // seek centres the input at start; discard only the documented
            // output latency. This work happens once, not once per audio block.
            self.copy(buf, base, start as i64, il);
            self.dsp.seek(&self.input, rate);
            self.feed = start + il as u64;
            self.copy(buf, base, self.feed as i64, prime);
            self.output.resize(ol * 2, 0.0);
            self.dsp.process(&self.input, &mut self.output);
            self.feed += prime as u64;
            self.initialized = true;
        }
        if self.ready.is_empty() {
            let exact = BLOCK as f64 * rate + self.fraction;
            let n = exact.floor() as usize;
            if self.feed + n as u64 > base + buf.len() as u64 {
                return None;
            }
            self.fraction = exact - n as f64;
            self.copy(buf, base, self.feed as i64, n);
            self.output.resize(BLOCK * 2, 0.0);
            self.dsp.process(&self.input, &mut self.output);
            self.feed += n as u64;
            self.ready
                .extend(self.output.chunks_exact(2).map(|f| [f[0], f[1]]));
            self.queued_rate = rate;
        }
        let f = self.ready.pop_front()?;
        self.position += self.queued_rate;
        Some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn different_tempo_preserves_pitch_and_stereo_through_rate_changes() {
        let sr = 48000;
        let raw: VecDeque<Frame> = (0..sr * 8)
            .map(|n| {
                let s = 0.4 * (std::f32::consts::TAU * 440.0 * n as f32 / sr as f32).sin();
                [s, -s]
            })
            .collect();
        let mut source = Source::new(sr as u32, 0.0);
        let mut audio = Vec::new();
        for n in 0..sr * 6 {
            let rate = match n / sr {
                0..=1 => 0.96,
                2..=3 => 1.25,
                _ => 1.0,
            };
            audio.push(source.next(&raw, 0, rate).expect("lookahead is available"));
        }
        let stereo_error = audio
            .iter()
            .map(|f| (f[0] + f[1]).abs())
            .fold(0.0f32, f32::max);
        eprintln!("maximum stereo cancellation error {stereo_error}");
        assert!(stereo_error < 0.001, "stereo phase drift {stereo_error}");
        for second in 1..6 {
            let segment = &audio[second * sr..(second + 1) * sr];
            let amplitude = |freq: f32| {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (n, f) in segment.iter().enumerate() {
                    let phase = std::f64::consts::TAU * freq as f64 * n as f64 / sr as f64;
                    re += f[0] as f64 * phase.cos();
                    im += f[0] as f64 * phase.sin();
                }
                2.0 * re.hypot(im) / sr as f64
            };
            assert!(
                amplitude(440.0) > 0.33,
                "pitch or level lost in second {second}"
            );
            assert!(amplitude(550.0) < 0.01, "tempo change shifted the pitch");
        }
        for chunk in audio[sr / 2..].chunks_exact(sr / 100) {
            let rms = (chunk.iter().map(|f| f[0] * f[0]).sum::<f32>() / chunk.len() as f32).sqrt();
            assert!(rms > 0.24, "audible flutter: {rms}");
        }
    }
}
