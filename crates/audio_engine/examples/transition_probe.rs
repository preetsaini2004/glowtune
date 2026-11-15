//! "Listening by numbers": loudness of a window's own sound and of what
//! reaches the speakers, every 100 ms, for a while. Run it across a song
//! change to see gaps, dips, cuts or doubling in the blend.
//!
//! `transition_probe <browser pid> <seconds>` — prints one line per 100 ms:
//! time, window level (dB), speakers level (dB), and bars. Nothing is saved.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use audio_engine::sessions::WHISPER;
    use std::collections::VecDeque;
    use wasapi::*;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid: u32 = args.first().ok_or("pass the browser pid")?.parse()?;
    let secs: f32 = args.get(1).map_or(Ok(20.0), |s| s.parse())?;
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

    // Sum of squares per 100 ms slot, by wall clock (process capture sends
    // nothing while the window is silent, so slots are filled by arrival time).
    let slots = (secs * 10.0) as usize + 1;
    let (mut src, mut out) = (vec![(0.0f64, 0usize); slots], vec![(0.0f64, 0usize); slots]);
    let started = std::time::Instant::now();
    let add = |q: &mut VecDeque<u8>, into: &mut Vec<(f64, usize)>, slot: usize| {
        let bytes: Vec<u8> = q.drain(..).collect();
        for b in bytes.chunks_exact(4) {
            let s = f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64;
            into[slot].0 += s * s;
            into[slot].1 += 1;
        }
    };
    let (mut a, mut b) = (VecDeque::new(), VecDeque::new());
    while started.elapsed().as_secs_f32() < secs {
        let slot = ((started.elapsed().as_secs_f32() * 10.0) as usize).min(slots - 1);
        while source_cap.get_next_packet_size()?.unwrap_or(0) > 0 {
            source_cap.read_from_device_to_deque(&mut a)?;
        }
        while speakers_cap.get_next_packet_size()?.unwrap_or(0) > 0 {
            speakers_cap.read_from_device_to_deque(&mut b)?;
        }
        add(&mut a, &mut src, slot);
        add(&mut b, &mut out, slot);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let db = |(sum, n): (f64, usize), gain: f64| -> f64 {
        if n == 0 {
            -120.0
        } else {
            (10.0 * (sum / n as f64 * gain * gain + 1e-12).log10()).max(-120.0)
        }
    };
    // Is the window whispered? Then show its level as it would sound.
    let peak = src.iter().map(|s| db(*s, 1.0)).fold(-120.0, f64::max);
    let gain = if peak < -60.0 {
        1.0 / WHISPER as f64
    } else {
        1.0
    };
    let top = out.iter().map(|s| db(*s, 1.0)).fold(-120.0, f64::max);
    let top_src = src.iter().map(|s| db(*s, gain)).fold(-120.0, f64::max);
    let bar =
        |level: f64, top: f64| "#".repeat(((level - top + 40.0) / 2.0).clamp(0.0, 20.0) as usize);
    println!(
        "  time   window  speakers   (bars: 2 dB per #, 40 dB range below each one's loudest)"
    );
    for i in 0..slots - 1 {
        let (s, o) = (db(src[i], gain), db(out[i], 1.0));
        println!(
            "{:6.1}  {:7.1}  {:7.1}   {:<20} | {:<20}",
            i as f32 / 10.0,
            s - top_src,
            o - top,
            bar(s, top_src),
            bar(o, top)
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
