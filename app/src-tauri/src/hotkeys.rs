//! Global hotkeys: control the music from any app, even in a game.
//!
//! ```text
//!  Ctrl+Alt+Space  play / pause       Ctrl+Alt+L  desktop lyrics on/off
//!  Ctrl+Alt+→ / ←  next / previous    Ctrl+Alt+P  mini player
//!  Ctrl+Alt+↑ / ↓  volume up / down   Ctrl+Alt+D  desktop scene on/off
//!  Ctrl+Alt+1..9   your presets       Ctrl+Alt+N  next preset
//!  Ctrl+Alt+S      sleep timer (15 → 30 → 45 → 60 min → end of song → off)
//!  Ctrl+Alt+C      lyric card of the line being sung (PicturesGlowtune)
//! ```
//! Ctrl+Alt on purpose: Ctrl+arrows alone move the cursor word by word in
//! every text box, so a global Ctrl+→ would break typing everywhere.
//! On many keyboards (German, French, Polish...) Ctrl+Alt is also AltGr,
//! which types characters like @ { [ ł: a key that types something with
//! Ctrl+Alt on the current layout is left alone, so typing keeps working.
//!
//! One thread owns the keys (Windows ties a hotkey to the thread that made
//! it): it registers them, waits in `GetMessage` (no CPU while idle) and turns
//! a press into an action. Settings switch them on/off by posting it a message.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::osd::{self, Toast};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Lyrics,
    Player,
    Scene,
    Sleep,
    Card,
    NextPreset,
    Preset(usize),
}

struct Key {
    vk: u32,
    name: &'static str,
    /// Holding the key repeats it (volume).
    repeat: bool,
    action: Action,
    label: &'static str,
}

const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;

const fn key(vk: u32, name: &'static str, action: Action, label: &'static str) -> Key {
    Key {
        vk,
        name,
        repeat: false,
        action,
        label,
    }
}

const KEYS: [Key; 20] = [
    key(VK_SPACE, "Space", Action::PlayPause, "Play / pause"),
    key(VK_RIGHT, "→", Action::Next, "Next song"),
    key(VK_LEFT, "←", Action::Previous, "Previous song"),
    Key {
        repeat: true,
        ..key(VK_UP, "↑", Action::VolumeUp, "Volume up")
    },
    Key {
        repeat: true,
        ..key(VK_DOWN, "↓", Action::VolumeDown, "Volume down")
    },
    key(b'L' as u32, "L", Action::Lyrics, "Desktop lyrics on / off"),
    key(
        b'P' as u32,
        "P",
        Action::Player,
        "Show / hide the mini player",
    ),
    key(b'D' as u32, "D", Action::Scene, "Desktop scene on / off"),
    key(b'S' as u32, "S", Action::Sleep, "Sleep timer"),
    key(b'C' as u32, "C", Action::Card, "Lyric card of this line"),
    key(b'N' as u32, "N", Action::NextPreset, "Next preset"),
    key(b'1' as u32, "1", Action::Preset(0), "Preset 1"),
    key(b'2' as u32, "2", Action::Preset(1), "Preset 2"),
    key(b'3' as u32, "3", Action::Preset(2), "Preset 3"),
    key(b'4' as u32, "4", Action::Preset(3), "Preset 4"),
    key(b'5' as u32, "5", Action::Preset(4), "Preset 5"),
    key(b'6' as u32, "6", Action::Preset(5), "Preset 6"),
    key(b'7' as u32, "7", Action::Preset(6), "Preset 7"),
    key(b'8' as u32, "8", Action::Preset(7), "Preset 8"),
    key(b'9' as u32, "9", Action::Preset(8), "Preset 9"),
];

/// Volume step per press (5 %).
const VOLUME_STEP: f32 = 0.05;
/// Posted to the hotkey thread: wParam 1 = register, 0 = unregister.
const WM_SET: u32 = 0x8000 + 1; // WM_APP + 1

#[derive(Default)]
pub struct Hotkeys {
    thread: AtomicU32,
    /// Keys another app already owns ("Ctrl+Alt+→").
    taken: Mutex<Vec<&'static str>>,
    /// Keys the keyboard layout uses to type a character (AltGr).
    typing: Mutex<Vec<&'static str>>,
    /// The preset the last Ctrl+Alt+number / P brought back.
    last_preset: AtomicUsize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyView {
    keys: String,
    label: &'static str,
    /// False when the key isn't active.
    ok: bool,
    /// Why not: another app has it, or the keyboard types with it.
    note: Option<&'static str>,
}

#[tauri::command]
pub fn hotkey_list(hotkeys: tauri::State<'_, Hotkeys>) -> Vec<HotkeyView> {
    let taken = hotkeys.taken.lock().map(|t| t.clone()).unwrap_or_default();
    let typing = hotkeys.typing.lock().map(|t| t.clone()).unwrap_or_default();
    KEYS.iter()
        .filter(|k| !matches!(k.action, Action::Preset(i) if i > 0))
        .map(|k| HotkeyView {
            keys: if k.action == Action::Preset(0) {
                "Ctrl+Alt+1…9".into()
            } else {
                format!("Ctrl+Alt+{}", k.name)
            },
            label: if k.action == Action::Preset(0) {
                "Your presets"
            } else {
                k.label
            },
            ok: !taken.contains(&k.name) && !typing.contains(&k.name),
            note: if typing.contains(&k.name) {
                Some("Your keyboard types with this")
            } else if taken.contains(&k.name) {
                Some("Used by another app")
            } else {
                None
            },
        })
        .collect()
}

/// Starts the hotkey thread (keys on when `on`).
pub fn start(app: &AppHandle, on: bool) {
    app.manage(Hotkeys::default());
    #[cfg(windows)]
    {
        let app = app.clone();
        let _ = std::thread::Builder::new()
            .name("glowtune-hotkeys".into())
            .spawn(move || win::run(app, on));
    }
    #[cfg(not(windows))]
    let _ = on;
}

/// Turns the keys on or off (from settings).
pub fn set_enabled(app: &AppHandle, on: bool) {
    #[cfg(windows)]
    if let Some(h) = app.try_state::<Hotkeys>() {
        let thread = h.thread.load(Ordering::SeqCst);
        if thread != 0 {
            win::post(thread, on);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, on);
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MapVirtualKeyExW,
        RegisterHotKey, ToUnicodeEx, UnregisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG, PM_NOREMOVE, PeekMessageW,
        PostThreadMessageW, WM_HOTKEY, WM_USER,
    };

    /// True when Ctrl+Alt+`vk` types a character on the keyboard layout in
    /// use (AltGr layouts): such a key must stay free for typing.
    fn types_with_altgr(vk: u32) -> bool {
        // SAFETY: read-only keyboard queries; the buffers outlive the calls.
        unsafe {
            let fg = GetForegroundWindow();
            let thread = if fg.0.is_null() {
                0
            } else {
                GetWindowThreadProcessId(fg, None)
            };
            let layout = GetKeyboardLayout(thread);
            let mut keys = [0u8; 256];
            keys[0x11] = 0x80; // VK_CONTROL
            keys[0x12] = 0x80; // VK_MENU (Alt)
            keys[0xA2] = 0x80; // VK_LCONTROL
            keys[0xA5] = 0x80; // VK_RMENU (AltGr)
            let scan = MapVirtualKeyExW(vk, MAPVK_VK_TO_VSC, Some(layout));
            let mut out = [0u16; 8];
            // Flag 4: don't change the keyboard's dead-key state.
            let n = ToUnicodeEx(vk, scan, &keys, &mut out, 4, Some(layout));
            // Control characters (Ctrl+letter) and a plain space aren't typing.
            n != 0 && out[..n.unsigned_abs() as usize].iter().any(|c| *c > 0x20)
        }
    }

    pub fn post(thread: u32, on: bool) {
        // SAFETY: posting a plain message to our own thread.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_SET, WPARAM(on as usize), LPARAM(0));
        }
    }

    fn register(state: &Hotkeys) {
        let mut taken = Vec::new();
        let mut typing = Vec::new();
        for (id, k) in KEYS.iter().enumerate() {
            if types_with_altgr(k.vk) {
                typing.push(k.name);
                continue;
            }
            let mut mods = MOD_CONTROL | MOD_ALT;
            if !k.repeat {
                mods |= MOD_NOREPEAT;
            }
            // SAFETY: thread-owned hotkey, no window.
            if unsafe { RegisterHotKey(None, id as i32 + 1, mods, k.vk) }.is_err() {
                taken.push(k.name);
            }
        }
        if !taken.is_empty() {
            tracing::warn!("hotkeys already used by another app: Ctrl+Alt+{taken:?}");
        }
        if !typing.is_empty() {
            tracing::info!("hotkeys left for typing (AltGr layout): Ctrl+Alt+{typing:?}");
        }
        if let Ok(mut t) = state.taken.lock() {
            *t = taken;
        }
        if let Ok(mut t) = state.typing.lock() {
            *t = typing;
        }
    }

    fn unregister(state: &Hotkeys) {
        for id in 0..KEYS.len() {
            // SAFETY: undoes our own registrations (unknown ids just fail).
            unsafe {
                let _ = UnregisterHotKey(None, id as i32 + 1);
            }
        }
        if let Ok(mut t) = state.taken.lock() {
            t.clear();
        }
        if let Ok(mut t) = state.typing.lock() {
            t.clear();
        }
    }

    pub fn run(app: AppHandle, on: bool) {
        let state = app.state::<Hotkeys>();
        let mut msg = MSG::default();
        // SAFETY: creates this thread's message queue before anyone posts to it.
        unsafe {
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
            state.thread.store(GetCurrentThreadId(), Ordering::SeqCst);
        }
        let mut registered = false;
        if on {
            register(&state);
            registered = true;
        }
        // SAFETY: standard message loop on this thread.
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            match msg.message {
                WM_HOTKEY => {
                    let id = msg.wParam.0;
                    if let Some(k) = id.checked_sub(1).and_then(|i| KEYS.get(i)) {
                        run_action(&app, k.action);
                    }
                }
                WM_SET => {
                    let want = msg.wParam.0 != 0;
                    if want != registered {
                        if want {
                            register(&state);
                        } else {
                            unregister(&state);
                        }
                        registered = want;
                    }
                }
                _ => {}
            }
        }
    }
}

fn is_playing(app: &AppHandle) -> (bool, String) {
    app.state::<std::sync::Arc<crate::hub::Shared>>()
        .snapshot
        .lock()
        .map(|s| {
            (
                s.playback.status == "playing",
                s.track
                    .as_ref()
                    .map(|t| t.title.clone())
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default()
}

fn run_action(app: &AppHandle, action: Action) {
    tracing::debug!("hotkey: {action:?}");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match action {
            Action::PlayPause => {
                let (playing, title) = is_playing(&app);
                crate::transport(app.clone(), "playPause");
                let toast = if playing {
                    Toast::new("pause", "Paused")
                } else if title.is_empty() {
                    Toast::new("play", "Play")
                } else {
                    Toast::new("play", title)
                };
                osd::show(&app, toast);
            }
            Action::Next => {
                crate::transport(app.clone(), "next");
                osd::show(&app, Toast::new("next", "Next song"));
            }
            Action::Previous => {
                crate::transport(app.clone(), "previous");
                osd::show(&app, Toast::new("previous", "Previous song"));
            }
            Action::VolumeUp | Action::VolumeDown => {
                let delta = if action == Action::VolumeUp {
                    VOLUME_STEP
                } else {
                    -VOLUME_STEP
                };
                match step_volume(&app, delta).await {
                    Some(level) => osd::show(
                        &app,
                        Toast {
                            level: Some(level),
                            ..Toast::new("volume", format!("Volume {}%", (level * 100.0).round()))
                        },
                    ),
                    None => osd::show(
                        &app,
                        Toast::new("volume", "This app's volume can't be changed"),
                    ),
                }
            }
            Action::Lyrics => {
                let on = !crate::current_settings(&app).lyrics_on_desktop;
                crate::update_settings(&app, |s| s.lyrics_on_desktop = on);
                let text = if on {
                    "Desktop lyrics on"
                } else {
                    "Desktop lyrics off"
                };
                osd::show(&app, Toast::new("lyrics", text));
            }
            Action::Player => {
                let shown = crate::toggle_player(&app);
                let text = if shown {
                    "Mini player"
                } else {
                    "Mini player hidden"
                };
                osd::show(&app, Toast::new("player", text));
            }
            Action::Scene => {
                let on = !crate::current_settings(&app).stage_enabled;
                crate::update_settings(&app, |s| s.stage_enabled = on);
                let text = if on {
                    "Desktop scene on"
                } else {
                    "Desktop scene off"
                };
                osd::show(&app, Toast::new("scene", text));
            }
            Action::Sleep => {
                let next = app
                    .try_state::<crate::sleep::SleepTimer>()
                    .and_then(|t| t.next_step());
                let view = crate::sleep::set(&app, next);
                osd::show(&app, Toast::new("sleep", crate::sleep::describe(&view)));
            }
            Action::Card => crate::card::request(&app),
            Action::NextPreset => {
                let count = crate::current_settings(&app).scene_presets.len();
                if count == 0 {
                    osd::show(&app, no_presets());
                    return;
                }
                let state = app.state::<Hotkeys>();
                let i = (state.last_preset.load(Ordering::SeqCst) + 1) % count;
                apply_preset(&app, i);
            }
            Action::Preset(i) => apply_preset(&app, i),
        }
    });
}

fn no_presets() -> Toast {
    Toast::new("preset", "Save a preset in the scene menu first")
}

fn apply_preset(app: &AppHandle, i: usize) {
    let list = crate::current_settings(app).scene_presets;
    let Some(p) = list.get(i) else {
        let toast = if list.is_empty() {
            no_presets()
        } else {
            Toast::new("preset", format!("No preset {} yet", i + 1))
        };
        osd::show(app, toast);
        return;
    };
    let name = p.name.clone();
    match crate::presets::apply(app, i) {
        Ok(()) => {
            app.state::<Hotkeys>()
                .last_preset
                .store(i, Ordering::SeqCst);
            osd::show(app, Toast::new("preset", name));
        }
        Err(e) => osd::show(app, Toast::new("preset", e)),
    }
}

/// Moves the playing app's volume by `delta`; returns the new level.
async fn step_volume(app: &AppHandle, delta: f32) -> Option<f32> {
    let shared = app
        .state::<std::sync::Arc<crate::hub::Shared>>()
        .inner()
        .clone();
    let web = shared
        .snapshot
        .lock()
        .ok()
        .and_then(|s| s.track.as_ref().map(|t| t.source == "Web player"))
        .unwrap_or(false);
    let level = if web {
        let now = crate::web_player_at(app, "state", None).await?.volume? as f32;
        let level = (now + delta).clamp(0.0, 1.0);
        crate::web_player_at(app, "volume", Some(level as f64)).await;
        level
    } else {
        let now = shared.media.volume()?;
        let level = (now + delta).clamp(0.0, 1.0);
        shared
            .media
            .control(media_source::Control::SetVolume(level));
        level
    };
    // The mini player's and dock's sliders follow.
    let _ = tauri::Emitter::emit(app, "volume-changed", level);
    Some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_unique_and_only_volume_repeats() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a.vk, b.vk, "{} and {} share a key", a.label, b.label);
            }
            assert_eq!(
                a.repeat,
                matches!(a.action, Action::VolumeUp | Action::VolumeDown)
            );
        }
    }
}
