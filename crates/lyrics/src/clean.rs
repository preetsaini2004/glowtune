/// Track metadata normalized for a lyrics lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Track title without common video and release annotations.
    pub title: String,
    /// First artist, without YouTube channel suffixes.
    pub artist: String,
}

/// Cleans common YouTube and Spotify title annotations while preserving meaningful subtitles.
pub fn clean_query(title: &str, artist: &str) -> Query {
    let mut artist = artist.trim();
    // Video titles: "Song | Artist | Label" → the part that isn't the artist.
    let piped = pipe_part(title.trim(), artist);
    let mut title = piped.as_str();
    // "Artist - Song", "Artist : Song", "Artist – Song".
    if let Some((prefix, track)) = split_artist(title) {
        let channel = strip_channel(artist).to_lowercase();
        // The lead artist ("Karan Aujla" of "Karan Aujla, Ikky").
        let lead = channel
            .split([',', '&'])
            .next()
            .unwrap_or_default()
            .split(" x ")
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let p = prefix.trim().to_lowercase();
        if artist.is_empty()
            || artist.to_ascii_lowercase().ends_with("vevo")
            || channel == p
            || (lead.chars().count() >= 3 && (p == lead || p.contains(&lead)))
        {
            artist = prefix.trim();
            title = track.trim();
        }
    }
    // Credits after the video tag ("Song (Official Video) Producer : Label").
    let mut title = cut_at_video_tag(title);
    if let Some((first, _)) = title.split_once(" : ")
        && !first.trim().is_empty()
    {
        title = first.trim().to_owned();
    }
    loop {
        let before = title.clone();
        title = remove_groups(&title);
        let lower = title.to_ascii_lowercase();
        if let Some(index) = [" feat. ", " ft. "]
            .iter()
            .filter_map(|marker| lower.find(marker))
            .min()
        {
            title.truncate(index);
        }
        if let Some((prefix, suffix)) = title.rsplit_once(" - ")
            && is_remaster(suffix)
        {
            title = prefix.to_owned();
        }
        title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title == before {
            break;
        }
    }
    artist = strip_channel(artist);
    if let Some(index) = [",", " & ", " x "]
        .iter()
        .filter_map(|marker| artist.find(marker))
        .min()
    {
        artist = &artist[..index];
    }
    Query {
        title,
        artist: artist.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// For "Song | Artist | Label" titles: the first part that isn't the artist.
fn pipe_part(title: &str, artist: &str) -> String {
    if !title.contains(" | ") {
        return title.to_owned();
    }
    let channel = strip_channel(artist).to_lowercase();
    title
        .split(" | ")
        .map(str::trim)
        .find(|part| !part.is_empty() && (channel.is_empty() || part.to_lowercase() != channel))
        .unwrap_or(title)
        .to_owned()
}

/// The first "artist ⟨sep⟩ track" split, for " - ", " – ", " — " and " : ".
fn split_artist(title: &str) -> Option<(&str, &str)> {
    [" - ", " – ", " — ", " : "]
        .iter()
        .filter_map(|sep| title.find(sep).map(|i| (i, sep.len())))
        .min_by_key(|(i, _)| *i)
        .map(|(i, len)| (&title[..i], &title[i + len..]))
}

/// Keeps what comes before a "(Official Video)"-style tag, if anything does.
fn cut_at_video_tag(title: &str) -> String {
    let mut search = 0;
    while let Some(rel) = title[search..].find(['(', '[']) {
        let start = search + rel;
        let close = if title[start..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(end_rel) = title[start..].find(close) else {
            break;
        };
        let tag = title[start + 1..start + end_rel].to_ascii_lowercase();
        if [
            "official",
            "video",
            "visualizer",
            "visualiser",
            "audio",
            "lyric",
        ]
        .iter()
        .any(|w| tag.contains(w))
            && !title[..start].trim().is_empty()
        {
            return title[..start].trim().to_owned();
        }
        search = start + end_rel + 1;
    }
    title.to_owned()
}

fn strip_channel(artist: &str) -> &str {
    let mut artist = artist.trim();
    for suffix in [" - topic", "vevo"] {
        if artist.to_ascii_lowercase().ends_with(suffix) {
            artist = artist[..artist.len() - suffix.len()].trim_end();
        }
    }
    artist
}

fn is_remaster(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let words: Vec<_> = lower.split_whitespace().collect();
    let year = |word: &str| word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit());
    match words.as_slice() {
        ["remaster" | "remastered"] => true,
        ["remaster" | "remastered", value] | [value, "remaster" | "remastered"] => year(value),
        _ => false,
    }
}

fn remove_groups(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(['(', '[']) {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let close = if rest.starts_with('(') { ')' } else { ']' };
        let Some(end) = rest.find(close) else { break };
        let tag = rest[1..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        let trailing = rest[end + 1..].trim().is_empty();
        let noise = matches!(
            tag.as_str(),
            "official video"
                | "official music video"
                | "official audio"
                | "lyrics"
                | "lyric video"
                | "visualizer"
                | "audio"
                | "hd"
                | "4k"
        ) || is_remaster(&tag)
            || (tag == "live" && trailing)
            || ((tag.starts_with("feat. ") || tag.starts_with("ft. ")) && trailing);
        if !noise {
            output.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_world_queries() {
        for (title, artist, expected_title, expected_artist) in [
            (
                "Blinding Lights (Official Video)",
                "The Weeknd",
                "Blinding Lights",
                "The Weeknd",
            ),
            (
                "Adele - Hello (Official Music Video)",
                "AdeleVEVO",
                "Hello",
                "Adele",
            ),
            (
                "Anti-Hero [Official Audio]",
                "TaylorSwiftVEVO",
                "Anti-Hero",
                "TaylorSwift",
            ),
            ("Yellow (Lyrics)", "Coldplay - Topic", "Yellow", "Coldplay"),
            ("Numb (Lyric Video)", "Linkin Park", "Numb", "Linkin Park"),
            ("Midnight City (Visualizer)", "M83", "Midnight City", "M83"),
            (
                "  Intro   (Audio) [HD] (4K) ",
                " The xx ",
                "Intro",
                "The xx",
            ),
            (
                "Come Together (Remastered 2011)",
                "The Beatles",
                "Come Together",
                "The Beatles",
            ),
            (
                "Heroes - Remastered 2011",
                "David Bowie",
                "Heroes",
                "David Bowie",
            ),
            (
                "Dreams - 2015 Remaster",
                "Fleetwood Mac",
                "Dreams",
                "Fleetwood Mac",
            ),
            ("Fix You (Live)", "Coldplay", "Fix You", "Coldplay"),
            ("Song (feat. Guest)", "A, B", "Song", "A"),
            ("Song (ft. Guest)", "A & B", "Song", "A"),
            ("Song feat. Guest", "A x B", "Song", "A"),
            (
                "Daft Punk - One More Time [OFFICIAL AUDIO]",
                "",
                "One More Time",
                "Daft Punk",
            ),
            ("Muse - Uprising", "Muse", "Uprising", "Muse"),
            ("Song (LIVE) (Official Video)", "Artist", "Song", "Artist"),
            (
                "Beyoncé - Halo (Official Video)",
                "BeyoncéVEVO",
                "Halo",
                "Beyoncé",
            ),
        ] {
            assert_eq!(
                clean_query(title, artist),
                Query {
                    title: expected_title.into(),
                    artist: expected_artist.into()
                },
                "{title}"
            );
        }
    }

    #[test]
    fn preserves_meaningful_groups_and_non_channel_hyphens() {
        for title in [
            "Song (Live) at Wembley",
            "Sweet Dreams (Are Made of This)",
            "Song [Acoustic]",
            "Artist - Song",
            "Song (unfinished",
            "東京 (愛)",
            "Song - Remastered tomorrow",
        ] {
            assert_eq!(clean_query(title, "Other Artist").title, title);
        }
        assert_eq!(clean_query("Song", "A, B & C").artist, "A");
        assert_eq!(clean_query("", "").title, "");
    }
}

#[cfg(test)]
mod video_title_tests {
    use super::*;

    #[test]
    fn youtube_music_video_titles() {
        for (title, artist, expected_title, expected_artist) in [
            (
                "Diljit Dosanjh : Devil (Official Video) Intense : Hizzy : I'm an Artist Bro",
                "Diljit Dosanjh",
                "Devil",
                "Diljit Dosanjh",
            ),
            (
                "Racks on Racks (Official Visuals ) Navaan Sandhu | Rxtro | New Punjabi Songs 2025",
                "Navaan Sandhu",
                "Racks on Racks",
                "Navaan Sandhu",
            ),
            ("Shubh – Bars (Official Audio)", "Shubh", "Bars", "Shubh"),
            (
                "Expert Jatt | Nawab | Official Video",
                "Nawab",
                "Expert Jatt",
                "Nawab",
            ),
            (
                "MF Gabhru!",
                "Karan Aujla & Ikky",
                "MF Gabhru!",
                "Karan Aujla",
            ),
        ] {
            assert_eq!(
                clean_query(title, artist),
                Query {
                    title: expected_title.into(),
                    artist: expected_artist.into()
                },
                "{title}"
            );
        }
    }
}
