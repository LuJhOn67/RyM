//! Fetches a song's audio into the local cache with yt-dlp.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Result, anyhow};
use tokio::process::Command;

/// Best first: Premium Opus 256k, Opus 160k, Premium AAC 256k, AAC 128k.
const FORMATS: &str = "774/251/141/140/bestaudio";

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

pub async fn fetch(video_id: String, cookies: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = cached(&video_id) {
        return Ok(p);
    }
    // Download into a private folder, then move into the cache: a prefetch and a play of the
    // same song can run at once, and the cache must never expose a half-written file.
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = cache_dir();
    let tmp = dir.join(format!(".dl-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
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

async fn download(video_id: &str, cookies: Option<PathBuf>, dir: &Path) -> Result<PathBuf> {
    let out = dir.join("%(id)s.%(ext)s");

    let mut cmd = Command::new("yt-dlp");
    cmd.args(["--js-runtimes", "node", "-f", FORMATS, "--no-playlist", "--no-progress"])
        .args(["--no-part", "--print", "after_move:filepath", "-o"])
        .arg(&out);
    if let Some(c) = &cookies {
        cmd.arg("--cookies").arg(c);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = cmd.output().await.map_err(|e| anyhow!("yt-dlp not found: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed");
        return Err(anyhow!("{last}"));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let path = printed.lines().last().map(|l| PathBuf::from(l.trim()));
    path.filter(|p| p.exists()).ok_or_else(|| anyhow!("download finished but file is missing"))
}

/// yt-dlp wants a Netscape cookie file; convert the browser's `Cookie:` header.
pub fn write_cookie_file(header: &str, path: &Path) -> Result<()> {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else { continue };
        out.push_str(&format!(".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{name}\t{value}\n"));
    }
    std::fs::write(path, out)?;
    Ok(())
}
