//! Themes (PLAN.md section 7): pick, preview, save, import/export, hot reload.
//!
//! ```text
//!  themes folder ──notify──► ThemeWatcher ──► reload active theme ──┐
//!  editor (preview/save) ───────────────────────────────────────────┼─► apply():
//!                                                                    │    • "theme" event → CSS variables in every window
//!                                                                    │    • overlay config (thickness, intensity, corners…)
//!                                                                    │    • fixed glow colors (static/gradient modes)
//!                                                                    └─   • lyrics window position
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use overlay::{MonitorSelection, OverlayConfig};
use themes::{ColorMode, Corners, Theme, ThemeInfo, ThemeStore, ThemeWatcher};

use crate::settings::Settings;

pub const DEFAULT_THEME: &str = "midnight-glass";

/// A theme list entry for the picker, with a few colors to draw as a swatch.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeCard {
    pub id: String,
    pub name: String,
    pub author: String,
    pub bundled: bool,
    pub swatch: Vec<String>,
}

pub struct ThemeService {
    store: ThemeStore,
    enabled: AtomicBool,
    user_dir: PathBuf,
    /// Active theme id and the theme currently shown (may be an unsaved preview).
    active: Mutex<(String, Theme)>,
    watcher: Mutex<Option<ThemeWatcher>>,
}

impl ThemeService {
    pub fn new(user_dir: PathBuf, active_id: &str) -> Result<Arc<Self>, String> {
        let store = ThemeStore::new(user_dir.clone()).map_err(|e| format!("themes folder: {e}"))?;
        let (id, theme) = match store.load(active_id) {
            Ok((t, warnings)) => {
                log_warnings(active_id, &warnings);
                (active_id.to_string(), t)
            }
            Err(e) => {
                tracing::warn!("theme {active_id} unavailable ({e}); using {DEFAULT_THEME}");
                let t = store
                    .load(DEFAULT_THEME)
                    .map(|(t, _)| t)
                    .unwrap_or_default();
                (DEFAULT_THEME.to_string(), t)
            }
        };
        Ok(Arc::new(Self {
            store,
            enabled: AtomicBool::new(true),
            user_dir,
            active: Mutex::new((id, theme)),
            watcher: Mutex::new(None),
        }))
    }

    /// Starts hot reload; `on_change(id)` runs on the watcher thread.
    pub fn watch(&self, dir: PathBuf, on_change: impl Fn(String) + Send + 'static) {
        match ThemeWatcher::watch(dir, on_change) {
            Ok(w) => {
                if let Ok(mut slot) = self.watcher.lock() {
                    *slot = Some(w);
                }
            }
            Err(e) => tracing::warn!("theme hot reload off: {e}"),
        }
    }

    pub fn current(&self) -> Theme {
        if !self.enabled.load(Ordering::Relaxed) {
            return Theme::default();
        }
        self.active.lock().map(|a| a.1.clone()).unwrap_or_default()
    }

    /// Retain the chosen theme and preview while showing built-in defaults.
    pub fn configure(&self, settings: &Settings) -> bool {
        let enabled = settings.use_theme && !settings.disabled_themes.contains(&self.active_id());
        self.enabled.swap(enabled, Ordering::Relaxed) != enabled
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn active_id(&self) -> String {
        self.active
            .lock()
            .map(|a| a.0.clone())
            .unwrap_or_else(|_| DEFAULT_THEME.into())
    }

    pub fn cards(&self) -> Vec<ThemeCard> {
        self.store
            .list()
            .into_iter()
            .map(|info: ThemeInfo| {
                let swatch = self
                    .store
                    .load(&info.id)
                    .map(|(t, _)| swatch(&t))
                    .unwrap_or_default();
                ThemeCard {
                    id: info.id,
                    name: info.name,
                    author: info.author,
                    bundled: info.bundled,
                    swatch,
                }
            })
            .collect()
    }

    pub fn select(&self, id: &str) -> Result<Theme, String> {
        let (theme, warnings) = self.store.load(id).map_err(|e| e.to_string())?;
        log_warnings(id, &warnings);
        if let Ok(mut a) = self.active.lock() {
            *a = (id.to_string(), theme.clone());
        }
        Ok(theme)
    }

    /// Shows an edited theme without saving. Returns it clamped, plus warnings.
    pub fn preview(&self, theme: Theme) -> (Theme, Vec<String>) {
        let (theme, warnings) = theme.sanitized();
        if let Ok(mut a) = self.active.lock() {
            a.1 = theme.clone();
        }
        (theme, warnings.iter().map(|w| w.to_string()).collect())
    }

    pub fn save(&self, theme: Theme) -> Result<String, String> {
        let (theme, _) = theme.sanitized();
        let id = self.store.save(&theme).map_err(|e| e.to_string())?;
        if let Ok(mut a) = self.active.lock() {
            *a = (id.clone(), theme);
        }
        Ok(id)
    }

    /// Re-reads the active theme after its file changed on disk.
    pub fn reload_if_active(&self, id: &str) -> Option<Theme> {
        if self.active_id() != id {
            return None;
        }
        self.select(id).ok()
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.store.delete(id).map_err(|e| e.to_string())
    }

    pub fn import(&self, path: &str) -> Result<String, String> {
        self.store.import(path).map_err(|e| e.to_string())
    }

    /// Reuse ThemeStore's sanitized save/export path while preserving the catalog id.
    /// Publishing with a hard link refuses to overwrite an existing user theme.
    pub fn install_pack(&self, id: &str, theme: &Theme) -> Result<(), String> {
        if !crate::store::valid_id(id) || themes::bundled().iter().any(|(key, _)| *key == id) {
            return Err("This theme id is reserved or invalid.".into());
        }
        // Each install owns its staging directory, including concurrent installs.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let staging = self.user_dir.join(format!(
            ".crate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let result = (|| {
            let store = ThemeStore::new(&staging).map_err(|e| e.to_string())?;
            let saved = store.save(theme).map_err(|e| e.to_string())?;
            let source = staging.join("pack.tmp");
            store.export(&saved, &source).map_err(|e| e.to_string())?;
            std::fs::hard_link(source, self.user_dir.join(format!("{id}.toml")))
                .map_err(|e| format!("Could not install theme (an existing theme is kept): {e}"))
        })();
        if let Err(e) = std::fs::remove_dir_all(&staging) {
            tracing::warn!(%e, "could not remove Crate staging directory");
        }
        result
    }

    pub fn export(&self, id: &str, path: &str) -> Result<(), String> {
        self.store.export(id, path).map_err(|e| e.to_string())
    }
}

/// Glow look from the theme + where/whether to glow from the user's settings.
pub fn overlay_config(theme: &Theme, settings: &Settings) -> OverlayConfig {
    let g = &theme.glow;
    OverlayConfig {
        enabled: settings.glow_enabled,
        hide_on_fullscreen: settings.glow_hide_fullscreen,
        thickness: g.thickness as f32,
        intensity: g.intensity as f32,
        beat_reactivity: g.beat_reactivity as f32,
        drift_speed: g.drift_speed as f32,
        effect_speed: g.effect_speed as f32,
        effect_strength: g.effect_strength as f32,
        reverse_motion: g.reverse_motion,
        particle_density: g.particle_density as f32,
        particle_size: g.particle_size as f32,
        particle_speed: g.particle_speed as f32,
        particle_sparkle: g.particle_sparkle as f32,
        particles: match g.particles {
            themes::ParticleStyle::Off => overlay::ParticleStyle::Off,
            themes::ParticleStyle::Dots => overlay::ParticleStyle::Dots,
            themes::ParticleStyle::Stars => overlay::ParticleStyle::Stars,
            themes::ParticleStyle::Dust => overlay::ParticleStyle::Dust,
        },
        effect: match g.effect {
            themes::GlowEffect::Ambient => overlay::GlowEffect::Ambient,
            themes::GlowEffect::Still => overlay::GlowEffect::Still,
            themes::GlowEffect::Pulse => overlay::GlowEffect::Pulse,
            themes::GlowEffect::Orbit => overlay::GlowEffect::Orbit,
            themes::GlowEffect::Distortion => overlay::GlowEffect::Distortion,
            themes::GlowEffect::LavaLamp => overlay::GlowEffect::LavaLamp,
        },
        corner_radius: match g.corners {
            Corners::Rounded => OverlayConfig::default().corner_radius,
            Corners::Square => 0.0,
        },
        monitors: if settings.glow_monitors.is_empty() {
            MonitorSelection::All
        } else {
            MonitorSelection::Only(settings.glow_monitors.clone())
        },
        ..OverlayConfig::default()
    }
}

/// Theme-fixed glow colors (static/gradient modes); `None` = follow the album.
pub fn fixed_palette(theme: &Theme) -> Option<Vec<[u8; 3]>> {
    let rgb = |c: &themes::Color| [c.r, c.g, c.b];
    match theme.colors.mode {
        ColorMode::Adaptive => None,
        ColorMode::Static => theme.colors.glow.first().map(|c| vec![rgb(c)]),
        ColorMode::Gradient => Some(theme.colors.glow.iter().map(rgb).collect()),
    }
}

fn swatch(t: &Theme) -> Vec<String> {
    match t.colors.mode {
        ColorMode::Adaptive => vec![
            t.colors.background.to_hex(),
            t.colors.accent.to_hex(),
            t.colors.text.to_hex(),
        ],
        _ => t.colors.glow.iter().map(|c| c.to_hex()).collect(),
    }
}

fn log_warnings(id: &str, warnings: &[themes::Warning]) {
    for w in warnings {
        tracing::warn!("theme {id}: {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_restore_defaults_without_losing_custom_theme_or_file() {
        let dir =
            std::env::temp_dir().join(format!("glowtune-theme-toggles-{}", std::process::id()));
        let service = ThemeService::new(dir.clone(), "aurora").unwrap();
        let mut custom = service.current();
        custom.meta.name = "Toggle test".into();
        let id = service.save(custom.clone()).unwrap();
        let path = dir.join(format!("{id}.toml"));
        let mut settings = Settings {
            active_theme: id.clone(),
            use_theme: false,
            ..Default::default()
        };
        assert!(service.configure(&settings));
        assert_eq!(service.current(), Theme::default());
        assert_eq!(service.active_id(), id);
        assert!(path.exists());
        service.reload_if_active(&id).unwrap();
        assert_eq!(service.current(), Theme::default());
        settings.use_theme = true;
        assert!(service.configure(&settings));
        assert_eq!(service.current(), custom);
        settings.disabled_themes.push(id.clone());
        assert!(service.configure(&settings));
        assert_eq!(service.current(), Theme::default());
        assert!(path.exists());
        let restarted = ThemeService::new(dir.clone(), &settings.active_theme).unwrap();
        restarted.configure(&settings);
        assert_eq!(restarted.current(), Theme::default());
        settings.disabled_themes.clear();
        assert!(restarted.configure(&settings));
        assert_eq!(restarted.current(), custom);
        assert!(!restarted.configure(&settings));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
