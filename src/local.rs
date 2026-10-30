//! Songs on this device: the Music folder (and any folders added in the settings), read with
//! their tags. A local song is a [`Track`] whose id is `file:` and its path, so the rest of the
//! player (queue, playback, lyrics) handles it like any other song.

use std::path::{Path, PathBuf};

use lofty::prelude::*;

use crate::api::Track;

pub const PREFIX: &str = "file:";

/// File types the player can decode.
const AUDIO: &[&str] = &["mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "opus", "wav", "aiff", "aif", "webm", "mka"];

pub fn is_local(video_id: &str) -> bool {
    video_id.starts_with(PREFIX)
}

pub fn path_of(video_id: &str) -> Option<PathBuf> {
    video_id.strip_prefix(PREFIX).map(PathBuf::from)
}

/// Where songs are looked for: the user's Music folder, then the folders from the settings.
pub fn folders(extra: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = directories::UserDirs::new().and_then(|u| u.audio_dir().map(Path::to_path_buf)).into_iter().collect();
    for f in extra {
        let p = PathBuf::from(f);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Every song in `folders` (and their subfolders), sorted by artist, album and track number.
/// Reads each file's tags, so it's slow on big libraries: call it off the UI thread.
pub fn scan(folders: &[PathBuf]) -> Vec<Track> {
    let mut files = Vec::new();
    for f in folders {
        walk(f, &mut files, 0);
    }
    files.sort();
    files.dedup();
    let mut songs: Vec<(String, String, u32, Track)> = files.iter().map(|p| read(p)).collect();
    songs.sort_by(|a, b| (a.0.to_lowercase(), a.1.to_lowercase(), a.2).cmp(&(b.0.to_lowercase(), b.1.to_lowercase(), b.2)));
    songs.into_iter().map(|(_, _, _, t)| t).collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    // Deep enough for Artist/Album/Disc folders; links can't loop forever.
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out, depth + 1),
            Ok(_) if p.extension().and_then(|x| x.to_str()).is_some_and(|x| AUDIO.contains(&x.to_ascii_lowercase().as_str())) => out.push(p),
            _ => {}
        }
    }
}

/// A song from its tags; files without tags take "Artist - Title" from their name.
fn read(path: &Path) -> (String, String, u32, Track) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let (name_artist, name_title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem.clone()),
    };
    let tagged = lofty::read_from_path(path).ok();
    let tag = tagged.as_ref().and_then(|t| t.primary_tag().or_else(|| t.first_tag()));
    let get = |f: fn(&lofty::tag::Tag) -> Option<std::borrow::Cow<'_, str>>| tag.and_then(f).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let title = get(|t| t.title()).unwrap_or(name_title);
    let artist = get(|t| t.artist()).unwrap_or(if name_artist.is_empty() { "Unknown artist".into() } else { name_artist });
    let album = get(|t| t.album()).unwrap_or_default();
    let number = tag.and_then(|t| t.track()).unwrap_or(0);
    let secs = tagged.as_ref().map(|t| t.properties().duration().as_secs()).unwrap_or(0);
    let duration = if secs > 0 { crate::fmt_time(secs as f32) } else { String::new() };
    let track = Track {
        video_id: format!("{PREFIX}{}", path.display()),
        title,
        artist: artist.clone(),
        album: album.clone(),
        duration,
        thumb: None,
        album_id: None,
        artist_id: None,
        link: None,
    };
    (artist, album, number, track)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_songs_in_subfolders_and_names_untagged_ones() {
        let dir = std::env::temp_dir().join(format!("rym-local-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Band/Album")).unwrap();
        std::fs::write(dir.join("Band/Album/Some Band - First Song.mp3"), b"not really audio").unwrap();
        std::fs::write(dir.join("Another - Second.flac"), b"x").unwrap();
        std::fs::write(dir.join("cover.jpg"), b"x").unwrap();
        let songs = scan(std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<(&str, &str)> = songs.iter().map(|t| (t.artist.as_str(), t.title.as_str())).collect();
        assert_eq!(names, [("Another", "Second"), ("Some Band", "First Song")]);
        assert!(songs.iter().all(|t| is_local(&t.video_id) && path_of(&t.video_id).is_some()));
    }
}
