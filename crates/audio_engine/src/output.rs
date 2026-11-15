//! Device output is isolated from capture, DSP, beat analysis and COM upkeep.
//! The bounded 100 ms queue covers their occasional work without delaying a
//! WASAPI wake. Queue and device padding are included in the heard clock.
use crate::player::Frame;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;
use wasapi::{
    AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta,
};
const SR: usize = 48000;
const TARGET: usize = SR / 10;

pub(crate) struct Output {
    queue: Arc<Mutex<VecDeque<Frame>>>,
    padding: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub device_id: String,
}
impl Output {
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let queue = Arc::new(Mutex::new(VecDeque::with_capacity(TARGET)));
        let padding = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let (q, p, s, f) = (queue.clone(), padding.clone(), stop.clone(), failed.clone());
        let thread = std::thread::Builder::new()
            .name("glowtune-output".into())
            .spawn(move || {
                crate::engine::audio_priority("output");
                let result = device_loop(&q, &p, &s, &tx);
                if let Err(e) = result {
                    let reason = e.to_string();
                    let _ = tx.try_send(Err(reason.clone()));
                    tracing::warn!("audio output stopped: {reason}");
                    f.store(true, Ordering::Relaxed);
                }
            })?;
        let device_id = match rx.recv()? {
            Ok(id) => id,
            Err(e) => {
                let _ = thread.join();
                return Err(e.into());
            }
        };
        Ok(Self {
            queue,
            padding,
            stop,
            failed,
            thread: Some(thread),
            device_id,
        })
    }
    pub fn space(&self) -> usize {
        TARGET
            .saturating_sub(self.queue.lock().unwrap().len())
            .min(480)
    }
    pub fn push(&self, samples: &[f32]) {
        self.queue
            .lock()
            .unwrap()
            .extend(samples.chunks_exact(2).map(|f| [f[0], f[1]]));
    }
    pub fn clear(&self) {
        self.queue.lock().unwrap().clear();
    }
    pub fn latency(&self) -> usize {
        self.queue.lock().unwrap().len() + self.padding.load(Ordering::Relaxed)
    }
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn device_loop(
    queue: &Mutex<VecDeque<Frame>>,
    padding: &AtomicUsize,
    stop: &AtomicBool,
    ready: &mpsc::SyncSender<Result<String, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    initialize_mta().ok()?;
    let device = DeviceEnumerator::new()?.get_default_device(Direction::Render)?;
    let id = device.get_id()?;
    let mut client: AudioClient = device.get_iaudioclient()?;
    let format = WaveFormat::new(32, 32, SampleType::Float, SR, 2, None);
    client.initialize_client(
        &format,
        Direction::Render,
        StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 400_000,
        },
        None,
    )?;
    let wake = client.set_get_eventhandle()?;
    let render = client.get_audiorenderclient()?;
    ready.send(Ok(id))?;
    // Pre-fill before starting the hardware clock.
    while queue.lock().unwrap().len() < TARGET && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut bytes = Vec::with_capacity(SR / 10 * 8);
    let mut started = false;
    let mut underruns = 0usize;
    while !stop.load(Ordering::Relaxed) {
        if started {
            wake.wait_for_event(100)?;
        }
        let space = client.get_available_space_in_frames()? as usize;
        if space > 0 {
            bytes.clear();
            let mut missing = 0usize;
            {
                let mut q = queue.lock().unwrap();
                for _ in 0..space {
                    let frame = q.pop_front().unwrap_or_else(|| {
                        missing += 1;
                        [0.0; 2]
                    });
                    bytes.extend(frame.into_iter().flat_map(f32::to_le_bytes));
                }
            }
            if missing > 0 {
                underruns += 1;
                // Rate limit log traffic on the output thread.
                if underruns.is_power_of_two() {
                    tracing::warn!(
                        underruns,
                        missing_frames = missing,
                        "audio output queue underrun"
                    );
                }
            }
            render.write_to_device(&bytes, None)?;
        }
        // Shared buffer is 40 ms. Include it in the media clock.
        padding.store(client.get_current_padding()? as usize, Ordering::Relaxed);
        if !started {
            client.start_stream()?;
            started = true;
        }
    }
    client.stop_stream()?;
    tracing::info!(underruns, "audio output finished");
    Ok(())
}
