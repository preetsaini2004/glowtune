//! Now-playing info from the OS: SMTC on Windows, MPRIS on Linux.
//!
//! This is Glowtune's "follow mode" foundation (PLAN.md section 2). The OS already
//! knows what every player (Spotify, browser tabs, Apple Music...) is playing, so
//! we just listen to it instead of integrating each service.
//!
//! ```text
//!  OS media events ──► backend worker thread ──► dedup ──► mpsc::Receiver<MediaEvent>
//!   (SMTC / MPRIS)        (blocks, no polling)              (consumed by the app)
//! ```
//!
//! The API is runtime-agnostic: a plain std thread plus a `std::sync::mpsc` channel.
//! The Tauri app can bridge it into async code later.
//!
//! ```no_run
//! let (_watcher, events) = media_source::MediaWatcher::start()?;
//! for event in events {
//!     println!("{event:?}");
//! }
//! # Ok::<(), media_source::Error>(())
//! ```

mod types;
pub use types::*;

#[cfg(windows)]
mod smtc;
#[cfg(windows)]
mod volume_win;
#[cfg(windows)]
use smtc as backend;

#[cfg(target_os = "linux")]
mod mpris;
#[cfg(target_os = "linux")]
use mpris as backend;

use std::sync::mpsc::Receiver;

/// Errors from starting or running a media backend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(windows)]
    #[error("Windows media API error: {0}")]
    Windows(#[from] ::windows::core::Error),

    #[cfg(target_os = "linux")]
    #[error("D-Bus error: {0}")]
    DBus(#[from] ::zbus::Error),

    #[cfg(target_os = "linux")]
    #[error("D-Bus call failed: {0}")]
    DBusCall(#[from] ::zbus::fdo::Error),

    #[error("media backend thread failed: {0}")]
    Thread(String),

    #[error("no media backend for this platform")]
    Unsupported,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Watches the OS "now playing" state and sends [`MediaEvent`]s.
///
/// Dropping the watcher stops the worker thread and unsubscribes from OS events.
pub struct MediaWatcher {
    #[cfg(any(windows, target_os = "linux"))]
    _inner: backend::Watcher,
}

impl MediaWatcher {
    /// Starts watching. Current state (session, track, art, playback, timeline, modes)
    /// is sent right away, then only changes.
    #[cfg(any(windows, target_os = "linux"))]
    pub fn start() -> Result<(Self, Receiver<MediaEvent>)> {
        let (inner, rx) = backend::Watcher::start()?;
        Ok((Self { _inner: inner }, rx))
    }

    /// Sends a transport command (play/pause, next, seek...) to the current
    /// player. Fire-and-forget: the resulting state change arrives as events.
    pub fn control(&self, control: Control) {
        let control = match control {
            Control::SetVolume(volume) => match types::clamp_volume(volume) {
                Some(volume) => Control::SetVolume(volume),
                None => return,
            },
            other => other,
        };
        #[cfg(any(windows, target_os = "linux"))]
        self._inner.control(control);
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = control;
    }

    /// Volume of the current player, 0.0..=1.0, or None if unknown.
    ///
    /// Waits at most 250 ms for the worker; a busy or unavailable player returns None.
    pub fn volume(&self) -> Option<f32> {
        #[cfg(any(windows, target_os = "linux"))]
        return self._inner.volume();
        #[cfg(not(any(windows, target_os = "linux")))]
        None
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub fn start() -> Result<(Self, Receiver<MediaEvent>)> {
        Err(Error::Unsupported)
    }
}
