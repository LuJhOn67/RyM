//! A song that plays while it downloads. A background task fetches it in ranges into memory; the
//! decoder reads from what has arrived and waits for the rest. When the whole song is in, it's
//! saved to the cache so the next play opens the file.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use symphonia::core::io::MediaSource;

/// Playback starts once this much has arrived.
pub const START_AFTER: usize = 128 << 10;
/// The first request is small so sound starts sooner; later ones are bigger.
const FIRST_RANGE: u64 = 256 << 10;
const RANGE: u64 = 4 << 20;
/// A link that sends nothing for this long is given up on (the caller tries another way).
const FIRST_BYTES_WITHIN: Duration = Duration::from_secs(12);

struct Shared {
    data: Mutex<Vec<u8>>,
    arrived: Condvar,
    /// The song's size once the server said it (0 until then).
    total: AtomicU64,
    done: AtomicBool,
    failed: Mutex<Option<String>>,
}

/// A song on its way. Cloning shares the same download.
#[derive(Clone)]
pub struct Arriving {
    shared: Arc<Shared>,
    /// File type for the decoder ("webm" or "m4a").
    pub ext: String,
    /// Length YouTube reported, for formats that don't carry one (WebM).
    pub seconds: Option<f32>,
    /// The song's id (its file's name in the cache).
    pub id: String,
}

impl std::fmt::Debug for Arriving {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Arriving({} of {} bytes)", self.shared.data.lock().map(|d| d.len()).unwrap_or(0), self.shared.total.load(Ordering::Relaxed))
    }
}

impl Arriving {
    /// Starts downloading `url` and returns once enough has arrived to start playing (or the
    /// song is shorter than that). `save_to` gets the whole song when it's done.
    pub async fn start(
        url: String,
        headers: Vec<(String, String)>,
        ext: String,
        seconds: Option<f32>,
        save_to: PathBuf,
    ) -> Result<Arriving> {
        let shared = Arc::new(Shared {
            data: Mutex::new(Vec::new()),
            arrived: Condvar::new(),
            total: AtomicU64::new(0),
            done: AtomicBool::new(false),
            failed: Mutex::new(None),
        });
        let id = save_to.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let worker = shared.clone();
        tokio::spawn(async move {
            let result = fetch(&url, &headers, &worker).await;
            if let Err(e) = &result {
                *worker.failed.lock().unwrap() = Some(e.to_string());
            }
            worker.done.store(true, Ordering::Release);
            worker.arrived.notify_all();
            if result.is_ok() {
                // Keep the song for next time: written aside, then moved in, so the cache never
                // holds half a file. Written straight from the buffer (no second copy in memory).
                let part = save_to.with_extension("part");
                let written = std::fs::write(&part, worker.data.lock().unwrap().as_slice()).is_ok();
                if written && std::fs::rename(&part, &save_to).is_err() {
                    let _ = std::fs::remove_file(&part);
                }
            }
        });
        // Wait for the start of the song (or the end, or a failure), up to FIRST_BYTES_WITHIN.
        let asked = std::time::Instant::now();
        loop {
            if asked.elapsed() > FIRST_BYTES_WITHIN && !shared.done.load(Ordering::Acquire) {
                return Err(anyhow!("no audio arrived in {} s", FIRST_BYTES_WITHIN.as_secs()));
            }
            let have = shared.data.lock().unwrap().len();
            if have >= START_AFTER || shared.done.load(Ordering::Acquire) {
                if have == 0 {
                    let why = shared.failed.lock().unwrap().clone().unwrap_or_else(|| "no audio arrived".into());
                    return Err(anyhow!(why));
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(Arriving { shared, ext, seconds, id })
    }

    /// A reader for the decoder, from the start of the song.
    pub fn reader(&self) -> ArrivingReader {
        ArrivingReader { shared: self.shared.clone(), pos: 0 }
    }
}

/// After the network goes away mid-song, the download keeps trying this long before it gives up.
const GIVE_UP_AFTER: Duration = Duration::from_secs(120);

/// How one range went.
enum Step {
    /// The whole song is in.
    Done,
    /// This range is in; on to the next.
    More,
}

/// Why a range failed: worth another try (the network dropped, a server hiccup), or not (the
/// link was refused, the song was skipped).
enum Fail {
    Retry(anyhow::Error),
    Stop(anyhow::Error),
}

fn retry(e: reqwest::Error) -> Fail {
    Fail::Retry(anyhow!("audio: {e}"))
}

/// Fetches the song in ranges, adding each piece as it arrives. When the network drops mid-song
/// it tries again from the byte where it stopped (waiting 1, 2, 4... up to 10 s between tries,
/// for up to two minutes), so the song goes on once the network is back. A song nobody plays any
/// more (skipped before it was all in) stops downloading and its memory is let go; it isn't
/// cached.
async fn fetch(url: &str, headers: &[(String, String)], shared: &Arc<Shared>) -> Result<()> {
    let nobody_listens = || Arc::strong_count(shared) == 1;
    let mut pos = 0u64;
    let mut down_since: Option<std::time::Instant> = None;
    let mut tries = 0u32;
    loop {
        match fetch_range(url, headers, shared, &mut pos).await {
            Ok(Step::Done) => return Ok(()),
            Ok(Step::More) => {
                down_since = None;
                tries = 0;
            }
            Err(Fail::Stop(e)) => return Err(e),
            Err(Fail::Retry(e)) => {
                let since = *down_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() > GIVE_UP_AFTER || nobody_listens() {
                    return Err(e);
                }
                tries += 1;
                let wait = Duration::from_secs(1u64 << tries.min(4)).min(Duration::from_secs(10));
                let until = std::time::Instant::now() + wait;
                while std::time::Instant::now() < until {
                    if nobody_listens() {
                        return Err(anyhow!("skipped"));
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }
}

/// One range from `pos` on; `pos` moves with every byte that arrives, so a retry goes on from there.
async fn fetch_range(url: &str, headers: &[(String, String)], shared: &Arc<Shared>, pos: &mut u64) -> std::result::Result<Step, Fail> {
    let nobody_listens = || Arc::strong_count(shared) == 1;
    let client = http();
    {
        let total = shared.total.load(Ordering::Relaxed);
        if total > 0 && *pos >= total {
            return Ok(Step::Done);
        }
        if nobody_listens() {
            return Err(Fail::Stop(anyhow!("skipped")));
        }
        let size = if *pos == 0 { FIRST_RANGE } else { RANGE };
        let end = if total > 0 { (*pos + size).min(total) } else { *pos + size } - 1;
        let mut req = client.get(url).header("Range", format!("bytes={}-{end}", *pos));
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req.send().await.map_err(retry)?;
        let status = resp.status();
        // A refused or gone link won't get better by asking again; a server hiccup might.
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(Fail::Retry(anyhow!("audio: HTTP {status}")));
        }
        let mut resp = resp.error_for_status().map_err(|e| Fail::Stop(anyhow!("audio: {e}")))?;
        let whole = resp.status() == reqwest::StatusCode::OK;
        if let Some(t) = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse::<u64>().ok())
        {
            shared.total.store(t, Ordering::Relaxed);
            // The whole size at once: a growing buffer would take up to twice the song.
            let mut data = shared.data.lock().unwrap();
            let need = (t as usize).saturating_sub(data.len());
            if data.capacity() - data.len() < need {
                data.reserve_exact(need);
            }
        }
        if whole && *pos > 0 {
            // A whole-song answer to a request for the middle: what arrived would come twice.
            return Err(Fail::Stop(anyhow!("audio: the server can't go on from the middle")));
        }
        let before = *pos;
        while let Some(chunk) = resp.chunk().await.map_err(retry)? {
            shared.data.lock().unwrap().extend_from_slice(&chunk);
            *pos += chunk.len() as u64;
            shared.arrived.notify_all();
            if nobody_listens() {
                return Err(Fail::Stop(anyhow!("skipped")));
            }
        }
        if whole {
            // The server sent the whole song at once.
            shared.total.store(*pos, Ordering::Relaxed);
            return Ok(Step::Done);
        }
        if *pos == before {
            return Err(Fail::Retry(anyhow!("audio stopped at {} bytes", *pos)));
        }
        Ok(Step::More)
    }
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    // A download that stops sending for 20 s fails instead of waiting forever.
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(20))
            .build()
            .expect("http client")
    })
}

/// Reads what has arrived; waits when it's ahead of the download.
pub struct ArrivingReader {
    shared: Arc<Shared>,
    pos: u64,
}

impl Read for ArrivingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut data = self.shared.data.lock().unwrap();
        loop {
            let have = data.len() as u64;
            if self.pos < have {
                let n = buf.len().min((have - self.pos) as usize);
                buf[..n].copy_from_slice(&data[self.pos as usize..self.pos as usize + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if self.shared.done.load(Ordering::Acquire) {
                return match self.shared.failed.lock().unwrap().clone() {
                    Some(e) => Err(io::Error::other(e)),
                    None => Ok(0),
                };
            }
            data = self.shared.arrived.wait_timeout(data, Duration::from_millis(200)).unwrap().0;
        }
    }
}

impl Seek for ArrivingReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let total = self.shared.total.load(Ordering::Relaxed);
        let target = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) if total > 0 => total as i64 + d,
            SeekFrom::End(_) => return Err(io::Error::new(io::ErrorKind::Unsupported, "size not known yet")),
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        // Past what has arrived is fine: reads wait until the download gets there.
        self.pos = target as u64;
        Ok(self.pos)
    }
}

impl MediaSource for ArrivingReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.shared.total.load(Ordering::Relaxed)).filter(|t| *t > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A local server answering `Range` requests slowly, a piece at a time. No internet involved.
    fn serve(data: Arc<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let data = data.clone();
                std::thread::spawn(move || {
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
                    let b = b.min(data.len() - 1);
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    let _ = conn.write_all(head.as_bytes());
                    for piece in data[a..=b].chunks(64 << 10) {
                        let _ = conn.write_all(piece);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
            }
        });
        format!("http://{addr}/audio")
    }

    /// Like [`serve`], but the first `cuts` answers stop halfway through (the network dropping).
    fn serve_dropping(data: Arc<Vec<u8>>, cuts: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let left = Arc::new(std::sync::atomic::AtomicUsize::new(cuts));
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let (data, left) = (data.clone(), left.clone());
                std::thread::spawn(move || {
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
                    let b = b.min(data.len() - 1);
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    let _ = conn.write_all(head.as_bytes());
                    let cut = a > 0 && left.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok();
                    let send = if cut { &data[a..a + (b - a + 1) / 2] } else { &data[a..=b] };
                    let _ = conn.write_all(send);
                    // A cut answer just closes: fewer bytes than it promised.
                });
            }
        });
        format!("http://{addr}/audio")
    }

    #[tokio::test]
    async fn goes_on_after_the_network_drops() {
        let data: Arc<Vec<u8>> = Arc::new((0..(9 << 20) + 123).map(|i| (i * 7 % 253) as u8).collect());
        let url = serve_dropping(data.clone(), 2);
        let dir = std::env::temp_dir().join(format!("rym-arriving-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = Arriving::start(url, Vec::new(), "webm".into(), None, dir.join("song.webm")).await.unwrap();
        let all = tokio::task::spawn_blocking(move || {
            let mut r = a.reader();
            let mut all = Vec::new();
            r.read_to_end(&mut all).unwrap();
            all
        })
        .await
        .unwrap();
        assert!(all == *data, "{} of {} bytes after two drops", all.len(), data.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn plays_while_it_arrives_then_keeps_the_file() {
        let data: Arc<Vec<u8>> = Arc::new((0..(6 << 20) + 777).map(|i| (i * 11 % 251) as u8).collect());
        let url = serve(data.clone());
        let dir = std::env::temp_dir().join(format!("rym-arriving-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("song.webm");
        let started = std::time::Instant::now();
        let a = Arriving::start(url, Vec::new(), "webm".into(), None, file.clone()).await.unwrap();
        // Ready to play long before the whole song is in.
        assert!(!a.shared.done.load(Ordering::Acquire), "returned only after the whole song: {:?}", started.elapsed());
        // The reader gives the whole song, waiting where it's ahead, and can jump around.
        let (all, jumped) = tokio::task::spawn_blocking(move || {
            let mut r = a.reader();
            let mut all = Vec::new();
            r.read_to_end(&mut all).unwrap();
            r.seek(SeekFrom::Start(3 << 20)).unwrap();
            let mut ten = [0u8; 10];
            r.read_exact(&mut ten).unwrap();
            (all, ten)
        })
        .await
        .unwrap();
        assert!(all == *data, "{} of {} bytes", all.len(), data.len());
        assert_eq!(jumped[..], data[3 << 20..(3 << 20) + 10]);
        // Saved for next time.
        for _ in 0..100 {
            if file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(std::fs::read(&file).unwrap().len(), data.len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
