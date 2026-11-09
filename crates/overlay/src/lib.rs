//! Native screen-edge glow overlay.
//!
//! ```text
//!  app thread                         overlay thread (winit event loop)
//!  ──────────                         ──────────────────────────────────
//!  Overlay::set_palette() ──proxy──►  crossfade to new colors
//!  Overlay::set_playing()  ──proxy──►  fade in / fade out → hide + sleep
//!  Overlay::set_energy()   ──proxy──►  beat pulse (Phase 3)
//!  Overlay::set_config()   ──proxy──►  rebuild strips if needed
//! ```
//!
//! Each monitor gets four thin, click-through, always-on-top strip windows
//! drawn by the GPU (wgpu, DX12 + DirectComposition for real transparency).
//! When nothing plays, the glow fades out, the windows hide and the thread
//! sleeps: no rendering, ~0% CPU.
//!
//! The public API doesn't expose winit, so a Linux layer-shell backend can
//! slot in later.

#[cfg(windows)]
mod win;

/// Which monitors get the glow.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MonitorSelection {
    #[default]
    All,
    Primary,
    /// Monitor names as reported by [`Overlay::monitors`] (e.g. `\\.\DISPLAY1`).
    Only(Vec<String>),
}

/// Spatial light formation. Discriminants are shared with the GPU shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum GlowEffect {
    /// Existing breathing edge glow.
    #[default]
    Ambient,
    /// Fixed formation with no beat response.
    Still,
    /// Beat-driven blooms concentrated at the corners.
    Pulse,
    /// A bright swell travelling around the screen.
    Orbit,
    /// Uneven ribbons that deform with the beat.
    Distortion,
    /// Soft moving blobs that merge and separate.
    LavaLamp,
}

/// Appearance of light-synchronized particles. Discriminants match the GPU shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum ParticleStyle {
    /// No particles.
    #[default]
    Off,
    /// Small round points of light.
    Dots,
    /// Four-point star glints.
    Stars,
    /// Soft floating dust motes.
    Dust,
}

/// Glow look & behavior. Mirrors the `[glow]` section of a theme (PLAN.md section 7).
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayConfig {
    pub enabled: bool,
    /// Glow depth in logical pixels (scaled per monitor DPI).
    pub thickness: f32,
    /// Overall brightness, 0..1.5.
    pub intensity: f32,
    /// Corner roundness of the glow's inner contour, logical pixels.
    pub corner_radius: f32,
    /// How much the glow reacts to the beat, 0 = calm, 1 = punchy (Phase 3).
    pub beat_reactivity: f32,
    /// How fast colors drift around the screen, in turns per minute.
    pub drift_speed: f32,
    /// Spatial light formation, independent of palette motion.
    pub effect: GlowEffect,
    /// Formation motion multiplier, 0..3.
    pub effect_speed: f32,
    /// Optional particle shape.
    pub particles: ParticleStyle,
    /// Particle cell occupancy, 0..1.
    pub particle_density: f32,
    /// Radius multiplier, 0.5..3.
    pub particle_size: f32,
    /// Movement relative to formation motion, 0..3.
    pub particle_speed: f32,
    /// Twinkle strength, 0..1.
    pub particle_sparkle: f32,
    /// Spatial formation strength, 0..1.
    pub effect_strength: f32,
    /// Reverse formation and particle travel.
    pub reverse_motion: bool,
    pub monitors: MonitorSelection,
    /// Frame cap while animating.
    pub max_fps: u32,
    /// Hide the glow on a screen while a fullscreen app or game is in front.
    pub hide_on_fullscreen: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            thickness: 56.0,
            intensity: 1.0,
            corner_radius: 24.0,
            beat_reactivity: 0.7,
            drift_speed: 0.6,
            effect: GlowEffect::Ambient,
            effect_speed: 1.0,
            particles: ParticleStyle::Off,
            particle_density: 0.35,
            particle_size: 1.0,
            particle_speed: 1.0,
            particle_sparkle: 0.5,
            effect_strength: 1.0,
            reverse_motion: false,
            monitors: MonitorSelection::All,
            max_fps: 60,
            hide_on_fullscreen: true,
        }
    }
}

/// A connected monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub name: String,
    pub primary: bool,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub scale: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the glow overlay isn't supported on this platform yet")]
    Unsupported,
    #[error("no usable GPU for the overlay: {0}")]
    Gpu(String),
    #[error("overlay window error: {0}")]
    Window(String),
    #[error("overlay thread failed: {0}")]
    Thread(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Handle to the running overlay. Cheap to call from any thread.
/// Dropping it closes all glow windows.
pub struct Overlay {
    #[cfg(windows)]
    inner: win::Handle,
}

impl Overlay {
    /// Starts the overlay thread. The glow stays hidden until `set_playing(true)`.
    pub fn spawn(config: OverlayConfig) -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: win::Handle::spawn(config)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Err(Error::Unsupported)
        }
    }

    /// New album colors (sRGB, 1–5 colors, most important first). Crossfades smoothly.
    pub fn set_palette(&self, colors: &[[u8; 3]]) {
        #[cfg(windows)]
        self.inner
            .send(win::Cmd::Palette(colors.iter().copied().take(5).collect()));
        #[cfg(not(windows))]
        let _ = colors;
    }

    /// Playing → glow fades in. Paused/stopped → fades out, then stops drawing.
    pub fn set_playing(&self, playing: bool) {
        #[cfg(windows)]
        self.inner.send(win::Cmd::Playing(playing));
        #[cfg(not(windows))]
        let _ = playing;
    }

    /// Current loudness/beat energy, 0..1 (Phase 3).
    pub fn set_energy(&self, energy: f32) {
        #[cfg(windows)]
        self.inner.send(win::Cmd::Energy(energy));
        #[cfg(not(windows))]
        let _ = energy;
    }

    pub fn set_config(&self, config: OverlayConfig) {
        #[cfg(windows)]
        self.inner.send(win::Cmd::Config(Box::new(config)));
        #[cfg(not(windows))]
        let _ = config;
    }

    /// Monitors as last seen by the overlay (for the settings UI).
    pub fn monitors(&self) -> Vec<MonitorInfo> {
        #[cfg(windows)]
        {
            self.inner.monitors()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }
}

/// Top-left corner (physical pixels) of the screen where a fullscreen app or
/// game is in front right now, if any. Cheap; safe to call from any thread.
pub fn fullscreen_screen() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        win::fullscreen_monitor_origin()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
