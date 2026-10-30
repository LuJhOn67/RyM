//! The video plugin on the PC: watch a song's music video, and download videos or audio from
//! YouTube or any site yt-dlp knows ("universal downloader"). Videos play in VLC (picture and
//! sound streamed separately and played together); without VLC, on YouTube in the browser.

use std::path::PathBuf;
use std::process::Child;

use anyhow::{Result, anyhow};

/// The video's picture and sound links (one link when the site has them together).
pub async fn links(video_id: &str) -> Result<(String, Option<String>)> {
    let mut cmd = ytdlp()?;
    cmd.args(["-f", "bv*[height<=1080]+ba/b", "-g", "--no-playlist"]).arg(watch_url(video_id));
    let out = run(cmd).await?;
    let mut lines = out.lines().map(str::trim).filter(|l| l.starts_with("http"));
    let video = lines.next().ok_or_else(|| anyhow!("yt-dlp gave no video link"))?.to_string();
    Ok((video, lines.next().map(String::from)))
}

pub fn watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// Plays the video in VLC: the picture link, with the sound link alongside when they're apart.
pub fn watch_in_vlc(exe: &std::path::Path, video: &str, audio: Option<&str>, title: &str) -> Result<Child> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(format!("--meta-title={title}")).arg("--play-and-exit").arg(video);
    if let Some(a) = audio {
        cmd.arg(format!(":input-slave={a}"));
    }
    Ok(cmd.spawn()?)
}

/// Where downloaded videos go: Videos/RymPlayer (audio goes to Music/RymPlayer).
pub fn videos_dir() -> PathBuf {
    directories::UserDirs::new().and_then(|d| d.video_dir().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from(".")).join("RymPlayer")
}

/// Downloads a link from YouTube or any site yt-dlp knows: the video (best up to 1080p, MP4 when
/// it can) or just the sound. Returns the saved file.
pub async fn download(url: &str, video: bool) -> Result<PathBuf> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow!("not a link: {url}"));
    }
    let dir = if video { videos_dir() } else { crate::stream::music_dir() };
    tokio::fs::create_dir_all(&dir).await?;
    let mut cmd = ytdlp()?;
    if video {
        cmd.args(["-f", "bv*[height<=1080]+ba/b", "--merge-output-format", "mp4"]);
    } else {
        cmd.args(["-f", "ba/b", "-x"]);
    }
    cmd.args(["--no-playlist", "--restrict-filenames", "-P"])
        .arg(&dir)
        .args(["-o", "%(title).120B [%(id)s].%(ext)s", "--print", "after_move:filepath"])
        .arg(url);
    let out = run(cmd).await?;
    out.lines().map(str::trim).rfind(|l| !l.is_empty()).map(PathBuf::from).ok_or_else(|| anyhow!("yt-dlp saved nothing"))
}

/// yt-dlp with the signed-in account (YouTube) and Node.js for YouTube's player code.
fn ytdlp() -> Result<tokio::process::Command> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args(["--js-runtimes", "node"]);
    if let Ok(cookie) = std::fs::read_to_string(crate::cookie_path())
        && !cookie.trim().is_empty()
    {
        let file = crate::data_dir().join("cookies-ytdlp.txt");
        crate::stream::write_cookie_file(&cookie, &file)?;
        cmd.arg("--cookies").arg(file);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    Ok(cmd)
}

async fn run(mut cmd: tokio::process::Command) -> Result<String> {
    let out = cmd.output().await.map_err(|e| anyhow!("yt-dlp not found (pip install -U yt-dlp): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("{}", err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed").trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
