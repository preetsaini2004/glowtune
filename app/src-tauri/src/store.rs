//! The Crate: validated data packs, with a bundled collection and optional remote catalog.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{Settings, parse_hex};
use crate::theme_service::{DEFAULT_THEME, ThemeService};
use crate::{AppSettings, save_settings, select_theme};

const BUNDLED: &str = include_str!("../store/catalog.json");
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Theme,
    Preset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub author: String,
    pub description: String,
    pub colors: Vec<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toml: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u32,
    packs: Vec<Value>,
}

#[derive(Serialize)]
pub struct PackView {
    #[serde(flatten)]
    pack: Pack,
    installed: bool,
    active: bool,
    /// Parsed values let sleeves render the actual theme without a TOML parser in JS.
    theme: Option<themes::Theme>,
}

#[derive(Default)]
pub struct Store {
    // Catalog contents remain stable across install/use commands and event refreshes.
    loaded: tokio::sync::Mutex<Option<(String, Vec<Pack>)>>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

impl Pack {
    fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id)
            || themes::bundled().iter().any(|(id, _)| *id == self.id)
            || !bounded(&self.name, 80)
            || !bounded(&self.author, 80)
            || !bounded(&self.description, 600)
            || !(2..=4).contains(&self.colors.len())
            || self.colors.iter().any(|c| parse_hex(c).is_none())
            || self.tags.len() > 8
            || self.tags.iter().any(|t| !bounded(t, 32))
        {
            return Err("invalid metadata, colors or reserved id".into());
        }
        match self.kind {
            Kind::Theme => {
                if self.settings.is_some() {
                    return Err("theme contains preset settings".into());
                }
                self.theme()?;
            }
            Kind::Preset => {
                if self.toml.is_some() {
                    return Err("preset contains theme TOML".into());
                }
                let patch = self.settings.as_ref().ok_or("preset needs settings")?;
                if patch.is_empty() {
                    return Err("preset is empty".into());
                }
                for (key, value) in patch {
                    let valid = match key.as_str() {
                        "stageLyricsStyle" => value.as_str().is_some_and(|v| {
                            matches!(v, "flow" | "bold" | "karaoke" | "outline" | "sticker")
                        }),
                        "stageClockStyle" => value
                            .as_str()
                            .is_some_and(|v| matches!(v, "minimal" | "solid" | "outline")),
                        "stageWallpaper" => {
                            value.as_str().is_some_and(|v| matches!(v, "art" | "video"))
                        }
                        "stageMotion" | "stageClock" | "stageRecent" | "artworkColors" => {
                            value.is_boolean()
                        }
                        "customColors" => value.as_array().is_some_and(|colors| {
                            (1..=3).contains(&colors.len())
                                && colors
                                    .iter()
                                    .all(|c| c.as_str().is_some_and(|c| parse_hex(c).is_some()))
                        }),
                        _ => false,
                    };
                    if !valid {
                        return Err(format!("invalid preset field: {key}"));
                    }
                }
            }
        }
        Ok(())
    }

    fn theme(&self) -> Result<themes::Theme, String> {
        let text = self.toml.as_ref().ok_or("theme needs TOML")?;
        if text.len() > 32 * 1024 {
            return Err("theme too large".into());
        }
        let (theme, warnings) = themes::Theme::from_toml_str(text).map_err(|e| e.to_string())?;
        // User themes are lenient; published packs must need no repairs.
        if !warnings.is_empty() {
            return Err(format!("theme needs repair: {}", warnings[0]));
        }
        if ![
            "Instrument Sans",
            "Bricolage Grotesque",
            "Segoe UI",
            "Georgia",
        ]
        .contains(&theme.font.family.as_str())
            || ![
                "Instrument Sans",
                "Bricolage Grotesque",
                "Segoe UI",
                "Georgia",
            ]
            .contains(&theme.font.lyrics_family.as_str())
        {
            return Err("unsupported font".into());
        }
        Ok(theme)
    }

    fn apply_to(&self, settings: &Settings) -> Result<Settings, String> {
        let mut value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
        let patch = self.settings.as_ref().ok_or("not a scene look")?;
        value
            .as_object_mut()
            .ok_or("invalid settings")?
            .extend(patch.clone());
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    fn active_preset(&self, current: &Value) -> bool {
        self.settings.as_ref().is_some_and(|patch| {
            patch
                .iter()
                .all(|(key, value)| current.get(key) == Some(value))
        })
    }
}

fn parse_catalog(text: &str) -> Result<Vec<Pack>, String> {
    if text.len() > MAX_BYTES {
        return Err("catalog too large".into());
    }
    let catalog: Catalog = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if catalog.version != 1 || catalog.packs.len() > 128 {
        return Err("unsupported catalog version or size".into());
    }
    let mut seen = HashSet::new();
    let mut packs = Vec::new();
    for value in catalog.packs {
        let result = serde_json::from_value::<Pack>(value).and_then(|pack| {
            pack.validate().map_err(serde::de::Error::custom)?;
            Ok(pack)
        });
        match result {
            Ok(pack) if seen.insert(pack.id.clone()) => packs.push(pack),
            Ok(pack) => tracing::warn!(id = pack.id, "skipping duplicate Crate pack"),
            Err(error) => tracing::warn!(%error, "skipping invalid Crate pack"),
        }
    }
    Ok(packs)
}

fn merge(mut bundled: Vec<Pack>, remote: Vec<Pack>) -> Vec<Pack> {
    for pack in remote {
        if let Some(existing) = bundled.iter_mut().find(|p| p.id == pack.id) {
            *existing = pack;
        } else {
            bundled.push(pack);
        }
    }
    bundled
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Glowtune/0.1 (desktop music companion)")
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default()
    })
}

async fn fetch(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "https" | "http") || url.len() > 2048 {
        return Err("catalog needs an HTTP URL".into());
    }
    let mut response = client()
        .get(parsed)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err("catalog too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
struct Cached {
    url: String,
    catalog: String,
}

async fn load(url: &str, cache: &Path) -> Vec<Pack> {
    let bundled = parse_catalog(BUNDLED).unwrap_or_default();
    if url.is_empty() {
        return bundled;
    }
    let cached = std::fs::read(cache)
        .ok()
        .filter(|bytes| bytes.len() <= MAX_BYTES * 2)
        .and_then(|bytes| serde_json::from_slice::<Cached>(&bytes).ok())
        .filter(|saved| saved.url == url)
        .and_then(|saved| parse_catalog(&saved.catalog).ok())
        .unwrap_or_default();
    match fetch(url)
        .await
        .and_then(|text| parse_catalog(&text).map(|packs| (text, packs)))
    {
        Ok((catalog, remote)) => {
            let saved = Cached {
                url: url.into(),
                catalog,
            };
            let write = (|| -> Result<(), Box<dyn std::error::Error>> {
                if let Some(parent) = cache.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let tmp = cache.with_extension("tmp");
                std::fs::write(&tmp, serde_json::to_vec(&saved)?)?;
                std::fs::rename(tmp, cache)?;
                Ok(())
            })();
            if let Err(error) = write {
                tracing::warn!(%error, "could not cache Crate catalog");
            }
            merge(bundled, remote)
        }
        Err(error) => {
            tracing::warn!(%error, "Crate using bundled and cached packs");
            merge(bundled, cached)
        }
    }
}

async fn catalog(app: &AppHandle) -> Vec<Pack> {
    let url = app
        .state::<AppSettings>()
        .current
        .lock()
        .map(|s| s.store_url.trim().to_owned())
        .unwrap_or_default();
    let state = app.state::<Store>();
    let mut loaded = state.loaded.lock().await;
    if let Some((source, packs)) = loaded.as_ref()
        && source == &url
    {
        return packs.clone();
    }
    let packs = match app.path().app_cache_dir() {
        Ok(dir) => load(&url, &dir.join("crate-catalog.json")).await,
        Err(error) => {
            tracing::warn!(%error, "Crate cache unavailable");
            parse_catalog(BUNDLED).unwrap_or_default()
        }
    };
    *loaded = Some((url, packs.clone()));
    packs
}

async fn find(app: &AppHandle, id: &str) -> Result<Pack, String> {
    catalog(app)
        .await
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("Pack unavailable.".into())
}

#[tauri::command]
pub async fn store_catalog(app: AppHandle) -> Vec<PackView> {
    let packs = catalog(&app).await;
    let service = app.state::<std::sync::Arc<ThemeService>>();
    let cards = service.cards();
    let active = service.active_id();
    let settings = app
        .state::<AppSettings>()
        .current
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let current = serde_json::to_value(settings).unwrap_or_default();
    packs
        .into_iter()
        .map(|pack| {
            let is_theme = pack.kind == Kind::Theme;
            PackView {
                installed: is_theme && cards.iter().any(|c| c.id == pack.id && !c.bundled),
                active: if is_theme {
                    service.enabled() && active == pack.id
                } else {
                    pack.active_preset(&current)
                },
                theme: if is_theme { pack.theme().ok() } else { None },
                pack,
            }
        })
        .collect()
}

#[tauri::command]
pub async fn store_install(id: String, app: AppHandle) -> Result<(), String> {
    let pack = find(&app, &id).await?;
    match pack.kind {
        Kind::Theme => {
            app.state::<std::sync::Arc<ThemeService>>()
                .install_pack(&pack.id, &pack.theme()?)?;
            let _ = app.emit("themes-changed", &pack.id);
        }
        Kind::Preset => {
            let current = app
                .state::<AppSettings>()
                .current
                .lock()
                .map_err(|e| e.to_string())?
                .clone();
            save_settings(pack.apply_to(&current)?, &app)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn store_use(id: String, app: AppHandle) -> Result<(), String> {
    if find(&app, &id).await?.kind != Kind::Theme {
        return Err("Use Apply for a scene look.".into());
    }
    select_theme(id, app)?;
    Ok(())
}

#[tauri::command]
pub async fn store_remove(id: String, app: AppHandle) -> Result<(), String> {
    if find(&app, &id).await?.kind != Kind::Theme
        || themes::bundled().iter().any(|(key, _)| *key == id)
    {
        return Err("Only user themes can be removed.".into());
    }
    let service = app.state::<std::sync::Arc<ThemeService>>();
    if service.active_id() == id {
        select_theme(DEFAULT_THEME.into(), app.clone())?;
    }
    service.delete(&id)?;
    let _ = app.emit("themes-changed", &id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled() -> Vec<Pack> {
        parse_catalog(BUNDLED).unwrap()
    }
    fn document(pack: Value) -> String {
        serde_json::json!({"version": 1, "packs": [pack]}).to_string()
    }

    #[test]
    fn every_bundled_pack_validates() {
        let raw: Catalog = serde_json::from_str(BUNDLED).unwrap();
        assert_eq!(raw.packs.len(), 14);
        for value in raw.packs {
            serde_json::from_value::<Pack>(value)
                .unwrap()
                .validate()
                .unwrap();
        }
        let packs = bundled();
        assert_eq!(packs.len(), 14);
        assert_eq!(packs.iter().filter(|p| p.kind == Kind::Theme).count(), 8);
    }

    #[test]
    fn rejects_bad_metadata_theme_and_preset() {
        let original = serde_json::to_value(&bundled()[0]).unwrap();
        for (key, value) in [
            ("colors", serde_json::json!(["#123456", "#gg0000"])),
            ("toml", Value::String("[broken".into())),
            ("toml", Value::String("[glow]\nintensity = 99".into())),
            ("id", Value::String("../escape".into())),
            ("id", Value::String("midnight-glass".into())),
            ("description", Value::String("x".repeat(601))),
        ] {
            let mut value_pack = original.clone();
            value_pack[key] = value;
            assert!(
                parse_catalog(&document(value_pack)).unwrap().is_empty(),
                "{key}"
            );
        }
        let preset = bundled()
            .into_iter()
            .find(|p| p.kind == Kind::Preset)
            .unwrap();
        for patch in [
            serde_json::json!({"blockAds": false}),
            serde_json::json!({"stageLyricsStyle": "unknown"}),
            serde_json::json!({"stageMotion": "yes"}),
            serde_json::json!({"customColors": ["#bad"]}),
            serde_json::json!({}),
        ] {
            let mut value = serde_json::to_value(&preset).unwrap();
            value["settings"] = patch;
            assert!(parse_catalog(&document(value)).unwrap().is_empty());
        }
        assert!(parse_catalog("{}").is_err());
        assert!(parse_catalog(r#"{"version":2,"packs":[]}"#).is_err());
        assert!(parse_catalog(&" ".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn duplicates_keep_first_and_remote_wins_merge() {
        let pack = bundled().remove(0);
        let text = serde_json::json!({"version": 1, "packs": [pack, pack]}).to_string();
        assert_eq!(parse_catalog(&text).unwrap().len(), 1);
        let mut remote = pack.clone();
        remote.name = "Remote edition".into();
        let merged = merge(bundled(), vec![remote]);
        assert_eq!(merged.len(), 14);
        assert_eq!(merged[0].name, "Remote edition");
    }

    #[tokio::test]
    async fn remote_is_cached_and_bad_json_or_network_uses_last_good_copy() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/catalog", listener.local_addr().unwrap());
        let mut remote = bundled().remove(0);
        remote.name = "Cached edition".into();
        let good = document(serde_json::to_value(remote).unwrap());
        let server = std::thread::spawn(move || {
            for body in [good, "not JSON".into()] {
                let (mut connection, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                assert!(connection.read(&mut request).unwrap() > 0);
                write!(
                    connection,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let dir = std::env::temp_dir().join(format!("glowtune-crate-cache-{}", std::process::id()));
        let cache = dir.join("catalog.json");
        assert_eq!(load(&url, &cache).await[0].name, "Cached edition");
        let saved = std::fs::read(&cache).unwrap();
        assert_eq!(load(&url, &cache).await[0].name, "Cached edition");
        assert_eq!(std::fs::read(&cache).unwrap(), saved);
        server.join().unwrap();
        assert_eq!(load(&url, &cache).await[0].name, "Cached edition");
        assert_eq!(load("", &cache).await[0].name, bundled()[0].name);
        std::fs::write(&cache, "corrupt cache").unwrap();
        assert_eq!(load(&url, &cache).await.len(), 14);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preset_merges_only_its_fields_and_reports_active() {
        let preset = bundled()
            .into_iter()
            .find(|p| p.kind == Kind::Preset)
            .unwrap();
        let original = Settings {
            player_on_top: true,
            store_url: "https://example.test".into(),
            ..Default::default()
        };
        let applied = preset.apply_to(&original).unwrap();
        assert!(applied.player_on_top);
        assert_eq!(applied.store_url, original.store_url);
        assert_eq!(applied.stage_enabled, original.stage_enabled);
        assert!(preset.active_preset(&serde_json::to_value(&applied).unwrap()));
        let mut changed = serde_json::to_value(applied).unwrap();
        let key = preset.settings.as_ref().unwrap().keys().next().unwrap();
        changed[key] = Value::Null;
        assert!(!preset.active_preset(&changed));
    }

    #[test]
    fn theme_install_keeps_catalog_id_metadata_and_existing_files() {
        let dir = std::env::temp_dir().join(format!("glowtune-crate-{}", std::process::id()));
        let service = ThemeService::new(dir.clone(), DEFAULT_THEME).unwrap();
        let theme = bundled().remove(0).theme().unwrap();
        {
            service.install_pack("different-id", &theme).unwrap();
            assert_eq!(service.active_id(), DEFAULT_THEME);
            assert_eq!(
                service.select("different-id").unwrap().meta.name,
                theme.meta.name
            );
            assert!(
                service
                    .install_pack("different-id", &themes::Theme::default())
                    .is_err()
            );
            assert!(service.install_pack("../bad", &theme).is_err());
            assert!(service.install_pack(DEFAULT_THEME, &theme).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
