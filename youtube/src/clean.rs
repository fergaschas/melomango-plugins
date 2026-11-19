//! Turns YouTube video titles into (artist, song title).

/// Words that mark a bracketed group as noise, e.g. "(Official Video)" or "[Lyrics]".
const NOISE: &[&str] = &[
    "official", "oficial", "video", "videoclip", "audio", "lyric", "lyrics", "letra", "visualizer",
    "visualiser", "mv", "hd", "hq", "4k", "clip", "explicit",
];

/// "Shakira, Burna Boy - Dai Dai (Official Video) | Shakira" -> ("Shakira, Burna Boy", "Dai Dai").
/// Without an "Artist - Title" pattern, the channel name is used as the artist.
pub fn artist_and_title(raw: &str, channel: &str) -> (String, String) {
    let base = raw.split(" | ").next().unwrap_or(raw);
    let base = strip_noise_groups(base);
    if let Some((artist, title)) = base.split_once(" - ") {
        let (artist, title) = (artist.trim(), title.trim());
        if !artist.is_empty() && !title.is_empty() {
            return (artist.to_string(), title.to_string());
        }
    }
    (clean_channel(channel), base)
}

/// "Bad Bunny - Topic" -> "Bad Bunny", "ShakiraVEVO" -> "Shakira".
fn clean_channel(channel: &str) -> String {
    let name = channel.strip_suffix(" - Topic").unwrap_or(channel);
    let name = name.strip_suffix("VEVO").unwrap_or(name);
    name.trim().to_string()
}

fn strip_noise_groups(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest[open..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[open..].find(close_char) else { break };
        let close = open + len;
        let inner = rest[open + 1..close].to_lowercase();
        let is_noise = inner
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| NOISE.contains(&word));
        out.push_str(&rest[..open]);
        if !is_noise {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: &str, channel: &str) -> (String, String) {
        artist_and_title(raw, channel)
    }

    #[test]
    fn splits_artist_and_title_and_drops_noise() {
        assert_eq!(
            at("Shakira, Burna Boy - Dai Dai (Official Video) | Shakira", "Shakira"),
            ("Shakira, Burna Boy".into(), "Dai Dai".into())
        );
        assert_eq!(
            at("BAD BUNNY - NUEVAYoL (Video Oficial) | DeBÍ TiRAR MÁS FOToS", "Bad Bunny"),
            ("BAD BUNNY".into(), "NUEVAYoL".into())
        );
    }

    #[test]
    fn keeps_meaningful_groups() {
        assert_eq!(
            at("Artist - Song (feat. Someone) [Remix]", "x"),
            ("Artist".into(), "Song (feat. Someone) [Remix]".into())
        );
    }

    #[test]
    fn falls_back_to_channel() {
        assert_eq!(at("Lose Yourself", "Eminem - Topic"), ("Eminem".into(), "Lose Yourself".into()));
        assert_eq!(at("Hips Don't Lie [HD]", "ShakiraVEVO"), ("Shakira".into(), "Hips Don't Lie".into()));
    }

    #[test]
    fn noise_detection_matches_whole_words() {
        // "Shadow" contains "hd"-like letters but is not the word "hd".
        assert_eq!(at("Song (Shadow Mix)", "x"), ("x".into(), "Song (Shadow Mix)".into()));
    }
}
