//! The on-screen toast: a small glass pill near the bottom of the main screen
//! that says what a hotkey just did ("Volume 45 %", "Night", "Sleep in 30 min").
//!
//! ```text
//!  hotkey ─► osd::show(app, toast) ─► window "osd" (made once, kept hidden)
//!                                     ├─ "osd" event → the page draws it
//!                                     └─ hidden again 1.8 s after the last toast
//! ```
//! Click-through, never takes focus, topmost (shows over borderless games).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const OSD: &str = "osd";
const SIZE: (f64, f64) = (360.0, 84.0);
/// Above the taskbar, roughly where Windows' own volume flyout lives.
const LIFT: f64 = 72.0;
const SHOW_FOR: Duration = Duration::from_millis(1800);

static SHOWN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    /// play, pause, next, previous, volume, lyrics, preset, sleep, player, scene
    pub icon: &'static str,
    pub text: String,
    /// 0..1 draws a level bar (volume).
    pub level: Option<f32>,
}

impl Toast {
    pub fn new(icon: &'static str, text: impl Into<String>) -> Self {
        Self {
            icon,
            text: text.into(),
            level: None,
        }
    }
}

/// Shows `toast`; safe to call from any thread.
pub fn show(app: &AppHandle, toast: Toast) {
    let app = app.clone();
    // Never on the main thread: building the toast window there waits for
    // the main thread itself and freezes the app (a sync command calling
    // this runs on it, and run_on_main_thread would run it right there).
    // Tauri hands window work over to the main thread on its own.
    let _ = std::thread::Builder::new()
        .name("glowtune-toast".into())
        .spawn(move || {
            if let Err(e) = show_now(&app, &toast) {
                tracing::warn!("toast: {e}");
            }
        });
}

fn show_now(app: &AppHandle, toast: &Toast) -> tauri::Result<()> {
    let w = match app.get_webview_window(OSD) {
        Some(w) => w,
        None => {
            let w = WebviewWindowBuilder::new(app, OSD, WebviewUrl::App("/osd".into()))
                .title("Glowtune")
                .inner_size(SIZE.0, SIZE.1)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .focused(false)
                .visible(false)
                .build()?;
            w.set_ignore_cursor_events(true)?;
            // The page isn't listening yet: it asks for the last toast when it loads.
            w
        }
    };
    place(app, &w)?;
    if let Some(last) = app.try_state::<LastToast>()
        && let Ok(mut l) = last.0.lock()
    {
        *l = Some(toast.clone());
    }
    let _ = w.emit("osd", toast);
    crate::taskbar::show_on_top(&w)?;
    let id = SHOWN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_FOR);
        if SHOWN.load(Ordering::SeqCst) == id
            && let Some(w) = app.get_webview_window(OSD)
        {
            let _ = w.hide();
        }
    });
    Ok(())
}

/// Bottom centre of the main screen, just above the taskbar.
fn place(app: &AppHandle, w: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(m) = app.primary_monitor()? else {
        return Ok(());
    };
    let scale = m.scale_factor();
    let area = m.work_area();
    let (width, height) = (
        (SIZE.0 * scale).round() as i32,
        (SIZE.1 * scale).round() as i32,
    );
    let x = area.position.x + (area.size.width as i32 - width) / 2;
    let y = area.position.y + area.size.height as i32 - height - (LIFT * scale).round() as i32;
    w.set_size(tauri::PhysicalSize::new(width as u32, height as u32))?;
    w.set_position(tauri::PhysicalPosition::new(x, y))
}

/// The newest toast, for a page that just loaded.
#[derive(Default)]
pub struct LastToast(pub std::sync::Mutex<Option<Toast>>);

/// A toast from a page (e.g. why a lyric card couldn't be made).
#[tauri::command]
pub fn toast(icon: String, text: String, app: AppHandle) {
    let icon: &'static str = match icon.as_str() {
        "card" => "card",
        "lyrics" => "lyrics",
        _ => "preset",
    };
    show(
        &app,
        Toast::new(icon, text.chars().take(80).collect::<String>()),
    );
}

#[tauri::command]
pub fn last_toast(last: tauri::State<'_, LastToast>) -> Option<Toast> {
    last.0.lock().ok().and_then(|l| l.clone())
}
