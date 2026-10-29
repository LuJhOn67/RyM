//! Fetches a song's audio into the local cache, straight from the link YouTube's player API
//! gives ([`crate::innertube`]). No outside programs are needed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::io::AsyncWriteExt;

/// Bytes asked for per request: YouTube slows down single requests for whole songs.
const RANGE: u64 = 4 << 20;

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
    std::fs::read_to_string(cache_dir().join(format!("{video_id}.secs"))).ok()?.trim().parse().ok()
}

pub async fn fetch(video_id: String) -> Result<PathBuf> {
    if let Some(p) = cached(&video_id) {
        return Ok(p);
    }
    // Download into a private folder, then move into the cache: a prefetch and a play of the
    // same song can run at once, and the cache must never expose a half-written file.
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = cache_dir();
    let tmp = dir.join(format!(".dl-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    tokio::fs::create_dir_all(&tmp).await?;
    let result = download(&video_id, &tmp).await.and_then(|p| {
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

/// Saves a song as a file in the Music folder: `Music\RymPlayer\Artist - Title.ext`.
pub async fn save_to_music(video_id: &str, artist: &str, title: &str) -> Result<PathBuf> {
    let src = fetch(video_id.to_string()).await?;
    let music = directories::UserDirs::new()
        .and_then(|d| d.audio_dir().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = music.join("RymPlayer");
    tokio::fs::create_dir_all(&dir).await?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("webm");
    let dest = dir.join(format!("{}.{ext}", file_name(artist, title)));
    tokio::fs::copy(&src, &dest).await?;
    Ok(dest)
}

/// "Artist - Title" with the characters Windows refuses in file names replaced.
fn file_name(artist: &str, title: &str) -> String {
    let clean: String = format!("{artist} - {title}")
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    clean.trim().trim_end_matches('.').to_string()
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    // No overall timeout: a range can take a while on a slow connection.
    C.get_or_init(|| reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().expect("http client"))
}

async fn download(video_id: &str, dir: &Path) -> Result<PathBuf> {
    let audio = crate::innertube::audio(video_id).await?;
    let path = dir.join(format!("{video_id}.{}", audio.ext));
    save(&audio.url, audio.user_agent, audio.size, &path).await?;
    // WebM files don't carry their length; keep the one YouTube reported next to the cache.
    if let Some(secs) = audio.seconds {
        let _ = std::fs::write(cache_dir().join(format!("{video_id}.secs")), secs.to_string());
    }
    Ok(path)
}

/// Writes `url` to `path`, in ranges when the size is known.
async fn save(url: &str, user_agent: &str, size: Option<u64>, path: &Path) -> Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut pos = 0u64;
    loop {
        let mut req = http().get(url).header("User-Agent", user_agent);
        if let Some(size) = size {
            if pos >= size {
                break;
            }
            req = req.header("Range", format!("bytes={pos}-{}", (pos + RANGE).min(size) - 1));
        }
        let mut resp = req.send().await?.error_for_status().map_err(|e| anyhow!("audio download: {e}"))?;
        let before = pos;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            pos += chunk.len() as u64;
        }
        if size.is_none() {
            break;
        }
        if pos == before {
            return Err(anyhow!("audio download stopped at {pos} bytes"));
        }
    }
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A local server answering `Range` requests, cutting every third answer short like a real
    /// one sometimes does. No internet involved.
    fn serve(data: std::sync::Arc<Vec<u8>>) -> String {
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
                let b = if n % 3 == 2 && b - a > 2000 { b - 1000 } else { b };
                let body = &data[a..=b];
                let head = format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(body);
            }
        });
        format!("http://{addr}/audio")
    }

    #[test]
    fn safe_file_names() {
        assert_eq!(file_name("AC/DC", "What? <Live>"), "AC_DC - What_ _Live_");
        assert_eq!(file_name("A", "Song..."), "A - Song");
    }

    #[tokio::test]
    async fn saves_in_ranges() {
        let data: std::sync::Arc<Vec<u8>> = std::sync::Arc::new((0..(RANGE as usize * 2 + 12345)).map(|i| (i * 7 % 251) as u8).collect());
        let url = serve(data.clone());
        let dir = std::env::temp_dir().join(format!("rym-range-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.webm");
        save(&url, "test", Some(data.len() as u64), &path).await.unwrap();
        let written = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(written == *data, "file matches ({} of {} bytes)", written.len(), data.len());
    }
}
