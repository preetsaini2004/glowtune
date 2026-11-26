fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    whats_new();
    tauri_build::build()
}

/// Copies this version's release notes (docs/release-notes-<version>.md)
/// into the build, for the "What's new" card shown after an update
/// (`update::whats_new`). Empty when there are none: then no card.
fn whats_new() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let notes = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(format!("release-notes-{version}.md"));
    println!("cargo:rerun-if-changed={}", notes.display());
    let text = std::fs::read_to_string(&notes).unwrap_or_default();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("whats_new.md"), text).expect("write whats_new.md");
}
