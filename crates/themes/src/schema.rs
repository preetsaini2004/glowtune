//! Theme values and their Midnight Glass defaults.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// An sRGB color with an unpremultiplied alpha channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel; 255 is opaque.
    pub a: u8,
}

impl Color {
    /// Parses `#rgb`, `#rrggbb`, or `#rrggbbaa`, ignoring surrounding whitespace.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().strip_prefix('#')?;
        if !hex.is_ascii() {
            return None;
        }
        let channel = |start, len| u8::from_str_radix(&hex[start..start + len], 16).ok();
        match hex.len() {
            3 => Some(Self {
                r: channel(0, 1)? * 17,
                g: channel(1, 1)? * 17,
                b: channel(2, 1)? * 17,
                a: 255,
            }),
            6 | 8 => Some(Self {
                r: channel(0, 2)?,
                g: channel(2, 2)?,
                b: channel(4, 2)?,
                a: if hex.len() == 8 { channel(6, 2)? } else { 255 },
            }),
            _ => None,
        }
    }

    /// Returns lowercase hex, omitting alpha when opaque.
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| de::Error::custom("invalid hex color"))
    }
}

/// Source of glow colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Derive colors from album artwork.
    #[default]
    Adaptive,
    /// Use the first configured glow color.
    Static,
    /// Use all configured glow colors.
    Gradient,
}

/// Spatial animation of the screen-edge light, independent of color drift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// Appearance of optional light-synchronized particles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// Shape of screen-edge glow corners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    /// Round the corners.
    #[default]
    Rounded,
    /// Keep square corners.
    Square,
}

/// Animation used for lyrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricsStyle {
    /// Raise each new line into view.
    #[default]
    Rise,
    /// Highlight the active lyric.
    Karaoke,
    /// Display without movement.
    Static,
}

/// Placement of desktop lyrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LyricsPosition {
    /// Center near the bottom of the screen.
    #[default]
    BottomCenter,
    /// Center near the top of the screen.
    TopCenter,
    /// Center in the screen.
    Center,
}

/// Displays on which the glow is enabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Monitors {
    /// Every display.
    #[default]
    All,
    /// The primary display.
    Primary,
    /// Displays identified by their names.
    Only(Vec<String>),
}

impl Serialize for Monitors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str("all"),
            Self::Primary => serializer.serialize_str("primary"),
            Self::Only(names) => names.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Monitors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Name(String),
            Names(Vec<String>),
        }
        match Input::deserialize(deserializer)? {
            Input::Name(name) if name == "all" => Ok(Self::All),
            Input::Name(name) if name == "primary" => Ok(Self::Primary),
            Input::Names(names) => Ok(Self::Only(names)),
            _ => Err(de::Error::custom("expected all, primary, or monitor names")),
        }
    }
}

macro_rules! section {
    ($(#[$doc:meta])* $name:ident { $($(#[$field_doc:meta])* $field:ident: $ty:ty = $default:expr),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $($(#[$field_doc])* pub $field: $ty,)*
        }
        impl Default for $name {
            fn default() -> Self { Self { $($field: $default,)* } }
        }
    };
}

const ACCENT: Color = Color {
    r: 139,
    g: 124,
    b: 255,
    a: 255,
};
const BLUE: Color = Color {
    r: 92,
    g: 160,
    b: 255,
    a: 255,
};

section! {
    /// Theme identity and schema version.
    Meta {
        /// Display name; missing names become `Untitled theme` when loading.
        name: String = "Midnight Glass".into(),
        /// Theme creator; missing authors become an empty string when loading.
        author: String = "Glowtune".into(),
        /// Schema version, preserved as a TOML integer (currently 1).
        version: i64 = 1,
    }
}
section! {
    /// UI and glow colors.
    Colors {
        /// How glow colors are selected.
        mode: ColorMode = ColorMode::Adaptive,
        /// Window background tint.
        background: Color = Color { r: 11, g: 11, b: 16, a: 255 },
        /// Raised surface tint.
        surface: Color = Color { r: 255, g: 255, b: 255, a: 20 },
        /// Foreground text color.
        text: Color = Color { r: 245, g: 245, b: 247, a: 255 },
        /// UI accent color.
        accent: Color = ACCENT,
        /// One to five glow colors, ignored in adaptive mode.
        glow: Vec<Color> = vec![ACCENT, BLUE],
    }
}
section! {
    /// Glass surface treatment.
    Glass {
        /// Blur radius in pixels (0 to 64).
        blur: u32 = 24,
        /// Tint strength over acrylic (0 to 1).
        opacity: f64 = 0.55,
        /// Corner radius in pixels (0 to 40).
        radius: u32 = 14,
    }
}
section! {
    /// UI and lyric typography.
    Font {
        /// UI font family, nonempty and at most 64 characters.
        family: String = "Instrument Sans".into(),
        /// Lyric font family, nonempty and at most 64 characters.
        lyrics_family: String = "Bricolage Grotesque".into(),
        /// Lyric font size in pixels (16 to 96).
        lyrics_size: u32 = 50,
    }
}
section! {
    /// Screen-edge light behavior.
    Glow {
        /// Whether to show the glow.
        enabled: bool = true,
        /// Glow thickness in pixels (20 to 120).
        thickness: u32 = 56,
        /// Glow strength (0.2 to 1.5).
        intensity: f64 = 1.0,
        /// Response to musical beats (0 to 1).
        beat_reactivity: f64 = 0.7,
        /// Color drift in turns per minute (0 to 10).
        drift_speed: f64 = 0.6,
        /// Spatial light formation.
        effect: GlowEffect = GlowEffect::Ambient,
        /// Formation motion multiplier (0 to 3); independent of color drift.
        effect_speed: f64 = 1.0,
        /// Optional particles moving with the light.
        particles: ParticleStyle = ParticleStyle::Off,
        /// Fraction of occupied particle cells (0 to 1).
        particle_density: f64 = 0.35,
        /// Particle radius multiplier (0.5 to 3).
        particle_size: f64 = 1.0,
        /// Particle movement relative to the formation (0 to 3).
        particle_speed: f64 = 1.0,
        /// Twinkle strength (0 to 1).
        particle_sparkle: f64 = 0.5,
        /// Strength of the spatial formation (0 to 1).
        effect_strength: f64 = 1.0,
        /// Reverse formation and particle movement.
        reverse_motion: bool = false,
        /// Glow corner shape.
        corners: Corners = Corners::Rounded,
        /// Displays to illuminate.
        monitors: Monitors = Monitors::All,
    }
}
section! {
    /// Desktop lyric presentation.
    Lyrics {
        /// Lyric animation style.
        style: LyricsStyle = LyricsStyle::Rise,
        /// Lyric placement.
        position: LyricsPosition = LyricsPosition::BottomCenter,
        /// Whether desktop lyrics are visible.
        show_on_desktop: bool = true,
    }
}
section! {
    /// A complete theme; use `from_toml_str` for fault-tolerant user input.
    Theme {
        /// Theme identity.
        meta: Meta = Meta::default(),
        /// Theme colors.
        colors: Colors = Colors::default(),
        /// Glass treatment.
        glass: Glass = Glass::default(),
        /// Typography.
        font: Font = Font::default(),
        /// Screen-edge glow.
        glow: Glow = Glow::default(),
        /// Desktop lyrics.
        lyrics: Lyrics = Lyrics::default(),
    }
}
