//! Desktop scene ("stage"): a full-screen live wallpaper that sits *behind*
//! every other window. Album art, a big clock, lyrics, recently played and a
//! dock with controls + search, all lit by the song.
//!
//! ```text
//!  settings.stage_enabled ─► "stage" + optional "stage-<n>" windows (work area,
//!                            always at the bottom of each screen’s window stack)
//!  gate thread (only while open + playing, every 1 s):
//!     another window covers ≥ 85 % of that screen? ─► "stage-covered" event
//!         → the page freezes its animations (no frames = ~0 % GPU/CPU)
//!     uncovered on the main screen ─► floating desktop lyrics step aside
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const STAGE: &str = "stage";
const RECHECK: Duration = Duration::from_secs(1);

fn is_stage(label: &str) -> bool {
    label == STAGE
        || label
            .strip_prefix("stage-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Primary first in all-screen mode; missing named screens fall back to primary.
fn selected_screens(names: &[Option<&str>], primary: Option<usize>, selection: &str) -> Vec<usize> {
    if selection == "*" {
        return primary
            .into_iter()
            .chain((0..names.len()).filter(|i| Some(*i) != primary))
            .collect();
    }
    names
        .iter()
        .position(|name| !selection.is_empty() && *name == Some(selection))
        .or(primary)
        .into_iter()
        .collect()
}

/// Opens one scene per selected monitor (empty = main screen, "*" = all screens).
pub fn show(app: &AppHandle, monitor: &str) -> tauri::Result<()> {
    let monitors = app.available_monitors()?;
    let primary = app.primary_monitor()?;
    let primary = primary
        .as_ref()
        .and_then(|p| monitors.iter().position(|m| m.position() == p.position()));
    let names: Vec<_> = monitors
        .iter()
        .map(|m| m.name().map(String::as_str))
        .collect();
    let selected = selected_screens(&names, primary, monitor);
    let mut labels = Vec::new();
    for (i, index) in selected.into_iter().enumerate() {
        let target = &monitors[index];
        let label = if i == 0 {
            STAGE.to_string()
        } else {
            format!("stage-{i}")
        };
        let window = match app.get_webview_window(&label) {
            Some(w) => w,
            None => {
                let url = if i == 0 { "/stage" } else { "/stage?primary=0" };
                let w = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
                    .title("Glowtune")
                    .decorations(false)
                    .shadow(false)
                    .resizable(false)
                    // A real app: in the taskbar and Alt+Tab. Picking it brings the
                    // scene to the front; switching away sends it back behind
                    // everything as the wallpaper.
                    .skip_taskbar(i != 0)
                    .always_on_bottom(true)
                    .focused(false)
                    .visible(false)
                    .build()?;
                #[cfg(windows)]
                if i == 0 {
                    crate::thumbbar::install(app, &w);
                }
                let handle = w.clone();
                w.on_window_event(move |e| {
                    if let tauri::WindowEvent::Focused(focused) = e
                        && !handle.is_fullscreen().unwrap_or(false)
                    {
                        let _ = handle.set_always_on_bottom(!*focused);
                        if *focused {
                            // Alt+Tab only activates; lift it above the other apps.
                            // Clearing "always on bottom" is applied after this
                            // handler returns, so raise a moment later.
                            let w = handle.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_millis(60));
                                let target = w.clone();
                                let _ = w.run_on_main_thread(move || {
                                    if target.is_focused().unwrap_or(false) {
                                        raise(&target);
                                    }
                                });
                            });
                        }
                    }
                });
                w
            }
        };
        // Settings changed while full screen: keep it as it is unless its monitor changed.
        if window.is_fullscreen()? {
            if window
                .current_monitor()?
                .is_some_and(|m| m.position() == target.position())
            {
                labels.push(label);
                continue;
            }
            set_fullscreen(app, &label, false)?;
        }
        let area = target.work_area();
        window.set_position(area.position)?;
        window.set_size(area.size)?;
        window.show()?;
        labels.push(label);
    }
    for (label, window) in app.webview_windows() {
        if is_stage(&label) && !labels.contains(&label) {
            window.destroy()?;
        }
    }
    if let Some(gate) = app.try_state::<StageGate>() {
        let _ = gate.tx.send(Msg::Opened);
    }
    Ok(())
}

/// Full screen: the whole monitor, over the taskbar and desktop widgets, under the glow.
/// Off: back to a wallpaper behind every window.
pub fn set_fullscreen(app: &AppHandle, label: &str, on: bool) -> tauri::Result<()> {
    if !is_stage(label) {
        return Ok(());
    }
    let Some(w) = app.get_webview_window(label) else {
        return Ok(());
    };
    let Some(m) = w.current_monitor()?.or(app.primary_monitor()?) else {
        return Ok(());
    };
    // The window system's own borderless full screen: one step instead of a
    // visible move-then-resize. Not "always on top", so the edge glow stays above.
    if on {
        w.set_always_on_bottom(false)?;
        w.set_fullscreen(true)?;
        w.set_focus()?;
    } else {
        w.set_fullscreen(false)?;
        let area = m.work_area();
        w.set_position(area.position)?;
        w.set_size(area.size)?;
        w.set_always_on_bottom(true)?;
    }
    let _ = app.emit_to(label, "stage-fullscreen", on);
    Ok(())
}

pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    for (label, window) in app.webview_windows() {
        if is_stage(&label) {
            window.destroy()?;
        }
    }
    if let Some(gate) = app.try_state::<StageGate>() {
        let _ = gate.tx.send(Msg::Closed);
    }
    Ok(())
}

enum Msg {
    Playing(bool),
    Opened,
    Closed,
}

/// Watches whether each scene is actually visible (not buried under windows).
pub struct StageGate {
    tx: Sender<Msg>,
    /// Open, uncovered and on the main screen: the floating lyrics hide.
    on_main_screen: Arc<AtomicBool>,
}

impl StageGate {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        let on_main_screen = Arc::new(AtomicBool::new(false));
        let flag = on_main_screen.clone();
        std::thread::Builder::new()
            .name("glowtune-stage-gate".into())
            .spawn(move || {
                let (mut playing, mut open) = (false, false);
                let mut covered = HashMap::new();
                let mut topology = String::new();
                loop {
                    let msg = if playing && open {
                        match rx.recv_timeout(RECHECK) {
                            Ok(m) => Some(m),
                            Err(RecvTimeoutError::Timeout) => None,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    } else {
                        match rx.recv() {
                            Ok(m) => Some(m),
                            Err(_) => return,
                        }
                    };
                    let opened = matches!(msg, Some(Msg::Opened));
                    match msg {
                        Some(Msg::Playing(p)) => playing = p,
                        Some(Msg::Opened) => {
                            open = true;
                            covered.clear();
                        }
                        Some(Msg::Closed) => {
                            open = false;
                            covered.clear();
                        }
                        None => {}
                    }
                    // Reconcile hot-plug/resolution changes only while active; sleep while paused.
                    if open && let Ok(monitors) = app.available_monitors() {
                        let next =
                            format!("{:?}:{:?}", monitors, app.primary_monitor().ok().flatten());
                        let changed = next != topology;
                        topology = next;
                        if changed && !opened {
                            let settings = app.state::<crate::AppSettings>();
                            let current = settings.current.lock().ok().map(|s| s.clone());
                            // Window creation stays on this worker, never the Windows UI thread.
                            if let Some(s) = current
                                && s.stage_enabled
                                && let Err(e) = show(&app, &s.stage_monitor)
                            {
                                eprintln!("stage monitor refresh: {e}");
                                // Retry a transient monitor/window error on the next active tick.
                                topology.clear();
                            }
                        }
                    }
                    let primary = app.primary_monitor().ok().flatten();
                    let mut main = false;
                    let windows = app.webview_windows();
                    covered.retain(|label, _| windows.contains_key(label));
                    for (label, window) in windows {
                        if !open || !is_stage(&label) {
                            continue;
                        }
                        let now_covered = is_covered(&window);
                        if covered.insert(label.clone(), now_covered) != Some(now_covered) {
                            let _ = app.emit_to(&label, "stage-covered", now_covered);
                        }
                        main |= !now_covered
                            && primary.as_ref().is_some_and(|p| {
                                window
                                    .current_monitor()
                                    .ok()
                                    .flatten()
                                    .is_some_and(|m| m.position() == p.position())
                            });
                    }
                    if flag.swap(main, Ordering::Relaxed) != main
                        && let Some(g) = app.try_state::<crate::lyrics_gate::LyricsGate>()
                    {
                        g.changed();
                    }
                }
            })
            .expect("spawn stage gate");
        Self { tx, on_main_screen }
    }

    pub fn playing(&self, playing: bool) {
        let _ = self.tx.send(Msg::Playing(playing));
    }

    /// The scene is showing its own lyrics on the main screen right now.
    pub fn on_main_screen(&self) -> bool {
        self.on_main_screen.load(Ordering::Relaxed)
    }
}

#[cfg(windows)]
pub(crate) fn is_covered(window: &tauri::WebviewWindow) -> bool {
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // Tauri may link a different `windows` crate version: pass the raw handle.
    cover::covered(hwnd.0 as isize)
}

#[cfg(not(windows))]
pub(crate) fn is_covered(_window: &tauri::WebviewWindow) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_selection_preserves_primary_and_falls_back() {
        let names = [Some("left"), Some("main"), None];
        assert_eq!(selected_screens(&names, Some(1), "*"), [1, 0, 2]);
        assert_eq!(selected_screens(&names, Some(1), ""), [1]);
        assert_eq!(selected_screens(&names, Some(1), "left"), [0]);
        assert_eq!(selected_screens(&names, Some(1), "unplugged"), [1]);
        assert_eq!(selected_screens(&names, None, "*"), [0, 1, 2]);
        assert!(selected_screens(&[], None, "*").is_empty());
    }

    #[test]
    fn stage_labels_exclude_unrelated_windows() {
        for label in ["stage", "stage-1", "stage-23"] {
            assert!(is_stage(label));
        }
        for label in ["service", "settings", "stage-", "stage-video"] {
            assert!(!is_stage(label));
        }
    }
}

#[cfg(windows)]
mod cover {
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongPtrW, GetWindowRect,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };
    use windows::core::BOOL;

    struct Scan {
        stage: HWND,
        area: RECT,
        own_pid: u32,
        covered: bool,
    }

    /// HWND_TOP without moving, resizing or stealing activation.
    pub fn to_top(hwnd: isize) {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
        };
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: z-order change on our own window handle.
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// True if a single other window above the scene covers ≥ 85 % of it.
    pub fn covered(stage: isize) -> bool {
        let stage = HWND(stage as *mut _);
        let mut area = RECT::default();
        // SAFETY: plain Win32 query on a window handle we own.
        if unsafe { GetWindowRect(stage, &mut area) }.is_err() {
            return false;
        }
        let mut scan = Scan {
            stage,
            area,
            own_pid: std::process::id(),
            covered: false,
        };
        // SAFETY: the callback only runs during this call; `scan` outlives it.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut scan as *mut Scan as isize)) };
        scan.covered
    }

    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Scan` passed by `covered`, alive for the whole enumeration.
        let scan = unsafe { &mut *(data.0 as *mut Scan) };
        // EnumWindows goes top to bottom: everything after the scene is below it.
        if hwnd == scan.stage {
            return false.into();
        }
        // SAFETY: read-only Win32 queries on a handle given by EnumWindows.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return true.into();
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TRANSPARENT.0 != 0 {
                return true.into(); // click-through overlays (game/Discord overlays)
            }
            // Invisible full-screen overlays and desktop widgets (NVIDIA GeForce
            // Overlay, Rainmeter): layered tool / no-activate windows.
            if ex & WS_EX_LAYERED.0 != 0 && ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == scan.own_pid {
                return true.into();
            }
            // The desktop itself (icons layer, wallpaper hosts) and the taskbar.
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            if matches!(
                class.as_str(),
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
            ) {
                return true.into();
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                return true.into(); // hidden UWP / other virtual desktop
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut r as *mut RECT as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
                && GetWindowRect(hwnd, &mut r).is_err()
            {
                return true.into();
            }
            if overlap(&r, &scan.area) >= 0.85 {
                scan.covered = true;
                return false.into();
            }
        }
        true.into()
    }

    fn overlap(r: &RECT, area: &RECT) -> f64 {
        let w = (r.right.min(area.right) - r.left.max(area.left)).max(0) as f64;
        let h = (r.bottom.min(area.bottom) - r.top.max(area.top)).max(0) as f64;
        let total = ((area.right - area.left) as f64 * (area.bottom - area.top) as f64).max(1.0);
        w * h / total
    }
}

/// Puts the scene at the top of the normal (not topmost) window stack.
fn raise(w: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(h) = w.hwnd() {
        cover::to_top(h.0 as isize);
    }
    #[cfg(not(windows))]
    let _ = w.set_focus();
}
