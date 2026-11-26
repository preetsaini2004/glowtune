//! Sound effects for Glowtune's service windows (see `docs/architecture.md`).
//!
//! ```text
//!  service window sound ─► capture ─► Player (delay, crossfade) ─► Equalizer ─► Limiter ─► speakers
//! ```
//! `eq` and `player` are plain math and run anywhere; the capture and
//! playback route (`engine`, `sessions`) is Windows-only for now.

pub mod dj;
#[cfg(windows)]
pub mod engine;
pub mod eq;
#[cfg(windows)]
pub mod listen;
#[cfg(windows)]
mod output;
pub mod player;
pub mod resample;
#[cfg(windows)]
pub mod sessions;
mod stretch;
pub mod tempo;
pub mod voice;

#[cfg(windows)]
pub use engine::{Command, Engine, Status};
pub use eq::{BANDS, EqSettings, Equalizer, Limiter, PRESETS, Volume};
pub use player::{Blend, Player};
