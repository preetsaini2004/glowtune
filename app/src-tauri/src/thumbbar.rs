//! Previous / play-pause / next buttons under Glowtune's taskbar preview (the
//! Windows "thumbnail toolbar"), like media players have.
//!
//! ```text
//!  scene window created ─► subclass it ─► "TaskbarButtonCreated" ─► add 3 buttons
//!  button pressed (WM_COMMAND, THBN_CLICKED) ─► crate::transport(action)
//!  playback changes ─► swap the middle icon (play ⇄ pause)
//! ```
//! Everything touching the window or the taskbar object runs on the main (UI)
//! thread, which owns the scene window.

use std::cell::RefCell;
use std::sync::OnceLock;

use tauri::AppHandle;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, THB_FLAGS, THB_ICON, THB_TOOLTIP,
    THBF_ENABLED, THUMBBUTTON, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, HICON, ICONINFO, RegisterWindowMessageW, WM_COMMAND,
};
use windows::core::w;

const ID_PREV: u32 = 1;
const ID_PLAY: u32 = 2;
const ID_NEXT: u32 = 3;
const THBN_CLICKED: usize = 0x1800;
const SUBCLASS_ID: usize = 0x676c_7774; // "glwt"

static APP: OnceLock<AppHandle> = OnceLock::new();
static CREATED_MSG: OnceLock<u32> = OnceLock::new();

struct Bar {
    list: ITaskbarList3,
    hwnd: HWND,
    play: HICON,
    pause: HICON,
    playing: bool,
}

thread_local! {
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

/// Hooks the (primary) scene window. Safe to call more than once.
pub fn install(app: &AppHandle, window: &tauri::WebviewWindow) {
    let _ = APP.set(app.clone());
    let Ok(h) = window.hwnd() else { return };
    let raw = h.0 as isize;
    let _ = app.run_on_main_thread(move || {
        let hwnd = HWND(raw as *mut _);
        // SAFETY: plain Win32 calls on our own window, on its UI thread.
        unsafe {
            CREATED_MSG.get_or_init(|| RegisterWindowMessageW(w!("TaskbarButtonCreated")));
            let _ = SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0);
        }
        // The taskbar button may already exist (window shown before the hook).
        add_buttons(hwnd);
    });
}

/// Swaps the middle button between play and pause.
pub fn set_playing(app: &AppHandle, playing: bool) {
    let _ = app.run_on_main_thread(move || {
        BAR.with(|bar| {
            if let Some(b) = bar.borrow_mut().as_mut()
                && b.playing != playing
            {
                b.playing = playing;
                let (icon, tip) = if playing {
                    (b.pause, "Pause")
                } else {
                    (b.play, "Play")
                };
                // SAFETY: COM call on the UI thread that created the object.
                let _ = unsafe {
                    b.list
                        .ThumbBarUpdateButtons(b.hwnd, &[button(ID_PLAY, icon, tip)])
                };
            }
        });
    });
}

fn add_buttons(hwnd: HWND) {
    // SAFETY: COM object creation and calls on the UI thread (COM is initialised
    // there by the webview runtime).
    let list: ITaskbarList3 =
        match unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) } {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("taskbar buttons: {e}");
                return;
            }
        };
    if unsafe { list.HrInit() }.is_err() {
        return;
    }
    let (prev, play, pause, next) = (
        icon(Glyph::Prev),
        icon(Glyph::Play),
        icon(Glyph::Pause),
        icon(Glyph::Next),
    );
    let buttons = [
        button(ID_PREV, prev, "Previous"),
        button(ID_PLAY, play, "Play"),
        button(ID_NEXT, next, "Next"),
    ];
    // Fails harmlessly if this window already has its buttons.
    if unsafe { list.ThumbBarAddButtons(hwnd, &buttons) }.is_ok() {
        BAR.with(|bar| {
            *bar.borrow_mut() = Some(Bar {
                list,
                hwnd,
                play,
                pause,
                playing: false,
            });
        });
    }
}

fn button(id: u32, icon: HICON, tip: &str) -> THUMBBUTTON {
    let mut b = THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        hIcon: icon,
        dwFlags: THBF_ENABLED,
        ..Default::default()
    };
    for (i, c) in tip.encode_utf16().take(b.szTip.len() - 1).enumerate() {
        b.szTip[i] = c;
    }
    b
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if CREATED_MSG.get() == Some(&msg) {
        // Explorer (re)created our taskbar button: (re)add the toolbar.
        add_buttons(hwnd);
    } else if msg == WM_COMMAND && (wparam.0 >> 16) & 0xffff == THBN_CLICKED {
        let action = match (wparam.0 & 0xffff) as u32 {
            ID_PREV => "previous",
            ID_PLAY => "playPause",
            ID_NEXT => "next",
            _ => "",
        };
        if !action.is_empty()
            && let Some(app) = APP.get()
        {
            crate::transport(app.clone(), action);
        }
        return LRESULT(0);
    }
    // SAFETY: forwarding to the next handler with the original arguments.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[derive(Clone, Copy)]
enum Glyph {
    Prev,
    Play,
    Pause,
    Next,
}

/// A 32×32 white glyph with smooth (4×4 supersampled) edges.
fn icon(glyph: Glyph) -> HICON {
    const S: usize = 32;
    fn tri(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
        let s = |p: (f32, f32), q: (f32, f32)| (x - q.0) * (p.1 - q.1) - (p.0 - q.0) * (y - q.1);
        let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
    }
    let inside = |x: f32, y: f32| -> bool {
        let next = |x: f32| {
            tri(x, y, (8.0, 8.0), (8.0, 24.0), (20.0, 16.0))
                || ((21.0..24.0).contains(&x) && (8.0..24.0).contains(&y))
        };
        match glyph {
            Glyph::Play => tri(x, y, (11.0, 8.0), (11.0, 24.0), (24.0, 16.0)),
            Glyph::Pause => {
                ((10.0..14.0).contains(&x) || (18.0..22.0).contains(&x)) && (8.0..24.0).contains(&y)
            }
            Glyph::Next => next(x),
            Glyph::Prev => next(32.0 - x),
        }
    };
    let mut px = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let mut hit = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    if inside(
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    ) {
                        hit += 1;
                    }
                }
            }
            let i = (y * S + x) * 4;
            px[i..i + 4].copy_from_slice(&[255, 255, 255, (hit * 255 / 16) as u8]);
        }
    }
    let mask = [0u8; S * S / 8];
    // SAFETY: bitmaps are built from buffers that outlive the calls; the icon
    // copies them, so they are deleted right after.
    unsafe {
        let color = CreateBitmap(S as i32, S as i32, 1, 32, Some(px.as_ptr().cast()));
        let mask = CreateBitmap(S as i32, S as i32, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
