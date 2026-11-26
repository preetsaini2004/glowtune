//! Glowtune Tauri app entry point.
//!
//! ```text
//!  tray icon ──menu──► show player / lyrics on-off / settings / quit
//!  hub thread ──events──► windows: "player" (glass mini player), "lyrics" (desktop lyrics)
//!  windows ──commands──► get_state, control, get_settings, set_settings, list_monitors
//! ```
//! Closing the player destroys its webview (frees ~100+ MB); the app keeps
//! running in the tray, and the glow keeps following the music.

mod adblock;
mod card;
mod feedback;
mod foreground;
mod glow;
mod hires;
mod hotkeys;
mod hub;
mod lyrics_gate;
#[cfg(windows)]
mod lyrics_listen;
mod lyrics_service;
mod music_browser;
mod osd;
mod presets;
mod search;
mod services;
mod settings;
mod sleep;
mod sound;
mod stage;
mod stats;
mod store;
mod taskbar;
mod theme_service;
#[cfg(windows)]
mod thumbbar;
mod update;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use media_source::Control;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::Effect;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

use hub::{Shared, Snapshot};
use lyrics_service::LyricsService;
use settings::Settings;
use theme_service::{ThemeCard, ThemeService};

/// Where the webviews load album art from (custom `glowart` scheme).
#[cfg(windows)]
pub const ART_ORIGIN: &str = "http://glowart.localhost";
#[cfg(not(windows))]
pub const ART_ORIGIN: &str = "glowart://localhost";

const PLAYER: &str = "player";
pub(crate) const LYRICS: &str = "lyrics";
const SETTINGS: &str = "settings";
/// The mini player's size for each look (logical px).
fn player_size(style: &str, size: &str) -> (f64, f64) {
    // Small / large scale the whole look (the page zooms to match).
    let k = match size {
        "s" => 0.84,
        "l" => 1.22,
        _ => 1.0,
    };
    let (w, h) = match style {
        "disc" => (240.0, 240.0),
        "card" => (300.0, 410.0),
        "pill" => (380.0, 68.0),
        "lyric" => (600.0, 104.0),
        "clear" => (460.0, 124.0),
        _ => (480.0, 132.0),
    };
    (w * k, h * k)
}

/// Round, capsule and clear looks draw their own shape: no glass rectangle
/// or square shadow behind them.
fn player_shaped(style: &str) -> bool {
    matches!(style, "disc" | "pill" | "clear")
}

/// Gives the open player its look's size and glass, keeping its bottom-right
/// corner where it was (it lives near the taskbar corner).
fn shape_player(w: &tauri::WebviewWindow, style: &str, size: &str) -> tauri::Result<()> {
    let scale = w.scale_factor()?;
    let (lw, lh) = player_size(style, size);
    let want = tauri::PhysicalSize::new((lw * scale).round() as u32, (lh * scale).round() as u32);
    let have = w.inner_size()?;
    if have != want {
        let pos = w.outer_position()?;
        let outer = w.outer_size()?;
        let right = pos.x + outer.width as i32;
        let bottom = pos.y + outer.height as i32;
        w.set_size(want)?;
        let outer = w.outer_size()?;
        w.set_position(tauri::PhysicalPosition::new(
            right - outer.width as i32,
            bottom - outer.height as i32,
        ))?;
    }
    if player_shaped(style) {
        w.set_effects(None)?;
        w.set_shadow(false)?;
    } else {
        w.set_effects(WindowEffectsConfig {
            effects: vec![Effect::Acrylic],
            ..Default::default()
        })?;
        w.set_shadow(true)?;
    }
    Ok(())
}
const LYRICS_SIZE: (f64, f64) = (1200.0, 260.0);
/// Smallest and largest desktop lyrics box (logical px).
const LYRICS_MIN: (f64, f64) = (420.0, 120.0);
const LYRICS_MAX: (f64, f64) = (3840.0, 1400.0);

/// The desktop lyrics box size: yours (Move lyrics), else the default.
fn lyrics_box(app: &AppHandle) -> (f64, f64) {
    let saved = app
        .state::<AppSettings>()
        .current
        .lock()
        .ok()
        .and_then(|s| s.lyrics_box);
    match saved {
        Some([w, h]) if w.is_finite() && h.is_finite() => (
            w.clamp(LYRICS_MIN.0, LYRICS_MAX.0),
            h.clamp(LYRICS_MIN.1, LYRICS_MAX.1),
        ),
        _ => LYRICS_SIZE,
    }
}
const SETTINGS_SIZE: (f64, f64) = (780.0, 640.0);

/// Settings plus where they live on disk.
pub(crate) struct AppSettings {
    path: PathBuf,
    pub(crate) current: Mutex<Settings>,
    /// Tray checkbox, kept in sync when lyrics are toggled from settings.
    lyrics_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    stage_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorView {
    name: String,
    primary: bool,
    width: u32,
    height: u32,
}

/// Where to report bugs, and the releases page (Settings → About). Only these, never a page's URL.
#[tauri::command]
fn open_support(to: String) -> Result<(), String> {
    let url = match to.as_str() {
        "issues" => "https://github.com/preetsaini2004/glowtune/issues/new",
        "email" => "mailto:Preet.saini.0@outlook.com?subject=Glowtune%20bug",
        "releases" => "https://github.com/preetsaini2004/glowtune/releases/latest",
        "rate" => "ms-windows-store://review/?ProductId=9NGDWTHP2QGW",
        _ => return Err("unknown link".into()),
    };
    std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// A copy of the settings as they are now.
fn current_settings(app: &AppHandle) -> Settings {
    app.state::<AppSettings>()
        .current
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn get_settings(settings: State<'_, AppSettings>) -> Settings {
    settings
        .current
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// Play/pause, next or previous from outside the webviews (taskbar buttons):
/// the service page's own buttons when music plays there, else media controls.
pub(crate) fn transport(app: AppHandle, action: &'static str) {
    if action != "playPause"
        && let Some(sound) = app.try_state::<sound::Sound>()
    {
        sound.note_command();
    }
    tauri::async_runtime::spawn(async move {
        let shared = app.state::<Arc<Shared>>().inner().clone();
        let web = shared
            .snapshot
            .lock()
            .ok()
            .and_then(|s| s.track.as_ref().map(|t| t.source == "Web player"))
            .unwrap_or(false);
        if web && services::web_transport(&app, action).await {
            return;
        }
        shared.media.control(match action {
            "next" => Control::Next,
            "previous" => Control::Previous,
            _ => Control::PlayPause,
        });
    });
}

/// Async on purpose: applying settings can create windows (desktop scene, lyrics),
/// and creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
async fn set_settings(new: Settings, app: AppHandle) -> Result<Settings, String> {
    let current = current_settings(&app);
    save_settings(keep_owned(new, &current), &app)
}

/// The windows send back their whole copy of the settings, which can be a
/// moment out of date. The theme and the saved presets have their own
/// commands, so keep those as they are now: a stale copy put back the
/// previous theme (seen live: Needle dust after a restart instead of the
/// theme just picked) or dropped a preset just saved.
fn keep_owned(mut new: Settings, current: &Settings) -> Settings {
    new.active_theme.clone_from(&current.active_theme);
    new.scene_presets.clone_from(&current.scene_presets);
    new.skipped_update.clone_from(&current.skipped_update);
    new.hints_seen.clone_from(&current.hints_seen);
    new.last_seen_version.clone_from(&current.last_seen_version);
    new
}

fn save_settings(new: Settings, app: &AppHandle) -> Result<Settings, String> {
    let new = new.sanitized();
    apply_settings(app, &new).map_err(|e| e.to_string())?;
    let state = app.state::<AppSettings>();
    new.save(&state.path)
        .map_err(|e| format!("could not save settings: {e}"))?;
    let lyrics_changed = match state.current.lock() {
        Ok(mut cur) => {
            let changed = cur.lyrics_differ(&new);
            *cur = new.clone();
            changed
        }
        Err(_) => false,
    };
    // Lyrics choices ride on the theme: send it again now that they're stored.
    if lyrics_changed {
        apply_theme(app, &app.state::<Arc<ThemeService>>().current());
    }
    let _ = app.emit("settings", &new);
    Ok(new)
}

#[tauri::command]
fn list_monitors(shared: State<'_, Arc<Shared>>) -> Vec<MonitorView> {
    let monitors = shared
        .overlay
        .as_ref()
        .map(|o| o.monitors())
        .unwrap_or_default();
    monitors
        .into_iter()
        .map(|m| MonitorView {
            name: m.name,
            primary: m.primary,
            width: m.size.0,
            height: m.size.1,
        })
        .collect()
}

// ── Themes ─────────────────────────────────────────────────────────

#[tauri::command]
fn list_themes(themes: State<'_, Arc<ThemeService>>) -> Vec<ThemeCard> {
    themes.cards()
}

#[tauri::command]
fn active_theme_id(themes: State<'_, Arc<ThemeService>>) -> String {
    themes.active_id()
}

#[tauri::command]
fn get_theme(themes: State<'_, Arc<ThemeService>>) -> themes::Theme {
    themes.current()
}

#[tauri::command]
fn select_theme(id: String, app: AppHandle) -> Result<themes::Theme, String> {
    let settings = app
        .state::<AppSettings>()
        .current
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    if settings.disabled_themes.contains(&id) {
        return Err("Enable this theme in Settings → Themes first.".into());
    }
    // Picking a theme leaves "Use the theme's colors" as it is: switched off,
    // the new theme's look shows with the album's colors.
    let service = app.state::<Arc<ThemeService>>();
    service.select(&id)?;
    update_settings(&app, |s| s.active_theme = id);
    let theme = service.current();
    apply_theme(&app, &theme);
    Ok(theme)
}

/// Live preview while editing; nothing is saved. Returns warnings to show.
#[tauri::command]
fn preview_theme(theme: themes::Theme, app: AppHandle) -> Vec<String> {
    let service = app.state::<Arc<ThemeService>>();
    if !service.look_shown() {
        return vec!["Enable the selected theme before editing.".into()];
    }
    let (theme, warnings) = service.preview(theme);
    apply_theme(&app, &theme);
    warnings
}

#[tauri::command]
fn save_theme(theme: themes::Theme, app: AppHandle) -> Result<String, String> {
    let service = app.state::<Arc<ThemeService>>();
    if !service.look_shown() {
        return Err("Enable the selected theme before saving.".into());
    }
    let id = service.save(theme)?;
    apply_theme(&app, &service.current());
    let saved = id.clone();
    update_settings(&app, |s| s.active_theme = saved);
    Ok(id)
}

#[tauri::command]
fn delete_theme(id: String, themes: State<'_, Arc<ThemeService>>) -> Result<(), String> {
    themes.delete(&id)
}

#[tauri::command]
fn import_theme(path: String, themes: State<'_, Arc<ThemeService>>) -> Result<String, String> {
    themes.import(&path)
}

#[tauri::command]
fn export_theme(
    id: String,
    path: String,
    themes: State<'_, Arc<ThemeService>>,
) -> Result<(), String> {
    themes.export(&id, &path)
}

/// The theme as the windows show it: the active theme with your lyrics
/// choices (Settings → Lyrics) on top. The theme editor uses `get_theme`
/// instead, so saving a theme never bakes those choices into it.
#[tauri::command]
fn get_shown_theme(app: AppHandle) -> themes::Theme {
    shown_theme(&app, app.state::<Arc<ThemeService>>().current())
}

fn shown_theme(app: &AppHandle, mut theme: themes::Theme) -> themes::Theme {
    current_settings(app).lyrics_over(&mut theme);
    theme
}

/// Shows a theme everywhere: CSS in all windows, the glow, lyrics placement.
fn apply_theme(app: &AppHandle, theme: &themes::Theme) {
    let settings = current_settings(app);
    let mut shown = theme.clone();
    settings.lyrics_over(&mut shown);
    let _ = app.emit("theme", &shown);
    let shared = app.state::<Arc<Shared>>();
    if let Ok(mut fixed) = shared.fixed_palette.lock() {
        *fixed = theme_service::fixed_palette(theme);
    }
    shared.refresh_glow_palette();
    if let Some(overlay) = shared.overlay.as_ref() {
        overlay.set_config(theme_service::overlay_config(theme, &settings));
    }
    if let Some(w) = app.get_webview_window(LYRICS)
        && let Err(e) = place_lyrics(app, &w)
    {
        tracing::warn!("lyrics window: {e}");
    }
}

/// How many ad and tracker requests were blocked this session.
#[tauri::command]
fn adblock_stats(adblock: State<'_, adblock::AdBlock>) -> u64 {
    adblock.blocked_count()
}

/// Whether sound effects are processing audio right now, and the current delay.
#[tauri::command]
fn sound_status(sound: State<'_, sound::Sound>) -> sound::SoundStatus {
    sound.status()
}

/// How many ad-block rules are loaded; 0 means the filter lists are missing.
#[tauri::command]
fn adblock_rules(adblock: State<'_, adblock::AdBlock>) -> usize {
    adblock.rule_count()
}

/// Opens Spotify, YouTube Music or SoundCloud in the service window.
#[tauri::command]
async fn open_service(service: services::Service, app: AppHandle) -> Result<(), String> {
    services::show(&app, service).map_err(|e| e.to_string())
}

/// Songs, artists and albums matching `query`, with artwork (search box).
#[tauri::command]
async fn search_catalog(query: String) -> Result<search::SearchResults, String> {
    search::search(&query).await
}

/// The service window's own player controls (shuffle, repeat, like, volume):
/// web players don't report these to Windows media controls.
#[tauri::command]
async fn web_player(
    action: String,
    value: Option<f64>,
    app: AppHandle,
) -> Result<Option<services::WebPlayerState>, String> {
    Ok(web_player_at(&app, &action, value).await)
}

/// The service page's player (state, volume...), as the `web_player` command.
pub(crate) async fn web_player_at(
    app: &AppHandle,
    action: &str,
    value: Option<f64>,
) -> Option<services::WebPlayerState> {
    // With Sound effects on, the page's volume would only be heard after the
    // delay (up to 34 s): move Glowtune's own volume instead, heard at once.
    let sound = app.try_state::<sound::Sound>();
    let Some((sound, gain)) = sound.and_then(|s| s.gain().map(|g| (s, g))) else {
        return services::web_player(app, action, value).await;
    };
    if action != "volume" {
        let mut state = services::web_player(app, action, value).await;
        if let Some(s) = &mut state {
            s.volume = s.volume.map(|page| (page * gain as f64).clamp(0.0, 1.0));
        }
        return state;
    }
    let mut state = services::web_player(app, "state", None).await?;
    let wanted = value
        .filter(|v| v.is_finite())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    match state.volume {
        // A page turned (almost) all the way down can't be raised by a gain.
        Some(page) if page >= 0.02 => {
            sound.set_gain((wanted / page) as f32);
            state.volume = Some(wanted);
        }
        _ => return services::web_player(app, "volume", Some(wanted)).await,
    }
    Some(state)
}

/// "Move lyrics": `on` makes the desktop lyrics draggable (shown with a sample
/// line); `on: false` saves where you left them and makes them click-through again.
#[tauri::command]
async fn lyrics_move(on: bool, app: AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    set_lyrics_visible(&app, true).map_err(|e| e.to_string())?;
    let Some(w) = app.get_webview_window(LYRICS) else {
        return Ok(());
    };
    lyrics_gate::MOVING.store(on, Ordering::Relaxed);
    w.set_ignore_cursor_events(!on).map_err(|e| e.to_string())?;
    // Resizable only while you place it (the corner grips).
    w.set_resizable(on).map_err(|e| e.to_string())?;
    if on {
        w.show().map_err(|e| e.to_string())?;
    } else {
        let p = w.outer_position().map_err(|e| e.to_string())?;
        let k = w.scale_factor().map_err(|e| e.to_string())?;
        let size = w
            .inner_size()
            .map_err(|e| e.to_string())?
            .to_logical::<f64>(k);
        let changed =
            (size.width - LYRICS_SIZE.0).abs() > 2.0 || (size.height - LYRICS_SIZE.1).abs() > 2.0;
        update_settings(&app, |s| {
            s.lyrics_position = Some([p.x, p.y]);
            s.lyrics_box = changed.then_some([size.width, size.height]);
        });
    }
    let _ = app.emit_to(LYRICS, "lyrics-move", on);
    if let Some(g) = app.try_state::<lyrics_gate::LyricsGate>() {
        g.changed();
    }
    Ok(())
}

/// Puts the desktop lyrics back at the theme's position.
#[tauri::command]
async fn lyrics_reset_position(app: AppHandle) -> Result<(), String> {
    update_settings(&app, |s| {
        s.lyrics_position = None;
        s.lyrics_box = None;
    });
    if let Some(w) = app.get_webview_window(LYRICS) {
        let (width, height) = LYRICS_SIZE;
        w.set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
        place_lyrics(&app, &w).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Volume of the current player (0..1), if Windows exposes it for that app.
#[tauri::command]
async fn get_volume(shared: State<'_, Arc<Shared>>) -> Result<Option<f32>, String> {
    Ok(shared.media.volume())
}

/// YouTube id of the song's music video (scene video wallpaper), if one fits.
#[tauri::command]
async fn music_video(
    title: String,
    artist: String,
    duration_ms: Option<f64>,
) -> Result<Option<String>, String> {
    search::music_video_cached(&title, &artist, duration_ms).await
}

/// A big copy of the current song's cover (glowart URL), if one is found.
#[tauri::command]
async fn hires_art(
    title: String,
    artist: String,
    app: AppHandle,
) -> Result<Option<String>, String> {
    Ok(hires::hires_art(&app, &title, &artist).await)
}

/// Opens `service` on its own search page for `query`.
#[tauri::command]
async fn open_search(
    service: services::Service,
    query: String,
    app: AppHandle,
) -> Result<(), String> {
    let url = service.search_url(&query);
    services::show_at(&app, service, Some(url)).map_err(|e| e.to_string())
}

/// Plays `query` in `service` (top match), pausing whatever plays elsewhere first.
#[tauri::command]
async fn play_on(
    service: services::Service,
    query: String,
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
) -> Result<(), String> {
    let playing = shared
        .snapshot
        .lock()
        .map(|s| s.playback.status == "playing")
        .unwrap_or(false);
    if playing {
        shared.media.control(media_source::Control::Pause);
    }
    services::play(&app, service, &query).map_err(|e| e.to_string())
}

/// Desktop scene: wallpaper mode (behind windows) or full screen (over everything).
#[tauri::command]
async fn stage_fullscreen(
    on: bool,
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    stage::set_fullscreen(&app, window.label(), on).map_err(|e| e.to_string())
}

/// Snapshot after the scene installs listeners, so initial cover events cannot be missed.
#[tauri::command]
async fn get_stage_state(window: tauri::WebviewWindow) -> Result<(bool, bool), String> {
    Ok((
        stage::is_covered(&window),
        window.is_fullscreen().map_err(|e| e.to_string())?,
    ))
}

/// Recently played this session, newest first.
#[tauri::command]
fn get_history(shared: State<'_, Arc<Shared>>) -> Vec<hub::HistoryItem> {
    shared
        .history
        .lock()
        .map(|h| h.iter().cloned().collect())
        .unwrap_or_default()
}

/// Brings up the mini player (the taskbar player's song button).
#[tauri::command]
async fn show_player_window(app: AppHandle) -> Result<(), String> {
    show_player(&app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), String> {
    show_settings(&app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_settings_tab(tab: String, app: AppHandle) -> Result<(), String> {
    if !matches!(
        tab.as_str(),
        "lyrics"
            | "scene"
            | "glow"
            | "sound"
            | "services"
            | "themes"
            | "store"
            | "player"
            | "listening"
            | "keys"
            | "about"
    ) {
        return Err("Unknown settings section.".into());
    }
    show_settings_tab(&app, &tab).map_err(|e| e.to_string())
}

/// Pushes settings to the glow and the windows.
fn apply_settings(app: &AppHandle, s: &Settings) -> tauri::Result<()> {
    let themes = app.state::<Arc<ThemeService>>();
    if themes.configure(s) {
        apply_theme(app, &themes.current());
    }
    {
        let shared = app.state::<Arc<Shared>>();
        if let Ok(mut u) = shared.user_palette.lock() {
            *u = (!s.artwork_colors).then(|| {
                s.custom_colors
                    .iter()
                    .filter_map(|c| settings::parse_hex(c))
                    .collect()
            });
        }
        shared.refresh_glow_palette();
    }
    if let Some(overlay) = app.state::<Arc<Shared>>().overlay.as_ref() {
        overlay.set_config(theme_service::overlay_config(
            &app.state::<Arc<ThemeService>>().current(),
            s,
        ));
    }
    set_lyrics_visible(app, s.lyrics_on_desktop)?;
    if let Ok(item) = app.state::<AppSettings>().lyrics_item.lock()
        && let Some(item) = item.as_ref()
    {
        item.set_checked(s.lyrics_on_desktop)?;
    }
    if let Some(w) = app.get_webview_window(PLAYER) {
        w.set_always_on_top(s.player_on_top)?;
        shape_player(&w, &s.player_style, &s.player_size)?;
    }
    taskbar::apply(app, &s.taskbar_player)?;
    hotkeys::set_enabled(app, s.hotkeys);
    if s.stage_enabled {
        stage::show(app, &s.stage_monitor)?;
    } else {
        stage::hide(app)?;
    }
    if let Ok(item) = app.state::<AppSettings>().stage_item.lock()
        && let Some(item) = item.as_ref()
    {
        item.set_checked(s.stage_enabled)?;
    }
    if let Some(g) = app.try_state::<lyrics_gate::LyricsGate>() {
        g.changed();
    }
    if let Some(sound) = app.try_state::<sound::Sound>() {
        sound.configure(s);
    }
    Ok(())
}

/// Updates one setting from the tray, saves, and applies.
fn update_settings(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let state = app.state::<AppSettings>();
    let mut s = state.current.lock().map(|s| s.clone()).unwrap_or_default();
    f(&mut s);
    if let Err(e) = save_settings(s, app) {
        tracing::error!("settings: {e}");
    }
}

fn show_settings(app: &AppHandle) -> tauri::Result<()> {
    show_settings_tab(app, "general")
}

/// Command line: `glowtune --open spotify|apple-music|youtube-music|soundcloud`, `glowtune --settings [theme]`.
/// Used by desktop shortcuts and scripted tests. Returns true if something was opened.
fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        let result = match arg.as_str() {
            "--open" => match it.next().and_then(|id| services::Service::from_id(id)) {
                Some(service) => services::show(app, service),
                None => continue,
            },
            "--settings" => match it.next().map(String::as_str) {
                Some("theme") => show_settings_tab(app, "theme"),
                _ => show_settings(app),
            },
            _ => continue,
        };
        if let Err(e) = result {
            tracing::error!("{arg}: {e}");
        }
        return true;
    }
    false
}

fn show_settings_tab(app: &AppHandle, tab: &str) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        w.emit("settings-tab", tab)?;
        w.unminimize()?;
        w.show()?;
        return w.set_focus();
    }
    let (w, h) = SETTINGS_SIZE;
    WebviewWindowBuilder::new(
        app,
        SETTINGS,
        WebviewUrl::App(format!("/settings?tab={tab}").into()),
    )
    .title("Glowtune settings")
    .inner_size(w, h)
    .resizable(false)
    .maximizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .center()
    .effects(WindowEffectsConfig {
        effects: vec![Effect::Acrylic],
        ..Default::default()
    })
    .build()?;
    Ok(())
}

/// Lyrics for the current track.
#[tauri::command]
fn get_lyrics(lyrics: State<'_, Arc<LyricsService>>) -> lyrics_service::LyricsView {
    lyrics.view()
}

/// Shift lyrics timing for this song (positive = earlier). Returns the new offset.
#[tauri::command]
fn nudge_lyrics(
    delta_ms: i64,
    app: AppHandle,
    lyrics: State<'_, Arc<LyricsService>>,
) -> Result<i64, String> {
    lyrics.nudge(&app, delta_ms)
}

/// Sets this song's lyrics timing exactly (positive = earlier): the number
/// box and tap to sync. Returns the timing kept (clamped to ±10 s).
#[tauri::command]
fn set_lyrics_offset(
    offset_ms: i64,
    app: AppHandle,
    lyrics: State<'_, Arc<LyricsService>>,
) -> Result<i64, String> {
    lyrics.set_offset(&app, offset_ms)
}

/// "Sync": listen for where the singing starts and line the lyrics up. The
/// answer comes as a "lyrics-sync" event; an error says why it can't (the
/// page then offers tap to sync).
#[tauri::command]
fn sync_lyrics(sound: State<'_, sound::Sound>) -> Result<(), String> {
    sound.resync()
}

/// "Try other lyrics": another version from LRCLIB. Returns what happened.
#[tauri::command]
async fn other_lyrics(app: AppHandle) -> Result<String, String> {
    let lyrics = app.state::<Arc<LyricsService>>().inner().clone();
    lyrics.other_version(app).await
}

#[tauri::command]
fn get_state(app: AppHandle, shared: State<'_, Arc<Shared>>) -> Snapshot {
    let mut snapshot = shared
        .snapshot
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    snapshot.playback = hub::heard_playback(&app, snapshot.playback);
    snapshot
}

/// The equalizer presets, for the Sound settings and the scene's quick menu.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EqPreset {
    id: &'static str,
    name: &'static str,
    preamp: f32,
    gains: [f32; 10],
}

#[tauri::command]
fn eq_presets() -> Vec<EqPreset> {
    audio_engine::PRESETS
        .iter()
        .map(|&(id, name, preamp, gains)| EqPreset {
            id,
            name,
            preamp,
            gains,
        })
        .collect()
}

/// `action`: "playPause" | "next" | "previous" | "seek" (`positionMs`) |
/// "shuffle" (`on`) | "repeat" (`mode`: off, track, list) | "volume" (`volume` 0..1).
#[tauri::command]
async fn control(
    action: String,
    position_ms: Option<f64>,
    on: Option<bool>,
    mode: Option<String>,
    volume: Option<f32>,
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
) -> Result<(), String> {
    // Music in one of Glowtune's service windows: press the page's own buttons
    // (Windows media controls don't reliably reach a web player).
    let web = shared
        .snapshot
        .lock()
        .ok()
        .and_then(|s| s.track.as_ref().map(|t| t.source == "Web player"))
        .unwrap_or(false);
    // A song change right after this is the user's doing, not the song ending.
    if matches!(action.as_str(), "next" | "previous" | "seek")
        && let Some(sound) = app.try_state::<sound::Sound>()
    {
        sound.note_command();
    }
    if web
        && matches!(action.as_str(), "playPause" | "next" | "previous")
        && services::web_transport(&app, &action).await
    {
        return Ok(());
    }
    let control = match action.as_str() {
        "shuffle" => Control::SetShuffle(on.ok_or("shuffle needs on")?),
        "repeat" => Control::SetRepeat(match mode.as_deref() {
            Some("off") => media_source::RepeatMode::Off,
            Some("track") => media_source::RepeatMode::Track,
            Some("list") => media_source::RepeatMode::List,
            _ => return Err("repeat needs mode off|track|list".into()),
        }),
        "volume" => Control::SetVolume(
            volume
                .filter(|v| v.is_finite())
                .ok_or("volume needs 0..1")?,
        ),
        "playPause" => Control::PlayPause,
        "next" => Control::Next,
        "previous" => Control::Previous,
        "seek" => {
            let ms = position_ms
                .filter(|m| m.is_finite() && *m >= 0.0)
                .ok_or("seek needs positionMs")?;
            Control::SeekTo(Duration::from_secs_f64(ms / 1000.0))
        }
        other => return Err(format!("unknown action {other}")),
    };
    shared.media.control(control);
    Ok(())
}

/// Ctrl+Alt+O: opens Settings, or closes it when it's already in front.
/// Returns true when Settings is now open.
fn toggle_settings(app: &AppHandle) -> bool {
    if let Some(w) = app.get_webview_window(SETTINGS)
        && w.is_visible().unwrap_or(false)
        && !w.is_minimized().unwrap_or(false)
        && w.is_focused().unwrap_or(false)
    {
        let _ = w.close();
        return false;
    }
    if let Err(e) = show_settings(app) {
        tracing::warn!("settings: {e}");
    }
    true
}

/// Shows the mini player, or closes it when it's already in front. True = shown.
fn toggle_player(app: &AppHandle) -> bool {
    if let Some(w) = app.get_webview_window(PLAYER)
        && w.is_visible().unwrap_or(false)
        && !w.is_minimized().unwrap_or(false)
    {
        let _ = w.close();
        return false;
    }
    if let Err(e) = show_player(app) {
        tracing::warn!("mini player: {e}");
    }
    true
}

fn show_player(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(PLAYER) {
        w.unminimize()?;
        w.show()?;
        return w.set_focus();
    }
    let (style, size) = app
        .state::<AppSettings>()
        .current
        .lock()
        .map(|s| (s.player_style.clone(), s.player_size.clone()))
        .unwrap_or_default();
    let (w, h) = player_size(&style, &size);
    let builder = WebviewWindowBuilder::new(app, PLAYER, WebviewUrl::App("/".into()))
        .title("Glowtune")
        .inner_size(w, h)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(
            app.state::<AppSettings>()
                .current
                .lock()
                .map(|s| s.player_on_top)
                .unwrap_or(false),
        )
        .effects(WindowEffectsConfig {
            effects: vec![Effect::Acrylic],
            ..Default::default()
        });
    let window = builder.build()?;
    if player_shaped(&style) {
        window.set_effects(None)?;
        window.set_shadow(false)?;
    }
    // Bottom-right of the primary screen, just above the taskbar. Set after
    // creation in physical pixels (the builder's logical position was ignored
    // with a second monitor at negative coordinates).
    if let Ok(Some(m)) = app.primary_monitor() {
        let area = m.work_area();
        let size = window.outer_size()?;
        let margin = (20.0 * m.scale_factor()) as i32;
        let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
        let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;
        window.set_position(tauri::PhysicalPosition::new(x, y))?;
    }
    Ok(())
}

/// Puts the lyrics window where the theme wants it (physical pixels, primary screen).
fn place_lyrics(app: &AppHandle, window: &tauri::WebviewWindow) -> tauri::Result<()> {
    // Your own spot (dragged with "Move lyrics"), if it's still on a screen.
    let saved = app
        .state::<AppSettings>()
        .current
        .lock()
        .ok()
        .and_then(|s| s.lyrics_position);
    if let Some([x, y]) = saved
        && app.available_monitors()?.iter().any(|m| {
            let (p, s) = (m.position(), m.size());
            x >= p.x - 200
                && y >= p.y - 50
                && x < p.x + s.width as i32 - 200
                && y < p.y + s.height as i32 - 50
        })
    {
        return window.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let Ok(Some(m)) = app.primary_monitor() else {
        return Ok(());
    };
    let area = m.work_area();
    let size = window.outer_size()?;
    let margin = (24.0 * m.scale_factor()) as i32;
    let free = area.size.height as i32 - size.height as i32;
    let position = shown_theme(app, app.state::<Arc<ThemeService>>().current())
        .lyrics
        .position;
    let y = area.position.y
        + match position {
            themes::LyricsPosition::TopCenter => margin,
            themes::LyricsPosition::Center => free / 2,
            themes::LyricsPosition::BottomCenter => free - margin,
        };
    let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
    window.set_position(tauri::PhysicalPosition::new(x, y))
}

fn set_lyrics_visible(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    if !visible {
        if let Some(w) = app.get_webview_window(LYRICS) {
            w.destroy()?;
        }
        return Ok(());
    }
    if app.get_webview_window(LYRICS).is_some() {
        return Ok(());
    }
    let (w, h) = lyrics_box(app);
    let builder = WebviewWindowBuilder::new(app, LYRICS, WebviewUrl::App("/lyrics".into()))
        .title("Glowtune lyrics")
        .inner_size(w, h)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .min_inner_size(LYRICS_MIN.0, LYRICS_MIN.1)
        .max_inner_size(LYRICS_MAX.0, LYRICS_MAX.1)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        // Starts hidden; the lyrics gate shows it only when there's something to show.
        .visible(false);
    let window = builder.build()?;
    place_lyrics(app, &window)?;
    // Lyrics float over everything but never catch the mouse.
    window.set_ignore_cursor_events(true)?;
    if let Some(gate) = app.try_state::<lyrics_gate::LyricsGate>() {
        gate.changed();
    }
    Ok(())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let lyrics_on = app
        .state::<AppSettings>()
        .current
        .lock()
        .map(|s| s.lyrics_on_desktop)
        .unwrap_or(true);
    let show = MenuItem::with_id(app, "show", "Show player", true, None::<&str>)?;
    let lyrics = CheckMenuItem::with_id(
        app,
        "lyrics",
        "Lyrics on desktop",
        true,
        lyrics_on,
        None::<&str>,
    )?;
    let stage_on = app
        .state::<AppSettings>()
        .current
        .lock()
        .map(|s| s.stage_enabled)
        .unwrap_or(false);
    let scene =
        CheckMenuItem::with_id(app, "stage", "Desktop scene", true, stage_on, None::<&str>)?;
    let move_lyrics = MenuItem::with_id(
        app,
        "move-lyrics",
        "Move desktop lyrics",
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Glowtune", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let sp = MenuItem::with_id(app, "service:spotify", "Spotify", true, None::<&str>)?;
    let am = MenuItem::with_id(
        app,
        "service:apple-music",
        "Apple Music",
        true,
        None::<&str>,
    )?;
    let yt = MenuItem::with_id(
        app,
        "service:youtube-music",
        "YouTube Music",
        true,
        None::<&str>,
    )?;
    let sc = MenuItem::with_id(app, "service:soundcloud", "SoundCloud", true, None::<&str>)?;
    let services =
        tauri::menu::Submenu::with_items(app, "Open service", true, &[&sp, &am, &yt, &sc])?;
    let sleep_items = [
        ("sleep:off", "Off"),
        ("sleep:15", "15 minutes"),
        ("sleep:30", "30 minutes"),
        ("sleep:45", "45 minutes"),
        ("sleep:60", "1 hour"),
        ("sleep:0", "After this song"),
    ]
    .into_iter()
    .map(|(id, name)| MenuItem::with_id(app, id, name, true, None::<&str>))
    .collect::<tauri::Result<Vec<_>>>()?;
    let sleep_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = sleep_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let sleep = tauri::menu::Submenu::with_items(app, "Sleep timer", true, &sleep_refs)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &services,
            &scene,
            &lyrics,
            &move_lyrics,
            &sleep,
            &settings,
            &separator,
            &quit,
        ],
    )?;
    if let Ok(mut slot) = app.state::<AppSettings>().lyrics_item.lock() {
        *slot = Some(lyrics.clone());
    }
    if let Ok(mut slot) = app.state::<AppSettings>().stage_item.lock() {
        *slot = Some(scene.clone());
    }
    let scene_item = scene.clone();

    let lyrics_item = lyrics.clone();
    let mut tray = TrayIconBuilder::with_id("glowtune")
        .tooltip("Glowtune")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let result = match event.id().as_ref() {
                "show" => show_player(app),
                "lyrics" => {
                    let on = lyrics_item.is_checked().unwrap_or(true);
                    update_settings(app, |s| s.lyrics_on_desktop = on);
                    Ok(())
                }
                "stage" => {
                    let on = scene_item.is_checked().unwrap_or(false);
                    update_settings(app, |s| s.stage_enabled = on);
                    Ok(())
                }
                "move-lyrics" => {
                    tauri::async_runtime::spawn(lyrics_move(true, app.clone()));
                    Ok(())
                }
                "settings" => show_settings(app),
                id if id.starts_with("sleep:") => {
                    let minutes = id["sleep:".len()..].parse::<u32>().ok();
                    let view = sleep::set(app, minutes);
                    osd::show(app, osd::Toast::new("sleep", sleep::describe(&view)));
                    Ok(())
                }
                id if id.starts_with("service:") => {
                    match services::Service::from_id(&id["service:".len()..]) {
                        Some(service) => services::show(app, service),
                        None => Ok(()),
                    }
                }
                "quit" => {
                    app.exit(0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::error!("tray action failed: {e}");
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_player(tray.app_handle());
            }
            // Double-click: Settings (the single click before it shows the player).
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
                && let Err(e) = show_settings(tray.app_handle())
            {
                tracing::warn!("settings: {e}");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Serves the current album art at `<ART_ORIGIN>/<key>` to the webviews.
fn art_response(
    app: &AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let shared = app.state::<Arc<Shared>>();
    // URL: http://glowart.localhost/<key in hex>
    let key = u64::from_str_radix(request.uri().path().trim_start_matches('/'), 16).ok();
    let art = key.and_then(|k| {
        shared
            .art
            .lock()
            .ok()
            .and_then(|a| a.get(k))
            .map(|(b, m)| (b, m, k))
    });
    match art {
        Some((bytes, mime, key)) if !bytes.is_empty() => tauri::http::Response::builder()
            .header("Content-Type", mime)
            .header("ETag", format!("\"{key:x}\""))
            .header("Cache-Control", "max-age=31536000, immutable")
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(bytes.to_vec()))
            .unwrap_or_default(),
        _ => tauri::http::Response::builder()
            .status(404)
            .body(Cow::Borrowed(&[][..]))
            .unwrap_or_default(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "glowtune=info,glowtune_lib=info,audio_engine=info,overlay=info,media_source=warn".into()
            }),
        )
        .init();

    let app = tauri::Builder::default()
        // Launching Glowtune again just brings the player forward.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second launch: run its command line here (or just show the player).
            if !handle_args(app, &args) {
                let _ = show_player(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("glowart", |ctx, request| {
            art_response(ctx.app_handle(), &request)
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_lyrics,
            get_shown_theme,
            nudge_lyrics,
            hotkeys::lyrics_timing,
            set_lyrics_offset,
            lyrics_gate::lyrics_hint,
            sync_lyrics,
            other_lyrics,
            control,
            get_settings,
            set_settings,
            list_monitors,
            open_settings,
            open_settings_tab,
            store::store_catalog,
            store::store_install,
            store::store_reinstall,
            store::store_use,
            store::store_remove,
            open_service,
            open_search,
            play_on,
            stage_fullscreen,
            get_stage_state,
            search_catalog,
            music_browser::get_music_library,
            music_browser::refresh_music_library,
            music_browser::get_music_queue,
            music_browser::open_music_item,
            music_browser::open_music_library,
            music_video,
            hires_art,
            presets::save_scene_preset,
            presets::delete_scene_preset,
            presets::apply_scene_preset,
            glow::want_kicks,
            open_support,
            feedback::feedback_info,
            update::update_status,
            update::check_update,
            update::install_update,
            update::skip_update,
            update::dismiss_update,
            update::update_details,
            update::whats_new,
            update::close_whats_new,
            hotkeys::hotkey_list,
            sleep::set_sleep_timer,
            sleep::get_sleep_timer,
            osd::last_toast,
            osd::toast,
            card::save_lyric_card,
            stats::listening_stats,
            stats::clear_listening,
            taskbar::taskbar_visible,
            show_player_window,
            get_volume,
            lyrics_move,
            lyrics_reset_position,
            web_player,
            get_history,
            adblock_stats,
            adblock_rules,
            sound_status,
            eq_presets,
            list_themes,
            active_theme_id,
            get_theme,
            select_theme,
            preview_theme,
            save_theme,
            delete_theme,
            import_theme,
            export_theme
        ])
        .setup(|app| {
            app.manage(store::Store::default());
            app.manage(taskbar::TaskbarState::default());
            app.manage(sleep::SleepTimer::default());
            app.manage(stats::Listening::new(
                app.path().app_data_dir()?.join("listening.jsonl"),
            ));
            app.manage(osd::LastToast::default());
            app.manage(update::Updates::new(app.package_info().version.to_string()));
            let path = app.path().app_config_dir()?.join("settings.json");
            let had_settings = path.exists();
            let settings = Settings::load(&path);
            app.manage(sound::Sound::new(app.handle().clone(), &settings));
            app.manage(lyrics_gate::LyricsGate::start(app.handle().clone()));
            #[cfg(windows)]
            lyrics_listen::start(app.handle().clone());
            app.manage(stage::StageGate::start(app.handle().clone()));
            app.manage(adblock::AdBlock::start(
                app.path().app_cache_dir()?.join("filters"),
            ));
            let themes_dir = app.path().app_config_dir()?.join("themes");
            let theme_service = ThemeService::new(themes_dir.clone(), &settings.active_theme)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            theme_service.configure(&settings);
            let theme = theme_service.current();
            // Hot reload: edits to theme files show up immediately.
            let watch_app = app.handle().clone();
            theme_service.watch(themes_dir, move |id| {
                let service = watch_app.state::<Arc<ThemeService>>();
                if service.reload_if_active(&id).is_some() {
                    tracing::info!("theme {id} changed on disk; reloaded");
                    apply_theme(&watch_app, &service.current());
                }
                let _ = watch_app.emit("themes-changed", id);
            });
            app.manage(theme_service);
            let lyrics = Arc::new(LyricsService::new(
                app.path().app_cache_dir()?.join("lyrics"),
            ));
            app.manage(lyrics.clone());
            let shared = hub::start(
                app.handle().clone(),
                theme_service::overlay_config(&theme, &settings),
                theme_service::fixed_palette(&theme),
                lyrics,
            )
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(shared);
            let lyrics_on = settings.lyrics_on_desktop;
            let stage_on = (settings.stage_enabled, settings.stage_monitor.clone());
            app.manage(AppSettings {
                path,
                current: Mutex::new(settings),
                lyrics_item: Mutex::new(None),
                stage_item: Mutex::new(None),
            });
            build_tray(app.handle())?;
            hotkeys::start(app.handle(), current_settings(app.handle()).hotkeys);
            update::start(app.handle());
            update::whats_new_on_start(app.handle(), had_settings);
            show_player(app.handle())?;
            let taskbar_side = app
                .state::<AppSettings>()
                .current
                .lock()
                .map(|s| s.taskbar_player.clone())
                .unwrap_or_default();
            taskbar::apply(app.handle(), &taskbar_side)?;
            set_lyrics_visible(app.handle(), lyrics_on)?;
            if stage_on.0 {
                stage::show(app.handle(), &stage_on.1)?;
            }
            let args: Vec<String> = std::env::args().collect();
            handle_args(app.handle(), &args);
            // Dev aid for screenshots: GLOWTUNE_SHOW=settings opens settings at start.
            if cfg!(debug_assertions) {
                match std::env::var("GLOWTUNE_SHOW").as_deref() {
                    Ok("settings") => show_settings(app.handle())?,
                    Ok("themes") => show_settings_tab(app.handle(), "theme")?,
                    _ => {}
                }
            }
            #[cfg(debug_assertions)]
            if std::env::var("GLOWTUNE_SHOW").as_deref() == Ok("lyrics-demo") {
                let (handle, lyrics) = (
                    app.handle().clone(),
                    app.state::<Arc<LyricsService>>().inner().clone(),
                );
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    lyrics.show_demo(&handle);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Glowtune");

    app.run(|app, event| {
        // The song playing when Glowtune quits still counts.
        if let RunEvent::Exit = event
            && let Some(l) = app.try_state::<stats::Listening>()
        {
            l.flush();
        }
        // Closing the last window keeps Glowtune running in the tray.
        // Only "Quit" (an explicit exit code) really exits.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_window_cannot_put_back_an_old_theme_or_drop_presets() {
        let current = Settings {
            active_theme: "my-theme".into(),
            scene_presets: vec![Default::default()],
            ..Default::default()
        };
        let stale = Settings {
            active_theme: "needle-dust".into(),
            stage_wallpaper: "dots".into(),
            ..Default::default()
        };
        let kept = keep_owned(stale, &current);
        assert_eq!(kept.active_theme, "my-theme");
        assert_eq!(kept.scene_presets.len(), 1);
        assert_eq!(kept.stage_wallpaper, "dots");
    }

    #[test]
    fn a_stale_window_cannot_undo_skip_this_version() {
        let current = Settings {
            skipped_update: "0.4.0".into(),
            ..Default::default()
        };
        let kept = keep_owned(Settings::default(), &current);
        assert_eq!(kept.skipped_update, "0.4.0");
    }

    #[test]
    fn a_stale_window_cannot_bring_back_the_whats_new_card() {
        let current = Settings {
            last_seen_version: "0.5.0".into(),
            ..Default::default()
        };
        let stale = Settings {
            last_seen_version: "0.4.0".into(),
            ..Default::default()
        };
        assert_eq!(keep_owned(stale, &current).last_seen_version, "0.5.0");
    }
}
