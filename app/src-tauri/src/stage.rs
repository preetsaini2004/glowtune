//! Desktop scene ("stage"): a full-screen live wallpaper that sits *behind*
//! every other window. Album art, a big clock, lyrics, recently played and a
//! dock with controls + search, all lit by the song.
//!
//! ```text
//!  settings.stage_enabled ─► "stage" + optional "stage-<n>" windows (work area,
//!                            always at the bottom of each screen’s window stack)
//!  gate thread (only while open: every 1 s, ¼ s while asleep, and at once on
//!  any foreground / minimize / move change anywhere on the desktop):
//!     another window covers ≥ 85 % of that screen? ─► "stage-covered" event
//!         → the page freezes its animations (no frames = ~0 % GPU/CPU)
//!     covered completely (≥ 98 %, e.g. a maximized window or a game)?
//!         → the page is also put to sleep: WebView2 is told it's hidden and
//!           to keep as little memory as it can, which frees its graphics memory
//!     uncovered on the main screen ─► floating desktop lyrics step aside
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const STAGE: &str = "stage";
const RECHECK: Duration = Duration::from_secs(1);
/// While a scene sleeps, look more often: waking late shows a dark screen.
const RECHECK_ASLEEP: Duration = Duration::from_millis(250);
/// Share of the scene one window must cover to freeze it…
const COVERED: f64 = 0.85;
/// …and to put it to sleep (nothing of it is visible).
const BURIED: f64 = 0.98;
/// The scene's own dark, shown for the moment before a waking page draws.
const BACKGROUND: tauri::window::Color = tauri::window::Color(5, 4, 10, 255);

/// Where window-change events wake the gate (set once, by `StageGate::start`).
static WAKE: OnceLock<Sender<Msg>> = OnceLock::new();

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
                    .background_color(BACKGROUND)
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
    Opened,
    Closed,
    /// Some window came to the front, was minimized or moved: look again.
    Recheck,
}

/// Waits for the next message (or `wait`, when given). Rechecks that piled up
/// meanwhile (a window dragged around) count once. `Err` = the app is closing.
fn next(rx: &Receiver<Msg>, wait: Option<Duration>) -> Result<Vec<Msg>, ()> {
    let first = match wait {
        Some(t) => match rx.recv_timeout(t) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Err(()),
        },
        None => Some(rx.recv().map_err(|_| ())?),
    };
    let mut all: Vec<Msg> = first.into_iter().collect();
    all.extend(rx.try_iter().filter(|m| !matches!(m, Msg::Recheck)));
    Ok(all)
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
        let _ = WAKE.set(tx.clone());
        let on_main_screen = Arc::new(AtomicBool::new(false));
        let flag = on_main_screen.clone();
        std::thread::Builder::new()
            .name("glowtune-stage-gate".into())
            .spawn(move || {
                let mut open = false;
                let mut covered = HashMap::new();
                let mut asleep: HashMap<String, bool> = HashMap::new();
                let mut topology = String::new();
                loop {
                    // Paused or playing, an open scene is watched: one window
                    // list a second costs nothing, and a sleeping scene must
                    // wake when it's uncovered even while the music is paused.
                    let wait = open.then(|| {
                        if asleep.values().any(|a| *a) {
                            RECHECK_ASLEEP
                        } else {
                            RECHECK
                        }
                    });
                    let Ok(msgs) = next(&rx, wait) else { return };
                    let mut opened = false;
                    for msg in msgs {
                        match msg {
                            Msg::Recheck => {}
                            Msg::Opened => {
                                open = true;
                                opened = true;
                                covered.clear();
                                asleep.clear();
                                watch_windows(&app, true);
                            }
                            Msg::Closed => {
                                open = false;
                                covered.clear();
                                asleep.clear();
                                watch_windows(&app, false);
                            }
                        }
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
                    asleep.retain(|label, _| windows.contains_key(label));
                    for (label, window) in windows {
                        if !open || !is_stage(&label) {
                            continue;
                        }
                        let share = coverage(&window);
                        let now_covered = share >= COVERED;
                        let now_asleep = share >= BURIED;
                        let was_asleep = asleep.insert(label.clone(), now_asleep).unwrap_or(false);
                        // Waking: draw again first, then tell the page it's seen.
                        if was_asleep && !now_asleep {
                            set_awake(&window, true);
                        }
                        if covered.insert(label.clone(), now_covered) != Some(now_covered) {
                            let _ = app.emit_to(&label, "stage-covered", now_covered);
                        }
                        // Sleeping: the page has frozen itself, now let it go.
                        if now_asleep && !was_asleep {
                            set_awake(&window, false);
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

    /// Play/pause: an open scene is watched either way; just look again now.
    pub fn playing(&self, _playing: bool) {
        let _ = self.tx.send(Msg::Recheck);
    }

    /// The scene is showing its own lyrics on the main screen right now.
    pub fn on_main_screen(&self) -> bool {
        self.on_main_screen.load(Ordering::Relaxed)
    }
}

pub(crate) fn is_covered(window: &tauri::WebviewWindow) -> bool {
    coverage(window) >= COVERED
}

/// How much of the scene the biggest single window above it hides (0–1).
#[cfg(windows)]
fn coverage(window: &tauri::WebviewWindow) -> f64 {
    let Ok(hwnd) = window.hwnd() else {
        return 0.0;
    };
    // Tauri may link a different `windows` crate version: pass the raw handle.
    cover::coverage(hwnd.0 as isize)
}

#[cfg(not(windows))]
fn coverage(_window: &tauri::WebviewWindow) -> f64 {
    0.0
}

/// Asleep: WebView2 is told the page is hidden (no frames, timers slowed,
/// `document.hidden`) and to keep as little memory as it can, which frees the
/// graphics memory it holds for the scene. Awake: back to normal.
#[cfg(windows)]
fn set_awake(window: &tauri::WebviewWindow, awake: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows::core::Interface;
    let result = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: COM calls on the webview's own (main) thread, inside with_webview.
        unsafe {
            let _ = controller.SetIsVisible(awake);
            if let Ok(core) = controller.CoreWebView2()
                && let Ok(core) = core.cast::<ICoreWebView2_19>()
            {
                let _ = core.SetMemoryUsageTargetLevel(if awake {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                });
            }
        }
    });
    if let Err(e) = result {
        tracing::warn!("desktop scene sleep: {e}");
    }
}

#[cfg(not(windows))]
fn set_awake(_window: &tauri::WebviewWindow, _awake: bool) {}

/// While a scene is open, any window coming to the front, minimizing,
/// restoring or being dropped somewhere wakes the gate at once, so a sleeping
/// scene is drawing again before the window in front has finished moving away.
#[cfg(windows)]
fn watch_windows(app: &AppHandle, on: bool) {
    let _ = app.run_on_main_thread(move || cover::watch(on));
}

#[cfg(not(windows))]
fn watch_windows(_app: &AppHandle, _on: bool) {}

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
        /// Largest share of the scene one window above it hides.
        most: f64,
    }

    /// The window-change hook while a scene is open (0 = none).
    static HOOK: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    /// Starts or stops the window-change hook. Main thread only: the hook's
    /// events arrive through this thread's message loop.
    pub fn watch(on: bool) {
        use std::sync::atomic::Ordering;
        use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
        use windows::Win32::UI::WindowsAndMessaging::{
            EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, WINEVENT_OUTOFCONTEXT,
        };
        let old = HOOK.swap(0, Ordering::Relaxed);
        if old != 0 {
            // SAFETY: a hook this thread set earlier.
            let _ = unsafe { UnhookWinEvent(HWINEVENTHOOK(old as *mut _)) };
        }
        if on {
            // Foreground changes, move/resize ends and minimize start/end: the
            // system events between those two, all rare (not every mouse move).
            // SAFETY: out-of-context hook with a plain function as callback.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_MINIMIZEEND,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            HOOK.store(hook.0 as isize, Ordering::Relaxed);
        }
    }

    unsafe extern "system" fn changed(
        _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if let Some(tx) = super::WAKE.get() {
            let _ = tx.send(super::Msg::Recheck);
        }
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

    /// The largest share (0–1) of the scene that one other window above it covers.
    pub fn coverage(stage: isize) -> f64 {
        let stage = HWND(stage as *mut _);
        let mut area = RECT::default();
        // SAFETY: plain Win32 query on a window handle we own.
        if unsafe { GetWindowRect(stage, &mut area) }.is_err() {
            return 0.0;
        }
        // Only the part outside the taskbar counts: the taskbar is always
        // above the scene, so a maximized window hides all that's left of it
        // (unclipped, a full-screen scene under a maximized window read 97 %
        // and never went to sleep).
        {
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            };
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: plain Win32 queries on our own window handle.
            if unsafe {
                GetMonitorInfoW(
                    MonitorFromWindow(stage, MONITOR_DEFAULTTONEAREST),
                    &mut info,
                )
            }
            .as_bool()
            {
                let work = info.rcWork;
                let clipped = RECT {
                    left: area.left.max(work.left),
                    top: area.top.max(work.top),
                    right: area.right.min(work.right),
                    bottom: area.bottom.min(work.bottom),
                };
                if clipped.right > clipped.left && clipped.bottom > clipped.top {
                    area = clipped;
                }
            }
        }
        let mut scan = Scan {
            stage,
            area,
            own_pid: std::process::id(),
            most: 0.0,
        };
        // SAFETY: the callback only runs during this call; `scan` outlives it.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut scan as *mut Scan as isize)) };
        scan.most
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
            scan.most = scan.most.max(overlap(&r, &scan.area));
            if scan.most >= super::BURIED {
                return false.into(); // can't get more hidden than that
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
