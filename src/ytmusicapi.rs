//! The ytmusicapi engine: github.com/sigma67/ytmusicapi (Python, MIT) for search and radio,
//! with the built-in engine (ytmapi-rs and InnerTube, see [`crate::api`]) as the fallback.
//!
//! RyM keeps one Python process running `assets/ytm_bridge.py` (written into the data folder)
//! and talks to it in JSON lines. Python comes from the system (the one yt-dlp's pip install
//! uses); ytmusicapi is installed with pip the first time it's needed, and by "Update parsers".
//!
//! Which engine answers is a setting:
//! - Auto: ytmusicapi first; when it fails, breaks or finds nothing, the built-in engine answers.
//! - ytmusicapi only.
//! - Built-in only (InnerTube): the rollback when a YouTube change breaks ytmusicapi.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::api::Track;

const SCRIPT: &str = include_str!("../assets/ytm_bridge.py");
/// How long one answer may take before the bridge is restarted.
const TIMEOUT: Duration = Duration::from_secs(20);
/// After the bridge couldn't start (no Python, no ytmusicapi), the next try waits this long.
const RETRY_AFTER: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// ytmusicapi first, the built-in engine when it fails.
    Auto,
    /// ytmusicapi only.
    YtMusicApi,
    /// The built-in engine only (the rollback).
    Builtin,
}

impl Engine {
    pub fn from_index(i: u8) -> Engine {
        match i {
            1 => Engine::YtMusicApi,
            2 => Engine::Builtin,
            _ => Engine::Auto,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            Engine::Auto => 0,
            Engine::YtMusicApi => 1,
            Engine::Builtin => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::Auto => "Auto (ytmusicapi, built-in when it fails)",
            Engine::YtMusicApi => "ytmusicapi only (new)",
            Engine::Builtin => "Built-in InnerTube only (rollback)",
        }
    }
}

static ENGINE: AtomicU8 = AtomicU8::new(0);
/// The app's language for YouTube Music's answers ("en" or "ar").
static LANG: Mutex<&'static str> = Mutex::new("en");

pub fn use_engine(e: Engine) {
    ENGINE.store(e.index(), Ordering::Relaxed);
}

pub fn engine() -> Engine {
    Engine::from_index(ENGINE.load(Ordering::Relaxed))
}

pub fn use_language(arabic: bool) {
    if let Ok(mut l) = LANG.lock() {
        *l = if arabic { "ar" } else { "en" };
    }
}

/// A result worth keeping: an empty list from ytmusicapi counts as a failure in Auto (a parser
/// that broke quietly), so the built-in engine gets its turn.
pub trait Found {
    fn found(&self) -> bool;
}

impl<T> Found for Vec<T> {
    fn found(&self) -> bool {
        !self.is_empty()
    }
}

impl Found for crate::api::Paged {
    fn found(&self) -> bool {
        !self.tracks.is_empty()
    }
}

/// Runs `new` (ytmusicapi) and/or `builtin` as the engine setting says.
pub async fn hybrid<T: Found, N, B>(what: &str, new: N, builtin: impl FnOnce() -> B) -> Result<T>
where
    N: Future<Output = Result<T>>,
    B: Future<Output = Result<T>>,
{
    match engine() {
        Engine::Builtin => builtin().await,
        Engine::YtMusicApi => new.await,
        Engine::Auto => match new.await {
            Ok(v) if v.found() => Ok(v),
            Ok(_) => builtin().await,
            Err(e) => {
                note_failure(what, &e);
                builtin().await
            }
        },
    }
}

/// The last fallback, for the settings ("search: TypeError: ... → built-in").
static LAST_FAILURE: Mutex<Option<String>> = Mutex::new(None);

fn note_failure(what: &str, e: &anyhow::Error) {
    if let Ok(mut f) = LAST_FAILURE.lock() {
        *f = Some(format!("{what}: {e}"));
    }
}

pub fn last_failure() -> Option<String> {
    LAST_FAILURE.lock().ok().and_then(|f| f.clone())
}

// ---------------------------------------------------------------- the bridge process

struct Bridge {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    /// The cookie it's signed in with (a hash; 0 = guest).
    signed_as: u64,
    version: String,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct State {
    bridge: Option<Bridge>,
    /// Starting failed at this time, with this reason (tried again after RETRY_AFTER).
    failed: Option<(Instant, String)>,
    /// pip was already asked to install ytmusicapi in this run.
    installed_once: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { bridge: None, failed: None, installed_once: false }))
}

fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// The system's Python (the same names yt-dlp's pip update tries).
fn pythons() -> &'static [&'static str] {
    if cfg!(windows) { &["python", "py", "python3"] } else { &["python3", "python"] }
}

fn script_path() -> std::path::PathBuf {
    crate::data_dir().join("ytm_bridge.py")
}

fn start() -> Result<Bridge> {
    let path = script_path();
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::create_dir_all(crate::data_dir())?;
        std::fs::write(&path, SCRIPT)?;
    }
    let mut last = anyhow!("Python wasn't found");
    for python in pythons() {
        let child = command(python).arg("-u").arg(&path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last = anyhow!("{python}: {e}");
                continue;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { continue };
        // Lines are read on their own thread, so a stuck bridge can be given up on.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let first = rx.recv_timeout(TIMEOUT).map_err(|_| anyhow!("ytmusicapi didn't start"))?;
        let hello: Value = serde_json::from_str(&first)?;
        if hello["ready"].as_bool() != Some(true) {
            let _ = child.kill();
            return Err(anyhow!("{}", hello["error"].as_str().unwrap_or("ytmusicapi didn't load")));
        }
        let version = hello["version"].as_str().unwrap_or_default().to_string();
        return Ok(Bridge { child, stdin, lines: rx, next_id: 1, signed_as: 0, version });
    }
    Err(last)
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish().max(1)
}

fn send(b: &mut Bridge, mut req: Value) -> Result<Value> {
    let id = b.next_id;
    b.next_id += 1;
    req["id"] = json!(id);
    req["lang"] = json!(*LANG.lock().map_err(|_| anyhow!("lock"))?);
    writeln!(b.stdin, "{req}")?;
    b.stdin.flush()?;
    let until = Instant::now() + TIMEOUT;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let line = match b.lines.recv_timeout(left) {
            Ok(l) => l,
            Err(RecvTimeoutError::Timeout) => return Err(anyhow!("ytmusicapi took too long")),
            Err(RecvTimeoutError::Disconnected) => return Err(anyhow!("ytmusicapi stopped")),
        };
        let Ok(answer) = serde_json::from_str::<Value>(&line) else { continue };
        if answer["id"].as_u64() != Some(id) {
            continue;
        }
        return if answer["ok"].as_bool() == Some(true) {
            Ok(answer["data"].clone())
        } else {
            Err(anyhow!("{}", answer["error"].as_str().unwrap_or("ytmusicapi failed")))
        };
    }
}

/// One request, on a blocking thread: starts (or restarts) the bridge as needed and signs it in
/// with `cookie` (None: as a guest).
fn ask_blocking(req: Value, cookie: Option<String>) -> Result<Value> {
    let mut s = state().lock().map_err(|_| anyhow!("ytmusicapi lock"))?;
    if s.bridge.as_mut().is_some_and(|b| b.child.try_wait().ok().flatten().is_some()) {
        s.bridge = None;
    }
    if s.bridge.is_none() {
        if let Some((at, why)) = &s.failed
            && at.elapsed() < RETRY_AFTER
        {
            return Err(anyhow!("{why}"));
        }
        let mut started = start();
        // Not installed yet: pip installs it once, then it starts again.
        if started.as_ref().is_err_and(|e| e.to_string().contains("missing")) && !s.installed_once {
            s.installed_once = true;
            if pip_install().is_ok() {
                started = start();
            }
        }
        match started {
            Ok(b) => {
                s.bridge = Some(b);
                s.failed = None;
            }
            Err(e) => {
                s.failed = Some((Instant::now(), e.to_string()));
                return Err(e);
            }
        }
    }
    let b = s.bridge.as_mut().expect("started");
    let want = cookie.as_deref().map_or(0, hash);
    if b.signed_as != want {
        send(b, json!({ "op": "auth", "cookie": cookie.unwrap_or_default() }))?;
        b.signed_as = want;
    }
    let answer = send(b, req);
    if answer.as_ref().is_err_and(|e| e.to_string().contains("took too long") || e.to_string().contains("stopped")) {
        s.bridge = None;
    }
    answer
}

/// A request waiting for the bridge: searches (priority 0) go before radios and mixes (1).
struct Job {
    priority: u8,
    seq: u64,
    req: Value,
    cookie: Option<String>,
    done: tokio::sync::oneshot::Sender<Result<Value>>,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        (self.priority, self.seq) == (other.priority, other.seq)
    }
}

impl Eq for Job {}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Job {
    /// The heap pops the greatest: the lowest priority number, then the oldest.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (other.priority, other.seq).cmp(&(self.priority, self.seq))
    }
}

struct Jobs {
    heap: Mutex<std::collections::BinaryHeap<Job>>,
    ready: std::sync::Condvar,
}

/// One thread answers the jobs in order of priority (a search typed now doesn't wait behind
/// the home page's radios).
fn jobs() -> &'static Jobs {
    static J: OnceLock<&'static Jobs> = OnceLock::new();
    J.get_or_init(|| {
        let jobs: &'static Jobs = Box::leak(Box::new(Jobs { heap: Mutex::new(std::collections::BinaryHeap::new()), ready: std::sync::Condvar::new() }));
        std::thread::Builder::new()
            .name("ytmusicapi".into())
            .spawn(move || {
                loop {
                    let job = {
                        let Ok(mut heap) = jobs.heap.lock() else { return };
                        loop {
                            if let Some(job) = heap.pop() {
                                break job;
                            }
                            heap = match jobs.ready.wait(heap) {
                                Ok(h) => h,
                                Err(_) => return,
                            };
                        }
                    };
                    let _ = job.done.send(ask_blocking(job.req, job.cookie));
                }
            })
            .expect("ytmusicapi thread");
        jobs
    })
}

async fn ask(req: Value, cookie: Option<String>) -> Result<Value> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let priority = if req["op"] == "search" { 0 } else { 1 };
    let (done, answer) = tokio::sync::oneshot::channel();
    let j = jobs();
    j.heap.lock().map_err(|_| anyhow!("ytmusicapi lock"))?.push(Job { priority, seq: SEQ.fetch_add(1, Ordering::Relaxed), req, cookie, done });
    j.ready.notify_one();
    answer.await.map_err(|_| anyhow!("ytmusicapi stopped"))?
}

fn to_tracks(v: Value) -> Result<Vec<Track>> {
    Ok(serde_json::from_value(v)?)
}

/// Search with ytmusicapi; `filter` is "songs" or "videos".
pub async fn search(query: String, filter: &str, cookie: Option<String>) -> Result<Vec<Track>> {
    to_tracks(ask(json!({ "op": "search", "query": query, "filter": filter, "limit": 40 }), cookie).await?)
}

/// A song's radio (YouTube Music's watch queue for it).
pub async fn radio(video_id: String, cookie: Option<String>) -> Result<Vec<Track>> {
    to_tracks(ask(json!({ "op": "radio", "videoId": video_id, "limit": 50 }), cookie).await?)
}

/// A mix's songs ("RD..." playlists, which only exist as watch queues).
pub async fn mix(playlist_id: String, cookie: Option<String>) -> Result<Vec<Track>> {
    to_tracks(ask(json!({ "op": "mix", "playlistId": playlist_id, "limit": 100 }), cookie).await?)
}

fn pip_install() -> Result<String> {
    for python in pythons() {
        let Ok(out) = command(python).args(["-m", "pip", "install", "-U", "ytmusicapi"]).output() else { continue };
        if out.status.success() {
            return Ok("ytmusicapi updated".into());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("pip: {}", err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("failed")));
    }
    Err(anyhow!("Python wasn't found (python.org), so ytmusicapi can't be installed"))
}

/// What's installed, for the settings: "ytmusicapi 1.12.3", or why it isn't working.
pub async fn status() -> String {
    tokio::task::spawn_blocking(|| {
        let s = state().lock();
        if let Ok(s) = &s {
            if let Some(b) = &s.bridge {
                return format!("ytmusicapi {}", b.version);
            }
            if let Some((_, why)) = &s.failed {
                return why.clone();
            }
        }
        drop(s);
        match ask_blocking(json!({ "op": "version" }), None) {
            Ok(v) => format!("ytmusicapi {}", v.as_str().unwrap_or("")),
            Err(e) => e.to_string(),
        }
    })
    .await
    .unwrap_or_default()
}

/// "Update parsers": yt-dlp and ytmusicapi to their newest, the bridge restarted, and the
/// saved answers (links, the client record, saved pages) forgotten so nothing old is reused.
pub async fn update_parsers(nightly: bool) -> String {
    let ytdlp = crate::ytdlp::update(nightly).await;
    let ytm = tokio::task::spawn_blocking(|| {
        let r = pip_install();
        if let Ok(mut s) = state().lock() {
            s.bridge = None;
            s.failed = None;
        }
        r
    })
    .await
    .unwrap_or_else(|e| Err(anyhow!("{e}")));
    let pot = if crate::bgutil::installed() {
        Some(crate::bgutil::install().await.unwrap_or_else(|e| format!("bgutil: {e}")))
    } else {
        None
    };
    crate::innertube::forget_cache();
    crate::page_cache::clear();
    let say = |r: &Result<String>, name: &str| match r {
        Ok(t) => t.clone(),
        Err(e) => format!("{name}: {e}"),
    };
    let fresh = status().await;
    match pot {
        Some(p) => format!("{} · {} · {p} · {fresh}", say(&ytdlp, "yt-dlp"), say(&ytm, "ytmusicapi")),
        None => format!("{} · {} · {fresh}", say(&ytdlp, "yt-dlp"), say(&ytm, "ytmusicapi")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live: the real ytmusicapi through the bridge (needs Python with ytmusicapi):
    /// `cargo test --lib ytmusicapi_live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn ytmusicapi_live() {
        let found = search("Tame Impala The Less I Know".into(), "songs", None).await.expect("search");
        println!("search: {} songs, first {:?} by {:?} ({})", found.len(), found[0].title, found[0].artist, found[0].duration);
        assert!(found.iter().any(|t| t.artist.contains("Tame Impala")));
        let radio = radio(found[0].video_id.clone(), None).await.expect("radio");
        println!("radio: {} songs", radio.len());
        assert!(radio.len() > 10);
        println!("status: {}", status().await);
    }

    /// Live, signed in: the same with your account (needs a sign-in; the cookie is never printed):
    /// `cargo test --lib ytmusicapi_signed_in -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn ytmusicapi_signed_in() {
        let cookie = crate::secret::read_cookie().expect("signed in");
        let found = search("Basim Karbalaei".into(), "songs", Some(cookie.clone())).await.expect("signed-in search");
        println!("signed-in search: {} songs, first {:?}", found.len(), found[0].title);
        let radio = radio(found[0].video_id.clone(), Some(cookie)).await.expect("signed-in radio");
        println!("signed-in radio: {} songs", radio.len());
        assert!(radio.len() > 10);
    }

    #[tokio::test]
    async fn auto_falls_back() {
        use_engine(Engine::Auto);
        let r: Vec<u8> = hybrid("test", async { Err(anyhow!("parser broke")) }, || async { Ok(vec![1]) }).await.unwrap();
        assert_eq!(r, vec![1]);
        let r: Vec<u8> = hybrid("test", async { Ok(vec![]) }, || async { Ok(vec![2]) }).await.unwrap();
        assert_eq!(r, vec![2], "an empty answer counts as a failure");
        let r: Vec<u8> = hybrid("test", async { Ok(vec![3]) }, || async { Ok(vec![4]) }).await.unwrap();
        assert_eq!(r, vec![3]);
        use_engine(Engine::Builtin);
        let r: Vec<u8> = hybrid("test", async { Ok(vec![3]) }, || async { Ok(vec![4]) }).await.unwrap();
        assert_eq!(r, vec![4], "rollback skips ytmusicapi");
        use_engine(Engine::YtMusicApi);
        assert!(hybrid::<Vec<u8>, _, _>("test", async { Err(anyhow!("down")) }, || async { Ok(vec![4]) }).await.is_err(), "forced: no fallback");
        use_engine(Engine::Auto);
    }
}
