//! Audio links straight from YouTube's own player API (InnerTube), without yt-dlp.
//!
//! The app clients (visionOS, Quest, iPhone) get ready-to-use links. The TV client and YouTube
//! Music's own web player ask with your account (songs the app clients are refused, and 256 kbps
//! for Premium) and their links need the player script's puzzles solved (see `ytjs`, with Node.js).
//! The client that worked last goes first next time, and each song's link is kept until it expires.

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

/// The visionOS app (Apple Vision Pro). YouTube currently answers it with direct links without
/// the extra token the other app clients now need; NewPipe uses it for the same reason. Each song
/// gets a fresh visitor id: reusing one for several songs got "Sign in to confirm you're not a bot".
mod visionos {
    pub const NAME: &str = "VISIONOS";
    pub const VERSION: &str = "1.02";
    pub const USER_AGENT: &str = "com.google.visionos.youtube/1.02(RealityDevice14,1; U; CPU visionOS 25_6_0 like Mac OS X; US)";
    /// The phone version streams these links with a browser's user agent, which works.
    pub const STREAM_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0";
}

fn visionos_context(visitor: Option<&str>) -> Value {
    let mut client = json!({
        "clientName": visionos::NAME,
        "clientVersion": visionos::VERSION,
        "clientScreen": "WATCH",
        "platform": "MOBILE",
        "deviceMake": "Apple",
        "deviceModel": "RealityDevice14,1",
        "osName": "visionOS",
        "osVersion": "25.6.0.23O471",
        "hl": "en",
        "gl": "US",
        "utcOffsetMinutes": 0
    });
    if let Some(v) = visitor {
        client["visitorData"] = json!(v);
    }
    json!({ "client": client, "request": { "internalExperimentFlags": [], "useSsl": true }, "user": { "lockedSafetyMode": false } })
}

async fn visionos_post(url: &str, body: &Value) -> Result<Value> {
    Ok(http()
        .post(url)
        .header("User-Agent", visionos::USER_AGENT)
        .header("X-Goog-Api-Format-Version", "2")
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Random letters from YouTube's alphabet, for the playback nonce (`cpn`) and the `t` parameter.
fn nonce(len: usize) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0);
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let mut x = now ^ SEED.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ALPHABET[(x % 64) as usize] as char
        })
        .collect()
}

async fn visionos_audio(video_id: &str) -> Result<Audio> {
    let visitor_answer = visionos_post("https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false", &json!({ "context": visionos_context(None) })).await?;
    let visitor = visitor_answer["responseContext"]["visitorData"].as_str().ok_or_else(|| anyhow!("no visitor id"))?.to_string();
    let cpn = nonce(16);
    let body = json!({ "context": visionos_context(Some(&visitor)), "videoId": video_id, "cpn": cpn, "contentCheckOk": true, "racyCheckOk": true });
    let url = format!("https://youtubei.googleapis.com/youtubei/v1/player?prettyPrint=false&t={}&id={video_id}", nonce(12));
    let mut a = pick(&visionos_post(&url, &body).await?, visionos::STREAM_AGENT)?;
    a.url.push_str(&format!("&cpn={cpn}"));
    a.client = format!("{} {}", visionos::NAME, visionos::VERSION);
    Ok(a)
}

/// The TV app: takes your account, needs no extra token; its links come with puzzles.
mod tv {
    pub const NAME: &str = "TVHTML5";
    pub const ID: &str = "7";
    pub const VERSION: &str = "7.20260707.07.00";
    pub const USER_AGENT: &str = "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.30.1034943-gold (unlike Gecko), Unknown_TV_Unknown_0/Unknown (Unknown, Unknown)";
}

/// YouTube Music's own web player: signed in with Premium it gives the 256 kbps Opus stream; for
/// other accounts YouTube wants a proof-of-origin token, asked from a local token server when one
/// runs (bgutil-ytdlp-pot-provider, port 4416).
mod music {
    pub const NAME: &str = "WEB_REMIX";
    pub const ID: &str = "67";
    pub const VERSION: &str = "1.20260707.12.00";
    pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0";
}

/// A way of asking: one of the clients.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Way {
    VisionOs,
    App(usize),
    Tv,
    Music,
}

impl Way {
    fn name(self) -> String {
        match self {
            Way::VisionOs => format!("{} {}", visionos::NAME, visionos::VERSION),
            Way::App(i) => format!("{} {}", CLIENTS[i].name, CLIENTS[i].version),
            Way::Tv => format!("{} {}", tv::NAME, tv::VERSION),
            Way::Music => format!("{} {}", music::NAME, music::VERSION),
        }
    }
}

static BEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// "Best download quality": YouTube Music's player (256 kbps with Premium) is asked first.
pub fn prefer_best(on: bool) {
    BEST.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn remembered_file() -> std::path::PathBuf {
    crate::data_dir().join("innertube-client.txt")
}

/// The client that gave the last working link (kept between runs).
fn remembered() -> &'static std::sync::Mutex<Option<String>> {
    static R: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
    R.get_or_init(|| std::sync::Mutex::new(std::fs::read_to_string(remembered_file()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())))
}

fn remember(way: Way) {
    let name = way.name();
    if let Ok(mut r) = remembered().lock()
        && r.as_deref() != Some(name.as_str())
    {
        let _ = std::fs::write(remembered_file(), &name);
        *r = Some(name);
    }
}

/// The ways to try, in order: the one that worked last first; the TV and Music players only with
/// Node.js (their puzzles), Music only signed in.
fn order() -> Vec<Way> {
    let node = crate::ytjs::node().is_some();
    let signed_in = account("https://music.youtube.com").is_some();
    let mut ways = vec![Way::VisionOs, Way::App(0), Way::App(1), Way::App(2)];
    if node {
        ways.push(Way::Tv);
        if signed_in {
            if BEST.load(std::sync::atomic::Ordering::Relaxed) {
                ways.insert(0, Way::Music);
            } else {
                ways.push(Way::Music);
            }
        }
    }
    let last = remembered().lock().ok().and_then(|r| r.clone());
    if let Some(i) = last.and_then(|l| ways.iter().position(|w| w.name() == l)) {
        let w = ways.remove(i);
        ways.insert(0, w);
    }
    ways
}

/// Links found, kept until shortly before they expire (YouTube gives about six hours).
fn links() -> &'static std::sync::Mutex<std::collections::HashMap<String, (Audio, u64)>> {
    static L: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (Audio, u64)>>> = std::sync::OnceLock::new();
    L.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// When a link stops working (its `expire` value).
fn expires(url: &str) -> Option<u64> {
    url::Url::parse(url).ok()?.query_pairs().find(|(k, _)| k == "expire").and_then(|(_, v)| v.parse().ok())
}

fn kept(video_id: &str, skip: &[String]) -> Option<Audio> {
    let mut l = links().lock().ok()?;
    let (a, until) = l.get(video_id)?.clone();
    if skip.contains(&a.client) || until <= now_secs() + 600 {
        l.remove(video_id);
        return None;
    }
    Some(a)
}

fn keep(video_id: &str, a: &Audio) {
    let Some(until) = expires(&a.url) else { return };
    if let Ok(mut l) = links().lock() {
        if l.len() > 300 {
            let now = now_secs();
            l.retain(|_, (_, u)| *u > now + 600);
        }
        l.insert(video_id.to_string(), (a.clone(), until));
    }
}

/// The signed-in account's headers for `origin`: its cookie and the SAPISIDHASH YouTube checks.
fn account(origin: &str) -> Option<Vec<(&'static str, String)>> {
    use sha1::{Digest, Sha1};
    let cookie = std::fs::read_to_string(crate::cookie_path()).ok()?.trim().to_string();
    let sapisid = cookie.split(';').map(str::trim).find_map(|kv| kv.strip_prefix("SAPISID=").or_else(|| kv.strip_prefix("__Secure-3PAPISID=")))?.to_string();
    let ts = now_secs();
    let hash: String = Sha1::digest(format!("{ts} {sapisid} {origin}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Some(vec![
        ("Cookie", cookie),
        ("Authorization", format!("SAPISIDHASH {ts}_{hash}")),
        ("X-Origin", origin.to_string()),
        ("Origin", origin.to_string()),
        ("X-Goog-AuthUser", "0".to_string()),
    ])
}

/// A proof-of-origin token from a local token server (bgutil-ytdlp-pot-provider) bound to
/// `binding`; none when no server runs.
async fn po_token(binding: &str) -> Option<String> {
    let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_millis(300)).timeout(std::time::Duration::from_secs(20)).build().ok()?;
    let v: Value = client.post("http://127.0.0.1:4416/get_pot").json(&json!({ "content_binding": binding })).send().await.ok()?.json().await.ok()?;
    v["poToken"].as_str().map(str::to_string)
}

/// The TV app's visitor id and current version, read off its page (youtube.com/tv) once an hour.
async fn tv_page() -> Result<(String, String)> {
    static V: std::sync::Mutex<Option<(String, String, std::time::Instant)>> = std::sync::Mutex::new(None);
    if let Some((v, ver, at)) = V.lock().ok().and_then(|g| g.clone())
        && at.elapsed() < std::time::Duration::from_secs(3600)
    {
        return Ok((v, ver));
    }
    // Over HTTP/1.1: the page's headers are too big for the HTTP/2 client ("protocol error").
    static PAGE: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let page_client = PAGE.get_or_init(|| reqwest::Client::builder().http1_only().timeout(std::time::Duration::from_secs(15)).build().expect("http client"));
    let page = page_client.get("https://www.youtube.com/tv").header("User-Agent", tv::USER_AGENT).send().await?.error_for_status()?.text().await?;
    let field = |key: &str| {
        let at = page.find(&format!("\"{key}\":\""))? + key.len() + 4;
        page[at..].split('"').next().map(str::to_string)
    };
    let visitor = field("visitorData").or_else(|| field("VISITOR_DATA")).ok_or_else(|| anyhow!("no visitor id on the TV page"))?;
    let visitor = url::form_urlencoded::parse(format!("v={visitor}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or(visitor);
    let version = field("INNERTUBE_CLIENT_VERSION").filter(|v| v.starts_with("7.")).unwrap_or_else(|| tv::VERSION.to_string());
    if let Ok(mut g) = V.lock() {
        *g = Some((visitor.clone(), version.clone(), std::time::Instant::now()));
    }
    Ok((visitor, version))
}

/// The TV or Music player, signed in when there's an account; the link's puzzles solved.
async fn web_player(way: Way, video_id: &str) -> Result<Audio> {
    let (host, name, id, version, ua) = match way {
        Way::Tv => ("https://www.youtube.com", tv::NAME, tv::ID, tv::VERSION, tv::USER_AGENT),
        _ => ("https://music.youtube.com", music::NAME, music::ID, music::VERSION, music::USER_AGENT),
    };
    let sts = crate::ytjs::signature_timestamp().await?;
    // The TV app answers "The page needs to be reloaded" without the visitor id and version of
    // its own page.
    let (visitor, version) = if way == Way::Tv {
        let (v, ver) = tv_page().await?;
        (Some(v), ver)
    } else {
        (None, version.to_string())
    };
    let mut client = json!({ "clientName": name, "clientVersion": version, "hl": "en", "gl": "US" });
    if let Some(v) = &visitor {
        client["userAgent"] = json!(ua);
        client["visitorData"] = json!(v);
    }
    let mut body = json!({
        "context": { "client": client },
        "videoId": video_id,
        "playbackContext": { "contentPlaybackContext": { "signatureTimestamp": sts, "html5Preference": "HTML5_PREF_WANTS" } },
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    // Not Premium: YouTube Music's links want a token (only when a token server runs).
    let pot = if way == Way::Music { po_token(video_id).await } else { None };
    if let Some(t) = &pot {
        body["serviceIntegrityDimensions"] = json!({ "poToken": t });
    }
    let mut req = http()
        .post(format!("{host}/youtubei/v1/player?prettyPrint=false"))
        .header("User-Agent", ua)
        .header("X-YouTube-Client-Name", id)
        .header("X-YouTube-Client-Version", version.as_str());
    for (k, v) in account(host).unwrap_or_default() {
        req = req.header(k, v);
    }
    if let Some(v) = &visitor {
        req = req.header("X-Goog-Visitor-Id", v);
    }
    let v: Value = req.json(&body).send().await?.error_for_status()?.json().await?;
    check_playable(&v)?;
    let f = best_format(&v)?;
    let mut url = crate::ytjs::unlock(f).await?;
    // The stream's token is bound to the account's session (or the visitor).
    if pot.is_some() {
        let binding = v["responseContext"]["mainAppWebResponseContext"]["datasyncId"]
            .as_str()
            .map(|d| d.split("||").next().unwrap_or(d).to_string())
            .or_else(|| v["responseContext"]["visitorData"].as_str().map(str::to_string));
        if let Some(gvs) = match binding {
            Some(b) => po_token(&b).await,
            None => None,
        } {
            url.push_str(&format!("&pot={gvs}"));
        }
    }
    let mut a = audio_of(f, url, ua);
    a.client = way.name();
    Ok(a)
}

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
    if let Some(a) = kept(video_id, skip) {
        return Ok(a);
    }
    let mut why = Vec::new();
    for way in order() {
        let name = way.name();
        if skip.contains(&name) {
            continue;
        }
        let got = match way {
            Way::VisionOs => visionos_audio(video_id).await,
            Way::App(i) => player(&CLIENTS[i], video_id).await,
            Way::Tv | Way::Music => web_player(way, video_id).await,
        };
        match got {
            Ok(a) => {
                remember(way);
                keep(video_id, &a);
                return Ok(a);
            }
            Err(e) => why.push(format!("{}: {e}", name.split(' ').next().unwrap_or(&name))),
        }
    }
    Err(anyhow!("YouTube gave no audio ({})", why.join("; ")))
}

/// A link the player couldn't open: it's asked again (from another client) next time.
pub fn forget(video_id: &str) {
    if let Ok(mut l) = links().lock() {
        l.remove(video_id);
    }
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

fn check_playable(v: &Value) -> Result<()> {
    let status = v["playabilityStatus"]["status"].as_str().unwrap_or("");
    if status != "OK" {
        let reason = v["playabilityStatus"]["reason"].as_str().unwrap_or(status);
        bail!("{reason}");
    }
    Ok(())
}

fn num(f: &Value, k: &str) -> Option<u64> {
    f[k].as_u64().or_else(|| f[k].as_str()?.parse().ok())
}

/// The best audio format: 256 kbps Opus (Premium) first, then 160 kbps Opus, then AAC. With a
/// ready link or one still to unlock (`signatureCipher`).
fn best_format(v: &Value) -> Result<&Value> {
    let rank = |itag: u64| match itag {
        774 => 5,
        251 => 4,
        140 => 3,
        250 => 2,
        249 => 1,
        _ => 0,
    };
    v["streamingData"]["adaptiveFormats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["mimeType"].as_str().is_some_and(|m| m.starts_with("audio/")) && (f["url"].is_string() || f["signatureCipher"].is_string()))
        .max_by_key(|f| (rank(num(f, "itag").unwrap_or(0)), num(f, "bitrate").unwrap_or(0)))
        .ok_or_else(|| anyhow!("no audio stream"))
}

fn audio_of(best: &Value, url: String, user_agent: &'static str) -> Audio {
    Audio {
        url,
        itag: num(best, "itag").unwrap_or(0),
        ext: if best["mimeType"].as_str().unwrap_or("").contains("webm") { "webm" } else { "m4a" },
        bitrate: num(best, "bitrate").unwrap_or(0),
        size: num(best, "contentLength"),
        seconds: num(best, "approxDurationMs").map(|ms| ms as f32 / 1000.0),
        user_agent,
        client: String::new(),
    }
}

/// Chooses the stream from an app client's answer (ready links only).
fn pick(v: &Value, user_agent: &'static str) -> Result<Audio> {
    check_playable(v)?;
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

#[cfg(test)]
mod web_tests {
    use super::*;

    #[test]
    fn link_lifetime_from_the_url() {
        assert_eq!(expires("https://r1.googlevideo.com/videoplayback?expire=1794200000&ei=x"), Some(1_794_200_000));
        assert_eq!(expires("https://x/y"), None);
    }

    #[test]
    fn premium_opus_ranks_first() {
        let v = json!({ "streamingData": { "adaptiveFormats": [
            { "itag": 251, "mimeType": "audio/webm", "signatureCipher": "s=a&url=b", "bitrate": 140000 },
            { "itag": 774, "mimeType": "audio/webm", "signatureCipher": "s=c&url=d", "bitrate": 260000 }
        ]}});
        assert_eq!(num(best_format(&v).unwrap(), "itag"), Some(774));
    }

    /// The TV and Music players with the saved account, links unlocked and opened:
    /// `cargo test --lib web_tests -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_web_players() {
        for way in [Way::Tv, Way::Music] {
            let t = std::time::Instant::now();
            match web_player(way, "lYBUbBu4W08").await {
                Ok(a) => {
                    let r = http().get(&a.url).header("User-Agent", a.user_agent).header("Range", "bytes=0-65535").send().await.unwrap();
                    println!("{}: itag {} {} kbps in {:?}, first range HTTP {}", a.client, a.itag, a.bitrate / 1000, t.elapsed(), r.status());
                }
                Err(e) => println!("{}: {e:#}", way.name()),
            }
        }
    }
}

#[cfg(test)]
mod visionos_tests {
    use super::*;

    #[test]
    fn nonces_use_youtubes_alphabet() {
        let (a, b) = (nonce(16), nonce(16));
        assert_eq!((a.len(), nonce(12).len()), (16, 12));
        assert_ne!(a, b, "two nonces in a row differ");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{a}");
    }

    #[test]
    fn context_carries_the_visitor_id() {
        let c = visionos_context(Some("VISITOR"));
        assert_eq!(c["client"]["clientName"], "VISIONOS");
        assert_eq!(c["client"]["visitorData"], "VISITOR");
        assert!(visionos_context(None)["client"].get("visitorData").is_none());
    }
}
