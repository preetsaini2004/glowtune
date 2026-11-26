//! In-app updates: Glowtune checks GitHub for a new version, offers it in a
//! small pop-up and in Settings → About, and installs it with one click.
//!
//! ```text
//!  GitHub release ── latest.json (version, notes, signed installer URL)
//!        │  checked 45 s after start, then every 6 h (a sleeping thread)
//!        ▼
//!  Updates ──"update-status"──► Settings → About, the /update pop-up
//!        │  "Update now": download, check the signature, clean up, run
//!        ▼  the installer (passive), which restarts Glowtune
//! ```
//!
//! Only the GitHub install updates itself. The Microsoft Store copy is an
//! MSIX package: the Store updates it, Store policy forbids updating it any
//! other way, and running the GitHub installer from it would install a
//! second Glowtune. So a packaged Glowtune never checks.
//!
//! Nothing installs on its own: an update found in the background only
//! offers itself, and never over a full-screen game. The installer must be
//! signed with Glowtune's update key (`plugins.updater.pubkey` in
//! tauri.conf.json), or it is refused.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The pop-up's window label (and route).
pub const POPUP: &str = "update";
const POPUP_SIZE: (f64, f64) = (400.0, 430.0);
/// Gap between the pop-up and the screen's corner (logical px).
const MARGIN: f64 = 18.0;
const FIRST_CHECK: Duration = Duration::from_secs(45);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// While a game is full screen, look again this often before offering.
const COVERED_RETRY: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(30);
const SIGNATURE: &str = "The download isn't signed by Glowtune, so it wasn't installed.";

/// Where this copy of Glowtune came from, which decides how it updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    /// The GitHub installer: updates itself.
    Github,
    /// The Microsoft Store package: the Store updates it.
    Store,
    /// Not installed by Glowtune's installer (a plain exe, a test or debug
    /// build): never offers an installer, which would put a second
    /// Glowtune next to it.
    Manual,
}

impl Channel {
    fn detect() -> Self {
        // Two guards for the Store copy: the package check, and the marker
        // the NSIS bundler stamps into the exe it installs (the Store's exe
        // is built with --no-bundle, so it has none).
        if packaged() {
            Self::Store
        } else if tauri::utils::platform::bundle_type()
            == Some(tauri::utils::config::BundleType::Nsis)
        {
            Self::Github
        } else {
            Self::Manual
        }
    }
}

/// True when running from an MSIX package (the Microsoft Store copy).
#[cfg(windows)]
fn packaged() -> bool {
    use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut len = 0u32;
    // SAFETY: a length query with no buffer; it only writes `len`.
    let rc = unsafe { GetCurrentPackageFullName(&mut len, None) };
    rc != APPMODEL_ERROR_NO_PACKAGE
}

#[cfg(not(windows))]
fn packaged() -> bool {
    false
}

/// What the windows show. Sent as "update-status" on every change.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub channel: Channel,
    /// The running version.
    pub current: String,
    /// "idle", "checking", "latest", "available", "downloading",
    /// "installing" or "error".
    pub status: &'static str,
    /// The new version, while one is available or installing.
    pub version: Option<String>,
    /// Its release notes (Markdown).
    pub notes: Option<String>,
    /// Its release date (RFC 3339).
    pub date: Option<String>,
    /// Download progress, 0..1, when the size is known.
    pub progress: Option<f32>,
    /// A short, plain-English reason when `status` is "error".
    pub error: Option<String>,
    /// When the last check finished (ms since 1970).
    pub checked_at: Option<u64>,
}

pub struct Updates {
    view: Mutex<UpdateView>,
    /// The update found by the last check, ready to download.
    found: Mutex<Option<Update>>,
    /// The version the pop-up was last shown for, so "Later" holds until
    /// the next start.
    offered: Mutex<Option<String>>,
}

impl Updates {
    pub fn new(current: String) -> Self {
        Self {
            view: Mutex::new(UpdateView {
                channel: Channel::detect(),
                current,
                status: "idle",
                version: None,
                notes: None,
                date: None,
                progress: None,
                error: None,
                checked_at: None,
            }),
            found: Mutex::new(None),
            offered: Mutex::new(None),
        }
    }

    pub(crate) fn view(&self) -> UpdateView {
        self.view
            .lock()
            .map(|v| v.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    fn change(&self, app: &AppHandle, f: impl FnOnce(&mut UpdateView)) -> UpdateView {
        let view = match self.view.lock() {
            Ok(mut v) => {
                f(&mut v);
                v.clone()
            }
            Err(p) => p.into_inner().clone(),
        };
        let _ = app.emit("update-status", &view);
        view
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Starts the background checks (GitHub installs only).
pub fn start(app: &AppHandle) {
    let updates = app.state::<Updates>();
    if updates.view().channel != Channel::Github {
        tracing::info!(
            "updates: {:?} copy, no background checks",
            updates.view().channel
        );
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("glowtune-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                let settings = crate::current_settings(&app);
                if settings.update_check {
                    let view = tauri::async_runtime::block_on(check(&app));
                    if let Some(version) = view.version.as_deref()
                        && view.status == "available"
                        && version != settings.skipped_update
                    {
                        offer_when_uncovered(&app, version);
                    }
                }
                std::thread::sleep(EVERY);
            }
        });
}

/// Asks GitHub for the newest version.
async fn check(app: &AppHandle) -> UpdateView {
    let updates = app.state::<Updates>();
    if matches!(updates.view().status, "downloading" | "installing") {
        return updates.view();
    }
    updates.change(app, |v| {
        v.status = "checking";
        v.error = None;
    });
    let handle = app.clone();
    let result = match app
        .updater_builder()
        .timeout(TIMEOUT)
        // The installer replaces this process: tidy up first (see `before_exit`).
        .on_before_exit(move || before_exit(&handle))
        .build()
    {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let found = match result {
        Ok(found) => found,
        // No update file on the latest release (one made before in-app
        // updates), or none for this kind of install: nothing to offer.
        Err(
            tauri_plugin_updater::Error::ReleaseNotFound
            | tauri_plugin_updater::Error::TargetNotFound(_)
            | tauri_plugin_updater::Error::TargetsNotFound(_),
        ) => {
            tracing::info!("updates: no update published for this copy");
            None
        }
        Err(e) => {
            tracing::warn!("updates: check failed: {e}");
            let offline = matches!(
                e,
                tauri_plugin_updater::Error::Reqwest(_) | tauri_plugin_updater::Error::Network(_)
            );
            return updates.change(app, |v| {
                v.status = "error";
                v.error = Some(if offline {
                    "Couldn't reach GitHub. Check your internet connection and try again.".into()
                } else {
                    "Couldn't check for updates right now. Try again later.".into()
                });
                v.checked_at = Some(now_ms());
            });
        }
    };
    let view = updates.change(app, |v| {
        v.checked_at = Some(now_ms());
        v.progress = None;
        match &found {
            Some(u) => {
                v.status = "available";
                v.version = Some(u.version.clone());
                v.notes = u.body.clone();
                v.date = u
                    .raw_json
                    .get("pub_date")
                    .and_then(|d| d.as_str())
                    .map(String::from);
            }
            None => {
                v.status = "latest";
                v.version = None;
                v.notes = None;
                v.date = None;
            }
        }
    });
    match &found {
        Some(u) => tracing::info!(
            "updates: {} is available (running {})",
            u.version,
            u.current_version
        ),
        None => tracing::info!("updates: up to date"),
    }
    if let Ok(mut f) = updates.found.lock() {
        *f = found;
    }
    view
}

/// True when `e` means the download wasn't signed with Glowtune's key.
fn bad_signature(e: &tauri_plugin_updater::Error) -> bool {
    use tauri_plugin_updater::Error as E;
    matches!(
        e,
        E::Minisign(_)
            | E::Base64(_)
            | E::SignatureUtf8(_)
            | E::SignedVersionMismatch { .. }
            | E::MissingSignedVersion
    )
}

/// Shows the pop-up for `version` once a full-screen game is out of the way.
fn offer_when_uncovered(app: &AppHandle, version: &str) {
    let updates = app.state::<Updates>();
    if updates
        .offered
        .lock()
        .is_ok_and(|o| o.as_deref() == Some(version))
    {
        return;
    }
    // Rule: never pop a window over a full-screen game.
    while overlay::fullscreen_screen().is_some() {
        std::thread::sleep(COVERED_RETRY);
    }
    if let Ok(mut o) = updates.offered.lock() {
        *o = Some(version.to_string());
    }
    if let Err(e) = show_popup(app) {
        tracing::warn!("updates: pop-up failed: {e}");
    }
}

/// The pop-up: a glass card in the bottom-right corner of the main screen,
/// shown without taking focus.
fn show_popup(app: &AppHandle) -> tauri::Result<()> {
    // Both cards sit in the same corner; the newer news wins.
    if let Some(w) = app.get_webview_window(WHATS_NEW) {
        let _ = w.close();
    }
    corner_card(app, POPUP, "/update", "Glowtune update")
}

/// A glass card in the bottom-right corner of the main screen (window
/// `label`, page `route`), shown without taking focus.
fn corner_card(app: &AppHandle, label: &str, route: &str, title: &str) -> tauri::Result<()> {
    let w = match app.get_webview_window(label) {
        Some(w) => w,
        None => WebviewWindowBuilder::new(app, label, WebviewUrl::App(route.into()))
            .title(title)
            .inner_size(POPUP_SIZE.0, POPUP_SIZE.1)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .skip_taskbar(true)
            .always_on_top(true)
            .focused(false)
            .visible(false)
            .effects(tauri::utils::config::WindowEffectsConfig {
                effects: vec![tauri::window::Effect::Acrylic],
                ..Default::default()
            })
            .build()?,
    };
    if let Some(m) = app.primary_monitor()? {
        let scale = m.scale_factor();
        let area = m.work_area();
        let (width, height) = (POPUP_SIZE.0 * scale, POPUP_SIZE.1 * scale);
        let x = area.position.x as f64 + area.size.width as f64 - width - MARGIN * scale;
        let y = area.position.y as f64 + area.size.height as f64 - height - MARGIN * scale;
        w.set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ))?;
    }
    crate::taskbar::show_on_top(&w)
}

/// Runs just before the installer replaces Glowtune (the process then exits
/// at once, so the normal quit path doesn't run).
fn before_exit(app: &AppHandle) {
    tracing::info!("updates: closing for the installer");
    // Stops the sound engine, which gives the service window its own volume
    // back (it is whispered to 1/10 000 while Glowtune plays the sound).
    if let Some(sound) = app.try_state::<crate::sound::Sound>() {
        sound.window_closed();
    }
    // The song playing now still counts.
    if let Some(l) = app.try_state::<crate::stats::Listening>() {
        l.flush();
    }
    // The pop-up (or About) just showed this version's notes: no "What's
    // new" card after the restart. Written straight to the file; applying
    // settings to windows makes no sense this close to exit.
    if let Some(version) = app.state::<Updates>().view().version {
        let state = app.state::<crate::AppSettings>();
        let mut s = state.current.lock().map(|s| s.clone()).unwrap_or_default();
        s.last_seen_version = version;
        if let Err(e) = s.save(&state.path) {
            tracing::warn!("updates: could not note the version: {e}");
        }
    }
}

/// The current state, for a window that just opened.
#[tauri::command]
pub fn update_status(updates: tauri::State<'_, Updates>) -> UpdateView {
    updates.view()
}

/// "Check for updates" in Settings → About.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateView, String> {
    if app.state::<Updates>().view().channel != Channel::Github {
        return Err("Only a copy installed with the Glowtune installer updates itself.".into());
    }
    Ok(check(&app).await)
}

/// "Update now": downloads, verifies and installs the update found by the
/// last check. On Windows this doesn't return: the installer takes over and
/// starts the new Glowtune when it's done.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updates = app.state::<Updates>();
    if updates.view().channel != Channel::Github {
        return Err("Only a copy installed with the Glowtune installer updates itself.".into());
    }
    let found = updates.found.lock().ok().and_then(|f| f.clone());
    let Some(update) = found else {
        return Err("No update to install. Check for updates first.".into());
    };
    updates.change(&app, |v| {
        v.status = "downloading";
        v.progress = Some(0.0);
        v.error = None;
    });
    let mut done = 0usize;
    let mut shown = 0.0f32;
    let progress_app = app.clone();
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk;
                let Some(total) = total.filter(|t| *t > 0) else {
                    return;
                };
                let p = (done as f64 / total as f64).min(1.0) as f32;
                // About 50 updates for the whole download, not one per chunk.
                if p - shown >= 0.02 || p >= 1.0 {
                    shown = p;
                    progress_app
                        .state::<Updates>()
                        .change(&progress_app, |v| v.progress = Some(p));
                }
            },
            || {},
        )
        .await;
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("updates: download failed: {e}");
            refused(
                &app,
                &e,
                "The download failed. Check your internet connection and try again.",
            );
            return Err("download failed".into());
        }
    };
    updates.change(&app, |v| {
        v.status = "installing";
        v.progress = Some(1.0);
    });
    tracing::info!("updates: installing {}", update.version);
    // Give the windows a moment to show "Restarting…".
    let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(600)))
        .await;
    match update.install(bytes) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("updates: install failed: {e}");
            refused(
                &app,
                &e,
                "The installer couldn't start. Try again, or download it from GitHub.",
            );
            Err(e.to_string())
        }
    }
}

/// Shows why an update didn't install. A download that isn't signed with
/// Glowtune's key is forgotten: trying the same file again can't work.
fn refused(app: &AppHandle, e: &tauri_plugin_updater::Error, otherwise: &str) {
    let updates = app.state::<Updates>();
    let forged = bad_signature(e);
    if forged && let Ok(mut f) = updates.found.lock() {
        *f = None;
    }
    updates.change(app, |v| {
        v.status = "error";
        v.progress = None;
        v.error = Some(if forged { SIGNATURE } else { otherwise }.into());
        if forged {
            v.version = None;
            v.notes = None;
            v.date = None;
        }
    });
}

/// "Skip this version": no more pop-ups until a newer one comes out.
#[tauri::command]
pub fn skip_update(app: AppHandle, version: String) {
    crate::update_settings(&app, |s| s.skipped_update = version);
    hide_popup(&app);
}

/// "Later": hides the pop-up until Glowtune starts again.
#[tauri::command]
pub fn dismiss_update(app: AppHandle) {
    hide_popup(&app);
}

/// Opens Settings → About from the pop-up (the release notes in full).
#[tauri::command]
pub fn update_details(app: AppHandle) -> Result<(), String> {
    hide_popup(&app);
    crate::show_settings_tab(&app, "about").map_err(|e| e.to_string())
}

fn hide_popup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(POPUP) {
        let _ = w.hide();
    }
}

// ---- "What's new" after an update --------------------------------------

/// The "What's new" card's window label (route `/update?whatsnew`).
pub const WHATS_NEW: &str = "whatsnew";
/// This version's release notes, built in (see build.rs). Empty: no card.
const NOTES: &str = include_str!(concat!(env!("OUT_DIR"), "/whats_new.md"));
/// Let Glowtune settle (and the player appear) before the card.
const WHATS_NEW_AFTER: Duration = Duration::from_secs(6);

/// At start: the first run of a new version shows its "What's new" card
/// once, but only to people who didn't already read the notes:
/// - Updated through the pop-up or About: `before_exit` already stored the
///   new version, so no card.
/// - The Store copy (the Store updates it silently) or an installer run by
///   hand over an older version: the card.
/// - A fresh install, with no settings file before: just remembers the
///   version.
/// - An installed copy with no version stored came from 0.4.0, whose pop-up
///   showed the notes but couldn't store the version yet: no card.
pub fn whats_new_on_start(app: &AppHandle, had_settings: bool) {
    let current = app.package_info().version.to_string();
    let seen = crate::current_settings(app).last_seen_version;
    if seen == current {
        return;
    }
    crate::update_settings(app, |s| s.last_seen_version.clone_from(&current));
    if !had_settings || card_notes(NOTES).is_empty() {
        return;
    }
    let channel = app.state::<Updates>().view().channel;
    if seen.is_empty() && channel == Channel::Github {
        return;
    }
    tracing::info!("updates: first start of {current} (was {seen:?}): what's new");
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("glowtune-whats-new".into())
        .spawn(move || {
            std::thread::sleep(WHATS_NEW_AFTER);
            // Never over a full-screen game: wait until it's gone.
            while overlay::fullscreen_screen().is_some() {
                std::thread::sleep(COVERED_RETRY);
            }
            if let Err(e) = corner_card(
                &app,
                WHATS_NEW,
                "/update?whatsnew",
                "What's new in Glowtune",
            ) {
                tracing::warn!("what's new: {e}");
            }
        });
}

/// The version and notes the "What's new" card shows.
#[derive(Serialize)]
pub struct WhatsNew {
    version: String,
    notes: String,
}

#[tauri::command]
pub fn whats_new(app: AppHandle) -> WhatsNew {
    WhatsNew {
        version: app.package_info().version.to_string(),
        notes: card_notes(NOTES),
    }
}

/// The part of the release notes the card shows: the "What's new" section
/// (without its heading; the card's title says it) and "Fixes" (with its
/// heading). The rest (early-build note, known issues, bug links) is for the
/// GitHub page. Empty when there's neither section: then no card.
fn card_notes(md: &str) -> String {
    let mut out = Vec::new();
    let mut on = false;
    for line in md.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            let h = heading.trim().to_lowercase().replace('\u{2019}', "'");
            let whats_new = h.starts_with("what's new") || h.starts_with("whats new");
            on = whats_new || h.starts_with("fixes");
            if whats_new {
                continue;
            }
        }
        if on {
            out.push(line);
        }
    }
    out.join("\n").trim().to_string()
}

/// "Got it" on the "What's new" card.
#[tauri::command]
pub fn close_whats_new(app: AppHandle) {
    if let Some(w) = app.get_webview_window(WHATS_NEW) {
        let _ = w.close();
    }
}

#[cfg(test)]
mod tests {
    use super::card_notes;

    #[test]
    fn the_card_shows_whats_new_and_fixes_only() {
        let md = "# Glowtune 0.5.0\n\n> **Early build.** Bugs.\n\n## What’s new\n\n**Lyrics Sync.**\n- Tap to sync.\n\n## Fixes\n\n- The bar.\n\n## Known issues\n\n- Apple Music.\n\n## Found a bug?\n\n- Email us\n";
        assert_eq!(
            card_notes(md),
            "**Lyrics Sync.**\n- Tap to sync.\n\n## Fixes\n\n- The bar."
        );
    }

    #[test]
    fn no_card_without_a_whats_new_or_fixes_section() {
        assert_eq!(card_notes(""), "");
        assert_eq!(card_notes("# Glowtune\n\n## Known issues\n\n- x\n"), "");
    }

    #[test]
    fn the_real_040_notes_make_a_card() {
        let notes = card_notes(include_str!("../../../docs/release-notes-0.4.0.md"));
        assert!(notes.starts_with("**Glowtune now updates itself.**"));
        assert!(!notes.contains("Known issues"));
        assert!(!notes.contains("Early build"));
    }
}
