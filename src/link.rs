//! RyM Link: the phone tells the PCs on its Wi-Fi what it plays, and a PC can take it over (the
//! song goes on here from the same second, with its queue, and the phone pauses).
//!
//! The phone sends a small JSON note by UDP broadcast every few seconds while RyM runs on it
//! ([`Announce`], port [`PORT`]); it answers `GET /state` (the queue and where it is) and
//! `POST /pause` on the TCP port the note gives. A computer sends the same notes, and also
//! answers `POST /control?do=…&value=…` (play, pause, toggle, next, prev, seek, volume) when
//! remote control is on: the phone's remote, and its "Play along" (the same song in step). Only devices signed in to the same Google
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

/// What this computer plays, told to the phones (Seamless Music the other way): a phone of
/// the same account that's idle takes it over. Kept up to date by the app.
#[derive(Debug, Clone, Default)]
pub struct Playing {
    /// The account's key; None: signed out (nothing is sent then).
    pub account: Option<String>,
    /// The setting is on.
    pub enabled: bool,
    pub songs: Vec<Track>,
    pub current: usize,
    /// Seconds into the song at `since`, and whether it moves on.
    pub position: f32,
    pub since: Option<std::time::Instant>,
    pub playing: bool,
    /// The song's length and the volume (0 to 1), for the phone's remote.
    pub duration: f32,
    pub volume: f32,
    /// The phone may control this computer's music.
    pub remote: bool,
}

impl Playing {
    fn position_now(&self) -> f32 {
        self.position + if self.playing { self.since.map_or(0.0, |s| s.elapsed().as_secs_f32()) } else { 0.0 }
    }

    fn track_json(t: &Track) -> serde_json::Value {
        serde_json::json!({ "video_id": t.video_id, "title": t.title, "artist": t.artist, "album": t.album, "duration": t.duration, "thumb": t.thumb })
    }

    /// The note sent every 3 seconds.
    pub fn note(&self, device: &str, port: u16) -> Option<String> {
        let key = self.account.as_ref().filter(|_| self.enabled)?;
        let t = self.songs.get(self.current);
        let mut n = serde_json::json!({ "rym": 1, "kind": "pc", "device": device, "account": key, "port": port, "playing": self.playing && t.is_some(), "remote": self.remote, "volume": self.volume, "duration": self.duration });
        if let Some(t) = t {
            n["video_id"] = t.video_id.clone().into();
            n["title"] = t.title.clone().into();
            n["artist"] = t.artist.clone().into();
            n["thumb"] = t.thumb.clone().into();
            n["position"] = self.position_now().into();
        }
        Some(n.to_string())
    }

    /// `GET /state`: the queue around the song (100 before, 400 after) and where it is.
    pub fn state(&self) -> Option<String> {
        self.songs.get(self.current)?;
        let from = self.current.saturating_sub(100);
        let to = (self.current + 401).min(self.songs.len());
        let songs: Vec<serde_json::Value> = self.songs[from..to].iter().map(Self::track_json).collect();
        Some(serde_json::json!({ "songs": songs, "current": self.current - from, "position": self.position_now(), "playing": self.playing, "duration": self.duration, "volume": self.volume, "remote": self.remote }).to_string())
    }
}

/// What the phone's remote asked: the action and its value (seconds for `seek`, 0 to 1 for `volume`).
pub fn control_of(query: &str) -> Option<(String, f32)> {
    let mut what = None;
    let mut value = 0.0;
    for kv in query.split('&') {
        match kv.split_once('=') {
            Some(("do", v)) if ["play", "pause", "toggle", "next", "prev", "seek", "volume"].contains(&v) => what = Some(v.to_string()),
            Some(("value", v)) => value = v.parse::<f32>().ok().filter(|x| x.is_finite()).unwrap_or(0.0),
            _ => {}
        }
    }
    what.map(|w| (w, value))
}

/// This computer's address on its network: the one its internet goes through (nothing is sent:
/// a UDP "connect" only picks the way). A note sent from a socket on it leaves by that network,
/// not by another one Windows may pick for 255.255.255.255 (VirtualBox's or Hyper-V's adapter,
/// a Wi-Fi that isn't connected), where the phone would never hear it.
pub fn lan_ipv4() -> Option<std::net::Ipv4Addr> {
    let s = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Tells the phones what this computer plays (a note every 3 seconds; every second while it
/// plays, so "Play along" keeps in step) and answers their `/state`, `/pause` and `/control`
/// (with the account's key); `pause` runs when a phone took the music, `control` for the remote.
pub fn announce(device: String, playing: std::sync::Arc<std::sync::Mutex<Playing>>, pause: impl Fn() + Send + Sync + 'static, control: impl Fn(String, f32) + Send + Sync + 'static) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let Ok(server) = std::net::TcpListener::bind(("0.0.0.0", 0)) else { return false };
    let Ok(port) = server.local_addr().map(|a| a.port()) else { return false };
    let answers = playing.clone();
    let spawned = std::thread::Builder::new().name("rym-link-server".into()).spawn(move || {
        for client in server.incoming().flatten() {
            let _ = client.set_read_timeout(Some(Duration::from_secs(3)));
            let mut line = String::new();
            if BufReader::new(&client).read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
            let asked = target.split_once('?').map_or("", |(_, q)| q).split('&').find_map(|kv| kv.strip_prefix("account=")).unwrap_or_default();
            let (code, body) = {
                let p = answers.lock().map(|p| p.clone()).unwrap_or_default();
                let ok = p.enabled && p.account.as_deref().is_some_and(|k| k == asked);
                match (ok, method) {
                    (false, _) => (403, "{}".to_string()),
                    (true, "GET") if target.starts_with("/state") => p.state().map_or((404, "{}".to_string()), |s| (200, s)),
                    (true, "POST") if target.starts_with("/pause") => {
                        pause();
                        (200, "{}".to_string())
                    }
                    (true, "POST") if target.starts_with("/control") => match (p.remote, control_of(target.split_once('?').map_or("", |(_, q)| q))) {
                        (false, _) => (403, "{}".to_string()),
                        (true, Some((what, value))) => {
                            control(what, value);
                            (200, "{}".to_string())
                        }
                        (true, None) => (400, "{}".to_string()),
                    },
                    _ => (404, "{}".to_string()),
                }
            };
            let mut c = &client;
            let _ = write!(c, "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    if spawned.is_err() {
        return false;
    }
    std::thread::Builder::new()
        .name("rym-link-notes".into())
        .spawn(move || {
            let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else { return };
            let _ = socket.set_broadcast(true);
            // A socket on the network's own address too (see [`lan_ipv4`]), made again when it changes.
            let mut lan: Option<(std::net::Ipv4Addr, UdpSocket)> = None;
            loop {
                let (note, often) = playing.lock().ok().map(|p| (p.note(&device, port), p.playing)).unwrap_or((None, false));
                if let Some(n) = note {
                    let _ = socket.send_to(n.as_bytes(), ("255.255.255.255", PORT));
                    let ip = lan_ipv4();
                    if lan.as_ref().map(|(a, _)| *a) != ip {
                        lan = ip.and_then(|ip| UdpSocket::bind((ip, 0)).ok().filter(|s| s.set_broadcast(true).is_ok()).map(|s| (ip, s)));
                    }
                    if let Some((_, s)) = &lan {
                        let _ = s.send_to(n.as_bytes(), ("255.255.255.255", PORT));
                    }
                }
                std::thread::sleep(Duration::from_secs(if often { 1 } else { 3 }));
            }
        })
        .is_ok()
}

// ---------------------------------------------------------------- one RyM: links handed over

/// The port on this computer (127.0.0.1 only) where the running RyM takes links from another
/// copy started with one ("Play in RyM": a `rym://` link, a link on the command line).
pub const OPEN_PORT: u16 = 47819;
const OPEN_HELLO: &str = "RYM-OPEN ";

/// Hands `link` to the RyM already running here; true when it took it (this copy can quit).
pub fn hand_over(link: &str) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], OPEN_PORT));
    let Ok(mut c) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return false };
    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
    let one_line: String = link.chars().filter(|c| !c.is_control()).take(4000).collect();
    if write!(c, "{OPEN_HELLO}{one_line}\n").is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(&c).read_line(&mut answer).is_ok() && answer.trim() == "OK"
}

/// Takes the links other copies hand over (see [`hand_over`]), on its own thread; `got` gets
/// each one. False when another RyM takes them already.
pub fn take_handed_links(got: impl Fn(String) + Send + 'static) -> bool {
    use std::io::{BufRead, BufReader, Read, Write};
    let Ok(server) = std::net::TcpListener::bind(("127.0.0.1", OPEN_PORT)) else { return false };
    std::thread::Builder::new()
        .name("rym-open".into())
        .spawn(move || {
            for client in server.incoming().flatten() {
                let _ = client.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                if BufReader::new((&client).take(4200)).read_line(&mut line).is_err() {
                    continue;
                }
                let Some(link) = line.trim_end().strip_prefix(OPEN_HELLO).map(str::to_string) else { continue };
                let mut c = &client;
                let _ = c.write_all(b"OK\n");
                if !link.is_empty() {
                    got(link);
                }
            }
        })
        .is_ok()
}

/// Windows: `rym://` links open this RyM (HKEY_CURRENT_USER\Software\Classes\rym, which Windows
/// shows as HKEY_CLASSES_ROOT\rym for this user; no administrator needed). Written only when it
/// points elsewhere; not for a build in a `target` folder.
#[cfg(windows)]
pub fn register_rym_links() {
    use std::os::windows::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.to_string_lossy().to_string();
    if exe.to_ascii_lowercase().contains(r"\target\") {
        return;
    }
    let key = r"HKCU\Software\Classes\rym";
    let command = format!("\"{exe}\" \"%1\"");
    let reg = |args: &[&str]| std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output();
    let now = reg(&["query", &format!(r"{key}\shell\open\command"), "/ve"]).map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if now.contains(&command) {
        return;
    }
    let icon = format!("\"{exe}\",0");
    let _ = reg(&["add", key, "/ve", "/d", "URL:RyM", "/f"]);
    let _ = reg(&["add", key, "/v", "URL Protocol", "/d", "", "/f"]);
    let _ = reg(&["add", &format!(r"{key}\DefaultIcon"), "/ve", "/d", &icon, "/f"]);
    let _ = reg(&["add", &format!(r"{key}\shell\open\command"), "/ve", "/d", &command, "/f"]);
}

/// Elsewhere the `.desktop` file says it (`MimeType=x-scheme-handler/rym;`).
#[cfg(not(windows))]
pub fn register_rym_links() {}

#[cfg(test)]
mod pc_tests {
    use super::*;

    #[test]
    fn the_networks_own_address() {
        // A PC with a network has one; it's never 0.0.0.0 or 127.x.
        if let Some(ip) = lan_ipv4() {
            println!("lan {ip}");
            assert!(!ip.is_loopback() && !ip.is_unspecified());
            assert!(UdpSocket::bind((ip, 0)).is_ok());
        }
    }

    #[test]
    fn a_link_is_handed_over() {
        let (tx, rx) = std::sync::mpsc::channel();
        if !take_handed_links(move |l| tx.send(l).unwrap()) {
            println!("port {OPEN_PORT} taken (RyM open?): not checked");
            return;
        }
        assert!(hand_over("rym://play?v=dQw4w9WgXcQ\nsecond line"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "rym://play?v=dQw4w9WgXcQsecond line");
    }

    #[test]
    fn remote_requests() {
        assert_eq!(control_of("account=k&do=seek&value=42.5"), Some(("seek".into(), 42.5)));
        assert_eq!(control_of("do=next&account=k"), Some(("next".into(), 0.0)));
        assert_eq!(control_of("do=format-disk"), None);
        assert_eq!(control_of("do=volume&value=NaN"), Some(("volume".into(), 0.0)));
    }

    #[test]
    fn the_pc_note_and_state() {
        let t = |id: &str| Track { video_id: id.into(), title: id.into(), artist: "A".into(), album: String::new(), duration: "3:00".into(), thumb: None, album_id: None, artist_id: None, link: None };
        let mut p = Playing { account: Some("k".into()), enabled: true, songs: vec![t("a"), t("b")], current: 1, position: 10.0, since: None, playing: true, duration: 200.0, volume: 0.5, remote: true };
        let note: Announce = parse(p.note("PC", 5000).unwrap().as_bytes()).unwrap();
        assert_eq!((note.kind.as_str(), note.video_id.as_str(), note.port), ("pc", "b", 5000));
        let state: State = serde_json::from_str(&p.state().unwrap()).unwrap();
        assert_eq!((state.current, state.songs.len()), (1, 2));
        p.enabled = false;
        assert!(p.note("PC", 5000).is_none());
    }
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
        let cookie = crate::secret::read_cookie_io().expect("signed in");
        let a = crate::api::Api::with_cookie(cookie).await.unwrap().account().await.unwrap();
        println!("key {}", account_key(&a.name, &a.handle));
    }
}
