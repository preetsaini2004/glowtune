use std::time::Duration;

/// One lyric line, including empty lines that mark instrumental gaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Playback time when this line becomes active.
    pub time: Duration,
    /// Trimmed text without enhanced LRC timing tags.
    pub text: String,
    /// Per-word timing from enhanced LRC (`<mm:ss.xx>word`); empty when the
    /// lyrics only time whole lines.
    pub words: Vec<Word>,
}

/// One timed word of an enhanced-LRC line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Playback time when this word starts being sung.
    pub time: Duration,
    /// The word (trimmed).
    pub text: String,
}

/// Parsed lyrics; synced lines are ordered by timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lyrics {
    /// Lyric lines, sorted stably by time when parsed from LRC.
    pub lines: Vec<Line>,
    /// Whether the lyrics contain usable timestamps.
    pub synced: bool,
}

impl Lyrics {
    /// Parses LRC, ignoring metadata and malformed tag lines.
    /// Positive file offsets move timestamps earlier. Untimed input becomes plain lyrics.
    pub fn parse_lrc(text: &str) -> Lyrics {
        let text = text.trim_start_matches('\u{feff}');
        let offset = text
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("[offset:")?
                    .strip_suffix(']')?
                    .trim()
                    .parse::<i64>()
                    .ok()
            })
            .next_back()
            .unwrap_or_default();
        let mut lines = Vec::new();
        let mut plain = Vec::new();
        for raw in text.lines() {
            let mut rest = raw.trim();
            if rest.starts_with('#') {
                continue;
            }
            if !rest.starts_with('[') {
                plain.push(rest);
                continue;
            }
            let mut times = Vec::new();
            while let Some(tag) = rest.strip_prefix('[') {
                let Some((tag, tail)) = tag.split_once(']') else {
                    break;
                };
                let Some(time) = timestamp(tag) else { break };
                times.push(time);
                rest = tail;
            }
            // A malformed prefix must not become visible lyric text.
            if times.is_empty() || rest.starts_with('[') {
                continue;
            }
            let text = strip_word_tags(rest).trim().to_owned();
            let shift = Duration::from_millis(offset.unsigned_abs());
            let adjust = |time: Duration| {
                if offset >= 0 {
                    time.saturating_sub(shift)
                } else {
                    time.saturating_add(shift)
                }
            };
            let words: Vec<Word> = word_tags(rest)
                .into_iter()
                .map(|w| Word {
                    time: adjust(w.time),
                    text: w.text,
                })
                .collect();
            for time in times {
                lines.push(Line {
                    time: adjust(time),
                    text: text.clone(),
                    words: words.clone(),
                });
            }
        }
        if lines.is_empty() {
            return Self::from_plain(&plain.join("\n"));
        }
        lines.sort_by_key(|line| line.time);
        Lyrics {
            lines,
            synced: true,
        }
    }

    /// Creates untimed lyrics, trimming each line and handling BOM and CRLF.
    pub fn from_plain(text: &str) -> Lyrics {
        Lyrics {
            lines: text
                .trim_start_matches('\u{feff}')
                .lines()
                .map(|text| Line {
                    time: Duration::ZERO,
                    text: text.trim().to_owned(),
                    words: Vec::new(),
                })
                .collect(),
            synced: false,
        }
    }

    /// Finds the active line in logarithmic time; equal timestamps select the last line.
    /// Returns `None` for plain lyrics or positions before the first line.
    pub fn line_at(&self, position: Duration) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines
            .partition_point(|line| line.time <= position)
            .checked_sub(1)
    }

    /// Returns clamped progress until the next line, assuming five seconds for the last line.
    /// Invalid indices and plain lyrics return zero; zero-length lines complete at their start.
    pub fn line_progress(&self, index: usize, position: Duration) -> f32 {
        let Some(line) = self.lines.get(index).filter(|_| self.synced) else {
            return 0.0;
        };
        if position < line.time {
            return 0.0;
        }
        let span = self
            .lines
            .get(index + 1)
            .map_or(Duration::from_secs(5), |next| {
                next.time.saturating_sub(line.time)
            });
        if span.is_zero() {
            return 1.0;
        }
        (position.saturating_sub(line.time).as_secs_f64() / span.as_secs_f64()).clamp(0.0, 1.0)
            as f32
    }

    /// Whether there are no lyric lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn timestamp(tag: &str) -> Option<Duration> {
    let (minutes, seconds) = tag.split_once(':')?;
    if !(1..=3).contains(&minutes.len()) || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (seconds, fraction) = seconds
        .split_once(['.', ':'])
        .map_or((seconds, None), |(s, f)| (s, Some(f)));
    if seconds.len() != 2 || !seconds.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: u64 = seconds.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let millis = match fraction {
        Some(f) if (1..=3).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()) => {
            f.parse::<u64>().ok()? * 10_u64.pow(3 - f.len() as u32)
        }
        Some(_) => return None,
        None => 0,
    };
    Some(Duration::from_millis(
        (minutes.parse::<u64>().ok()? * 60 + seconds) * 1000 + millis,
    ))
}

/// Words with their own `<mm:ss.xx>` start tags (enhanced LRC). A trailing tag
/// with no text only marks where the last word ends, so it adds no word.
fn word_tags(mut text: &str) -> Vec<Word> {
    let mut words = Vec::new();
    while let Some(start) = text.find('<') {
        let after = &text[start + 1..];
        let Some(end) = after.find('>') else { break };
        let Some(time) = timestamp(&after[..end]) else {
            text = &after[end + 1..];
            continue;
        };
        let body = &after[end + 1..];
        let word_end = body.find('<').unwrap_or(body.len());
        let word = body[..word_end].trim();
        if !word.is_empty() {
            words.push(Word {
                time,
                text: word.to_owned(),
            });
        }
        text = &body[word_end..];
    }
    words
}

fn strip_word_tags(mut text: &str) -> String {
    let mut result = String::new();
    while let Some(start) = text.find('<') {
        result.push_str(&text[..start]);
        text = &text[start..];
        if let Some(end) = text.find('>')
            && timestamp(&text[1..end]).is_some()
        {
            text = &text[end + 1..];
        } else {
            result.push('<');
            text = &text[1..];
        }
    }
    result.push_str(text);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_formats_and_long_minutes() {
        for (tag, ms) in [
            ("1:02", 62000),
            ("00:12.3", 12300),
            ("00:12.34", 12340),
            ("00:12.345", 12345),
            ("00:12:34", 12340),
            ("123:04", 7384000),
        ] {
            let parsed = Lyrics::parse_lrc(&format!("[{tag}] lyric "));
            assert!(parsed.synced);
            assert_eq!(
                parsed.lines[0],
                Line {
                    time: Duration::from_millis(ms),
                    text: "lyric".into(),
                    words: Vec::new(),
                }
            );
        }
    }

    #[test]
    fn multiple_tags_stable_sort_and_empty_gaps() {
        let parsed = Lyrics::parse_lrc("[00:12.00][00:45.50] Chorus\n[00:12]Second\n[00:20]");
        assert_eq!(
            parsed
                .lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            ["Chorus", "Second", "", "Chorus"]
        );
        assert_eq!(parsed.lines[3].time, Duration::from_millis(45500));
        assert_eq!(parsed.line_at(Duration::from_secs(12)), Some(1));
    }

    #[test]
    fn offsets_apply_globally_and_clamp() {
        for (offset, expected) in [("+250", 750), ("-100", 1100), ("+2000", 0)] {
            let parsed = Lyrics::parse_lrc(&format!("[00:01]Text\n[offset:{offset}]"));
            assert_eq!(parsed.lines[0].time, Duration::from_millis(expected));
        }
    }

    #[test]
    fn metadata_bom_crlf_and_word_tags() {
        let parsed = Lyrics::parse_lrc(
            "\u{feff}[ar:Artist]\r\n[ti:Title]\r\n[al:Album]\n[by:Author]\n[length:3:00]\n[re:Editor]\n[ve:1]\n# comment\n[00:02]<00:02.00>  Hello <00:02.50>world  \r\n[bad]Oops\n[00:03][broken\n[00:99]No\n[0000:00]No",
        );
        assert_eq!(
            parsed.lines,
            vec![Line {
                time: Duration::from_secs(2),
                text: "Hello world".into(),
                words: vec![
                    Word {
                        time: Duration::from_secs(2),
                        text: "Hello".into(),
                    },
                    Word {
                        time: Duration::from_millis(2500),
                        text: "world".into(),
                    },
                ],
            }]
        );
    }

    #[test]
    fn plain_fallback_and_empty_input() {
        let parsed = Lyrics::parse_lrc("[ar:Artist]\n# comment\n First \r\nSecond");
        assert_eq!(parsed, Lyrics::from_plain("\u{feff}First\r\nSecond"));
        assert!(!parsed.synced);
        assert!(parsed.lines.iter().all(|l| l.time.is_zero()));
        assert_eq!(parsed.line_at(Duration::from_secs(5)), None);
        assert!(Lyrics::parse_lrc("").is_empty());
        assert!(Lyrics::parse_lrc("[bad]").is_empty());
    }

    #[test]
    fn lookup_and_progress_boundaries() {
        let parsed = Lyrics::parse_lrc("[00:10]A\n[00:20]B");
        assert_eq!(parsed.line_at(Duration::from_secs(9)), None);
        assert_eq!(parsed.line_at(Duration::from_secs(10)), Some(0));
        assert_eq!(parsed.line_at(Duration::MAX), Some(1));
        for (index, seconds, expected) in [
            (0, 9, 0.0),
            (0, 15, 0.5),
            (0, 21, 1.0),
            (1, 22, 0.4),
            (1, 40, 1.0),
            (2, 30, 0.0),
        ] {
            assert_eq!(
                parsed.line_progress(index, Duration::from_secs(seconds)),
                expected
            );
        }
        assert_eq!(
            Lyrics::from_plain("A").line_progress(0, Duration::from_secs(1)),
            0.0
        );
        assert_eq!(
            Lyrics::parse_lrc("[00:01]A\n[00:01]B").line_progress(0, Duration::from_secs(1)),
            1.0
        );
    }

    #[test]
    fn malformed_timestamps_and_literal_angle_text() {
        for tag in [
            "-1:00",
            "1:2",
            "1:60",
            "1:00.",
            "1:00.1234",
            "1:00:xx",
            "é:00",
        ] {
            assert!(!Lyrics::parse_lrc(&format!("[{tag}]bad")).synced);
        }
        assert_eq!(
            Lyrics::parse_lrc("[00:01]<love> <broken").lines[0].text,
            "<love> <broken"
        );
    }
}
