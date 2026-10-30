//! Fetches a song's audio into the local cache. The link comes from YouTube's player API
//! ([`crate::innertube`]) or yt-dlp, and the audio downloads over several connections at once.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};

/// Connections a song downloads over at once, and the piece size in MB (0 = open: the song is
/// split evenly over the connections). Set from the settings.
static CONNECTIONS: AtomicU8 = AtomicU8::new(8);
static PIECE_MB: AtomicU8 = AtomicU8::new(0);

pub fn download_parallel(connections: u8, piece_mb: u8) {
    CONNECTIONS.store(connections.clamp(1, 16), Ordering::Relaxed);
    PIECE_MB.store(piece_mb, Ordering::Relaxed);
}

/// The first request is small: it tells the song's size so the rest can be split up.
const FIRST: u64 = 256 << 10;

/// Set from the "Audio source" setting (see [`crate::config::Source`]).
static SOURCE: AtomicU8 = AtomicU8::new(0);

/// Downloads in the best quality through yt-dlp (else the playback file); set from the settings.
static DOWNLOAD_BEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn download_best(best: bool) {
    DOWNLOAD_BEST.store(best, Ordering::Relaxed);
}

/// Which way songs are fetched; the apps call this with their setting.
pub fn use_source(source: crate::config::Source) {
    use crate::config::Source;
    SOURCE.store(match source { Source::Auto => 0, Source::YtDlp => 1, Source::InnerTube => 2 }, Ordering::Relaxed);
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

/// A song ready to play: a file in the cache, or one still arriving that plays while it downloads.
#[derive(Debug, Clone)]
pub enum Playable {
    File(PathBuf),
    Arriving(crate::arriving::Arriving),
}

/// Opens a song for playback: the cached file, or the stream, ready after its first 128 KB (the
/// rest keeps downloading and is cached when done). In the default mode a link YouTube refuses
/// from InnerTube is fetched again through yt-dlp.
pub async fn open(video_id: String) -> Result<Playable> {
    if let Some(p) = cached(&video_id) {
        return Ok(Playable::File(p));
    }
    let dir = cache_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let start = |(url, headers, ext, secs): (String, Vec<(String, String)>, String, Option<f32>)| {
        let save_to = dir.join(format!("{video_id}.{ext}"));
        if let Some(secs) = secs {
            let _ = std::fs::write(dir.join(format!("{video_id}.secs")), secs.to_string());
        }
        crate::arriving::Arriving::start(url, headers, ext, secs, save_to)
    };
    let arriving = match SOURCE.load(Ordering::Relaxed) {
        1 => start(ytdlp_link(&video_id).await?).await?,
        2 => start(innertube_link(&video_id).await?).await?,
        _ => {
            let quick = match innertube_link(&video_id).await {
                Ok(link) => start(link).await,
                Err(e) => Err(e),
            };
            match quick {
                Ok(a) => a,
                Err(e) => start(ytdlp_link(&video_id).await.map_err(|y| anyhow!("InnerTube: {e}; yt-dlp: {y}"))?).await?,
            }
        }
    };
    Ok(Playable::Arriving(arriving))
}

/// The InnerTube link for a song with the header its stream wants, its file type and length.
async fn innertube_link(video_id: &str) -> Result<(String, Vec<(String, String)>, String, Option<f32>)> {
    let a = crate::innertube::audio(video_id).await?;
    Ok((a.url, vec![("User-Agent".to_string(), a.user_agent.to_string())], a.ext.to_string(), a.seconds))
}

/// A song's direct link and the headers to send with it, for an outside player (VLC).
pub async fn link(video_id: &str) -> Result<(String, Vec<(String, String)>)> {
    let (url, headers, ..) = match SOURCE.load(Ordering::Relaxed) {
        1 => ytdlp_link(video_id).await?,
        2 => innertube_link(video_id).await?,
        _ => match innertube_link(video_id).await {
            Ok(l) => l,
            Err(e) => ytdlp_link(video_id).await.map_err(|y| anyhow!("InnerTube: {e}; yt-dlp: {y}"))?,
        },
    };
    Ok((url, headers))
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
    let name = file_name(artist, title);
    if !DOWNLOAD_BEST.load(Ordering::Relaxed) {
        // Fast: the same file playback uses (in seconds, about 160 kbps).
        let src = fetch(video_id.to_string()).await?;
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("webm");
        let dest = dir.join(format!("{name}.{ext}"));
        tokio::fs::copy(&src, &dest).await?;
        return Ok(dest);
    }
    let (_, path) = ytdlp_parallel(video_id, &dir.join(&name)).await?;
    Ok(path)
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    // No overall timeout: a range can take a while on a slow connection.
    C.get_or_init(|| reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().expect("http client"))
}

/// Writes `url` to `path`. A small first request tells the size; the rest is split into pieces
/// fetched over several connections at once, each written at its place in the file. The app sets
/// no speed limit: YouTube limits each connection, so more connections use more of the line.
async fn save(url: &str, headers: &[(String, String)], path: &Path) -> Result<()> {
    let file = Arc::new(std::fs::File::create(path)?);
    let Some(total) = piece(url, headers, 0, FIRST, &file).await? else {
        return Ok(()); // the server sent the whole song at once
    };
    file.set_len(total)?;
    if total <= FIRST {
        return Ok(());
    }
    let connections = CONNECTIONS.load(Ordering::Relaxed).max(1) as u64;
    let size = match PIECE_MB.load(Ordering::Relaxed) {
        0 => (total - FIRST).div_ceil(connections).max(FIRST),
        mb => (mb as u64) << 20,
    };
    let next = Arc::new(AtomicU64::new(FIRST));
    let workers: Vec<_> = (0..connections)
        .map(|_| {
            let (url, headers, file, next) = (url.to_string(), headers.to_vec(), file.clone(), next.clone());
            tokio::spawn(async move {
                loop {
                    let start = next.fetch_add(size, Ordering::Relaxed);
                    if start >= total {
                        return Ok::<(), anyhow::Error>(());
                    }
                    piece(&url, &headers, start, size.min(total - start), &file).await?;
                }
            })
        })
        .collect();
    for w in workers {
        w.await??;
    }
    Ok(())
}

/// Writes bytes `[start, start + len)` of `url` at their place in the file, asking again from
/// where an answer stopped early. Returns the file's size, or None when the server ignored the
/// range and sent the whole file (then it's all written).
async fn piece(url: &str, headers: &[(String, String)], start: u64, len: u64, file: &std::fs::File) -> Result<Option<u64>> {
    let end = start + len;
    let mut pos = start;
    let mut total = None;
    while pos < end {
        let mut req = http().get(url).header("Range", format!("bytes={pos}-{}", end - 1));
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let mut resp = req.send().await?.error_for_status().map_err(|e| anyhow!("audio download: {e}"))?;
        if resp.status() == reqwest::StatusCode::OK {
            let mut at = 0;
            while let Some(chunk) = resp.chunk().await? {
                write_at(file, &chunk, at)?;
                at += chunk.len() as u64;
            }
            return Ok(None);
        }
        total = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse().ok())
            .or(total);
        let before = pos;
        while let Some(chunk) = resp.chunk().await? {
            let take = chunk.len().min((end - pos) as usize);
            write_at(file, &chunk[..take], pos)?;
            pos += take as u64;
            if pos >= end {
                break;
            }
        }
        if pos == before {
            return Err(anyhow!("audio download stopped at {pos} bytes"));
        }
    }
    total.map(Some).ok_or_else(|| anyhow!("the server didn't say the song's size"))
}

/// Writes `buf` at `offset` without moving a shared position, so connections can write at once.
fn write_at(file: &std::fs::File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            done += file.seek_write(&buf[done..], offset + done as u64)?;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, offset)
    }
}

/// yt-dlp wants a Netscape cookie file; this converts the browser's `Cookie:` header.
fn write_cookie_file(header: &str, path: &Path) -> Result<()> {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else { continue };
        out.push_str(&format!(".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{name}\t{value}\n"));
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
    let base = dir.join(video_id);
    let (secs, path) = match SOURCE.load(Ordering::Relaxed) {
        // yt-dlp with your account keeps up with YouTube (Premium: 256 kbps Opus), but takes
        // 6-7 s per song to get the link.
        1 => ytdlp_parallel(video_id, &base).await?,
        2 => innertube(video_id, dir).await?,
        // InnerTube first (about half a second), yt-dlp when YouTube refuses it.
        _ => match innertube(video_id, dir).await {
            Ok(found) => found,
            Err(e) => ytdlp_parallel(video_id, &base).await.map_err(|y| anyhow!("InnerTube: {e}; yt-dlp: {y}"))?,
        },
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
        match save(&audio.url, &[("User-Agent".to_string(), audio.user_agent.to_string())], &path).await {
            Ok(()) => return Ok((audio.seconds, path)),
            Err(e) => {
                why.push(format!("{}: {e}", audio.client));
                skip.push(audio.client.clone());
            }
        }
    }
}

/// yt-dlp finds the link (with the signed-in account), the song downloads over our parallel
/// connections into `base` plus its extension. If that fails, yt-dlp downloads it itself.
async fn ytdlp_parallel(video_id: &str, base: &Path) -> Result<(Option<f32>, PathBuf)> {
    match ytdlp_link(video_id).await {
        Ok((url, headers, ext, secs)) => {
            let path = PathBuf::from(format!("{}.{ext}", base.display()));
            match save(&url, &headers, &path).await {
                Ok(()) => return Ok((secs, path)),
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Err(_) => {}
    }
    ytdlp(video_id, &PathBuf::from(format!("{}.%(ext)s", base.display()))).await
}

fn ytdlp_command(video_id: &str) -> Result<tokio::process::Command> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args(["--js-runtimes", "node", "-f", "774/251/141/140/bestaudio", "--no-playlist"]);
    if let Ok(cookie) = std::fs::read_to_string(crate::cookie_path())
        && !cookie.trim().is_empty()
    {
        let file = crate::data_dir().join("cookies-ytdlp.txt");
        write_cookie_file(&cookie, &file)?;
        cmd.arg("--cookies").arg(file);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let _ = video_id;
    Ok(cmd)
}

/// The chosen format's link, the headers yt-dlp would send with it, its file type and length.
async fn ytdlp_link(video_id: &str) -> Result<(String, Vec<(String, String)>, String, Option<f32>)> {
    let mut cmd = ytdlp_command(video_id)?;
    cmd.arg("-j").arg(format!("https://music.youtube.com/watch?v={video_id}"));
    let output = cmd.output().await.map_err(|e| anyhow!("yt-dlp not found (pip install -U yt-dlp yt-dlp-ejs): {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("{}", err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed")));
    }
    let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let pick = info["requested_downloads"].get(0).unwrap_or(&info);
    let url = pick["url"].as_str().or_else(|| info["url"].as_str()).ok_or_else(|| anyhow!("yt-dlp gave no link"))?.to_string();
    let headers = pick["http_headers"]
        .as_object()
        .or_else(|| info["http_headers"].as_object())
        .map(|h| h.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let ext = pick["ext"].as_str().or_else(|| info["ext"].as_str()).unwrap_or("webm").to_string();
    Ok((url, headers, ext, info["duration"].as_f64().map(|d| d as f32)))
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
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len(),
                    body.len()
                );
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(body);
            }
        });
        format!("http://{addr}/audio")
    }

    #[tokio::test]
    async fn saves_in_ranges() {
        let data: std::sync::Arc<Vec<u8>> = std::sync::Arc::new((0..((8usize << 20) + 12345)).map(|i| (i * 7 % 251) as u8).collect());
        let url = serve(data.clone());
        let dir = std::env::temp_dir().join(format!("rym-range-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.webm");
        save(&url, &[("User-Agent".to_string(), "test".to_string())], &path).await.unwrap();
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
