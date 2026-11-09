//! Sends one transport command to the current player, for scripted tests.
//!
//! `cargo run -p media_source --example control -- pause|play|toggle|next|prev|seek <secs>`
//! Also: `shuffle on|off`, `repeat off|track|list`, `volume 0.5`.

use std::time::Duration;

use media_source::{Control, MediaWatcher, RepeatMode};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let control = match args.first().map(String::as_str) {
        Some("pause") => Control::Pause,
        Some("play") => Control::Play,
        Some("toggle") => Control::PlayPause,
        Some("next") => Control::Next,
        Some("prev") => Control::Previous,
        Some("shuffle") => Control::SetShuffle(match args.get(1).map(String::as_str) {
            Some("on") => true,
            Some("off") => false,
            _ => usage(),
        }),
        Some("repeat") => Control::SetRepeat(match args.get(1).map(String::as_str) {
            Some("off") => RepeatMode::Off,
            Some("track") => RepeatMode::Track,
            Some("list") => RepeatMode::List,
            _ => usage(),
        }),
        Some("volume") => Control::SetVolume(match args.get(1).and_then(|s| s.parse().ok()) {
            Some(volume) => volume,
            None => usage(),
        }),
        Some("seek") => {
            let secs: f64 = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .expect("seek needs seconds");
            Control::SeekTo(Duration::from_secs_f64(secs))
        }
        _ => usage(),
    };
    let (watcher, rx) = MediaWatcher::start().expect("media watcher");
    // Wait for the first snapshot so a session is selected.
    let _ = rx.recv_timeout(Duration::from_secs(2));
    watcher.control(control);
    // Give the command time to be delivered before the worker shuts down.
    std::thread::sleep(Duration::from_millis(400));
}

fn usage() -> ! {
    eprintln!(
        "usage: control pause|play|toggle|next|prev|seek <secs>|shuffle on|off|repeat off|track|list|volume <0..1>"
    );
    std::process::exit(2);
}
