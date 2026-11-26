//! Listens to one outside app (Spotify desktop, a browser) for where the
//! singing is: lyrics alignment for music Glowtune doesn't play itself.
//!
//! ```text
//!  app's process tree ──process loopback──► voice meter ──► trace (60 s)
//!  (nothing is played, no volume touched)                   + when it was heard
//! ```
//! Unlike the sound engine, this never changes the app's sound: it only
//! reads a copy. It runs from the start of a song until its first sung line
//! is decided (seconds), then the caller drops it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat, initialize_mta};

use crate::voice::{HOP_MS, VoiceMeter};

const SAMPLE_RATE: u32 = 48_000;
/// Hops kept: one minute.
const MAX_HOPS: usize = 3000;

/// What has been heard so far.
#[derive(Debug, Default, Clone)]
pub struct Heard {
    /// `voice::Hop::voice` every `voice::HOP_MS`, oldest first.
    pub hops: VecDeque<f32>,
    /// When the sound of the last hop was heard (its end).
    pub last_at: Option<Instant>,
}

impl Heard {
    /// Wall time when hop `k` began.
    pub fn hop_at(&self, k: usize) -> Option<Instant> {
        let last = self.last_at?;
        let back = (self.hops.len() - k) as f64 * HOP_MS;
        last.checked_sub(Duration::from_secs_f64(back / 1000.0))
    }
}

/// Listening to one app. Dropping it stops (and joins) the thread.
pub struct Listener {
    stop: Arc<AtomicBool>,
    heard: Arc<Mutex<Heard>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    /// Starts listening to `pid`'s process tree.
    pub fn start(pid: u32) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let heard = Arc::new(Mutex::new(Heard::default()));
        let (s, h) = (stop.clone(), heard.clone());
        let thread = std::thread::Builder::new()
            .name("glowtune-listen".into())
            .spawn(move || {
                if initialize_mta().is_err() {
                    tracing::warn!("lyrics listen: COM could not start");
                    return;
                }
                if let Err(e) = run(pid, &s, &h) {
                    tracing::warn!("lyrics listen (pid {pid}): {e}");
                }
            })?;
        Ok(Self {
            stop,
            heard,
            thread: Some(thread),
        })
    }

    /// A copy of what has been heard so far.
    pub fn heard(&self) -> Heard {
        self.heard.lock().map(|h| h.clone()).unwrap_or_default()
    }

    /// Forget what was heard (a seek: that sound no longer lines up).
    pub fn restart(&self) {
        if let Ok(mut h) = self.heard.lock() {
            *h = Heard::default();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    pid: u32,
    stop: &AtomicBool,
    heard: &Mutex<Heard>,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = WaveFormat::new(32, 32, SampleType::Float, SAMPLE_RATE as usize, 2, None);
    let mut client = AudioClient::new_application_loopback_client(pid, true)?;
    client.initialize_client(
        &format,
        Direction::Capture,
        StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        },
        None,
    )?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    tracing::info!("lyrics listen: listening to pid {pid}");
    let mut meter = VoiceMeter::new(SAMPLE_RATE);
    let mut bytes = VecDeque::new();
    let mut block = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
        while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
            capture.read_from_device_to_deque(&mut bytes)?;
        }
        if bytes.is_empty() {
            continue;
        }
        let now = Instant::now();
        let gap = heard
            .lock()
            .ok()
            .and_then(|h| h.last_at)
            .map(|at| now.duration_since(at).as_secs_f64() * 1000.0);
        block.clear();
        let mut sample = [0u8; 4];
        for (i, byte) in bytes.drain(..).enumerate() {
            sample[i % 4] = byte;
            if i % 4 == 3 {
                block.push(f32::from_le_bytes(sample));
            }
        }
        let Ok(mut h) = heard.lock() else {
            break;
        };
        // The app went quiet and sent nothing (no packets while silent):
        // keep the trace in step with the clock with silent hops.
        let chunk_ms = block.len() as f64 / 2.0 / SAMPLE_RATE as f64 * 1000.0;
        if let Some(gap) = gap {
            let missing = ((gap - chunk_ms - 100.0) / HOP_MS).floor();
            for _ in 0..(missing.max(0.0) as usize).min(MAX_HOPS) {
                h.hops.push_back(0.0);
            }
        }
        meter.process(&block, |hop| {
            h.hops.push_back(hop.voice());
            if h.hops.len() > MAX_HOPS {
                h.hops.pop_front();
            }
        });
        while h.hops.len() > MAX_HOPS {
            h.hops.pop_front();
        }
        h.last_at = Some(now);
    }
    let _ = client.stop_stream();
    Ok(())
}
