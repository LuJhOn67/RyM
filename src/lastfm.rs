//! Last.fm scrobbling: the songs played go into the user's Last.fm profile. The user makes an
//! API account (last.fm/api) and puts its key and secret in the settings; "Connect" opens
//! Last.fm in the browser to allow RyM, then the session is kept.
//! A song is scrobbled once half of it (or 4 minutes) has played; "now playing" as it starts.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use serde_json::Value;

const API: &str = "https://ws.audioscrobbler.com/2.0/";

/// The signature Last.fm wants: md5 of the parameters sorted by name (name then value), then the secret.
fn sign(params: &BTreeMap<&str, String>, secret: &str) -> String {
    let mut s = String::new();
    for (k, v) in params {
        s.push_str(k);
        s.push_str(v);
    }
    s.push_str(secret);
    format!("{:x}", md5::compute(s.as_bytes()))
}

async fn call(mut params: BTreeMap<&str, String>, secret: &str) -> Result<Value> {
    let sig = sign(&params, secret);
    params.insert("api_sig", sig);
    params.insert("format", "json".into());
    let body = {
        let mut u = reqwest::Url::parse("http://x/")?;
        for (k, v) in &params {
            u.query_pairs_mut().append_pair(k, v);
        }
        u.query().unwrap_or_default().to_string()
    };
    let resp: Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .post(API)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(msg) = resp.get("message").and_then(Value::as_str).filter(|_| resp.get("error").is_some()) {
        return Err(anyhow!("Last.fm: {msg}"));
    }
    Ok(resp)
}

/// Step one: a token, and the page where the user allows RyM.
pub async fn start(key: &str, secret: &str) -> Result<(String, String)> {
    let params = BTreeMap::from([("method", "auth.getToken".to_string()), ("api_key", key.to_string())]);
    let resp = call(params, secret).await?;
    let token = resp.get("token").and_then(Value::as_str).ok_or_else(|| anyhow!("Last.fm gave no token"))?.to_string();
    let page = format!("https://www.last.fm/api/auth/?api_key={key}&token={token}");
    Ok((token, page))
}

/// Step two, once allowed: the session key and the user's name.
pub async fn finish(key: &str, secret: &str, token: &str) -> Result<(String, String)> {
    let params = BTreeMap::from([("method", "auth.getSession".to_string()), ("api_key", key.to_string()), ("token", token.to_string())]);
    let resp = call(params, secret).await?;
    let s = resp.get("session").ok_or_else(|| anyhow!("Not allowed on Last.fm yet"))?;
    let session = s.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
    let user = s.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
    Ok((session, user))
}

/// What Last.fm needs to know of a song.
pub struct Scrobble {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration: u32,
    /// When it started (seconds since 1970).
    pub started: u64,
}

fn song_params(s: &Scrobble, key: &str, session: &str, method: &str) -> BTreeMap<&'static str, String> {
    let mut p = BTreeMap::from([
        ("method", method.to_string()),
        ("api_key", key.to_string()),
        ("sk", session.to_string()),
        ("artist", s.artist.split([',', '&']).next().unwrap_or_default().trim().to_string()),
        ("track", s.title.clone()),
    ]);
    if !s.album.is_empty() {
        p.insert("album", s.album.clone());
    }
    if s.duration > 0 {
        p.insert("duration", s.duration.to_string());
    }
    p
}

pub async fn now_playing(s: Scrobble, key: String, secret: String, session: String) -> Result<()> {
    call(song_params(&s, &key, &session, "track.updateNowPlaying"), &secret).await.map(|_| ())
}

pub async fn scrobble(s: Scrobble, key: String, secret: String, session: String) -> Result<()> {
    let mut p = song_params(&s, &key, &session, "track.scrobble");
    p.insert("timestamp", s.started.to_string());
    call(p, &secret).await.map(|_| ())
}

/// Played enough to count: half the song, or 4 minutes (songs over 30 seconds only).
pub fn counts(played_secs: f32, duration: f32) -> bool {
    duration > 30.0 && (played_secs >= duration / 2.0 || played_secs >= 240.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_and_when_it_counts() {
        let p = BTreeMap::from([("method", "auth.getToken".to_string()), ("api_key", "abc".to_string())]);
        // md5("api_keyabcmethodauth.getTokensecret")
        assert_eq!(sign(&p, "secret"), format!("{:x}", md5::compute(b"api_keyabcmethodauth.getTokensecret")));
        assert!(!counts(100.0, 25.0));
        assert!(counts(100.0, 200.0));
        assert!(counts(240.0, 900.0));
        assert!(!counts(60.0, 200.0));
    }
}
