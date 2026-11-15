//! The taskbar player: a slim strip of controls that sits *on* the Windows
//! taskbar, in the empty stretch at its left or right end, while music plays.
//!
//! ```text
//!  ┌──────────────────────────── taskbar ─────────────────────────────┐
//!  │ [▣ Song — Artist  ⏮ ⏯ ⏭]      ⊞ 🔍 apps…       [▣ … ⏭] ^ 🔊 12:30 │
//!  └── left end ─────────────────────────────────────── right end ────┘
//! ```
//! It's an ordinary topmost tool window placed over the taskbar's area (the
//! taskbar itself is untouched). The taskbar raises itself when clicked, so
//! while shown a light thread puts the strip back on top every second.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const TASKBAR: &str = "taskbar";
/// Width of the strip (logical px).
const WIDTH: f64 = 300.0;
/// Space kept clear of the right end (system tray, clock, notifications).
const RIGHT_CLEAR: f64 = 330.0;

#[derive(Default)]
pub struct TaskbarState {
    /// The strip is showing (the raise thread runs only then).
    shown: Arc<AtomicBool>,
}

/// Where the setting puts the strip: None = off.
fn side(app: &AppHandle) -> Option<String> {
    let s = app
        .state::<crate::AppSettings>()
        .current
        .lock()
        .ok()?
        .taskbar_player
        .clone();
    (s == "left" || s == "right").then_some(s)
}

/// Creates (hidden) or removes the strip to match the setting, and places it.
/// Takes the new settings: they're applied before they're stored.
pub fn apply(app: &AppHandle, setting: &str) -> tauri::Result<()> {
    let side = (setting == "left" || setting == "right").then(|| setting.to_string());
    let Some(side) = side else {
        if let Some(w) = app.get_webview_window(TASKBAR) {
            stop(app);
            w.destroy()?;
        }
        return Ok(());
    };
    let window = match app.get_webview_window(TASKBAR) {
        Some(w) => w,
        None => WebviewWindowBuilder::new(app, TASKBAR, WebviewUrl::App("/taskbar".into()))
            .title("Glowtune taskbar player")
            .inner_size(WIDTH, 40.0)
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
            .build()?,
    };
    place(app, &window, &side)
}

/// The strip's rectangle on the taskbar of the primary screen (physical px).
fn place(app: &AppHandle, w: &tauri::WebviewWindow, side: &str) -> tauri::Result<()> {
    let Some(m) = app.primary_monitor()? else {
        return Ok(());
    };
    let scale = m.scale_factor();
    let (mp, ms) = (m.position(), m.size());
    let area = m.work_area();
    // The taskbar is the part of the screen outside the work area, at the
    // bottom on almost every setup (top works the same way).
    let bottom = (mp.y + ms.height as i32) - (area.position.y + area.size.height as i32);
    let top = area.position.y - mp.y;
    let (y, bar_h) = if bottom >= top {
        (area.position.y + area.size.height as i32, bottom)
    } else {
        (mp.y, top)
    };
    if bar_h < 24 {
        // Auto-hidden or no taskbar: nowhere to sit.
        return w.hide();
    }
    let width = (WIDTH * scale).round() as i32;
    let height = (bar_h as f64 - 8.0 * scale).max(28.0 * scale).round() as i32;
    let margin = (10.0 * scale).round() as i32;
    let x = if side == "left" {
        mp.x + margin
    } else {
        mp.x + ms.width as i32 - width - (RIGHT_CLEAR * scale).round() as i32
    };
    w.set_size(tauri::PhysicalSize::new(width as u32, height as u32))?;
    w.set_position(tauri::PhysicalPosition::new(x, y + (bar_h - height) / 2))
}

/// Shown while there is a song; hidden otherwise (called by the strip's page).
#[tauri::command]
pub async fn taskbar_visible(on: bool, app: AppHandle) -> Result<(), String> {
    let Some(w) = app.get_webview_window(TASKBAR) else {
        return Ok(());
    };
    if on && side(&app).is_some() {
        w.show().map_err(|e| e.to_string())?;
        start(&app, &w);
    } else {
        stop(&app);
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn start(app: &AppHandle, w: &tauri::WebviewWindow) {
    let Some(state) = app.try_state::<TaskbarState>() else {
        return;
    };
    if state.shown.swap(true, Ordering::SeqCst) {
        return;
    }
    let shown = state.shown.clone();
    let Ok(hwnd) = w.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let _ = std::thread::Builder::new()
        .name("glowtune-taskbar".into())
        .spawn(move || {
            while shown.load(Ordering::SeqCst) {
                raise(hwnd);
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
}

fn stop(app: &AppHandle) {
    if let Some(state) = app.try_state::<TaskbarState>() {
        state.shown.store(false, Ordering::SeqCst);
    }
}

/// Shows `w` topmost without activating it (a game in front keeps focus).
pub fn show_on_top(w: &tauri::WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
        let hwnd = w.hwnd()?;
        // SAFETY: shows our own window handle.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        raise(hwnd.0 as isize);
        Ok(())
    }
    #[cfg(not(windows))]
    w.show()
}

/// Back above the taskbar, without taking focus.
#[cfg(windows)]
fn raise(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    // SAFETY: z-order change on our own window handle.
    unsafe {
        let _ = SetWindowPos(
            HWND(hwnd as *mut _),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn raise(_hwnd: isize) {}
