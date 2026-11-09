//! System audio loopback capture: "what you hear", never the microphone.
//!
//! ```text
//!  default output device ──WASAPI loopback──► cpal callback (audio thread)
//!                                                   │ downmix to mono
//!                                                   ▼
//!                                       lock-free ring buffer (≈1 s)
//!                                                   │
//!                         LoopbackCapture::read() ◄─┘  (analysis thread)
//! ```
//!
//! Capture only runs while a `LoopbackCapture` exists. Drop it when playback
//! pauses so nothing runs in the background (PLAN.md section 9).
//! Linux (PipeWire/Pulse monitor source) comes with the Linux overlay.

#[cfg(windows)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(windows)]
use cpal::{SampleFormat, Stream};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no audio output device")]
    NoDevice,
    #[error("audio capture unsupported here: {0}")]
    Unsupported(String),
    #[error("audio capture error: {0}")]
    Cpal(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct LoopbackCapture {
    #[cfg(windows)]
    _stream: Stream,
    #[cfg(windows)]
    consumer: rtrb::Consumer<f32>,
    sample_rate: u32,
}

impl LoopbackCapture {
    /// Starts capturing the system's default output (follows device changes).
    #[cfg(not(windows))]
    pub fn start() -> Result<Self> {
        Err(Error::Unsupported(
            "loopback capture is Windows-only for now".into(),
        ))
    }

    #[cfg(windows)]
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(Error::NoDevice)?;
        let supported = device
            .default_output_config()
            .map_err(|e| Error::Cpal(e.to_string()))?;
        let format = supported.sample_format();
        let config = supported.config();
        let channels = config.channels.max(1) as usize;
        let sample_rate = config.sample_rate;

        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(sample_rate as usize);
        let err = |e: cpal::Error| tracing::warn!("loopback stream error: {e}");

        // Downmix in the audio callback; drop samples if the reader falls behind
        // (never block the audio thread).
        macro_rules! stream {
            ($t:ty, $to_f32:expr) => {
                device.build_input_stream::<$t, _, _>(
                    config,
                    move |data: &[$t], _| {
                        for frame in data.chunks_exact(channels) {
                            let sum: f32 = frame.iter().map(|&s| $to_f32(s)).sum();
                            let _ = producer.push(sum / channels as f32);
                        }
                    },
                    err,
                    None,
                )
            };
        }
        let stream = match format {
            SampleFormat::F32 => stream!(f32, |s: f32| s),
            SampleFormat::I16 => stream!(i16, |s: i16| s as f32 / 32768.0),
            SampleFormat::I32 => stream!(i32, |s: i32| s as f32 / 2_147_483_648.0),
            SampleFormat::U16 => stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
            other => return Err(Error::Unsupported(format!("sample format {other:?}"))),
        }
        .map_err(|e| Error::Cpal(e.to_string()))?;
        stream.play().map_err(|e| Error::Cpal(e.to_string()))?;
        tracing::info!(
            "loopback capture: {} Hz, {channels} ch, {format:?}",
            sample_rate
        );
        Ok(Self {
            _stream: stream,
            consumer,
            sample_rate,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Moves all captured samples into `out` (cleared first). No allocation
    /// once `out` has grown to its steady-state size.
    pub fn read(&mut self, out: &mut Vec<f32>) {
        out.clear();
        #[cfg(windows)]
        {
            let n = self.consumer.slots();
            if let Ok(chunk) = self.consumer.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                out.extend_from_slice(a);
                out.extend_from_slice(b);
                chunk.commit_all();
            }
        }
    }
}
