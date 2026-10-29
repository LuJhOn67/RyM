//! RymPlayer's shared core: YouTube Music access, playback, lyrics, settings and sign-in.
//! Used by the terminal app (`RymPlayer`) and the windowed app (`RymPlayer-GUI`).

pub mod api;
pub mod audio;
pub mod config;
pub mod innertube;
pub mod login;
pub mod lyrics;
pub mod stream;

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "RymPlayer")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn cookie_path() -> PathBuf {
    data_dir().join("cookie.txt")
}

pub fn ytdlp_cookie_path() -> PathBuf {
    data_dir().join("cookies-ytdlp.txt")
}

/// "3:45" or "1:02:03" to seconds.
pub fn parse_duration(d: &str) -> Option<f32> {
    let mut secs = 0.0;
    for part in d.split(':') {
        secs = secs * 60.0 + part.trim().parse::<f32>().ok()?;
    }
    Some(secs)
}

pub fn fmt_time(secs: f32) -> String {
    let t = secs.max(0.0) as u32;
    format!("{}:{:02}", t / 60, t % 60)
}
