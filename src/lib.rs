//! RymPlayer's shared core: YouTube Music access, playback, lyrics, settings and sign-in.
//! Used by the terminal app (`RymPlayer`) and the window apps (`RyM-Legacy-Player`, `RymPlayer-GTK`).

pub mod automix;
pub mod api;
pub mod arriving;
pub mod audio;
pub mod colors;
pub mod config;
pub mod discord;
pub mod feed;
pub mod fx;
pub mod i18n;
pub mod innertube;
pub mod likes;
pub mod local;
pub mod login;
pub mod lyric_sources;
pub mod link;
pub mod lyrics;
pub mod memory;
pub mod page_cache;
pub mod plugins;
pub mod queue;
pub mod radio;
pub mod romaji;
pub mod lastfm;
pub mod ytdlp;
pub mod ram_points;
pub mod recent;
pub mod session;
pub mod soundcloud;
pub mod spotify;
pub mod stats;
pub mod stream;
pub mod updates;
pub mod video;
pub mod vlc;

use std::path::PathBuf;

/// The version shown in the apps ("6.7").
pub const VERSION: &str = "7.1.1";

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "RymPlayer")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A YouTube (Music) song: not a file on this device, not a SoundCloud song. Likes, radio,
/// artist pages and videos are for these.
pub fn is_youtube(id: &str) -> bool {
    !local::is_local(id) && !soundcloud::is_soundcloud(id)
}

pub fn cookie_path() -> PathBuf {
    data_dir().join("cookie.txt")
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
