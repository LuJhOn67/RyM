//! "Prefer lossless (FLAC)": the same song as a FLAC file from the Internet Archive (archive.org,
//! a free and legal library: live recordings, netlabels, public domain and artists' own uploads),
//! found by its title, artist and length. YouTube's Opus plays when it isn't there.

use serde_json::Value;

use crate::api::Track;

/// A FLAC file found for a song: where to stream it from and its length (seconds).
pub struct Found {
    pub url: String,
    pub secs: Option<f32>,
}

/// Lowercase letters and digits only, for comparing names.
fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// "3:45" or "1:02:03" as seconds.
fn secs_of(duration: &str) -> Option<f32> {
    let mut total = 0.0;
    for part in duration.trim().split(':') {
        total = total * 60.0 + part.trim().parse::<f32>().ok()?;
    }
    (total > 0.0).then_some(total)
}

/// Quotes for the Archive's search (its own quote marks taken out).
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', " "))
}

/// The file of an Archive item that is this song: FLAC, its title (or file name) the song's, and
/// as long as the song (within 4 seconds) when both lengths are known.
pub fn pick_file(files: &[Value], title: &str, secs: Option<f32>) -> Option<(String, Option<f32>)> {
    let want = norm(title);
    if want.is_empty() {
        return None;
    }
    files.iter().find_map(|f| {
        let name = f["name"].as_str()?;
        if !name.to_lowercase().ends_with(".flac") {
            return None;
        }
        let named = f["title"].as_str().map(norm).filter(|t| !t.is_empty()).unwrap_or_else(|| norm(name.rsplit('/').next().unwrap_or(name).trim_end_matches(".flac")));
        if !(named == want || named.ends_with(&want) || (want.len() >= 6 && named.contains(&want))) {
            return None;
        }
        let length = f["length"].as_str().and_then(|l| l.parse::<f32>().ok().or_else(|| secs_of(l))).or_else(|| f["length"].as_f64().map(|l| l as f32));
        if let (Some(a), Some(b)) = (length, secs)
            && (a - b).abs() > 4.0
        {
            return None;
        }
        Some((name.to_string(), length))
    })
}

/// The song as FLAC from the Internet Archive, if it's there.
pub async fn find(t: &Track) -> Option<Found> {
    let artist = t.artist.split([',', '&']).next().unwrap_or_default().trim();
    if t.title.trim().is_empty() || artist.is_empty() {
        return None;
    }
    let http = reqwest::Client::builder().user_agent("RyM").timeout(std::time::Duration::from_secs(6)).build().ok()?;
    let q = format!("(title:({}) OR {}) AND creator:({}) AND format:(FLAC)", quoted(&t.title), quoted(&t.title), quoted(artist));
    let mut url = reqwest::Url::parse("https://archive.org/advancedsearch.php").ok()?;
    url.query_pairs_mut().append_pair("q", &q).append_pair("fl[]", "identifier").append_pair("rows", "5").append_pair("output", "json");
    let found: Value = http.get(url).send().await.ok()?.json().await.ok()?;
    let secs = secs_of(&t.duration);
    for doc in found["response"]["docs"].as_array()? {
        let Some(id) = doc["identifier"].as_str() else { continue };
        let Ok(meta) = http.get(format!("https://archive.org/metadata/{id}")).send().await else { continue };
        let Ok(meta) = meta.json::<Value>().await else { continue };
        let Some(files) = meta["files"].as_array() else { continue };
        if let Some((name, length)) = pick_file(files, &t.title, secs) {
            let mut link = reqwest::Url::parse("https://archive.org/download/").ok()?;
            link.path_segments_mut().ok()?.pop_if_empty().push(id).extend(name.split('/'));
            return Some(Found { url: link.to_string(), secs: length.or(secs) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_flac_with_the_same_title_and_length() {
        let files = vec![
            json!({"name": "01 - Intro.flac", "title": "Intro", "length": "61.2"}),
            json!({"name": "02 - Moonlight.mp3", "title": "Moonlight", "length": "200"}),
            json!({"name": "02 - Moonlight.flac", "title": "Moonlight", "length": "3:21"}),
        ];
        assert_eq!(pick_file(&files, "Moonlight", Some(200.0)).map(|f| f.0), Some("02 - Moonlight.flac".to_string()));
        // Too long or short: another recording.
        assert_eq!(pick_file(&files, "Moonlight", Some(260.0)), None);
        // No title: the file's name tells.
        let untitled = vec![json!({"name": "disc1/03 Blue Sky.flac"})];
        assert!(pick_file(&untitled, "Blue Sky", None).is_some());
        assert!(pick_file(&untitled, "Red Sky", None).is_none());
    }

    /// A well-known song the Archive has as FLAC (the Grateful Dead's concerts are shared there).
    #[tokio::test]
    #[ignore]
    async fn live_finds_a_flac() {
        let t = Track { video_id: "x".into(), title: "Ripple".into(), artist: "Grateful Dead".into(), duration: String::new(), album: String::new(), thumb: None, album_id: None, artist_id: None, link: None };
        let found = find(&t).await;
        println!("{:?}", found.as_ref().map(|f| (&f.url, f.secs)));
        assert!(found.is_some_and(|f| f.url.ends_with(".flac")));
    }

    #[test]
    fn lengths_read() {
        assert_eq!(secs_of("3:45"), Some(225.0));
        assert_eq!(secs_of("1:02:03"), Some(3723.0));
        assert_eq!(secs_of(""), None);
    }
}
