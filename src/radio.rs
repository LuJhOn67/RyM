//! YouTube Music's radio for a song (`/next` with `RDAMVM<id>`), the songs Up next is made of,
//! and how songs are told apart (the same song under another upload, an artist's first name).
//! Up next itself is ranked by RaM points ([`crate::ram_points::up_next`]).

use std::collections::HashSet;

use crate::api::Track;

/// YouTube Music's radio for a song. A SoundCloud song or a file on this device has none: the
/// radio of the same song found on YouTube Music by its name stands in.
pub async fn for_track(api: crate::api::Api, t: Track) -> anyhow::Result<Vec<Track>> {
    if crate::is_youtube(&t.video_id) {
        return api.radio(t.video_id).await;
    }
    let first = t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string();
    let found = api.clone().search(format!("{} {first}", t.title)).await?;
    let seed = found.into_iter().next().ok_or_else(|| anyhow::anyhow!("No radio for \"{}\"", t.title))?;
    api.radio(seed.video_id).await
}

/// The song's radio; when it has fewer than three songs by the song's artist, the artist's top
/// songs join it (at the end: RaM points moves them up).
pub async fn with_artist(api: crate::api::Api, t: Track) -> anyhow::Result<Vec<Track>> {
    let mut radio = for_track(api.clone(), t.clone()).await?;
    let artist = first_artist(&t.artist);
    let same = radio.iter().filter(|r| first_artist(&r.artist) == artist).count();
    if same < 3
        && let Some(id) = t.artist_id.clone()
        && let Ok(songs) = api.artist_songs(id).await
    {
        radio.extend(songs.into_iter().take(6));
    }
    Ok(radio)
}

/// "Song (Official Video) [HD]" by "A, B" -> "song|a": the same song under another upload.
pub fn same_song_key(t: &Track) -> String {
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

pub fn first_artist(artist: &str) -> String {
    artist.split([',', '&']).next().unwrap_or_default().trim().to_lowercase()
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
    fn same_song_under_another_upload() {
        assert_eq!(same_song_key(&t("a", "Beta (Official Video)", "Other, B")), same_song_key(&t("b", "Beta [HD]", "Other")));
        assert_ne!(same_song_key(&t("a", "Beta", "Other")), same_song_key(&t("b", "Beta", "Someone")));
    }
}
