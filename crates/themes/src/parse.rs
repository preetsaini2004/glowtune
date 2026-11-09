use std::fmt;

use serde::de::DeserializeOwned;
use toml::{Table, Value};

use crate::{Colors, Error, Font, Glass, Glow, Lyrics, Meta, Result, Theme};

/// A recoverable issue in one theme field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// Dotted field path, such as `glow.thickness`.
    pub path: String,
    /// Explanation of the fallback or clamp.
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

fn warn(warnings: &mut Vec<Warning>, path: impl Into<String>, message: impl Into<String>) {
    warnings.push(Warning {
        path: path.into(),
        message: message.into(),
    });
}

struct Section<'a> {
    name: &'static str,
    table: Table,
    warnings: &'a mut Vec<Warning>,
}

impl<'a> Section<'a> {
    fn new(root: &mut Table, name: &'static str, warnings: &'a mut Vec<Warning>) -> Self {
        let table = match root.remove(name) {
            Some(Value::Table(table)) => table,
            None => Table::new(),
            Some(_) => {
                warn(warnings, name, "expected a table; using defaults");
                Table::new()
            }
        };
        Self {
            name,
            table,
            warnings,
        }
    }

    fn field<T: DeserializeOwned>(&mut self, key: &str, default: T) -> T {
        match self.table.remove(key) {
            None => default,
            Some(value) => match value.try_into() {
                Ok(value) => value,
                Err(_) => {
                    warn(
                        self.warnings,
                        format!("{}.{key}", self.name),
                        "invalid value or type; using default",
                    );
                    default
                }
            },
        }
    }

    fn integer(&mut self, key: &str, default: u32, min: u32, max: u32) -> u32 {
        let value = self.field(key, i64::from(default));
        let clamped = value.clamp(i64::from(min), i64::from(max));
        if value != clamped {
            warn(
                self.warnings,
                format!("{}.{key}", self.name),
                format!("clamped to {min}..={max}"),
            );
        }
        clamped as u32
    }

    fn float(&mut self, key: &str, default: f64) -> f64 {
        match self.table.get(key) {
            Some(Value::Integer(value)) => {
                let value = *value as f64;
                self.table.remove(key);
                value
            }
            _ => self.field(key, default),
        }
    }

    fn colors(&mut self, default: Vec<crate::Color>) -> Vec<crate::Color> {
        match self.table.remove("glow") {
            None => default,
            Some(Value::Array(values)) => {
                if values.is_empty() {
                    warn(self.warnings, "colors.glow", "empty list; using default");
                    return default;
                }
                if values.len() > 5 {
                    warn(self.warnings, "colors.glow", "truncated to five colors");
                }
                values
                    .into_iter()
                    .take(5)
                    .enumerate()
                    .map(|(index, value)| {
                        value.try_into().unwrap_or_else(|_| {
                            warn(
                                self.warnings,
                                format!("colors.glow[{index}]"),
                                "invalid color; using default",
                            );
                            default[index % default.len()]
                        })
                    })
                    .collect()
            }
            Some(_) => {
                warn(
                    self.warnings,
                    "colors.glow",
                    "expected a color list; using default",
                );
                default
            }
        }
    }
}

impl Drop for Section<'_> {
    fn drop(&mut self) {
        for key in self.table.keys() {
            warn(self.warnings, format!("{}.{key}", self.name), "unknown key");
        }
    }
}

impl Theme {
    /// Loads user TOML field by field; only invalid TOML syntax is fatal.
    /// Missing identity fields use `Untitled theme` and an empty author.
    pub fn from_toml_str(text: &str) -> Result<(Self, Vec<Warning>)> {
        let mut root = text
            .parse::<Table>()
            .map_err(|error| Error::TomlSyntax(error.to_string()))?;
        let mut warnings = Vec::new();
        let mut theme = Self::default();
        {
            let mut s = Section::new(&mut root, "meta", &mut warnings);
            theme.meta = Meta {
                name: s.field("name", "Untitled theme".to_owned()),
                author: s.field("author", String::new()),
                version: s.field("version", 1),
            };
        }
        {
            let mut s = Section::new(&mut root, "colors", &mut warnings);
            let d = theme.colors;
            theme.colors = Colors {
                mode: s.field("mode", d.mode),
                background: s.field("background", d.background),
                surface: s.field("surface", d.surface),
                text: s.field("text", d.text),
                accent: s.field("accent", d.accent),
                glow: s.colors(d.glow),
            };
        }
        {
            let mut s = Section::new(&mut root, "glass", &mut warnings);
            let d = theme.glass;
            theme.glass = Glass {
                blur: s.integer("blur", d.blur, 0, 64),
                opacity: s.float("opacity", d.opacity),
                radius: s.integer("radius", d.radius, 0, 40),
            };
        }
        {
            let mut s = Section::new(&mut root, "font", &mut warnings);
            let d = theme.font;
            theme.font = Font {
                family: s.field("family", d.family),
                lyrics_family: s.field("lyrics_family", d.lyrics_family),
                lyrics_size: s.integer("lyrics_size", d.lyrics_size, 16, 96),
            };
        }
        {
            let mut s = Section::new(&mut root, "glow", &mut warnings);
            let d = theme.glow;
            theme.glow = Glow {
                enabled: s.field("enabled", d.enabled),
                thickness: s.integer("thickness", d.thickness, 20, 120),
                intensity: s.float("intensity", d.intensity),
                beat_reactivity: s.float("beat_reactivity", d.beat_reactivity),
                drift_speed: s.float("drift_speed", d.drift_speed),
                effect: s.field("effect", d.effect),
                effect_speed: s.float("effect_speed", d.effect_speed),
                particles: s.field("particles", d.particles),
                particle_density: s.float("particle_density", d.particle_density),
                particle_size: s.float("particle_size", d.particle_size),
                particle_speed: s.float("particle_speed", d.particle_speed),
                particle_sparkle: s.float("particle_sparkle", d.particle_sparkle),
                effect_strength: s.float("effect_strength", d.effect_strength),
                reverse_motion: s.field("reverse_motion", d.reverse_motion),
                corners: s.field("corners", d.corners),
                monitors: s.field("monitors", d.monitors),
            };
        }
        {
            let mut s = Section::new(&mut root, "lyrics", &mut warnings);
            let d = theme.lyrics;
            theme.lyrics = Lyrics {
                style: s.field("style", d.style),
                position: s.field("position", d.position),
                show_on_desktop: s.field("show_on_desktop", d.show_on_desktop),
            };
        }
        for key in root.keys() {
            warn(&mut warnings, key, "unknown key");
        }
        let (theme, validation) = theme.sanitized();
        warnings.extend(validation);
        Ok((theme, warnings))
    }

    /// Serializes all sections in schema order, with canonical hex colors.
    pub fn to_toml_string(&self) -> String {
        // Every field has a TOML representation; retain a safe fallback if that changes.
        toml::to_string_pretty(self).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not serialize theme; using Midnight Glass");
            include_str!("../../../themes/midnight-glass.toml").to_owned()
        })
    }

    /// Clamps in-memory values and replaces invalid values, returning every correction.
    pub fn sanitized(mut self) -> (Self, Vec<Warning>) {
        let mut warnings = Vec::new();
        if self.meta.name.trim().is_empty() {
            self.meta.name = "Untitled theme".into();
            warn(
                &mut warnings,
                "meta.name",
                "empty name; using Untitled theme",
            );
        }
        for (path, value, default) in [
            ("font.family", &mut self.font.family, "Instrument Sans"),
            (
                "font.lyrics_family",
                &mut self.font.lyrics_family,
                "Bricolage Grotesque",
            ),
        ] {
            if value.trim().is_empty() || value.chars().count() > 64 {
                *value = default.into();
                warn(
                    &mut warnings,
                    path,
                    "expected a nonempty font name of at most 64 characters; using default",
                );
            }
        }
        for (path, value, min, max) in [
            ("glass.blur", &mut self.glass.blur, 0, 64),
            ("glass.radius", &mut self.glass.radius, 0, 40),
            ("font.lyrics_size", &mut self.font.lyrics_size, 16, 96),
            ("glow.thickness", &mut self.glow.thickness, 20, 120),
        ] {
            let clamped = (*value).clamp(min, max);
            if *value != clamped {
                *value = clamped;
                warn(&mut warnings, path, format!("clamped to {min}..={max}"));
            }
        }
        for (path, value, min, max, default) in [
            ("glass.opacity", &mut self.glass.opacity, 0.0, 1.0, 0.55),
            ("glow.intensity", &mut self.glow.intensity, 0.2, 1.5, 1.0),
            (
                "glow.particle_density",
                &mut self.glow.particle_density,
                0.0,
                1.0,
                0.35,
            ),
            (
                "glow.particle_size",
                &mut self.glow.particle_size,
                0.5,
                3.0,
                1.0,
            ),
            (
                "glow.particle_speed",
                &mut self.glow.particle_speed,
                0.0,
                3.0,
                1.0,
            ),
            (
                "glow.particle_sparkle",
                &mut self.glow.particle_sparkle,
                0.0,
                1.0,
                0.5,
            ),
            (
                "glow.effect_strength",
                &mut self.glow.effect_strength,
                0.0,
                1.0,
                1.0,
            ),
            (
                "glow.effect_speed",
                &mut self.glow.effect_speed,
                0.0,
                3.0,
                1.0,
            ),
            (
                "glow.beat_reactivity",
                &mut self.glow.beat_reactivity,
                0.0,
                1.0,
                0.7,
            ),
            (
                "glow.drift_speed",
                &mut self.glow.drift_speed,
                0.0,
                10.0,
                0.6,
            ),
        ] {
            if !value.is_finite() {
                *value = default;
                warn(
                    &mut warnings,
                    path,
                    "expected a finite number; using default",
                );
            } else if *value < min || *value > max {
                *value = value.clamp(min, max);
                warn(&mut warnings, path, format!("clamped to {min}..={max}"));
            }
        }
        if self.colors.glow.is_empty() {
            self.colors.glow = Colors::default().glow;
            warn(&mut warnings, "colors.glow", "empty list; using default");
        } else if self.colors.glow.len() > 5 {
            self.colors.glow.truncate(5);
            warn(&mut warnings, "colors.glow", "truncated to five colors");
        }
        (self, warnings)
    }
}
