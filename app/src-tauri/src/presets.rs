//! Scene presets: your own saved looks for the desktop scene.
//!
//! ```text
//!  "Save current look" ─► snapshot: scene settings + theme + glow shape/particles
//!                                   + the scene page's layout (block positions…)
//!  pick a preset / Ctrl+Alt+1..9 ─► settings back ─► theme back ─► glow back
//!                                   └─► "scene-preset" event: every scene window
//!                                       puts its blocks back where they were
//! ```
//! Presets live in `settings.json` (so hotkeys can apply them from Rust).
//! Volume, equalizer and logins are never part of a preset.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::Settings;

/// Most presets you can keep: one per Ctrl+Alt+1..9 hotkey.
pub const MAX_PRESETS: usize = 9;
const MAX_NAME: usize = 40;
/// The page's layout is small JSON; anything bigger is not ours.
const MAX_LAYOUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ScenePreset {
    pub name: String,
    pub look: PresetLook,
    /// What the scene page stored for its layout (block positions and sizes,
    /// the lyrics view). Opaque here; the page reads it back.
    pub layout: serde_json::Value,
}

/// The parts of the settings (and theme) a preset brings back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PresetLook {
    pub stage_clock: bool,
    pub stage_lyrics: bool,
    pub stage_recent: bool,
    pub stage_upcoming: bool,
    pub stage_motion: bool,
    pub stage_emoji: bool,
    pub stage_beat: bool,
    pub stage_dock: bool,
    pub stage_lyrics_style: String,
    pub stage_clock_style: String,
    pub stage_wallpaper: String,
    pub stage_art_style: String,
    pub stage_dock_style: String,
    pub artwork_colors: bool,
    pub custom_colors: Vec<String>,
    pub glow_enabled: bool,
    pub desktop_lyrics_style: String,
    pub active_theme: String,
    /// The theme's glow shape and particles when saved (theme ids, e.g. "aurora").
    pub glow_effect: Option<String>,
    pub glow_particles: Option<String>,
}

impl Default for PresetLook {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PresetLook {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            stage_clock: s.stage_clock,
            stage_lyrics: s.stage_lyrics,
            stage_recent: s.stage_recent,
            stage_upcoming: s.stage_upcoming,
            stage_motion: s.stage_motion,
            stage_emoji: s.stage_emoji,
            stage_beat: s.stage_beat,
            stage_dock: s.stage_dock,
            stage_lyrics_style: s.stage_lyrics_style.clone(),
            stage_clock_style: s.stage_clock_style.clone(),
            stage_wallpaper: s.stage_wallpaper.clone(),
            stage_art_style: s.stage_art_style.clone(),
            stage_dock_style: s.stage_dock_style.clone(),
            artwork_colors: s.artwork_colors,
            custom_colors: s.custom_colors.clone(),
            glow_enabled: s.glow_enabled,
            desktop_lyrics_style: s.desktop_lyrics_style.clone(),
            active_theme: s.active_theme.clone(),
            glow_effect: None,
            glow_particles: None,
        }
    }

    /// Writes the look into `s`. Bad values are repaired by `Settings::sanitized`.
    pub fn apply_to(&self, s: &mut Settings) {
        s.stage_clock = self.stage_clock;
        s.stage_lyrics = self.stage_lyrics;
        s.stage_recent = self.stage_recent;
        s.stage_upcoming = self.stage_upcoming;
        s.stage_motion = self.stage_motion;
        s.stage_emoji = self.stage_emoji;
        s.stage_beat = self.stage_beat;
        s.stage_dock = self.stage_dock;
        s.stage_lyrics_style = self.stage_lyrics_style.clone();
        s.stage_clock_style = self.stage_clock_style.clone();
        s.stage_wallpaper = self.stage_wallpaper.clone();
        s.stage_art_style = self.stage_art_style.clone();
        s.stage_dock_style = self.stage_dock_style.clone();
        s.artwork_colors = self.artwork_colors;
        s.custom_colors = self.custom_colors.clone();
        s.glow_enabled = self.glow_enabled;
        s.desktop_lyrics_style = self.desktop_lyrics_style.clone();
    }
}

/// Trimmed, length-capped name; None when nothing is left.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(MAX_NAME).collect();
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Repairs a hand-edited list: no blank or duplicate names, sane layouts, at most 9.
pub fn sanitize(list: &mut Vec<ScenePreset>) {
    let mut seen: Vec<String> = Vec::new();
    list.retain_mut(|p| {
        let Some(name) = clean_name(&p.name) else {
            return false;
        };
        let key = name.to_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        p.name = name;
        let too_big = serde_json::to_vec(&p.layout).map_or(true, |v| v.len() > MAX_LAYOUT_BYTES);
        if too_big || !(p.layout.is_object() || p.layout.is_null()) {
            p.layout = serde_json::Value::Null;
        }
        true
    });
    list.truncate(MAX_PRESETS);
}

/// Adds the preset, or replaces the one with the same name (any case).
pub fn upsert(list: &mut Vec<ScenePreset>, preset: ScenePreset) -> Result<(), String> {
    if let Some(p) = list
        .iter_mut()
        .find(|p| p.name.to_lowercase() == preset.name.to_lowercase())
    {
        *p = preset;
        return Ok(());
    }
    if list.len() >= MAX_PRESETS {
        return Err(format!(
            "You can keep up to {MAX_PRESETS} presets. Delete one first."
        ));
    }
    list.push(preset);
    Ok(())
}

fn theme_glow(app: &AppHandle) -> (Option<String>, Option<String>) {
    let theme = app.state::<std::sync::Arc<crate::ThemeService>>().current();
    let text = |v: serde_json::Result<serde_json::Value>| {
        v.ok().and_then(|v| v.as_str().map(str::to_string))
    };
    (
        text(serde_json::to_value(theme.glow.effect)),
        text(serde_json::to_value(theme.glow.particles)),
    )
}

#[tauri::command]
pub async fn save_scene_preset(
    name: String,
    layout: serde_json::Value,
    app: AppHandle,
) -> Result<Settings, String> {
    let name = clean_name(&name).ok_or("Give the preset a name.")?;
    let mut s = crate::current_settings(&app);
    let mut look = PresetLook::from_settings(&s);
    (look.glow_effect, look.glow_particles) = theme_glow(&app);
    let preset = ScenePreset { name, look, layout };
    upsert(&mut s.scene_presets, preset)?;
    crate::save_settings(s, &app)
}

#[tauri::command]
pub async fn delete_scene_preset(name: String, app: AppHandle) -> Result<Settings, String> {
    let mut s = crate::current_settings(&app);
    s.scene_presets
        .retain(|p| p.name.to_lowercase() != name.trim().to_lowercase());
    crate::save_settings(s, &app)
}

#[tauri::command]
pub async fn apply_scene_preset(name: String, app: AppHandle) -> Result<(), String> {
    let s = crate::current_settings(&app);
    let index = s
        .scene_presets
        .iter()
        .position(|p| p.name.to_lowercase() == name.trim().to_lowercase())
        .ok_or("That preset is gone.")?;
    apply(&app, index)
}

/// Brings preset `index` (0-based) back. Used by the menu and the hotkeys.
pub fn apply(app: &AppHandle, index: usize) -> Result<(), String> {
    let mut s = crate::current_settings(app);
    let preset = s
        .scene_presets
        .get(index)
        .cloned()
        .ok_or("No preset there yet.")?;
    preset.look.apply_to(&mut s);

    // Theme first (only when it differs and is still installed and enabled).
    let themes = app.state::<std::sync::Arc<crate::ThemeService>>();
    let want = &preset.look.active_theme;
    if !want.is_empty() && *want != themes.active_id() && !s.disabled_themes.contains(want) {
        match themes.select(want) {
            Ok(_) => s.active_theme = want.clone(),
            Err(e) => tracing::warn!("preset {}: theme {want} unavailable: {e}", preset.name),
        }
    }
    crate::save_settings(s, app)?;

    // Then the glow, saved into the theme only when it actually changes.
    if themes.enabled() {
        let mut theme = themes.current();
        let mut changed = false;
        if let Some(effect) = preset
            .look
            .glow_effect
            .as_deref()
            .and_then(|e| serde_json::from_value(serde_json::Value::String(e.into())).ok())
            && theme.glow.effect != effect
        {
            theme.glow.effect = effect;
            changed = true;
        }
        if let Some(particles) = preset
            .look
            .glow_particles
            .as_deref()
            .and_then(|p| serde_json::from_value(serde_json::Value::String(p.into())).ok())
            && theme.glow.particles != particles
        {
            theme.glow.particles = particles;
            changed = true;
        }
        if changed {
            match themes.save(theme) {
                Ok(id) => {
                    crate::apply_theme(app, &themes.current());
                    crate::update_settings(app, |s| s.active_theme = id);
                }
                Err(e) => tracing::warn!("preset {}: glow not saved: {e}", preset.name),
            }
        } else {
            crate::apply_theme(app, &themes.current());
        }
    }

    let _ = app.emit("scene-preset", &preset);
    tracing::info!("scene preset applied: {}", preset.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> ScenePreset {
        ScenePreset {
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn saving_the_same_name_replaces_and_the_list_stops_at_nine() {
        let mut list = Vec::new();
        upsert(&mut list, named("Night")).unwrap();
        let mut again = named("night");
        again.look.stage_wallpaper = "water".into();
        upsert(&mut list, again).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].look.stage_wallpaper, "water");
        for i in 1..MAX_PRESETS {
            upsert(&mut list, named(&format!("P{i}"))).unwrap();
        }
        assert!(upsert(&mut list, named("One too many")).is_err());
        assert_eq!(list.len(), MAX_PRESETS);
    }

    #[test]
    fn hand_edited_lists_are_repaired() {
        let mut big = named("Big");
        big.layout = serde_json::json!({ "x": "a".repeat(MAX_LAYOUT_BYTES) });
        let mut list = vec![named("  "), named(" Gaming "), named("GAMING"), big];
        sanitize(&mut list);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Gaming");
        assert!(list[1].layout.is_null(), "oversized layouts are dropped");
        assert_eq!(clean_name(&"x".repeat(90)).unwrap().len(), MAX_NAME);
    }

    #[test]
    fn a_look_round_trips_through_the_settings() {
        let s = Settings {
            stage_lyrics_style: "stars".into(),
            stage_wallpaper: "water".into(),
            stage_art_style: "vinyl".into(),
            stage_dock_style: "line".into(),
            stage_clock: false,
            artwork_colors: false,
            ..Default::default()
        };
        let look = PresetLook::from_settings(&s);
        let mut other = Settings::default();
        look.apply_to(&mut other);
        assert_eq!(PresetLook::from_settings(&other), look);
        assert_eq!(
            other.eq_preset,
            Settings::default().eq_preset,
            "sound untouched"
        );
    }

    #[test]
    fn old_settings_files_have_no_presets() {
        let s: Settings = serde_json::from_str(r#"{"stageEnabled": true}"#).unwrap();
        assert!(s.sanitized().scene_presets.is_empty());
        let s: Settings = serde_json::from_str(
            r#"{"scenePresets": [{"name": "Night", "look": {"stageWallpaper": "frost"}}]}"#,
        )
        .unwrap();
        let s = s.sanitized();
        assert_eq!(s.scene_presets[0].look.stage_wallpaper, "frost");
        assert_eq!(
            s.scene_presets[0].look.stage_lyrics_style, "bold",
            "missing = default"
        );
    }
}
