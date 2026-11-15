//! Prints the tempo the engine hears in a window's sound, every 6 seconds:
//! `tempo_live <browser pid> [rounds]`. Nothing is saved.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::VecDeque;
    use wasapi::*;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid: u32 = args.first().ok_or("pass the browser pid")?.parse()?;
    let rounds: usize = args.get(1).map_or(Ok(5), |s| s.parse())?;
    initialize_mta().ok()?;
    let format = WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
    let mut client = AudioClient::new_application_loopback_client(pid, true)?;
    client.initialize_client(
        &format,
        Direction::Capture,
        StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: 2_000_000,
        },
        None,
    )?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    for _ in 0..rounds {
        let mut bytes = VecDeque::<u8>::new();
        let started = std::time::Instant::now();
        while started.elapsed().as_secs_f32() < 6.0 {
            while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                capture.read_from_device_to_deque(&mut bytes)?;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let raw: Vec<u8> = bytes.into_iter().collect();
        let frames: Vec<[f32; 2]> = raw
            .chunks_exact(8)
            .map(|b| {
                [
                    f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                ]
            })
            .collect();
        match audio_engine::tempo::estimate(&frames, 48000) {
            Some(t) => println!(
                "{:6.1} BPM  confidence {:.2}  ({} frames)",
                t.bpm(48000),
                t.confidence,
                frames.len()
            ),
            None => println!("no sound or too little ({} frames)", frames.len()),
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
