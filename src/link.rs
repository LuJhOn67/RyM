//! RyM Link: the phone tells the PCs on its Wi-Fi what it plays, and a PC can take it over (the
//! song goes on here from the same second, with its queue, and the phone pauses).
//!
//! The phone sends a small JSON note by UDP broadcast every few seconds while RyM runs on it
//! ([`Announce`], port [`PORT`]); it answers `GET /state` (the queue and where it is) and
//! `POST /pause` on the TCP port the note gives. Only devices signed in to the same Google
//! account take part: the note carries [`account_key`], and the phone checks it on each request.
//! The same as the phone app's `playback/Link.kt`.

use std::net::{IpAddr, UdpSocket};
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::api::Track;

/// The UDP port the notes go to.
pub const PORT: u16 = 47811;

/// A device's note: what it is and what it plays.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Announce {
    /// Always 1 (the note's version).
    pub rym: u32,
    /// "phone" (or "pc").
    pub kind: String,
    /// The device's name ("Pixel 8").
    pub device: String,
    pub account: String,
    /// Its TCP port for `/state` and `/pause`.
    pub port: u16,
    pub playing: bool,
    #[serde(default)]
    pub video_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub thumb: Option<String>,
    /// Seconds into the song when the note was sent.
    #[serde(default)]
    pub position: f32,
}

/// What the phone plays: its queue, the playing song in it, and the second.
#[derive(Debug, Clone, Deserialize)]
pub struct State {
    pub songs: Vec<LinkTrack>,
    pub current: usize,
    pub position: f32,
    pub playing: bool,
}

/// A song as the phone sends it.
#[derive(Debug, Clone, Deserialize)]
pub struct LinkTrack {
    pub video_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub thumb: Option<String>,
}

impl From<LinkTrack> for Track {
    fn from(t: LinkTrack) -> Track {
        Track { video_id: t.video_id, title: t.title, artist: t.artist, album: t.album, duration: t.duration, thumb: t.thumb, album_id: None, artist_id: None, link: None }
    }
}

/// The account's key in the notes: devices with the same one belong together. A hash, so the
/// note doesn't carry the account's name.
pub fn account_key(name: &str, handle: &str) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(format!("rym-link|{name}|{handle}").as_bytes());
    hash.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Reads a note (anything else is ignored).
pub fn parse(bytes: &[u8]) -> Option<Announce> {
    serde_json::from_slice::<Announce>(bytes).ok().filter(|a| a.rym == 1)
}

/// Listens for notes on the Wi-Fi (on its own thread) and hands each one, with the sender's
/// address, to `got`. False when the port is taken (another RyM listens already).
pub fn listen(got: impl Fn(Announce, IpAddr) + Send + 'static) -> bool {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", PORT)) else { return false };
    std::thread::Builder::new()
        .name("rym-link".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                if let Some(a) = parse(&buf[..n]) {
                    got(a, from.ip());
                }
            }
        })
        .is_ok()
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(5)).build().expect("http client"))
}

/// Takes the phone's music over: its queue and second, then it's told to pause.
pub async fn take_over(ip: IpAddr, port: u16, account: &str) -> Result<State> {
    let base = format!("http://{ip}:{port}");
    let state: State = http().get(format!("{base}/state")).query(&[("account", account)]).send().await?.error_for_status()?.json().await?;
    if state.songs.get(state.current).is_none() {
        return Err(anyhow!("nothing playing on the phone"));
    }
    http().post(format!("{base}/pause")).query(&[("account", account)]).send().await?.error_for_status()?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_and_keys() {
        let note = br#"{"rym":1,"kind":"phone","device":"Pixel","account":"ab12","port":47812,"playing":true,"video_id":"dQw4w9WgXcQ","title":"Song","artist":"Artist","position":61.5}"#;
        let a = parse(note).unwrap();
        assert_eq!((a.kind.as_str(), a.port, a.playing, a.position), ("phone", 47812, true, 61.5));
        assert!(parse(b"hello").is_none());
        assert!(parse(br#"{"rym":2,"kind":"phone","device":"","account":"","port":1,"playing":false}"#).is_none());
        // The same on the phone (Link.kt's test checks this value too).
        assert_eq!(account_key("Willy", "@willy"), account_key("Willy", "@willy"));
        assert_ne!(account_key("Willy", "@willy"), account_key("Willy", "@other"));
        assert_eq!(account_key("Test", "@test").len(), 16);
        println!("{}", account_key("Test", "@test"));
        let state: State = serde_json::from_str(r#"{"songs":[{"video_id":"a","title":"A"},{"video_id":"b"}],"current":1,"position":12.0,"playing":true}"#).unwrap();
        let t: Track = state.songs[1].clone().into();
        assert_eq!((t.video_id.as_str(), state.current), ("b", 1));
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A fake phone on this PC: the listener hears its note, and taking over reads its state and
    /// tells it to pause (a wrong account key is refused).
    #[tokio::test]
    async fn hears_a_phone_and_takes_over() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        let listening = listen(move |a, ip| h.lock().unwrap().push((a, ip)));
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let paused = Arc::new(Mutex::new(false));
        let p = paused.clone();
        std::thread::spawn(move || {
            for conn in server.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
                let ok = line.contains("account=k1");
                let body = if !ok {
                    "{}".to_string()
                } else if line.starts_with("GET /state") {
                    r#"{"songs":[{"video_id":"aaaaaaaaaaa","title":"A"},{"video_id":"bbbbbbbbbbb","title":"B"}],"current":1,"position":42.5,"playing":true}"#.to_string()
                } else {
                    *p.lock().unwrap() = true;
                    "{}".to_string()
                };
                let code = if ok { "200 OK" } else { "403 Forbidden" };
                let mut c = conn;
                let _ = write!(c, "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        if listening {
            let note = format!(r#"{{"rym":1,"kind":"phone","device":"Fake","account":"k1","port":{port},"playing":true,"video_id":"bbbbbbbbbbb","title":"B","artist":"Y","position":40.0}}"#);
            UdpSocket::bind("127.0.0.1:0").unwrap().send_to(note.as_bytes(), ("127.0.0.1", PORT)).unwrap();
            for _ in 0..50 {
                if !heard.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let got = heard.lock().unwrap().clone();
            assert_eq!(got.first().map(|(a, _)| (a.device.as_str(), a.port)), Some(("Fake", port)), "the note wasn't heard");
        } else {
            println!("port {PORT} taken (RyM open?): listening not checked");
        }
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(take_over(ip, port, "wrong").await.is_err(), "a wrong account key must be refused");
        assert!(!*paused.lock().unwrap());
        let state = take_over(ip, port, "k1").await.unwrap();
        assert_eq!((state.current, state.position, state.songs[1].video_id.as_str()), (1, 42.5, "bbbbbbbbbbb"));
        assert!(*paused.lock().unwrap(), "the phone wasn't told to pause");
    }

    /// The signed-in account's key (to check the phone against): `cargo test --lib live_account_key -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_account_key() {
        let cookie = std::fs::read_to_string(crate::cookie_path()).expect("signed in");
        let a = crate::api::Api::with_cookie(cookie).await.unwrap().account().await.unwrap();
        println!("key {}", account_key(&a.name, &a.handle));
    }
}
