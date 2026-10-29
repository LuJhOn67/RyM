//! Audio links straight from YouTube's own player API (InnerTube), without yt-dlp.
//!
//! The player answer for app clients carries ready-to-use stream URLs, so no page scripts have
//! to be run. Clients are tried in order until one returns a playable song.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

const PLAYER: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";

/// An app client InnerTube knows: its name in requests, its id and version, and its user agent.
struct Client {
    name: &'static str,
    id: &'static str,
    version: &'static str,
    user_agent: &'static str,
    extra: fn() -> Value,
}

const CLIENTS: &[Client] = &[
    Client {
        name: "ANDROID_VR",
        id: "28",
        version: "1.62.27",
        user_agent: "com.google.android.apps.youtube.vr.oculus/1.62.27 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
        extra: || json!({ "deviceMake": "Oculus", "deviceModel": "Quest 3", "androidSdkVersion": 32, "osName": "Android", "osVersion": "12L" }),
    },
    Client {
        name: "ANDROID_VR",
        id: "28",
        version: "1.65.10",
        user_agent: "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
        extra: || json!({ "deviceMake": "Oculus", "deviceModel": "Quest 3", "androidSdkVersion": 32, "osName": "Android", "osVersion": "12L" }),
    },
    Client {
        name: "IOS",
        id: "5",
        version: "20.10.4",
        user_agent: "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
        extra: || json!({ "deviceMake": "Apple", "deviceModel": "iPhone16,2", "osName": "iPhone", "osVersion": "18.3.2.22D82" }),
    },
];

/// One audio-only stream of a song.
#[derive(Debug, Clone)]
pub struct Audio {
    pub url: String,
    pub itag: u64,
    /// "webm" (Opus) or "m4a" (AAC).
    pub ext: &'static str,
    pub bitrate: u64,
    pub size: Option<u64>,
    pub seconds: Option<f32>,
    /// Sent with every request for the stream.
    pub user_agent: &'static str,
    /// Which client gave it ("NAME version"), to skip it if the link doesn't work.
    pub client: String,
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().expect("http client"))
}

/// The best audio stream: Opus first (itag 251, 160 kbps), then AAC.
pub async fn audio(video_id: &str) -> Result<Audio> {
    audio_skipping(video_id, &[]).await
}

/// Like [`audio`], without asking the clients in `skip` (their links didn't work).
pub async fn audio_skipping(video_id: &str, skip: &[String]) -> Result<Audio> {
    let mut why = Vec::new();
    for c in CLIENTS.iter().filter(|c| !skip.contains(&format!("{} {}", c.name, c.version))) {
        match player(c, video_id).await {
            Ok(a) => return Ok(a),
            Err(e) => why.push(format!("{}: {e}", c.name)),
        }
    }
    Err(anyhow!("YouTube gave no audio ({})", why.join("; ")))
}

async fn player(c: &Client, video_id: &str) -> Result<Audio> {
    let mut client = json!({ "clientName": c.name, "clientVersion": c.version, "userAgent": c.user_agent, "hl": "en", "gl": "US" });
    if let (Some(obj), Value::Object(more)) = (client.as_object_mut(), (c.extra)()) {
        obj.extend(more);
    }
    let body = json!({ "context": { "client": client }, "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true });
    let v: Value = http()
        .post(PLAYER)
        .header("User-Agent", c.user_agent)
        .header("X-YouTube-Client-Name", c.id)
        .header("X-YouTube-Client-Version", c.version)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut a = pick(&v, c.user_agent)?;
    a.client = format!("{} {}", c.name, c.version);
    Ok(a)
}

/// Chooses the stream from a player answer.
fn pick(v: &Value, user_agent: &'static str) -> Result<Audio> {
    let status = v["playabilityStatus"]["status"].as_str().unwrap_or("");
    if status != "OK" {
        let reason = v["playabilityStatus"]["reason"].as_str().unwrap_or(status);
        bail!("{reason}");
    }
    let num = |f: &Value, k: &str| f[k].as_u64().or_else(|| f[k].as_str()?.parse().ok());
    let rank = |itag: u64| match itag {
        251 => 4,
        140 => 3,
        250 => 2,
        249 => 1,
        _ => 0,
    };
    let best = v["streamingData"]["adaptiveFormats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["mimeType"].as_str().is_some_and(|m| m.starts_with("audio/")) && f["url"].is_string())
        .max_by_key(|f| (rank(num(f, "itag").unwrap_or(0)), num(f, "bitrate").unwrap_or(0)))
        .ok_or_else(|| anyhow!("no direct audio stream"))?;
    let itag = num(best, "itag").unwrap_or(0);
    Ok(Audio {
        url: best["url"].as_str().unwrap_or_default().to_string(),
        itag,
        ext: if best["mimeType"].as_str().unwrap_or("").contains("webm") { "webm" } else { "m4a" },
        bitrate: num(best, "bitrate").unwrap_or(0),
        size: num(best, "contentLength"),
        seconds: num(best, "approxDurationMs").map(|ms| ms as f32 / 1000.0),
        user_agent,
        client: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_opus_over_aac() {
        let v = json!({
            "playabilityStatus": { "status": "OK" },
            "streamingData": { "adaptiveFormats": [
                { "itag": 137, "mimeType": "video/mp4", "url": "v", "bitrate": 4000000 },
                { "itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "url": "a", "bitrate": 130000, "contentLength": "100" },
                { "itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "url": "o", "bitrate": 140000, "contentLength": "90", "approxDurationMs": "200500" }
            ]}
        });
        let a = pick(&v, "ua").unwrap();
        assert_eq!((a.itag, a.ext, a.size, a.seconds), (251, "webm", Some(90), Some(200.5)));
    }

    #[test]
    fn reports_why_a_song_cant_play() {
        let v = json!({ "playabilityStatus": { "status": "LOGIN_REQUIRED", "reason": "Sign in to confirm your age" } });
        assert_eq!(pick(&v, "ua").unwrap_err().to_string(), "Sign in to confirm your age");
    }

    /// Asks YouTube for a real song and reads its first bytes: `cargo test --lib innertube -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_audio() {
        let a = audio("lYBUbBu4W08").await.expect("audio");
        println!("itag {} {} {} kbps, {:?} bytes, {:?} s", a.itag, a.ext, a.bitrate / 1000, a.size, a.seconds);
        let r = http().get(&a.url).header("User-Agent", a.user_agent).header("Range", "bytes=0-65535").send().await.unwrap();
        println!("first range: HTTP {}, {} bytes", r.status(), r.bytes().await.unwrap().len());
    }
}
