//! Runs the engine on a window's sound from the command line:
//! `live <browser pid> [seconds] [preset id] [crossfade seconds]`

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use audio_engine::{Blend, Engine, EqSettings, PRESETS};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid: u32 = args.first().ok_or("pass the browser pid")?.parse()?;
    let secs: u64 = args.get(1).map_or(Ok(10), |s| s.parse())?;
    let eq = match args
        .get(2)
        .and_then(|id| PRESETS.iter().find(|p| p.0 == id))
    {
        Some(&(_, _, preamp_db, gains_db)) => EqSettings {
            enabled: true,
            preamp_db,
            gains_db,
        },
        None => EqSettings::default(),
    };
    let blend = match args.get(3).and_then(|s| s.parse::<f32>().ok()) {
        Some(secs) => Blend::Crossfade { secs },
        None => Blend::Off,
    };
    let engine = Engine::start(
        pid,
        eq,
        blend,
        1.0,
        Box::new(|| eprintln!("  (timing changed)")),
    )?;
    let status = engine.status();
    for i in 0..secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!(
            "{i:>3}s running={} latency={} ms speed={} blending={}",
            status.running(),
            status.latency_ms(),
            status.speed(),
            status.blending()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
