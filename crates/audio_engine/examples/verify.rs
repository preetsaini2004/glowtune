//! Measures what the engine does to a window's sound, without ears:
//! compares the window's own sound (process capture) with what reaches the
//! speakers (system loopback) while the engine runs.
//!
//! `verify <browser pid> [preset id]` — play music in the window first, and
//! keep other apps quiet.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use audio_engine::{Blend, Engine, EqSettings, PRESETS, sessions::WHISPER};
    use std::collections::VecDeque;
    use wasapi::*;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid: u32 = args.first().ok_or("pass the browser pid")?.parse()?;
    let eq = match args
        .get(1)
        .and_then(|id| PRESETS.iter().find(|p| p.0 == id))
    {
        Some(&(_, _, preamp_db, gains_db)) => EqSettings {
            enabled: true,
            preamp_db,
            gains_db,
        },
        None => EqSettings::default(),
    };
    initialize_mta().ok()?;
    let format = WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: 2_000_000,
    };

    let mut source = AudioClient::new_application_loopback_client(pid, true)?;
    source.initialize_client(&format, Direction::Capture, mode, None)?;
    let source_cap = source.get_audiocaptureclient()?;
    let device = DeviceEnumerator::new()?.get_default_device(Direction::Render)?;
    let mut speakers = device.get_iaudioclient()?;
    speakers.initialize_client(&format, Direction::Capture, mode, None)?;
    let speakers_cap = speakers.get_audiocaptureclient()?;
    source.start_stream()?;
    speakers.start_stream()?;

    let to_f32 = |q: &mut VecDeque<u8>| -> Vec<f32> {
        let bytes: Vec<u8> = q.drain(..).collect();
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    };
    let measure = |secs: f32| -> Result<(Vec<f32>, Vec<f32>), Box<dyn std::error::Error>> {
        let (mut a, mut b) = (VecDeque::new(), VecDeque::new());
        let started = std::time::Instant::now();
        while started.elapsed().as_secs_f32() < secs {
            while source_cap.get_next_packet_size()?.unwrap_or(0) > 0 {
                source_cap.read_from_device_to_deque(&mut a)?;
            }
            while speakers_cap.get_next_packet_size()?.unwrap_or(0) > 0 {
                speakers_cap.read_from_device_to_deque(&mut b)?;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok((to_f32(&mut a), to_f32(&mut b)))
    };
    // Level of the lows (< ~120 Hz), mids and highs (> ~3 kHz), in dB.
    let bands = |x: &[f32]| -> [f32; 3] {
        let (mut lp, mut lp2) = (0.0f32, 0.0f32);
        let (mut low, mut mid, mut high) = (0.0f64, 0.0f64, 0.0f64);
        for s in x.iter().step_by(2) {
            lp += (s - lp) * 0.0156; // ~120 Hz
            lp2 += (s - lp2) * 0.33; // ~3 kHz
            low += (lp * lp) as f64;
            mid += ((lp2 - lp) * (lp2 - lp)) as f64;
            high += ((s - lp2) * (s - lp2)) as f64;
        }
        let n = (x.len() / 2).max(1) as f64;
        [low, mid, high].map(|e| (10.0 * (e / n + 1e-20).log10()) as f32)
    };
    let report = |label: &str, src: &[f32], out: &[f32], src_gain: f32| {
        let src: Vec<f32> = src.iter().map(|s| s * src_gain).collect();
        let (s, o) = (bands(&src), bands(out));
        println!(
            "{label:<22} window: {:6.1} {:6.1} {:6.1} dB   speakers: {:6.1} {:6.1} {:6.1} dB   difference: {:+5.1} {:+5.1} {:+5.1} dB (low mid high)",
            s[0],
            s[1],
            s[2],
            o[0],
            o[1],
            o[2],
            o[0] - s[0],
            o[1] - s[1],
            o[2] - s[2]
        );
    };

    // `verify <pid> observe`: only watch (the app runs its own engine).
    if args.get(1).is_some_and(|a| a == "observe") {
        for _ in 0..args.get(2).and_then(|n| n.parse().ok()).unwrap_or(2) {
            let (src, out) = measure(3.0)?;
            let peak = src.iter().fold(0f32, |m, s| m.max(s.abs()));
            let whispered = peak < WHISPER * 10.0;
            report(
                if whispered {
                    "window whispered"
                } else {
                    "window at full volume"
                },
                &src,
                &out,
                if whispered { 1.0 / WHISPER } else { 1.0 },
            );
        }
        return Ok(());
    }

    let (src, out) = measure(3.0)?;
    report("engine off", &src, &out, 1.0);

    let engine = Engine::start(pid, eq, Blend::Off, 1.0, Box::new(|| {}))?;
    measure(1.5)?; // let it settle
    let (src, out) = measure(4.0)?;
    report("engine on", &src, &out, 1.0 / WHISPER);
    println!(
        "  running={} latency={} ms",
        engine.status().running(),
        engine.status().latency_ms()
    );
    drop(engine);
    measure(1.0)?;
    let (src, out) = measure(3.0)?;
    report("engine off again", &src, &out, 1.0);
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
