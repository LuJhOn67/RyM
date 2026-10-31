//! SoundCloud plugin: SoundCloud's songs searched, played and downloaded through yt-dlp (a search,
//! or a SoundCloud link: a song, a set or an artist's page). A SoundCloud song is a [`Track`]
//! whose id is `sc.` and SoundCloud's number for it, so the queue, playback, downloads and
//! lyrics handle it like any other song.

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::api::Track;

pub const PREFIX: &str = "sc.";

/// Songs a link or a search gives at most.
const MOST: usize = 100;

pub fn is_soundcloud(id: &str) -> bool {
    id.strip_prefix(PREFIX).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The link yt-dlp opens for a SoundCloud song.
pub fn url(id: &str) -> Option<String> {
    is_soundcloud(id).then(|| format!("https://api.soundcloud.com/tracks/{}", &id[PREFIX.len()..]))
}

/// A link anyone can open: SoundCloud's player with the song.
pub fn share_link(id: &str) -> Option<String> {
    is_soundcloud(id).then(|| format!("https://w.soundcloud.com/player/?url=https%3A//api.soundcloud.com/tracks/{}", &id[PREFIX.len()..]))
}

/// A SoundCloud page: a song, a set, an artist.
pub fn is_link(text: &str) -> bool {
    let t = text.trim().trim_start_matches("https://").trim_start_matches("http://");
    ["soundcloud.com/", "m.soundcloud.com/", "on.soundcloud.com/", "www.soundcloud.com/"].iter().any(|p| t.starts_with(p))
}

/// Songs on SoundCloud for a search.
pub async fn search(query: String) -> Result<Vec<Track>> {
    list(&format!("scsearch40:{query}")).await
}

/// The songs behind a SoundCloud link (the first 100 of a set or an artist's page).
pub async fn open_link(link: String) -> Result<Vec<Track>> {
    list(link.trim()).await
}

async fn list(what: &str) -> Result<Vec<Track>> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args(["--flat-playlist", "-J", "--no-warnings", "--playlist-end", "100"]).arg(what);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output().await.map_err(|e| anyhow!("yt-dlp not found (pip install -U yt-dlp): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("{}", err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed")));
    }
    Ok(tracks(&serde_json::from_slice(&out.stdout)?))
}

/// yt-dlp's answer as songs: a list's entries, or the one song a song link gives.
fn tracks(info: &Value) -> Vec<Track> {
    let page_owner = info["uploader"].as_str().unwrap_or_default();
    match info["entries"].as_array() {
        Some(entries) => entries.iter().filter_map(|e| track(e, page_owner)).take(MOST).collect(),
        None => track(info, "").into_iter().collect(),
    }
}

/// One song. SoundCloud Go+ songs (only a 30-second preview here) and sets are left out.
fn track(e: &Value, page_owner: &str) -> Option<Track> {
    let url = e["url"].as_str().or(e["webpage_url"].as_str()).unwrap_or_default();
    if url.contains("/sets/") {
        return None;
    }
    let id = match &e["id"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let secs = e["duration"].as_f64();
    if secs.is_some_and(|s| s <= 30.5) {
        return None;
    }
    let artist = e["artists"][0].as_str().or(e["uploader"].as_str()).unwrap_or(page_owner).to_string();
    // The biggest picture is the original upload; 500 px is plenty.
    let thumb = e["thumbnails"].as_array().and_then(|t| t.last()).and_then(|t| t["url"].as_str()).map(|u| u.replace("-original.", "-t500x500."));
    let track = Track {
        video_id: format!("{PREFIX}{id}"),
        title: e["title"].as_str()?.to_string(),
        artist,
        album: "SoundCloud".into(),
        duration: secs.map(|s| crate::fmt_time(s as f32)).unwrap_or_default(),
        thumb,
        album_id: None,
        artist_id: None,
        link: None,
    };
    is_soundcloud(&track.video_id).then_some(track)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_links_and_answers() {
        assert!(is_soundcloud("sc.1886998806"));
        assert!(!is_soundcloud("dQw4w9WgXcQ") && !is_soundcloud("sc.") && !is_soundcloud("sc.12a"));
        assert_eq!(url("sc.42").as_deref(), Some("https://api.soundcloud.com/tracks/42"));
        assert!(is_link("https://soundcloud.com/levity_music/tameflip") && is_link("on.soundcloud.com/x"));
        assert!(!is_link("https://music.youtube.com/watch?v=x"));
        let answer = serde_json::json!({
            "uploader": "Page",
            "entries": [
                {"id": "1", "title": "Full song", "uploader": "Someone", "artists": ["Band"], "duration": 200.5,
                 "url": "https://api.soundcloud.com/tracks/1", "thumbnails": [{"url": "https://i1.sndcdn.com/a-original.jpg"}]},
                {"id": "2", "title": "Go+ preview", "duration": 30.0, "url": "https://api.soundcloud.com/tracks/2"},
                {"id": "3", "title": "A set", "url": "https://soundcloud.com/x/sets/y"},
                {"id": "4", "title": "From a page", "url": "https://soundcloud.com/x/z"}
            ]
        });
        let got = tracks(&answer);
        assert_eq!(got.iter().map(|t| t.video_id.as_str()).collect::<Vec<_>>(), ["sc.1", "sc.4"]);
        assert_eq!((got[0].artist.as_str(), got[0].duration.as_str()), ("Band", "3:20"));
        assert_eq!(got[0].thumb.as_deref(), Some("https://i1.sndcdn.com/a-t500x500.jpg"));
        assert_eq!(got[1].artist, "Page");
    }

    /// Search, then the first song through the player's own path (into the cache) and decoded.
    #[tokio::test]
    #[ignore]
    async fn search_fetch_and_decode() {
        let found = search("tame impala less i know flip".into()).await.unwrap();
        println!("{} songs, first: {} - {} ({})", found.len(), found[0].artist, found[0].title, found[0].video_id);
        let path = crate::stream::fetch(found[0].video_id.clone()).await.unwrap();
        let secs = crate::audio::decode_seconds(&path).unwrap();
        println!("{} decoded {secs:.1} s", path.display());
        assert!(secs > 60.0);
    }

    /// A downloaded SoundCloud song (HLS AAC joined by yt-dlp) decodes: RYM_SC_FILE=<path>.
    #[test]
    #[ignore]
    fn soundcloud_file_decodes() {
        let path = std::env::var("RYM_SC_FILE").expect("RYM_SC_FILE");
        let secs = crate::audio::decode_seconds(std::path::Path::new(&path)).unwrap();
        println!("decoded {secs:.1} s");
        assert!(secs > 60.0);
    }
}
