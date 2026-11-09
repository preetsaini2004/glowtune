//! End-to-end index tests using generated PCM WAV files, never external fixtures.

use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use library::{AlbumId, Library, LibraryWatcher, ScanReport, TrackId};
use lofty::{
    config::WriteOptions,
    picture::{MimeType, Picture, PictureType},
    tag::{Accessor, ItemKey, Tag, TagExt, TagType},
};
use tempfile::TempDir;

fn wav(path: &Path, bits: u16, rate: u32, frames: u32) {
    let channels = 2_u16;
    let align = channels * (bits / 8);
    let len = frames * u32::from(align);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    bytes.extend_from_slice(&align.to_le_bytes());
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.resize(44 + len as usize, 0);
    fs::write(path, bytes).unwrap();
}

fn tagged(path: &Path, title: &str, artist: &str, album: &str, disc: u32, track: u32) {
    wav(path, 16, 44100, 4410);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    tag.set_disk(disc);
    tag.set_track(track);
    tag.insert_text(ItemKey::RecordingDate, "2001-02-03".into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn setup() -> (TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::open_in_memory().unwrap();
    library.add_folder(dir.path()).unwrap();
    (dir, library)
}

fn scan(library: &Library) -> ScanReport {
    library
        .scan(None, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

#[test]
fn fts5_prefixes_diacritics_and_album_artist() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch("CREATE VIRTUAL TABLE fts_probe USING fts5(text)")
        .unwrap();
    let (dir, library) = setup();
    tagged(
        &dir.path().join("beat.wav"),
        "Beat It",
        "Michael Jackson",
        "Thriller",
        1,
        1,
    );
    let beyonce = dir.path().join("halo.wav");
    tagged(&beyonce, "Halo", "Beyoncé", "I Am", 1, 2);
    let mut tag = lofty::read_from_path(&beyonce).unwrap();
    use lofty::file::{AudioFile, TaggedFileExt};
    tag.primary_tag_mut()
        .unwrap()
        .insert_text(ItemKey::AlbumArtist, "Destiny's Child".into());
    tag.save_to_path(&beyonce, WriteOptions::default()).unwrap();
    assert_eq!(scan(&library).added, 2);
    assert_eq!(
        library.search("beat it mich", 10).unwrap()[0].title,
        "Beat It"
    );
    assert_eq!(library.search("beyonce", 10).unwrap()[0].title, "Halo");
    assert_eq!(library.search("destiny chi", 10).unwrap()[0].title, "Halo");
    assert_eq!(library.search("THRILL", usize::MAX).unwrap().len(), 1);
    assert!(library.search("beat", 0).unwrap().is_empty());
}

#[test]
fn hostile_search_is_always_literal_and_never_errors() {
    let (dir, library) = setup();
    tagged(&dir.path().join("x.wav"), "x", "A", "Album", 1, 1);
    scan(&library);
    for query in [
        "\"",
        "*",
        "NEAR(",
        "a OR",
        "-x",
        ":",
        "🎵",
        "",
        "a\" OR \"*",
        "^x:()",
        "\0",
        "a + {x}",
    ] {
        assert!(library.search(query, 100).is_ok(), "query: {query:?}");
    }
    assert!(library.search("", 100).unwrap().is_empty());
    assert!(library.search("*", 100).unwrap().is_empty());
}

#[test]
fn incremental_scan_updates_deletes_and_cleans_fts_and_albums() {
    let (dir, library) = setup();
    let first = dir.path().join("a.wav");
    let second = dir.path().join("b.wav");
    tagged(&first, "Original", "Artist", "First", 1, 1);
    tagged(&second, "Gone", "Artist", "Second", 1, 1);
    assert_eq!(scan(&library).added, 2);
    let old = library.track_by_path(&first).unwrap().unwrap();
    assert_eq!(scan(&library).unchanged, 2);
    tagged(
        &first,
        "Replacement title of a different length",
        "Artist",
        "New album",
        1,
        1,
    );
    let report = scan(&library);
    assert_eq!((report.added, report.updated, report.unchanged), (0, 1, 1));
    assert_eq!(library.track_by_path(&first).unwrap().unwrap().id, old.id);
    assert!(library.search("original", 5).unwrap().is_empty());
    assert_eq!(library.search("replacement", 5).unwrap().len(), 1);
    fs::remove_file(&second).unwrap();
    assert_eq!(scan(&library).removed, 1);
    assert!(library.search("gone", 10).unwrap().is_empty());
    let albums = library.albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].title, "New album");
    assert!(scan(&library).failed.is_empty());
}

#[test]
fn corrupt_files_do_not_abort_or_erase_previous_metadata() {
    let (dir, library) = setup();
    let good = dir.path().join("good.wav");
    let corrupt = dir.path().join("broken.flac");
    wav(&good, 16, 44100, 4410);
    fs::write(&corrupt, b"random bytes, not FLAC").unwrap();
    let report = scan(&library);
    assert_eq!(report.added, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, fs::canonicalize(&corrupt).unwrap());
    fs::write(&good, b"corrupted after indexing").unwrap();
    assert_eq!(scan(&library).failed.len(), 2);
    assert_eq!(library.stats().unwrap().tracks, 1);
}

#[test]
fn cancellation_before_and_during_scan_is_safe_and_callbacks_can_read() {
    let (dir, library) = setup();
    for index in 0..105 {
        wav(&dir.path().join(format!("{index}.wav")), 16, 44100, 44);
    }
    let cancel = AtomicBool::new(true);
    let report = library
        .scan(None, &cancel, &mut |_| panic!("unexpected callback"))
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(library.stats().unwrap().tracks, 0);
    cancel.store(false, Ordering::Relaxed);
    let mut calls = 0;
    let report = library
        .scan(None, &cancel, &mut |p| {
            assert_eq!(p.seen, 100);
            assert_eq!(p.updated, 100);
            library.stats().unwrap();
            calls += 1;
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();
    assert_eq!(calls, 1);
    assert!(report.cancelled);
    assert_eq!(report.added, 100);
    assert_eq!(library.stats().unwrap().tracks, 100);
}

#[test]
fn albums_group_case_insensitively_and_sort_tracks() {
    let (dir, library) = setup();
    for (name, title, artist, album, disc, number) in [
        ("a", "Third", "Beyoncé", "Record", 2, 1),
        ("b", "Second", "BEYONCÉ", "RECORD", 1, 2),
        ("c", "First", "beyoncé", "record", 1, 1),
        ("d", "Zebra", "alpha", "zebra", 1, 1),
        ("e", "Apple", "Alpha", "Apple", 1, 1),
    ] {
        tagged(
            &dir.path().join(format!("{name}.wav")),
            title,
            artist,
            album,
            disc,
            number,
        );
    }
    wav(&dir.path().join("untagged.wav"), 16, 44100, 4410);
    assert_eq!(scan(&library).added, 6);
    let albums = library.albums().unwrap();
    assert_eq!(
        albums.iter().map(|a| a.title.as_str()).collect::<Vec<_>>()[..3],
        ["Unknown album", "Apple", "zebra"]
    );
    let album = albums.iter().find(|a| a.track_count == 3).unwrap();
    assert_eq!(album.year, Some(2001));
    assert_eq!(
        library
            .album_tracks(album.id)
            .unwrap()
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second", "Third"]
    );
    let untagged = library
        .track_by_path(&dir.path().join("untagged.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(untagged.title, "untagged");
    assert!(untagged.artist.is_empty());
    assert_eq!(untagged.album, "Unknown album");
    assert!(library.album_tracks(AlbumId(-1)).unwrap().is_empty());
}

#[test]
fn adjacent_cover_is_case_insensitive_and_updated_by_changes() {
    let (dir, library) = setup();
    let path = dir.path().join("song.wav");
    let art = dir.path().join("Cover.JPG");
    wav(&path, 16, 44100, 4410);
    fs::write(&art, b"jpeg fixture bytes").unwrap();
    scan(&library);
    let track = library.track_by_path(&path).unwrap().unwrap();
    assert!(track.has_cover);
    let cover = library.cover(track.id).unwrap().unwrap();
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, b"jpeg fixture bytes");
    assert_eq!(library.albums().unwrap()[0].cover_track, Some(track.id));
    fs::remove_file(&art).unwrap();
    library.apply_changes(&[art]).unwrap();
    assert!(!library.track(track.id).unwrap().unwrap().has_cover);
    assert!(library.cover(track.id).unwrap().is_none());
    assert!(library.cover(TrackId(-1)).unwrap().is_none());
}

#[test]
fn embedded_front_cover_wins_over_other_and_adjacent_art() {
    let (dir, library) = setup();
    let path = dir.path().join("song.wav");
    wav(&path, 16, 44100, 4410);
    let mut tag = Tag::new(TagType::Id3v2);
    for (kind, data) in [
        (PictureType::CoverBack, b"back".to_vec()),
        (PictureType::CoverFront, b"front".to_vec()),
    ] {
        tag.push_picture(
            Picture::unchecked(data)
                .pic_type(kind)
                .mime_type(MimeType::Jpeg)
                .build(),
        );
    }
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    fs::write(dir.path().join("cover.png"), b"external").unwrap();
    scan(&library);
    let track = library.track_by_path(&path).unwrap().unwrap();
    assert!(track.has_cover);
    assert_eq!(library.cover(track.id).unwrap().unwrap().data, b"front");
}

#[test]
fn folder_removal_and_changes_create_delete_and_directory_rename() {
    let (dir, library) = setup();
    library.add_folder(dir.path()).unwrap();
    assert_eq!(library.folders().unwrap().len(), 1);
    let path = dir.path().join("new.wav");
    wav(&path, 16, 44100, 4410);
    assert_eq!(
        library
            .apply_changes(&[path.clone(), path.clone()])
            .unwrap()
            .added,
        1
    );
    let outside = tempfile::tempdir().unwrap();
    let foreign = outside.path().join("outside.wav");
    wav(&foreign, 16, 44100, 4410);
    assert_eq!(
        library.apply_changes(&[foreign]).unwrap(),
        ScanReport::default()
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(library.apply_changes(&[path]).unwrap().removed, 1);
    assert_eq!(library.stats().unwrap().albums, 0);
    let old = dir.path().join("old");
    let new = dir.path().join("new");
    fs::create_dir(&old).unwrap();
    wav(&old.join("nested.wav"), 16, 44100, 4410);
    assert_eq!(
        library
            .apply_changes(std::slice::from_ref(&old))
            .unwrap()
            .added,
        1
    );
    fs::rename(&old, &new).unwrap();
    let report = library.apply_changes(&[old, new]).unwrap();
    assert_eq!((report.added, report.removed), (1, 1));
    library.remove_folder(dir.path()).unwrap();
    assert!(library.folders().unwrap().is_empty());
    assert_eq!(library.stats().unwrap().tracks, 0);
    assert_eq!(library.stats().unwrap().albums, 0);
    assert!(library.search("nested", 10).unwrap().is_empty());
}

#[test]
fn high_resolution_wav_properties_and_total_duration() {
    let (dir, library) = setup();
    let path = dir.path().join("hires.WAV");
    wav(&path, 24, 96000, 9600);
    scan(&library);
    let track = library.track_by_path(&path).unwrap().unwrap();
    assert!(track.format.lossless);
    assert_eq!(track.format.codec, "WAV");
    assert_eq!(track.format.bit_depth, Some(24));
    assert_eq!(track.format.sample_rate, Some(96000));
    assert_eq!(track.format.channels, Some(2));
    assert_eq!(track.format.bitrate_kbps, Some(4608));
    assert_eq!(track.duration, Some(Duration::from_millis(100)));
    assert_eq!(
        library.stats().unwrap().total_duration,
        Duration::from_millis(100)
    );
}

#[test]
fn disk_database_reopens_with_idempotent_migration_and_wal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.sqlite");
    let audio = dir.path().join("track.wav");
    wav(&audio, 16, 44100, 4410);
    let id;
    {
        let library = Library::open(&db).unwrap();
        library.add_folder(dir.path()).unwrap();
        scan(&library);
        id = library.track_by_path(&audio).unwrap().unwrap().id;
    }
    let reopened = Library::open(&db).unwrap();
    assert_eq!(reopened.track(id).unwrap().unwrap().title, "track");
    assert_eq!(reopened.search("tra", 10).unwrap().len(), 1);
    assert_eq!(scan(&reopened).unchanged, 1);
    let raw = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        raw.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        raw.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        raw.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn scans_skip_hidden_directories_and_limit_to_one_root() {
    let (dir, library) = setup();
    let other = tempfile::tempdir().unwrap();
    library.add_folder(other.path()).unwrap();
    fs::create_dir(dir.path().join(".hidden")).unwrap();
    wav(&dir.path().join(".hidden/skip.wav"), 16, 44100, 44);
    wav(&dir.path().join("yes.wav"), 16, 44100, 44);
    fs::write(dir.path().join("not-audio.txt"), b"text").unwrap();
    wav(&other.path().join("other.wav"), 16, 44100, 44);
    assert_eq!(
        library
            .scan(Some(dir.path()), &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .added,
        1
    );
    assert_eq!(scan(&library).added, 1);
    let unknown = tempfile::tempdir().unwrap();
    assert!(matches!(
        library.scan(Some(unknown.path()), &AtomicBool::new(false), &mut |_| {}),
        Err(library::Error::NotAFolder(_))
    ));
    assert!(matches!(
        library.add_folder(&dir.path().join("yes.wav")),
        Err(library::Error::NotAFolder(_))
    ));
}

#[test]
fn large_scan_crosses_transaction_boundaries_and_search_caps_results() {
    let (dir, library) = setup();
    for index in 0..505 {
        wav(&dir.path().join(format!("song-{index}.wav")), 16, 44100, 44);
    }
    assert_eq!(scan(&library).added, 505);
    assert_eq!(library.search("song", usize::MAX).unwrap().len(), 500);
    assert_eq!(scan(&library).unchanged, 505);
    for entry in fs::read_dir(dir.path()).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    assert_eq!(scan(&library).removed, 505);
    assert_eq!(library.stats().unwrap().albums, 0);
}

#[test]
fn native_watcher_batches_changes_and_drop_disconnects() {
    let dir = tempfile::tempdir().unwrap();
    let (watcher, receiver) = LibraryWatcher::start(&[dir.path().to_owned()]).unwrap();
    let path = dir.path().join("watched.wav");
    wav(&path, 16, 44100, 44);
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut found = false;
    while Instant::now() < deadline {
        let batch = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        if batch.iter().any(|p| p.file_name() == path.file_name()) {
            found = true;
            break;
        }
    }
    assert!(found);
    let start = Instant::now();
    drop(watcher);
    assert!(start.elapsed() < Duration::from_secs(2));
    while receiver.try_recv().is_ok() {}
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[cfg(unix)]
#[test]
fn symlink_files_and_directories_are_ignored_in_scans_and_changes() {
    let (dir, library) = setup();
    let outside = tempfile::tempdir().unwrap();
    let audio = outside.path().join("outside.wav");
    wav(&audio, 16, 44100, 44);
    let file_link = dir.path().join("linked.wav");
    let dir_link = dir.path().join("linked-dir");
    std::os::unix::fs::symlink(&audio, &file_link).unwrap();
    std::os::unix::fs::symlink(outside.path(), &dir_link).unwrap();
    assert_eq!(scan(&library).added, 0);
    assert_eq!(
        library
            .apply_changes(&[file_link, dir_link.join("outside.wav")])
            .unwrap()
            .added,
        0
    );
}
