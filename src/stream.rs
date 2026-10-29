//! Song audio: links come from YouTube's player API ([`crate::innertube`]); yt-dlp is only a
//! fallback when it happens to be installed. Playback streams the link in ranges (so seeking
//! works before the song has arrived); only explicit downloads write files.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use symphonia::core::io::MediaSource;
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};

/// Best first: Premium Opus 256k, Opus 160k, Premium AAC 256k, AAC 128k.
const FORMATS: &str = "774/251/141/140/bestaudio";
const STREAM_URL_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct ResolvedStream {
    url: String,
    headers: Vec<(String, String)>,
    expires_at: Instant,
    /// Known for InnerTube links: allows ranged reads and seeking.
    size: Option<u64>,
    ext: Option<&'static str>,
    seconds: Option<f32>,
}

fn stream_url_cache() -> &'static StdMutex<HashMap<String, ResolvedStream>> {
    static CACHE: OnceLock<StdMutex<HashMap<String, ResolvedStream>>> = OnceLock::new();
    CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn cached_stream(video_id: &str) -> Option<ResolvedStream> {
    let mut cache = stream_url_cache().lock().ok()?;
    let entry = cache.get(video_id).cloned()?;
    if entry.expires_at > Instant::now() {
        Some(entry)
    } else {
        cache.remove(video_id);
        None
    }
}

async fn stream_resolve_lock(video_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .await;
    locks
        .entry(video_id.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn resolve_stream(video_id: &str, cookies: Option<PathBuf>) -> Result<ResolvedStream> {
    if let Some(stream) = cached_stream(video_id) {
        return Ok(stream);
    }
    let lock = stream_resolve_lock(video_id).await;
    let _guard = lock.lock().await;
    if let Some(stream) = cached_stream(video_id) {
        return Ok(stream);
    }

    let stream = match crate::innertube::audio(video_id).await {
        Ok(a) => ResolvedStream {
            url: a.url,
            headers: vec![("User-Agent".into(), a.user_agent.into())],
            // InnerTube links last about six hours; refresh well before.
            expires_at: Instant::now() + Duration::from_secs(60 * 60),
            size: a.size,
            ext: Some(a.ext),
            seconds: a.seconds,
        },
        Err(e) => ytdlp_stream(video_id, cookies)
            .await
            .map_err(|y| anyhow!("{e}; yt-dlp fallback: {y}"))?,
    };
    if let Ok(mut cache) = stream_url_cache().lock() {
        cache.insert(video_id.to_owned(), stream.clone());
        while cache.len() > 128 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
    }
    Ok(stream)
}

/// The old way, kept for songs InnerTube refuses: ask an installed yt-dlp for the link.
async fn ytdlp_stream(video_id: &str, cookies: Option<PathBuf>) -> Result<ResolvedStream> {
    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "--js-runtimes",
        "node",
        "--quiet",
        "--no-warnings",
        "--no-progress",
        "--no-check-formats",
        "--no-playlist",
        "-f",
        FORMATS,
        "--dump-single-json",
    ]);
    if let Some(path) = cookies {
        cmd.arg("--cookies").arg(path);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow!("yt-dlp not found: {e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .find(|line| line.contains("ERROR:"))
            .unwrap_or("yt-dlp could not resolve this song")
            .to_owned();
        return Err(anyhow!(detail));
    }

    let info: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow!("yt-dlp returned invalid stream details: {e}"))?;
    let selected = info
        .get("requested_downloads")
        .and_then(serde_json::Value::as_array)
        .and_then(|formats| formats.first())
        .unwrap_or(&info);
    let url = selected
        .get("url")
        .and_then(serde_json::Value::as_str)
        .or_else(|| info.get("url").and_then(serde_json::Value::as_str))
        .ok_or_else(|| anyhow!("yt-dlp did not return a playable stream URL"))?
        .to_owned();
    let headers = selected
        .get("http_headers")
        .or_else(|| info.get("http_headers"))
        .and_then(serde_json::Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value.as_str().map(|value| (name.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ResolvedStream {
        url,
        headers,
        expires_at: Instant::now() + STREAM_URL_TTL,
        size: None,
        ext: None,
        seconds: None,
    })
}

/// Resolve a URL ahead of time. Only the short-lived URL and headers stay in memory; audio bytes
/// are not fetched until the user starts playback.
pub async fn warm(video_id: String, cookies: Option<PathBuf>) {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    let slots = SLOTS.get_or_init(|| Semaphore::new(2));
    let Ok(_permit) = slots.acquire().await else {
        return;
    };
    let _ = resolve_stream(&video_id, cookies).await;
}

pub fn cache_dir() -> PathBuf {
    crate::data_dir().join("cache")
}

/// Already-downloaded file for this video, if any.
pub fn cached(video_id: &str) -> Option<PathBuf> {
    let dir = cache_dir();
    ["webm", "m4a", "opus", "mp4"]
        .iter()
        .map(|ext| dir.join(format!("{video_id}.{ext}")))
        .find(|p| p.exists())
}

/// Song length saved by an earlier download, in seconds.
pub fn cached_duration(video_id: &str) -> Option<f32> {
    std::fs::read_to_string(cache_dir().join(format!("{video_id}.secs")))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn blocking_http() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("build stream HTTP client")
    })
}

/// Bytes asked for per request. YouTube slows down single requests for whole songs.
const RANGE: u64 = 4 << 20;

/// Opens the song's audio for the decoder. Nothing is saved to disk.
pub async fn open_playback(video_id: String, cookies: Option<PathBuf>) -> Result<StreamingAudio> {
    let stream = resolve_stream(&video_id, cookies).await?;
    tokio::task::spawn_blocking(move || {
        let mut audio = StreamingAudio { stream, pos: 0, body: None };
        // Fail here (not inside the decoder) when the link doesn't work.
        audio.open_at(0)?;
        Ok(audio)
    })
    .await?
}

/// The song's audio over HTTP, read in ranges; only the current response buffer is in memory.
pub struct StreamingAudio {
    stream: ResolvedStream,
    pos: u64,
    /// The open response and the byte after its last one.
    body: Option<(reqwest::blocking::Response, u64)>,
}

impl StreamingAudio {
    /// File type, for the decoder's format guess.
    pub fn ext(&self) -> Option<&'static str> {
        self.stream.ext
    }

    /// Length YouTube reported, for formats that don't carry one (WebM).
    pub fn seconds(&self) -> Option<f32> {
        self.stream.seconds
    }

    fn open_at(&mut self, pos: u64) -> io::Result<()> {
        let mut request = blocking_http().get(&self.stream.url);
        for (name, value) in &self.stream.headers {
            request = request.header(name, value);
        }
        let end = match self.stream.size {
            Some(size) => {
                let end = (pos + RANGE).min(size);
                request = request.header("Range", format!("bytes={pos}-{}", end - 1));
                end
            }
            None => u64::MAX,
        };
        let response = request
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| io::Error::other(format!("audio stream: {e}")))?;
        self.body = Some((response, end));
        Ok(())
    }
}

impl Read for StreamingAudio {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stream.size.is_some_and(|size| self.pos >= size) {
                return Ok(0);
            }
            if self.body.as_ref().is_none_or(|(_, end)| self.pos >= *end) {
                self.open_at(self.pos)?;
            }
            let (body, end) = self.body.as_mut().expect("opened above");
            let n = body.read(buf)?;
            if n == 0 {
                if self.stream.size.is_none() {
                    return Ok(0);
                }
                // The range ended early: ask again from here.
                *end = self.pos;
                continue;
            }
            self.pos += n as u64;
            return Ok(n);
        }
    }
}

impl Seek for StreamingAudio {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let Some(size) = self.stream.size else {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "this stream is not seekable"));
        };
        let target = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(d) => size as i64 + d,
            SeekFrom::Current(d) => self.pos as i64 + d,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        let target = target as u64;
        // Just ahead inside the open range: read on instead of making a new request.
        let near = self.body.as_ref().is_some_and(|(_, end)| target >= self.pos && target < *end && target - self.pos < 256 << 10);
        if near {
            let mut skip = target - self.pos;
            let mut scratch = [0u8; 8192];
            while skip > 0 {
                let n = self.read(&mut scratch[..skip.min(8192) as usize])?;
                if n == 0 {
                    break;
                }
                skip -= n as u64;
            }
        } else {
            self.body = None;
            self.pos = target;
        }
        Ok(self.pos)
    }
}

impl MediaSource for StreamingAudio {
    fn is_seekable(&self) -> bool {
        self.stream.size.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.stream.size
    }
}

pub async fn fetch(video_id: String, cookies: Option<PathBuf>) -> Result<PathBuf> {
    let lock = fetch_lock(&video_id).await;
    let _guard = lock.lock().await;
    if let Some(p) = cached(&video_id) {
        return Ok(p);
    }
    // Download into a private folder, then move into the cache: a prefetch and a play of the
    // same song can run at once, and the cache must never expose a half-written file.
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = cache_dir();
    let tmp = dir.join(format!(
        ".dl-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    tokio::fs::create_dir_all(&tmp).await?;
    let result = download(&video_id, cookies, &tmp).await.and_then(|p| {
        let name = p.file_name().ok_or_else(|| anyhow!("bad file name"))?;
        let dest = dir.join(name);
        if !dest.exists() {
            std::fs::rename(&p, &dest)?;
        }
        Ok(dest)
    });
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    result
}

/// A play request and a background prefetch for the same song share one download.
async fn fetch_lock(video_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .await;
    locks
        .entry(video_id.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn download(video_id: &str, cookies: Option<PathBuf>, dir: &Path) -> Result<PathBuf> {
    let stream = resolve_stream(video_id, cookies.clone()).await?;
    let Some(ext) = stream.ext else {
        return download_ytdlp(video_id, cookies, dir).await;
    };
    let path = dir.join(format!("{video_id}.{ext}"));
    let seconds = stream.seconds;
    let dest = path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut audio = StreamingAudio { stream, pos: 0, body: None };
        let mut file = std::fs::File::create(&dest)?;
        io::copy(&mut audio, &mut file)?;
        Ok(())
    })
    .await??;
    if let Some(secs) = seconds {
        let _ = std::fs::write(cache_dir().join(format!("{video_id}.secs")), secs.to_string());
    }
    Ok(path)
}

async fn download_ytdlp(video_id: &str, cookies: Option<PathBuf>, dir: &Path) -> Result<PathBuf> {
    let out = dir.join("%(id)s.%(ext)s");

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "--js-runtimes",
        "node",
        "-f",
        FORMATS,
        "--no-playlist",
        "--no-progress",
    ])
    .args([
        "--no-part",
        "--print",
        "after_move:%(duration)s\t%(filepath)s",
        "-o",
    ])
    .arg(&out);
    if let Some(c) = &cookies {
        cmd.arg("--cookies").arg(c);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow!("yt-dlp not found: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .unwrap_or("yt-dlp failed");
        return Err(anyhow!("{last}"));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let (secs, path) = printed
        .lines()
        .last()
        .and_then(|l| l.trim().split_once('\t'))
        .unzip();
    // WebM files don't carry their length; keep the one YouTube reported next to the cache.
    if let Some(secs) = secs.and_then(|s| s.parse::<f32>().ok()) {
        let _ = std::fs::write(
            cache_dir().join(format!("{video_id}.secs")),
            secs.to_string(),
        );
    }
    path.map(PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| anyhow!("download finished but file is missing"))
}

/// yt-dlp wants a Netscape cookie file; convert the browser's `Cookie:` header.
pub fn write_cookie_file(header: &str, path: &Path) -> Result<()> {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        out.push_str(&format!(
            ".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{name}\t{value}\n"
        ));
    }
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod range_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A tiny local server that answers `Range` requests from `data`, a few bytes short at
    /// times like a real one that cuts a response early.
    fn serve(data: Arc<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (n, conn) in listener.incoming().enumerate() {
                let Ok(mut conn) = conn else { continue };
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                    }
                }
                let (a, b) = range.unwrap_or((0, data.len() - 1));
                // Every third answer stops 1000 bytes early.
                let b = if n % 3 == 2 && b - a > 2000 { b - 1000 } else { b };
                let body = &data[a..=b];
                let head = format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(body);
            }
        });
        format!("http://{addr}/audio")
    }

    fn audio(url: String, size: u64) -> StreamingAudio {
        let stream = ResolvedStream { url, headers: Vec::new(), expires_at: Instant::now(), size: Some(size), ext: Some("webm"), seconds: None };
        StreamingAudio { stream, pos: 0, body: None }
    }

    #[test]
    fn reads_in_ranges_and_seeks() {
        let data: Arc<Vec<u8>> = Arc::new((0..(RANGE as usize * 2 + 12345)).map(|i| (i * 7 % 251) as u8).collect());
        let url = serve(data.clone());

        let mut all = Vec::new();
        audio(url.clone(), data.len() as u64).read_to_end(&mut all).unwrap();
        assert!(all == *data, "whole read matches");

        let mut a = audio(url, data.len() as u64);
        let mut buf = [0u8; 1000];
        a.read_exact(&mut buf).unwrap();
        for target in [RANGE + 77, 5, data.len() as u64 - 300, 6000] {
            assert_eq!(a.seek(SeekFrom::Start(target)).unwrap(), target);
            let n = a.read(&mut buf).unwrap();
            assert!(n > 0 && buf[..n] == data[target as usize..target as usize + n], "read after seek to {target}");
        }
        assert_eq!(a.seek(SeekFrom::End(0)).unwrap(), data.len() as u64);
        assert_eq!(a.read(&mut buf).unwrap(), 0);
    }
}
