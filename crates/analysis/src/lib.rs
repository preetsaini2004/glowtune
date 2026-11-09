//! Audio analysis (FFT, beat/onset) and album-art color extraction.

pub mod beat;
pub mod colors;
pub mod oklab;

pub use beat::{BeatDetector, BeatFrame};
pub use colors::{GlowPalette, PaletteError, extract_palette, palette_from_rgba};
