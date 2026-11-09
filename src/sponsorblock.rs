//! SponsorBlock (sponsor.ajay.app): the parts of a music video that aren't music (an intro, a
//! talk, an outro), marked by its users as `music_offtopic`, so the player skips them.
//!
//! Asked by the first 4 characters of the video id's SHA-256, so the server isn't told which
//! song plays; the matching video is picked here.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use serde_json::Value;

/// The parts to skip, as (start, end) in seconds.
pub type Segments = Vec<(f32, f32)>;

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(std::time::Duration::from_secs(8)).build().expect("http client"))
}

fn known() -> &'static Mutex<HashMap<String, Segments>> {
    static K: std::sync::OnceLock<Mutex<HashMap<String, Segments>>> = std::sync::OnceLock::new();
    K.get_or_init(Default::default)
}

fn prefix(video_id: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(video_id.as_bytes()).iter().take(2).map(|b| format!("{b:02x}")).collect()
}

/// The video's non-music parts (none when it has none, or the server can't be reached).
pub async fn segments(video_id: &str) -> Segments {
    if !crate::is_youtube(video_id) || video_id.is_empty() {
        return Vec::new();
    }
    if let Some(s) = known().lock().ok().and_then(|k| k.get(video_id).cloned()) {
        return s;
    }
    let found = ask(video_id).await.unwrap_or_default();
    if let Ok(mut k) = known().lock() {
        if k.len() > 500 {
            k.clear();
        }
        k.insert(video_id.to_string(), found.clone());
    }
    found
}

async fn ask(video_id: &str) -> Result<Segments> {
    let url = format!("https://sponsor.ajay.app/api/skipSegments/{}?categories=[\"music_offtopic\"]&actionTypes=[\"skip\"]", prefix(video_id));
    let resp = http().get(url).send().await?;
    // 404: no video with that prefix has any.
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let list: Value = resp.error_for_status()?.json().await?;
    Ok(parse(&list, video_id))
}

fn parse(list: &Value, video_id: &str) -> Segments {
    let mut out: Segments = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["videoID"].as_str() == Some(video_id))
        .flat_map(|v| v["segments"].as_array().cloned().unwrap_or_default())
        .filter(|s| s["category"].as_str() == Some("music_offtopic"))
        .filter_map(|s| {
            let a = s["segment"][0].as_f64()? as f32;
            let b = s["segment"][1].as_f64()? as f32;
            (b > a + 1.0).then_some((a, b))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// The end of the part `pos` is in (where to jump), unless it's within its last half second.
pub fn skip_to(segments: &Segments, pos: f32) -> Option<f32> {
    segments.iter().find(|(a, b)| pos >= *a && pos < *b - 0.5).map(|(_, b)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_this_videos_music_offtopic_parts() {
        let list = serde_json::json!([
            { "videoID": "other", "segments": [{ "category": "music_offtopic", "segment": [0.0, 30.0] }] },
            { "videoID": "abc", "segments": [
                { "category": "music_offtopic", "segment": [200.0, 240.5] },
                { "category": "sponsor", "segment": [50.0, 60.0] },
                { "category": "music_offtopic", "segment": [0.0, 12.0] }
            ]}
        ]);
        let s = parse(&list, "abc");
        assert_eq!(s, vec![(0.0, 12.0), (200.0, 240.5)]);
        assert_eq!(skip_to(&s, 3.0), Some(12.0));
        assert_eq!(skip_to(&s, 11.8), None);
        assert_eq!(skip_to(&s, 100.0), None);
    }

    #[test]
    fn the_prefix_is_four_hex_letters() {
        let p = prefix("dQw4w9WgXcQ");
        assert_eq!(p.len(), 4);
        assert!(p.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// A music video known to have a non-music part: `cargo test --lib sponsorblock -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_segments() {
        for id in ["dQw4w9WgXcQ", "kJQP7kiw5Fk", "JGwWNGJdvx8"] {
            println!("{id}: {:?}", segments(id).await);
        }
    }
}
