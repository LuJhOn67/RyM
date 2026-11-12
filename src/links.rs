//! "Play in RyM": links sent to RyM from outside (the command line, a `rym://` link from the
//! browser, another copy of RyM handing one over) and what they point at. The same as the phone
//! app's `data/YouTubeLinks.kt`.
//!
//! - YouTube Music and YouTube: `music.youtube.com/watch?v=…`, `youtu.be/…`, `youtube.com/watch`,
//!   `/playlist?list=…`, `/browse/MPREb…` (an album), `/channel/UC…` (an artist), shorts.
//! - `rym://play?url=<a link>`, `rym://play?v=<video id>`, `rym://play?list=<list id>`.
//! - Spotify and SoundCloud (their plugins): one song is played, a list is opened.

use url::Url;

/// What a link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Song(String),
    Playlist(String),
    /// A mix made for a song ("RD…").
    Mix(String),
    Album(String),
    Artist(String),
    /// A Spotify link; `one_song`: a song (played), not an album or a playlist (opened).
    Spotify { link: String, one_song: bool },
    /// A SoundCloud link; `one_song`: a song's page, not a set or an artist.
    SoundCloud { link: String, one_song: bool },
}

impl Target {
    /// Played right away (a song), not opened as a list.
    pub fn plays(&self) -> bool {
        matches!(self, Target::Song(_) | Target::Spotify { one_song: true, .. } | Target::SoundCloud { one_song: true, .. })
    }
}

/// RyM's page for shared songs (GitHub Pages, `docs/play/`): it opens the song in RyM.
pub const PLAY_PAGE: &str = "https://lujhon67.github.io/RyM/play/";

const YOUTUBE: [&str; 6] = ["music.youtube.com", "youtube.com", "www.youtube.com", "m.youtube.com", "youtu.be", "www.youtu.be"];

fn is_video_id(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A list id: a mix ("RD…", made for a song) or a playlist (YouTube Music's own "RDCLAK…" are playlists).
fn list_target(id: &str) -> Option<Target> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    Some(if id.starts_with("RD") && !id.starts_with("RDCLAK") { Target::Mix(id.into()) } else { Target::Playlist(id.into()) })
}

/// Given to RyM on its command line to play (not a file, not a `--switch`).
pub fn is_incoming(arg: &str) -> bool {
    let a = arg.trim().to_ascii_lowercase();
    ["rym:", "http://", "https://", "spotify:"].iter().any(|p| a.starts_with(p))
}

/// The first link in shared text (apps share "Song name – https://…").
pub fn first_link(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let at = ["rym://", "https://", "http://", "spotify:"].iter().filter_map(|p| lower.find(p)).min()?;
    let link: String = text[at..].chars().take_while(|c| !c.is_whitespace()).collect();
    Some(link.trim_end_matches(['.', ',', ')', '"', '\'']).to_string())
}

/// What a link (or text with one in it) points at; None when RyM can't play it.
pub fn parse(text: &str) -> Option<Target> {
    parse_at(&first_link(text)?, 0)
}

fn parse_at(link: &str, depth: u8) -> Option<Target> {
    if crate::spotify::is_link(link) {
        let one_song = crate::spotify::parse_link(link).is_some_and(|(kind, _)| kind == "track");
        return Some(Target::Spotify { link: link.to_string(), one_song });
    }
    if crate::soundcloud::is_link(link) {
        return Some(Target::SoundCloud { link: link.to_string(), one_song: soundcloud_song(link) });
    }
    let url = Url::parse(link).ok()?;
    let query = |k: &str| url.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.into_owned());
    if url.scheme() == "rym" {
        // rym://play?url=… (one level only), rym://play?v=…, rym://play?list=…, rym://play/<id>.
        if let Some(inner) = query("url").filter(|_| depth == 0) {
            return parse_at(inner.trim(), depth + 1);
        }
        if let Some(v) = query("v").filter(|v| is_video_id(v)) {
            return Some(Target::Song(v));
        }
        if let Some(list) = query("list") {
            return list_target(&list);
        }
        return url.path_segments()?.find(|s| !s.is_empty()).filter(|s| is_video_id(s)).map(|s| Target::Song(s.into()));
    }
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    // RyM's own share page: ?v=<video id> or ?list=<list id>.
    if host == "lujhon67.github.io" && url.path().to_ascii_lowercase().starts_with("/rym/play") {
        if let Some(v) = query("v").filter(|v| is_video_id(v)) {
            return Some(Target::Song(v));
        }
        return query("list").as_deref().and_then(list_target);
    }
    if !YOUTUBE.contains(&host.as_str()) {
        return None;
    }
    let segments: Vec<&str> = url.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();
    if host.ends_with("youtu.be") {
        return segments.first().filter(|s| is_video_id(s)).map(|s| Target::Song(s.to_string()));
    }
    let list = query("list");
    match segments.first().copied() {
        Some("watch") => query("v").filter(|v| is_video_id(v)).map(Target::Song).or_else(|| list.as_deref().and_then(list_target)),
        Some("shorts" | "live" | "embed") => segments.get(1).filter(|s| is_video_id(s)).map(|s| Target::Song(s.to_string())),
        Some("playlist") => list.as_deref().and_then(list_target),
        Some("browse") => {
            let id = segments.get(1)?;
            if id.starts_with("MPREb") {
                Some(Target::Album(id.to_string()))
            } else if let Some(l) = id.strip_prefix("VL") {
                list_target(l)
            } else if id.starts_with("UC") {
                Some(Target::Artist(id.to_string()))
            } else {
                None
            }
        }
        Some("channel") => segments.get(1).filter(|s| s.starts_with("UC")).map(|s| Target::Artist(s.to_string())),
        _ => None,
    }
}

/// soundcloud.com/<artist>/<song>: one song (not a set, the artist's likes or their songs).
fn soundcloud_song(link: &str) -> bool {
    let Ok(url) = Url::parse(link.trim()) else { return false };
    if url.host_str().is_some_and(|h| h.starts_with("on.")) {
        return false;
    }
    let parts: Vec<&str> = url.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();
    parts.len() == 2 && !["sets", "likes", "tracks", "albums", "reposts", "popular-tracks", "followers", "following"].contains(&parts[1])
}

#[cfg(test)]
mod tests {
    use super::Target::*;
    use super::*;

    #[test]
    fn what_links_point_at() {
        assert_eq!(parse("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=x"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("Never Gonna – https://youtu.be/dQw4w9WgXcQ?si=abc."), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("https://www.youtube.com/shorts/dQw4w9WgXcQ"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("https://music.youtube.com/playlist?list=PL123abc"), Some(Playlist("PL123abc".into())));
        assert_eq!(parse("https://music.youtube.com/playlist?list=RDCLAK5uy_abc"), Some(Playlist("RDCLAK5uy_abc".into())));
        assert_eq!(parse("https://music.youtube.com/watch?list=RDAMVMdQw4w9WgXcQ"), Some(Mix("RDAMVMdQw4w9WgXcQ".into())));
        assert_eq!(parse("https://music.youtube.com/browse/MPREb_abc"), Some(Album("MPREb_abc".into())));
        assert_eq!(parse("https://music.youtube.com/channel/UCabc"), Some(Artist("UCabc".into())));
        assert_eq!(parse("https://example.com/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(parse("just words"), None);
    }

    #[test]
    fn rym_share_page() {
        assert_eq!(parse("Song - Artist\nhttps://lujhon67.github.io/RyM/play/?v=dQw4w9WgXcQ&t=Song%20-%20Artist"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("https://lujhon67.github.io/RyM/play/?list=PLxyz"), Some(Playlist("PLxyz".into())));
        let t = crate::api::Track { video_id: "dQw4w9WgXcQ".into(), title: "Never Gonna".into(), artist: "Rick & co".into(), album: String::new(), duration: String::new(), thumb: None, album_id: None, artist_id: None, link: None };
        let link = crate::queue::share_link(&t);
        assert_eq!(link, "https://lujhon67.github.io/RyM/play/?v=dQw4w9WgXcQ&t=Never+Gonna+-+Rick+%26+co");
        assert_eq!(parse(&link), Some(Song("dQw4w9WgXcQ".into())));
    }

    #[test]
    fn rym_links() {
        assert_eq!(parse("rym://play?url=https%3A%2F%2Fmusic.youtube.com%2Fwatch%3Fv%3DdQw4w9WgXcQ"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("rym://play?v=dQw4w9WgXcQ"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("rym://play/dQw4w9WgXcQ"), Some(Song("dQw4w9WgXcQ".into())));
        assert_eq!(parse("rym://play?list=PLxyz"), Some(Playlist("PLxyz".into())));
        // Not twice: rym:// inside rym:// is refused.
        assert_eq!(parse("rym://play?url=rym%3A%2F%2Fplay%3Furl%3Dx"), None);
        assert_eq!(parse("rym://play?v=not-an-id"), None);
        assert!(is_incoming("rym://play?v=x") && is_incoming("https://youtu.be/x") && !is_incoming("--login") && !is_incoming("C:\\song.mp3"));
    }

    #[test]
    fn spotify_and_soundcloud() {
        assert_eq!(parse("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=1"), Some(Spotify { link: "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=1".into(), one_song: true }));
        assert!(!parse("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M").unwrap().plays());
        assert!(parse("https://soundcloud.com/artist/a-song").unwrap().plays());
        assert!(!parse("https://soundcloud.com/artist/sets/a-set").unwrap().plays());
        assert!(!parse("https://soundcloud.com/artist").unwrap().plays());
    }
}
