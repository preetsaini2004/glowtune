//! Per-PC settings, saved as JSON in the app config folder and applied live.
//!
//! How things *look* (glow thickness, colors, lyrics style...) lives in the
//! theme (`theme_service`). Settings hold what's specific to this machine:
//! whether the glow is on, which screens glow, where windows go, which theme.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme_service::DEFAULT_THEME;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Optional Crate catalog URL. Empty uses only the bundled collection.
    pub store_url: String,
    pub glow_enabled: bool,
    /// Monitor names that glow. Empty = all monitors.
    pub glow_monitors: Vec<String>,
    pub lyrics_on_desktop: bool,
    /// Floating desktop lyrics look: "theme" (the theme's own), "stars", "rain".
    pub desktop_lyrics_style: String,
    /// Keep the desktop lyrics up over full-screen games and videos.
    pub lyrics_over_fullscreen: bool,
    /// Mask swear words in displayed lyrics ("f***"); the audio is untouched.
    pub clean_lyrics: bool,
    /// Where you dragged the desktop lyrics (physical px, top-left); None = theme position.
    pub lyrics_position: Option<[i32; 2]>,
    /// Size you gave the desktop lyrics box (logical px); None = the default.
    pub lyrics_box: Option<[f64; 2]>,
    /// Hide the glow on a screen while a fullscreen app or game is in front.
    pub glow_hide_fullscreen: bool,
    /// Block ads and trackers inside the service windows (Spotify, YouTube Music...).
    pub block_ads: bool,
    pub player_on_top: bool,
    /// The taskbar player: "off", "left" or "right" end of the taskbar.
    pub taskbar_player: String,
    /// Mini player size: s, m, l.
    pub player_size: String,
    /// Mini player look: glass, disc, card, pill, lyric.
    pub player_style: String,
    /// Id of the active theme (bundled or in the themes folder).
    pub active_theme: String,
    /// Use the selected theme; off restores the built-in default look.
    pub use_theme: bool,
    /// Hidden from pickers without deleting theme files.
    pub disabled_themes: Vec<String>,
    /// Desktop scene: a full-screen live wallpaper that sits behind all windows.
    pub stage_enabled: bool,
    /// Screen for the desktop scene (monitor name); empty = main, "*" = all screens.
    pub stage_monitor: String,
    /// What the desktop scene shows.
    pub stage_clock: bool,
    pub stage_lyrics: bool,
    pub stage_recent: bool,
    /// Show the active service playback queue beside recent tracks.
    pub stage_upcoming: bool,
    /// Slowly drifting background (off = still image, zero animation).
    pub stage_motion: bool,
    /// Sung words with a clear emoji burst into a fan of it.
    pub stage_emoji: bool,
    /// Water / Frost / Halftone / Mosaic / Dots wallpapers answer the kick drum.
    pub stage_beat: bool,
    /// Lyrics look in the scene: bold, flow, karaoke, outline, sticker.
    pub stage_lyrics_style: String,
    /// Clock look in the scene: outline, solid, minimal.
    pub stage_clock_style: String,
    /// Scene background: "art" (album art wash) or "video" (the song's music video).
    pub stage_wallpaper: String,
    /// Album card look: tilt, print, vinyl, glow.
    pub stage_art_style: String,
    /// Dock look: bar, pill, islands, line.
    pub stage_dock_style: String,
    /// Show the dock (controls + search) at the bottom of the scene.
    pub stage_dock: bool,
    /// Colors come from the album art (true) or from `custom_colors`.
    pub artwork_colors: bool,
    /// 1–3 hex colors used everywhere (glow, scene, player) when `artwork_colors` is off.
    pub custom_colors: Vec<String>,
    /// Older builds had one switch for the equalizer and the song blend
    /// together. Read once, turned into `eq_enabled` / `sound_blend`, never
    /// written again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound_effects: Option<bool>,
    /// Equalizer on. Independent of `sound_blend`: either one alone makes
    /// Glowtune process the service windows' sound.
    pub eq_enabled: bool,
    /// Equalizer preset id (see `audio_engine::PRESETS`) or "custom".
    pub eq_preset: String,
    /// Equalizer preamp in dB (−12..12).
    pub eq_preamp: f32,
    /// Ten band gains in dB (−12..12), 31 Hz to 16 kHz.
    pub eq_gains: Vec<f32>,
    /// How one song hands over to the next: "off", "crossfade" or "automix".
    pub sound_blend: String,
    /// Crossfade length in seconds (1..12).
    pub crossfade_secs: f32,
    /// Glowtune's own volume on top of the service page's while Sound effects
    /// run (1 = as the page plays; see `Sound::set_gain`). Set from the
    /// player's volume slider, not shown in Settings.
    pub service_volume: f32,
    /// Global hotkeys (Ctrl+Alt+Space, arrows, 1..9...; see `hotkeys`).
    pub hotkeys: bool,
    /// Keep a private log of what you play, for Settings → Listening.
    pub listening_history: bool,
    /// Your saved scene looks (see `presets`), at most 9.
    pub scene_presets: Vec<crate::presets::ScenePreset>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            store_url: String::new(),
            glow_enabled: true,
            glow_monitors: Vec::new(),
            lyrics_on_desktop: true,
            lyrics_over_fullscreen: false,
            desktop_lyrics_style: "theme".to_string(),
            clean_lyrics: false,
            lyrics_position: None,
            lyrics_box: None,
            glow_hide_fullscreen: true,
            block_ads: true,
            player_on_top: false,
            player_style: "glass".to_string(),
            player_size: "m".to_string(),
            taskbar_player: "off".to_string(),
            active_theme: DEFAULT_THEME.to_string(),
            use_theme: true,
            disabled_themes: Vec::new(),
            stage_enabled: false,
            stage_monitor: String::new(),
            stage_clock: true,
            stage_lyrics: true,
            stage_recent: true,
            stage_upcoming: true,
            stage_motion: true,
            stage_emoji: true,
            stage_beat: true,
            stage_lyrics_style: "bold".to_string(),
            stage_clock_style: "outline".to_string(),
            stage_wallpaper: "art".to_string(),
            stage_art_style: "tilt".to_string(),
            stage_dock_style: "bar".to_string(),
            stage_dock: true,
            artwork_colors: true,
            custom_colors: vec!["#ff3da6".to_string(), "#3d7bff".to_string()],
            sound_effects: None,
            eq_enabled: false,
            eq_preset: "flat".to_string(),
            eq_preamp: 0.0,
            eq_gains: vec![0.0; 10],
            sound_blend: "off".to_string(),
            crossfade_secs: 6.0,
            service_volume: 1.0,
            hotkeys: true,
            listening_history: true,
            scene_presets: Vec::new(),
        }
    }
}

impl Settings {
    /// Repairs values that could break things (settings files can be hand-edited).
    pub fn sanitized(mut self) -> Self {
        let valid_id = !self.active_theme.is_empty()
            && self.active_theme.len() <= 48
            && self
                .active_theme
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid_id {
            self.active_theme = DEFAULT_THEME.to_string();
        }
        self.glow_monitors.retain(|m| !m.trim().is_empty());
        if !matches!(
            self.stage_lyrics_style.as_str(),
            "bold"
                | "flow"
                | "karaoke"
                | "outline"
                | "sticker"
                | "stars"
                | "depth"
                | "rain"
                | "orbit"
                | "shards"
                | "code"
                | "melt"
                | "tags"
        ) {
            self.stage_lyrics_style = "bold".to_string();
        }
        if !matches!(
            self.stage_clock_style.as_str(),
            "outline" | "solid" | "minimal"
        ) {
            self.stage_clock_style = "outline".to_string();
        }
        if !matches!(
            self.desktop_lyrics_style.as_str(),
            "theme" | "stars" | "rain" | "shards" | "code" | "melt"
        ) {
            self.desktop_lyrics_style = "theme".to_string();
        }
        if !matches!(self.player_size.as_str(), "s" | "m" | "l") {
            self.player_size = "m".to_string();
        }
        if !matches!(self.taskbar_player.as_str(), "off" | "left" | "right") {
            self.taskbar_player = "off".to_string();
        }
        if !matches!(
            self.player_style.as_str(),
            "glass" | "disc" | "card" | "pill" | "lyric" | "clear"
        ) {
            self.player_style = "glass".to_string();
        }
        if !matches!(
            self.stage_art_style.as_str(),
            "tilt" | "print" | "vinyl" | "glow"
        ) {
            self.stage_art_style = "tilt".to_string();
        }
        if !matches!(
            self.stage_dock_style.as_str(),
            "bar" | "pill" | "islands" | "line"
        ) {
            self.stage_dock_style = "bar".to_string();
        }
        if !matches!(
            self.stage_wallpaper.as_str(),
            "art" | "video" | "silk" | "water" | "frost" | "mosaic" | "halftone" | "cover" | "dots"
        ) {
            self.stage_wallpaper = "art".to_string();
        }
        self.custom_colors.retain(|c| parse_hex(c).is_some());
        self.custom_colors.truncate(3);
        if self.custom_colors.is_empty() {
            self.custom_colors = Settings::default().custom_colors;
        }
        // The old single switch: on meant "equalizer on, blend as chosen",
        // off meant neither.
        if let Some(on) = self.sound_effects.take() {
            self.eq_enabled = on;
            if !on {
                self.sound_blend = "off".to_string();
            }
        }
        let db = |v: f32| {
            if v.is_finite() {
                v.clamp(-12.0, 12.0)
            } else {
                0.0
            }
        };
        self.eq_preamp = db(self.eq_preamp);
        self.eq_gains.resize(10, 0.0);
        for g in &mut self.eq_gains {
            *g = db(*g);
        }
        let known = self.eq_preset == "custom"
            || audio_engine::PRESETS.iter().any(|p| p.0 == self.eq_preset);
        if !known {
            self.eq_preset = "custom".to_string();
        }
        if !matches!(self.sound_blend.as_str(), "off" | "crossfade" | "automix") {
            self.sound_blend = "off".to_string();
        }
        self.crossfade_secs = if self.crossfade_secs.is_finite() {
            self.crossfade_secs.clamp(1.0, 12.0)
        } else {
            6.0
        };
        self.service_volume = if self.service_volume.is_finite() {
            self.service_volume.clamp(0.0, audio_engine::Volume::MAX)
        } else {
            1.0
        };
        crate::presets::sanitize(&mut self.scene_presets);
        self
    }

    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            // Notepad and PowerShell like to add a UTF-8 byte-order mark.
            Ok(text) => serde_json::from_str::<Settings>(text.trim_start_matches('\u{feff}'))
                .map(Settings::sanitized)
                .unwrap_or_else(|e| {
                    tracing::warn!("settings file unreadable ({e}); using defaults");
                    Settings::default()
                }),
            Err(_) => Settings::default(),
        }
    }

    /// Atomic save: write a temp file, then rename over the old one.
    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_files_with_glow_look_fields_still_load() {
        // Phase 4 settings had glowIntensity etc.; those now live in themes.
        let s: Settings =
            serde_json::from_str(r#"{"glowIntensity": 99, "lyricsOnDesktop": false}"#).unwrap();
        let s = s.sanitized();
        assert!(!s.lyrics_on_desktop);
        assert!(s.glow_enabled, "missing fields use defaults");
        assert!(s.use_theme);
        assert!(s.disabled_themes.is_empty());
        assert!(s.stage_upcoming);
        assert_eq!(s.active_theme, DEFAULT_THEME);
    }

    #[test]
    fn bad_theme_id_falls_back() {
        let s = Settings {
            active_theme: "../../evil".into(),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.active_theme, DEFAULT_THEME);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("glowtune-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings {
            glow_monitors: vec![r"\\.\DISPLAY1".into()],
            player_on_top: true,
            player_style: "glass".to_string(),
            player_size: "m".to_string(),
            taskbar_player: "off".to_string(),
            active_theme: "aurora".into(),
            use_theme: false,
            stage_upcoming: false,
            disabled_themes: vec!["my-glow".into()],
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// "#rrggbb" → [r, g, b].
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod sound_tests {
    use super::*;

    #[test]
    fn old_files_get_sound_defaults_and_bad_values_are_repaired() {
        let s: Settings = serde_json::from_str(r#"{"blockAds": true}"#).unwrap();
        let s = s.sanitized();
        assert!(!s.eq_enabled, "the equalizer is opt-in");
        assert_eq!(s.eq_gains, vec![0.0; 10]);
        assert_eq!(s.sound_blend, "off");

        let s: Settings = serde_json::from_str(
            r#"{"eqGains": [99, -99, 3], "eqPreamp": 40, "eqPreset": "nope",
                "soundBlend": "sideways", "crossfadeSecs": 500}"#,
        )
        .unwrap();
        let s = s.sanitized();
        assert_eq!(s.eq_gains.len(), 10);
        assert_eq!(&s.eq_gains[..3], &[12.0, -12.0, 3.0]);
        assert_eq!(s.eq_preamp, 12.0);
        assert_eq!(s.eq_preset, "custom");
        assert_eq!(s.sound_blend, "off");
        assert_eq!(s.crossfade_secs, 12.0);
    }

    #[test]
    fn the_old_single_switch_becomes_two_settings() {
        let on: Settings =
            serde_json::from_str(r#"{"soundEffects": true, "soundBlend": "automix"}"#).unwrap();
        let on = on.sanitized();
        assert!(on.eq_enabled);
        assert_eq!(on.sound_blend, "automix");
        assert!(!serde_json::to_string(&on).unwrap().contains("soundEffects"));

        let off: Settings =
            serde_json::from_str(r#"{"soundEffects": false, "soundBlend": "crossfade"}"#).unwrap();
        let off = off.sanitized();
        assert!(!off.eq_enabled);
        assert_eq!(off.sound_blend, "off", "it was not running before either");
    }

    #[test]
    fn equalizer_and_blend_are_independent() {
        let s: Settings =
            serde_json::from_str(r#"{"eqEnabled": false, "soundBlend": "crossfade"}"#).unwrap();
        let s = s.sanitized();
        assert!(!s.eq_enabled);
        assert_eq!(s.sound_blend, "crossfade");
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn hex_parsing_and_custom_color_repair() {
        assert_eq!(parse_hex("#ff3da6"), Some([255, 61, 166]));
        assert_eq!(parse_hex("ff3da6"), None);
        assert_eq!(parse_hex("#ff3d"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        let s = Settings {
            custom_colors: vec![
                "nope".into(),
                "#000000".into(),
                "#111111".into(),
                "#222222".into(),
                "#333333".into(),
            ],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.custom_colors, ["#000000", "#111111", "#222222"]);
        let empty = Settings {
            custom_colors: vec![],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(empty.custom_colors, Settings::default().custom_colors);
    }
}
