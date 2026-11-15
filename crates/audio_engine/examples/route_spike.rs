//! Spike for UPDATE-PLAN §2 Step A, route a: does process loopback capture stay
//! at full volume while the app's own audio session is muted?
//!
//! `cargo run -p audio_engine --example route_spike -- <browser pid>`

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use audio_engine::sessions;
    use wasapi::*;
    use windows::core::GUID;

    let pid: u32 = std::env::args()
        .nth(1)
        .ok_or("pass the browser pid")?
        .parse()?;
    initialize_mta().ok()?;

    let format = WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
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
    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;

    let measure = |label: &str, secs: f32| -> Result<f32, Box<dyn std::error::Error>> {
        let mut queue = std::collections::VecDeque::<u8>::new();
        let want = (48000.0 * secs) as usize * 8;
        let started = std::time::Instant::now();
        while queue.len() < want && started.elapsed().as_secs_f32() < secs + 2.0 {
            if capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                capture.read_from_device_to_deque(&mut queue)?;
            } else {
                let _ = event.wait_for_event(200);
            }
        }
        let bytes: Vec<u8> = queue.into_iter().collect();
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt();
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        let mut distinct: Vec<u32> = samples.iter().map(|s| s.to_bits()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        println!(
            "{label:<28} frames={:>7} rms={rms:.3e} peak={peak:.3e} distinct values={}",
            samples.len() / 2,
            distinct.len()
        );
        Ok(rms)
    };

    let tree = sessions::process_tree(pid);
    let vols = sessions::session_volumes(&tree)?;
    println!(
        "process tree: {} processes, {} audio sessions",
        tree.len(),
        vols.len()
    );
    for (p, v) in &vols {
        // SAFETY: COM calls on live interfaces.
        unsafe {
            println!(
                "  session pid={p} volume={:.2} muted={}",
                v.GetMasterVolume()?,
                v.GetMute()?.as_bool()
            );
        }
    }

    let before = measure("normal", 2.0)?;
    sessions::set_muted(&tree, true)?;
    let _ = measure("(settling)", 0.5)?;
    let muted = measure("session muted", 2.0)?;
    sessions::set_muted(&tree, false)?;
    println!("muted/normal = {:.3}", muted / before);
    // Route a2: turn the session down to a whisper instead of muting it, and
    // make up the gain on the captured copy (the mix is 32-bit float).
    for scalar in [0.01f32, 0.001, 0.0001] {
        for (_, v) in &vols {
            unsafe { v.SetMasterVolume(scalar, &GUID::zeroed())? };
        }
        let _ = measure("(settling)", 0.5)?;
        let rms = measure(&format!("session volume {scalar}"), 2.0)?;
        println!("  ratio to normal = {:.6} (ideal {scalar})", rms / before);
    }
    for (_, v) in &vols {
        unsafe { v.SetMasterVolume(1.0, &GUID::zeroed())? };
    }
    let after = measure("restored", 1.0)?;
    println!("restored/normal = {:.3}", after / before);
    client.stop_stream()?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
