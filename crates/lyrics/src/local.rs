use std::path::{Path, PathBuf};

/// Finds a sibling file with the audio file's stem and a `.lrc` or `.LRC` extension.
/// Lowercase takes priority when both exist; directories are excluded.
pub fn find_local_lrc(audio_path: &Path) -> Option<PathBuf> {
    audio_path.file_stem()?;
    ["lrc", "LRC"]
        .into_iter()
        .map(|extension| audio_path.with_extension(extension))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lowercase_uppercase_and_missing_siblings() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let audio = dir.path().join("Song.v2.flac");
        assert_eq!(find_local_lrc(&audio), None);
        for extension in ["lrc", "LRC"] {
            let lyrics = audio.with_extension(extension);
            std::fs::write(&lyrics, "[00:00]Hello").expect("write lyrics");
            let found = find_local_lrc(&audio).expect("lyrics sibling");
            // Windows may resolve the lowercase candidate to an uppercase filename.
            assert_eq!(
                std::fs::canonicalize(found).expect("resolved lyrics"),
                std::fs::canonicalize(&lyrics).expect("expected lyrics")
            );
            std::fs::remove_file(lyrics).expect("remove lyrics");
        }
        std::fs::create_dir(audio.with_extension("lrc")).expect("create directory");
        assert_eq!(find_local_lrc(&audio), None);
        assert_eq!(find_local_lrc(Path::new("/")), None);
    }
}
