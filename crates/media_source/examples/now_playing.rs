//! Prints live "now playing" events from the OS.
//!
//! Run: `cargo run -p media_source --example now_playing`
//! Then play / pause / skip / seek in Spotify or a browser tab. Ctrl+C to quit.
//! The latest album art is saved to `target/now_playing_art.<ext>`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use media_source::{AlbumArt, MediaEvent, MediaWatcher, PlaybackStatus};

fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let (watcher, events) = match MediaWatcher::start() {
        Ok(started) => started,
        Err(e) => {
            eprintln!("could not start media watcher: {e}");
            std::process::exit(1);
        }
    };
    println!("Listening for media events. Play / pause / skip / seek something. Ctrl+C to quit.\n");
    println!("VOLUME {:?}", watcher.volume());

    let t0 = Instant::now();
    // crates/media_source → workspace root → target/
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .ancestors()
        .nth(2)
        .unwrap_or(&manifest_dir)
        .join("target");

    let mut status = PlaybackStatus::Stopped;
    for event in events {
        let ts = format!("[{:>8.3}s]", t0.elapsed().as_secs_f64());
        match event {
            MediaEvent::ModesChanged(modes) => println!(
                "{ts} MODES shuffle={:?} repeat={:?}",
                modes.shuffle, modes.repeat
            ),
            MediaEvent::SessionChanged(app) => {
                println!(
                    "{ts} SESSION  {}",
                    app.as_deref().unwrap_or("(nothing playing)")
                )
            }
            MediaEvent::TrackChanged(t) => println!(
                "{ts} TRACK    {} — {} ({}) {} [{}]",
                t.title,
                t.artist,
                if t.album.is_empty() {
                    "no album"
                } else {
                    &t.album
                },
                t.duration.map(fmt_dur).unwrap_or_else(|| "?:??".into()),
                t.source_app
            ),
            MediaEvent::ArtChanged(None) => println!("{ts} ART      (none)"),
            MediaEvent::ArtChanged(Some(AlbumArt::Url(url))) => println!("{ts} ART      {url}"),
            MediaEvent::ArtChanged(Some(AlbumArt::Bytes { data, mime })) => {
                let ext = mime.strip_prefix("image/").unwrap_or("bin");
                let path = target_dir.join(format!("now_playing_art.{ext}"));
                let saved = match std::fs::write(&path, &data) {
                    Ok(()) => format!("saved to {}", path.display()),
                    Err(e) => format!("not saved: {e}"),
                };
                println!(
                    "{ts} ART      {:.1} KB {mime}, {saved}",
                    data.len() as f64 / 1024.0
                );
            }
            MediaEvent::PlaybackChanged(new_status) => {
                status = new_status;
                println!("{ts} STATUS   {status:?}");
            }
            MediaEvent::TimelineChanged(tl) => {
                // Interpolate only while playing; a paused position stays put.
                let now = if status == PlaybackStatus::Playing {
                    tl.position_now()
                } else {
                    tl.position
                };
                println!(
                    "{ts} TIMELINE {} / {}  rate {:.2}, reported {:.1}s ago → now ≈ {}",
                    fmt_dur(tl.position),
                    tl.duration.map(fmt_dur).unwrap_or_else(|| "?:??".into()),
                    tl.rate,
                    tl.reported_at.elapsed().as_secs_f64(),
                    fmt_dur(now),
                )
            }
        }
    }
}

fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}
