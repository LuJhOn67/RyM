//! "Up next" from YouTube Music's radio for a song (`/next` with `RDAMVM<id>`), tidied: no
//! repeats of the same song (also under another upload), nothing already played or queued this
//! session, long videos kept (whole albums and mixes are fine), and songs by the same artist as
//! the one playing (or ones heard this session) moved up a little, so the mood holds.

use std::collections::HashSet;

use crate::api::Track;

/// How many places a song by the playing artist moves up; one heard this session, one place.
const SAME_ARTIST_LIFT: usize = 3;
const SESSION_ARTIST_LIFT: usize = 1;

/// "Song (Official Video) [HD]" by "A, B" -> "song|a": the same song under another upload.
fn key(t: &Track) -> String {
    let mut title = t.title.to_lowercase();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let Some(a) = title.find(open) {
            let Some(b) = title[a..].find(close) else { break };
            title.replace_range(a..a + b + close.len_utf8(), "");
        }
    }
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{title}|{}", first_artist(&t.artist))
}

fn first_artist(artist: &str) -> String {
    artist.split([',', '&']).next().unwrap_or_default().trim().to_lowercase()
}

/// The radio for `seed`, tidied (see the module notes). `played` and `queued` are video ids;
/// `session_artists` are first artists heard this session (lower case).
pub fn tidy(radio: Vec<Track>, seed: &Track, played: &HashSet<String>, queued: &HashSet<String>, session_artists: &HashSet<String>) -> Vec<Track> {
    let mut seen_keys: HashSet<String> = HashSet::from([key(seed)]);
    let mut seen_ids: HashSet<String> = HashSet::from([seed.video_id.clone()]);
    let seed_artist = first_artist(&seed.artist);
    let mut kept: Vec<(usize, Track)> = Vec::new();
    for t in radio {
        if t.video_id.is_empty() || played.contains(&t.video_id) || queued.contains(&t.video_id) {
            continue;
        }
        if !seen_ids.insert(t.video_id.clone()) || !seen_keys.insert(key(&t)) {
            continue;
        }
        let artist = first_artist(&t.artist);
        let lift = if !seed_artist.is_empty() && artist == seed_artist {
            SAME_ARTIST_LIFT
        } else if session_artists.contains(&artist) {
            SESSION_ARTIST_LIFT
        } else {
            0
        };
        // Ranked by YouTube's order, lifted a few places; earlier stays first on a tie.
        let place = kept.len();
        kept.push((place.saturating_sub(lift) * 2 + usize::from(lift == 0), t));
    }
    kept.sort_by_key(|(rank, _)| *rank);
    kept.into_iter().map(|(_, t)| t).collect()
}

/// First artists of `tracks` (lower case), for `session_artists`.
pub fn artists_of<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> HashSet<String> {
    tracks.into_iter().map(|t| first_artist(&t.artist)).filter(|a| !a.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str, artist: &str) -> Track {
        Track {
            video_id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: "1:02:03".into(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    #[test]
    fn repeats_and_played_songs_go_same_artist_moves_up() {
        let seed = t("s", "Seed", "Band");
        let radio = vec![
            t("s", "Seed", "Band"),
            t("a", "Alpha", "Other"),
            t("b", "Beta (Official Video)", "Other"),
            t("b2", "Beta [HD]", "Other"),
            t("c", "Gamma", "Someone"),
            t("d", "Delta", "Band"),
            t("p", "Played", "X"),
            t("q", "Queued", "Y"),
        ];
        let played = HashSet::from(["p".to_string()]);
        let queued = HashSet::from(["q".to_string()]);
        let out = tidy(radio, &seed, &played, &queued, &HashSet::new());
        let ids: Vec<&str> = out.iter().map(|t| t.video_id.as_str()).collect();
        // b2 is the same song as b; d (same artist as the seed) moved up three places.
        assert_eq!(ids, ["d", "a", "b", "c"]);
        // Long videos (an hour-long mix) stay.
        assert!(out.iter().all(|t| t.duration == "1:02:03"));
    }
}
