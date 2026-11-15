//! Lyric cards: pictures of the line being sung, saved in Pictures\Glowtune.
//!
//! ```text
//!  Ctrl+Alt+C / scene menu ─► a window with lyrics ("make-lyric-card")
//!      page draws the card (canvas) ─raw PNG bytes─► save_lyric_card
//!          ─► Pictures\Glowtune\Artist - Title.png  (+ toast)
//! ```

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAX_BYTES: usize = 25 * 1024 * 1024;

/// Windows that can draw a card (they follow the lyrics), best first.
const DRAWERS: [&str; 3] = ["stage", crate::LYRICS, "player"];

/// Asks a window that follows the lyrics to make a card (hotkey).
pub fn request(app: &AppHandle) {
    let target = DRAWERS.iter().find(|label| {
        app.get_webview_window(label)
            .is_some_and(|w| w.is_visible().unwrap_or(false))
    });
    match target {
        Some(label) => {
            // The label rides along: every page hears the event, only
            // the named one draws (else two cards were saved per press).
            let _ = app.emit_to(*label, "make-lyric-card", *label);
        }
        None => crate::osd::show(
            app,
            crate::osd::Toast::new("card", "Turn on the scene, desktop lyrics or player first"),
        ),
    }
}

/// "%41rtist%20-%20Title" → "Artist - Title" (the header is URI-encoded).
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A safe Windows file name (no path parts or reserved characters).
fn file_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = joined.chars().take(90).collect();
    let short = short
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    if short.is_empty() {
        "Lyric card".into()
    } else {
        short
    }
}

/// `dir\name.png`, or `name (2).png`... when taken.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(format!("{name}.png"));
    if !first.exists() {
        return first;
    }
    (2..10_000)
        .map(|n| dir.join(format!("{name} ({n}).png")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[tauri::command]
pub fn save_lyric_card(request: tauri::ipc::Request<'_>, app: AppHandle) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("The card didn't arrive as a picture.".into());
    };
    if bytes.len() > MAX_BYTES || !bytes.starts_with(PNG_MAGIC) {
        return Err("The card didn't arrive as a picture.".into());
    }
    let name = request
        .headers()
        .get("x-card-name")
        .and_then(|v| v.to_str().ok())
        .map(decode)
        .unwrap_or_default();
    let dir = app
        .path()
        .picture_dir()
        .map_err(|e| format!("No Pictures folder: {e}"))?
        .join("Glowtune");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    let path = free_path(&dir, &file_name(&name));
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save the card: {e}"))?;
    tracing::info!("lyric card saved: {}", path.display());
    crate::osd::show(
        &app,
        crate::osd::Toast::new("card", "Lyric card saved in Pictures › Glowtune"),
    );
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_decoded_and_made_safe() {
        assert_eq!(
            decode("AP%20Dhillon%20-%20With%20You"),
            "AP Dhillon - With You"
        );
        assert_eq!(decode("%E0%A8%B8%E0%A8%A4"), "ਸਤ");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(
            file_name("AC/DC - Back: In \"Black\"?"),
            "AC DC - Back In Black"
        );
        assert_eq!(file_name("..\\..\\evil"), "evil");
        assert_eq!(file_name("   "), "Lyric card");
    }

    #[test]
    fn taken_names_get_a_number() {
        let dir = std::env::temp_dir().join(format!("glowtune-card-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = free_path(&dir, "Song");
        std::fs::write(&a, b"x").unwrap();
        let b = free_path(&dir, "Song");
        assert!(b.ends_with("Song (2).png"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
