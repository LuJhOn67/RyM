//! Fetches a song's audio into the local cache, straight from the link YouTube's player API
//! gives ([`crate::innertube`]). No outside programs are needed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::io::AsyncWriteExt;

/// Bytes asked for per request: YouTube slows down single requests for whole songs.
const RANGE: u64 = 4 << 20;

/// Set from the "Audio source" setting: InnerTube when true, yt-dlp otherwise.
static INNERTUBE: AtomicBool = AtomicBool::new(false);

/// Which way songs are fetched; the apps call this with their setting.
pub fn use_source(source: crate::config::Source) {
    INNERTUBE.store(source == crate::config::Source::InnerTube, Ordering::Relaxed);
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
    let music = directories::UserDirs::new()
        .and_then(|d| d.audio_dir().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = music.join("RymPlayer");
    tokio::fs::create_dir_all(&dir).await?;
    let (_, path) = ytdlp(video_id, &dir.join(format!("{}.%(ext)s", file_name(artist, title)))).await?;
    Ok(path)
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    // No overall timeout: a range can take a while on a slow connection.
    C.get_or_init(|| reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().expect("http client"))
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

/// yt-dlp wants a Netscape cookie file; this converts the browser's `Cookie:` header.
fn write_cookie_file(header: &str, path: &Path) -> Result<()> {
    let mut out = String::from("# Netscape HTTP Cookie File
");
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else { continue };
        out.push_str(&format!(".youtube.com	TRUE	/	TRUE	2147483647	{name}	{value}
"));
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// "Artist - Title" with the characters Windows refuses in file names replaced.
fn file_name(artist: &str, title: &str) -> String {
    let clean: String = format!("{artist} - {title}")
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    clean.trim().trim_end_matches('.').to_string()
}

async fn download(video_id: &str, dir: &Path) -> Result<PathBuf> {
    let (secs, path) = if INNERTUBE.load(Ordering::Relaxed) {
        innertube(video_id, dir).await?
    } else {
        // yt-dlp with your account keeps up with YouTube (Premium: 256 kbps Opus).
        ytdlp(video_id, &dir.join("%(id)s.%(ext)s")).await?
    };
    // WebM files don't carry their length; keep the one YouTube reported next to the cache.
    if let Some(secs) = secs {
        let _ = std::fs::write(cache_dir().join(format!("{video_id}.secs")), secs.to_string());
    }
    Ok(path)
}

/// YouTube's player API straight from the app; a client whose link is refused is skipped.
async fn innertube(video_id: &str, dir: &Path) -> Result<(Option<f32>, PathBuf)> {
    let mut skip = Vec::new();
    let mut why = Vec::new();
    loop {
        let audio = crate::innertube::audio_skipping(video_id, &skip).await.map_err(|e| {
            why.push(e.to_string());
            anyhow!(why.join("; "))
        })?;
        let path = dir.join(format!("{video_id}.{}", audio.ext));
        match save(&audio.url, audio.user_agent, audio.size, &path).await {
            Ok(()) => return Ok((audio.seconds, path)),
            Err(e) => {
                why.push(format!("{}: {e}", audio.client));
                skip.push(audio.client.clone());
            }
        }
    }
}

/// Runs yt-dlp for one song with the signed-in account; returns its length and the file.
async fn ytdlp(video_id: &str, out: &Path) -> Result<(Option<f32>, PathBuf)> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args(["--js-runtimes", "node", "-f", "774/251/141/140/bestaudio", "--no-playlist", "--no-progress", "--no-part"])
        .args(["--print", "after_move:%(duration)s\t%(filepath)s", "-o"])
        .arg(out);
    if let Ok(cookie) = std::fs::read_to_string(crate::cookie_path())
        && !cookie.trim().is_empty()
    {
        let file = crate::data_dir().join("cookies-ytdlp.txt");
        write_cookie_file(&cookie, &file)?;
        cmd.arg("--cookies").arg(file);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let output = cmd.output().await.map_err(|e| anyhow!("yt-dlp not found (pip install -U yt-dlp yt-dlp-ejs): {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed");
        return Err(anyhow!("{last}"));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let (secs, path) = printed.lines().last().and_then(|l| l.trim().split_once('\t')).unzip();
    let path = path.map(PathBuf::from).filter(|p| p.exists()).ok_or_else(|| anyhow!("download finished but the file is missing"))?;
    Ok((secs.and_then(|s| s.parse().ok()), path))
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


    #[test]
    fn cookie_file_for_ytdlp() {
        let path = std::env::temp_dir().join(format!("rym-cookie-test-{}.txt", std::process::id()));
        write_cookie_file("SAPISID=abc; HSID=x=y", &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.starts_with("# Netscape HTTP Cookie File
"));
        assert!(text.contains("	SAPISID	abc
") && text.contains("	HSID	x=y
"), "{text}");
    }

    #[test]
    fn safe_file_names() {
        assert_eq!(file_name("AC/DC", "What? <Live>"), "AC_DC - What_ _Live_");
        assert_eq!(file_name("A", "Song..."), "A - Song");
    }

}
