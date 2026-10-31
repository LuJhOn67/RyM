//! Spotify plugin: the songs of a Spotify link (a song, an album or a public playlist) found on
//! YouTube Music, to play or download. Spotify's own sound is locked (DRM), so only the list of
//! songs is read, from the page Spotify gives for embedding (no account or key needed; it holds
//! the first 100 songs of a playlist).

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::api::{Api, Track};

/// A song on the Spotify list.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub title: String,
    pub artist: String,
    pub secs: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub name: String,
    pub songs: Vec<Wanted>,
}

/// "https://open.spotify.com/intl-ar/playlist/ID?si=..." or "spotify:album:ID" -> ("playlist", ID).
pub fn parse_link(text: &str) -> Option<(&'static str, String)> {
    let t = text.trim();
    let rest = t.strip_prefix("spotify:").map(|r| r.replace(':', "/")).or_else(|| {
        let r = t.trim_start_matches("https://").trim_start_matches("http://");
        r.strip_prefix("open.spotify.com/").map(String::from)
    })?;
    let mut parts = rest.split('/').filter(|p| !p.is_empty() && !p.starts_with("intl-"));
    let kind = match parts.next()? {
        "track" => "track",
        "album" => "album",
        "playlist" => "playlist",
        _ => return None,
    };
    let id: String = parts.next()?.split(['?', '#']).next()?.to_string();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some((kind, id))
}

pub fn is_link(text: &str) -> bool {
    parse_link(text).is_some()
}

/// The list behind a Spotify link.
pub async fn read(link: &str) -> Result<List> {
    let (kind, id) = parse_link(link).ok_or_else(|| anyhow!("Not a Spotify song, album or playlist link"))?;
    let html = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_embed(&html)
}

/// The song list in the embed page's data (`__NEXT_DATA__`).
fn parse_embed(html: &str) -> Result<List> {
    let start = html.find(r#"<script id="__NEXT_DATA__" type="application/json">"#).ok_or_else(|| anyhow!("Spotify's page changed: no song list"))?;
    let json = &html[start..];
    let json = &json[json.find('>').map_or(0, |i| i + 1)..];
    let json = &json[..json.find("</script>").ok_or_else(|| anyhow!("Spotify's page changed: no song list"))?];
    let data: Value = serde_json::from_str(json)?;
    let e = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let secs = |v: &Value| v.as_f64().filter(|ms| *ms > 0.0).map(|ms| (ms / 1000.0) as f32);
    let songs: Vec<Wanted> = match e["trackList"].as_array() {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(|t| Some(Wanted { title: t["title"].as_str()?.to_string(), artist: t["subtitle"].as_str().unwrap_or_default().to_string(), secs: secs(&t["duration"]) }))
            .collect(),
        _ => {
            let artists: Vec<&str> = e["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect()).unwrap_or_default();
            let title = e["name"].as_str().or(e["title"].as_str()).ok_or_else(|| anyhow!("No songs on this Spotify page"))?;
            vec![Wanted { title: title.to_string(), artist: artists.join(", "), secs: secs(&e["duration"]) }]
        }
    };
    Ok(List { name: e["name"].as_str().or(e["title"].as_str()).unwrap_or("Spotify").to_string(), songs })
}

/// Lower case letters and digits only, without the "(feat. ...)" and "[...]" parts.
fn norm(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            _ if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ if !out.ends_with(' ') && !out.is_empty() => out.push(' '),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn first_artist(a: &str) -> &str {
    a.split([',', '&']).next().unwrap_or(a).trim()
}

/// How well a YouTube Music song matches: title, artist and length.
fn score(w: &Wanted, t: &Track) -> i32 {
    let (want, got) = (norm(&w.title), norm(&t.title));
    let mut s = if want == got {
        3
    } else if !want.is_empty() && (got.contains(&want) || want.contains(&got)) {
        2
    } else {
        0
    };
    let artist = norm(first_artist(&w.artist));
    if !artist.is_empty() && norm(&t.artist).contains(&artist) {
        s += 2;
    }
    if let (Some(a), Some(b)) = (w.secs, crate::parse_duration(&t.duration)) {
        s += match (a - b).abs() {
            d if d <= 3.0 => 2,
            d if d <= 10.0 => 1,
            d if d > 30.0 => -2,
            _ => 0,
        };
    }
    s
}

/// The song on YouTube Music: the best of the first results, when it's close enough.
pub fn best(w: &Wanted, results: Vec<Track>) -> Option<Track> {
    // The earlier result wins a tie: YouTube Music puts the song itself first.
    let mut pick: Option<(i32, Track)> = None;
    for t in results.into_iter().take(6) {
        let s = score(w, &t);
        if s >= 3 && pick.as_ref().is_none_or(|(b, _)| s > *b) {
            pick = Some((s, t));
        }
    }
    pick.map(|(_, t)| t)
}

/// Every song of the list found on YouTube Music (six searches at a time), in the list's order;
/// songs not found are left out.
pub async fn find_all(api: Api, list: &List) -> Vec<Track> {
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let mut jobs = tokio::task::JoinSet::new();
    for (i, w) in list.songs.iter().cloned().enumerate() {
        let (api, limit) = (api.clone(), limit.clone());
        jobs.spawn(async move {
            let _turn = limit.acquire().await.ok()?;
            let results = api.search(format!("{} {}", w.title, first_artist(&w.artist))).await.ok()?;
            Some((i, best(&w, results)?))
        });
    }
    let mut found: Vec<(usize, Track)> = Vec::new();
    while let Some(r) = jobs.join_next().await {
        if let Ok(Some(hit)) = r {
            found.push(hit);
        }
    }
    found.sort_by_key(|(i, _)| *i);
    let mut seen = std::collections::HashSet::new();
    found.into_iter().map(|(_, t)| t).filter(|t| seen.insert(t.video_id.clone())).collect()
}

/// A Spotify link's songs, found on YouTube Music.
pub async fn import(api: Api, link: String) -> Result<Vec<Track>> {
    let list = read(&link).await?;
    let songs = find_all(api, &list).await;
    if songs.is_empty() {
        return Err(anyhow!("None of the songs of \"{}\" were found on YouTube Music", list.name));
    }
    Ok(songs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(title: &str, artist: &str, duration: &str) -> Track {
        Track {
            video_id: title.into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: duration.into(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    #[test]
    fn links() {
        assert_eq!(parse_link("https://open.spotify.com/intl-ar/playlist/37i9dQZF1DXcBWIGoYBM5M?si=ab12"), Some(("playlist", "37i9dQZF1DXcBWIGoYBM5M".into())));
        assert_eq!(parse_link("spotify:album:4aawyAB9vmqN3uQ7FjRGTy"), Some(("album", "4aawyAB9vmqN3uQ7FjRGTy".into())));
        assert_eq!(parse_link("open.spotify.com/track/7bxaFZ1O3cHkgLKMsdC3xR"), Some(("track", "7bxaFZ1O3cHkgLKMsdC3xR".into())));
        assert_eq!(parse_link("https://open.spotify.com/artist/abc"), None);
        assert_eq!(parse_link("https://music.youtube.com/watch?v=x"), None);
    }

    #[test]
    fn embed_page_lists() {
        let page = |entity: Value| format!(r#"<html><script id="__NEXT_DATA__" type="application/json">{}</script></html>"#, serde_json::json!({"props": {"pageProps": {"state": {"data": {"entity": entity}}}}}));
        let pl = parse_embed(&page(serde_json::json!({"name": "Hits", "trackList": [{"title": "Loser", "subtitle": "Tame Impala", "duration": 223069}]}))).unwrap();
        assert_eq!(pl, List { name: "Hits".into(), songs: vec![Wanted { title: "Loser".into(), artist: "Tame Impala".into(), secs: Some(223.069) }] });
        let one = parse_embed(&page(serde_json::json!({"name": "Loser", "artists": [{"name": "Tame Impala"}, {"name": "X"}], "duration": 223069, "trackList": []}))).unwrap();
        assert_eq!(one.songs[0].artist, "Tame Impala, X");
    }

    /// A real public album through the guest API: prints how many songs were found.
    #[tokio::test]
    #[ignore]
    async fn imports_a_public_album() {
        let api = Api::guest().await.unwrap();
        let list = read("https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy").await.unwrap();
        let found = find_all(api, &list).await;
        println!("{}: {} of {} songs found", list.name, found.len(), list.songs.len());
        for t in found.iter().take(5) {
            println!("  {} - {} ({})", t.artist, t.title, t.duration);
        }
        assert!(found.len() * 2 > list.songs.len());
    }

    #[test]
    fn best_match_by_title_artist_and_length() {
        let w = Wanted { title: "Loser (feat. Someone)".into(), artist: "Tame Impala, Other".into(), secs: Some(223.0) };
        let pick = best(&w, vec![t("Loser (Live)", "Cover Band", "4:10"), t("Loser", "Tame Impala", "3:43"), t("Something else", "Tame Impala", "3:43")]);
        assert_eq!(pick.unwrap().artist, "Tame Impala");
        assert!(best(&w, vec![t("Unrelated", "Nobody", "9:00")]).is_none());
    }
}
