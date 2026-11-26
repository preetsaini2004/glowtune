//! Settings › Feedback: the app info a message carries, if the user leaves
//! "Include app info" on. Sending happens in the Settings window itself
//! (`lib/feedback.ts`): Web3Forms takes messages from a browser only.
//!
//! Only what's listed here goes with a message, and the page shows it
//! before sending ("See what's sent"). Never song history, never what's
//! playing, never anything personal.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// One line of app info, as shown and as sent.
#[derive(Serialize)]
pub struct InfoLine {
    pub name: &'static str,
    pub value: String,
}

#[tauri::command]
pub fn feedback_info(app: AppHandle) -> Vec<InfoLine> {
    let s = crate::current_settings(&app);
    let channel = app
        .try_state::<crate::update::Updates>()
        .map(|u| match u.view().channel {
            crate::update::Channel::Github => "GitHub installer",
            crate::update::Channel::Store => "Microsoft Store",
            crate::update::Channel::Manual => "Plain exe (test or portable)",
        })
        .unwrap_or("Unknown");
    let player = app
        .try_state::<Arc<crate::hub::Shared>>()
        .and_then(|shared| {
            let snapshot = shared.snapshot.lock().ok()?;
            snapshot.track.as_ref().map(|t| t.source.clone())
        })
        .unwrap_or_else(|| "Nothing playing".into());
    let on_off = |on: bool| if on { "on" } else { "off" };
    let sound = match (s.eq_enabled, s.sound_blend.as_str()) {
        (false, "off") => "off".to_string(),
        (eq, blend) => [eq.then_some("Equalizer"), (blend != "off").then_some(blend)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" + "),
    };
    vec![
        InfoLine {
            name: "Glowtune",
            value: format!("{} ({channel})", app.package_info().version),
        },
        InfoLine {
            name: "Windows",
            value: windows_version(),
        },
        InfoLine {
            name: "Music app",
            value: player,
        },
        InfoLine {
            name: "Edge glow",
            value: on_off(s.glow_enabled).into(),
        },
        InfoLine {
            name: "Desktop lyrics",
            value: if s.lyrics_on_desktop {
                format!("on ({})", s.desktop_lyrics_style)
            } else {
                "off".into()
            },
        },
        InfoLine {
            name: "Desktop scene",
            value: on_off(s.stage_enabled).into(),
        },
        InfoLine {
            name: "Sound effects",
            value: sound,
        },
    ]
}

/// "Windows 11 25H2 (build 26300)", from the registry.
#[cfg(windows)]
fn windows_version() -> String {
    use std::os::windows::process::CommandExt;
    let read = |name: &str| -> Option<String> {
        let out = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
                "/v",
                name,
            ])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // "    CurrentBuild    REG_SZ    26300"
        text.lines()
            .find(|l| l.trim_start().starts_with(name))
            .and_then(|l| l.split("REG_SZ").nth(1))
            .map(|v| v.trim().to_string())
    };
    let build = read("CurrentBuild").unwrap_or_default();
    let number: u32 = build.parse().unwrap_or(0);
    // ProductName says "Windows 10" on Windows 11 too: the build tells.
    let name = if number >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    };
    match read("DisplayVersion") {
        Some(v) => format!("{name} {v} (build {build})"),
        None => format!("{name} (build {build})"),
    }
}

#[cfg(not(windows))]
fn windows_version() -> String {
    std::env::consts::OS.to_string()
}
