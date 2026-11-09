use std::{
    fs, io,
    path::{Path, PathBuf},
};

use lofty::{
    config::ParseOptions,
    file::{AudioFile, FileType, TaggedFile, TaggedFileExt},
    mp4::{Mp4Codec, Mp4File},
    picture::{Picture, PictureType},
    prelude::Accessor,
    probe::Probe,
    tag::ItemKey,
};

use crate::{AlbumId, AudioFormat, Cover, Result, Track, TrackId};

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn read(path: &Path) -> io::Result<(TaggedFile, &'static str, bool)> {
    // Guess from bytes as well as the extension (including standalone .alac files).
    let probe = Probe::open(path).map_err(invalid)?.guess_file_type()?;
    let (codec, lossless) = match probe.file_type() {
        Some(FileType::Mp4) => {
            let file = Mp4File::read_from(&mut probe.into_inner(), ParseOptions::default())
                .map_err(invalid)?;
            let (codec, lossless) = match file.properties().codec() {
                Some(Mp4Codec::ALAC) => ("ALAC", true),
                Some(Mp4Codec::FLAC) => ("FLAC", true),
                Some(Mp4Codec::MP3) => ("MP3", false),
                Some(Mp4Codec::AAC) => ("AAC", false),
                _ => ("Unknown", false),
            };
            return Ok((file.into(), codec, lossless));
        }
        Some(FileType::Flac) => ("FLAC", true),
        Some(FileType::Mpeg) => ("MP3", false),
        Some(FileType::Aac) => ("AAC", false),
        Some(FileType::Opus) => ("Opus", false),
        Some(FileType::Vorbis) => ("Vorbis", false),
        Some(FileType::Wav) => ("WAV", true),
        Some(FileType::Aiff) => ("AIFF", true),
        Some(FileType::WavPack) => ("WavPack", true),
        Some(FileType::Ape) => ("APE", true),
        _ => return Err(invalid("Unsupported audio format")),
    };
    Ok((probe.read().map_err(invalid)?, codec, lossless))
}

fn picture(file: &TaggedFile) -> Option<&Picture> {
    file.tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .find(|pic| pic.pic_type() == PictureType::CoverFront)
        .or_else(|| file.tags().iter().flat_map(|tag| tag.pictures()).next())
}

pub(crate) fn read_track(path: &Path) -> io::Result<Track> {
    let (file, codec, lossless) = read(path)?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    let title = tag
        .and_then(|t| t.title())
        .filter(|s| !s.is_empty())
        .map(|s| s.into_owned())
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = tag
        .and_then(|t| t.artist())
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let album = tag
        .and_then(|t| t.album())
        .filter(|s| !s.is_empty())
        .map(|s| s.into_owned())
        .unwrap_or_else(|| "Unknown album".into());
    let album_artist = tag
        .and_then(|t| t.get_string(ItemKey::AlbumArtist))
        .filter(|s| !s.is_empty())
        .unwrap_or(&artist)
        .to_owned();
    let properties = file.properties();
    Ok(Track {
        id: TrackId(0),
        path: path.to_owned(),
        title,
        artist,
        album,
        album_artist,
        album_id: AlbumId(0),
        track_no: tag.and_then(|t| t.track()),
        disc_no: tag.and_then(|t| t.disk()),
        year: tag.and_then(|t| t.date()).map(|d| u32::from(d.year)),
        duration: (!properties.duration().is_zero()).then_some(properties.duration()),
        format: AudioFormat {
            codec: codec.into(),
            lossless,
            bit_depth: properties.bit_depth(),
            sample_rate: properties.sample_rate(),
            channels: properties.channels(),
            bitrate_kbps: properties.audio_bitrate(),
        },
        has_cover: picture(&file).is_some() || adjacent(path)?.is_some(),
    })
}

pub(crate) fn adjacent(path: &Path) -> io::Result<Option<(PathBuf, &'static str)>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let Some(order) = ["cover", "folder", "front", "album"]
            .iter()
            .position(|s| *s == stem)
        else {
            continue;
        };
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            _ => continue,
        };
        candidates.push((order, path, mime));
    }
    candidates.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, path, mime)| (path, mime)))
}

pub(crate) fn cover(path: &Path) -> Result<Option<Cover>> {
    let parsed = read(path);
    if let Ok((file, _, _)) = &parsed
        && let Some(pic) = picture(file)
    {
        return Ok(Some(Cover {
            data: pic.data().to_vec(),
            mime: pic
                .mime_type()
                .map_or("application/octet-stream", |m| m.as_str())
                .to_owned(),
        }));
    }
    if let Some((path, mime)) = adjacent(path)? {
        return Ok(Some(Cover {
            data: fs::read(path)?,
            mime: mime.into(),
        }));
    }
    parsed?;
    Ok(None)
}
