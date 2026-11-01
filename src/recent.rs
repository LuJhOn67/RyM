//! Songs played on this computer, newest first, with how often each was played: the home page's
//! "Recently played" row, and the seeds and favourites its mix is made from. Saved as
//! `recently-played.json` in the data folder.

use serde::{Deserialize, Serialize};

use crate::api::Track;

/// Songs kept, at most.
const KEEP: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Played {
    pub track: Track,
    pub plays: u32,
    /// When last played (seconds since 1970).
    pub last: u64,
}

fn path() -> std::path::PathBuf {
    crate::data_dir().join("recently-played.json")
}

/// Newest first.
pub fn load() -> Vec<Played> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(list: &[Played]) {
    if let Ok(json) = serde_json::to_string(list) {
        let p = path();
        let _ = std::fs::create_dir_all(crate::data_dir());
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// A song started playing: it goes to the front, its count goes up.
pub fn played(t: &Track) {
    if t.link.is_some() || t.video_id.is_empty() {
        return;
    }
    let mut list = load();
    let plays = match list.iter().position(|p| p.track.video_id == t.video_id) {
        Some(i) => list.remove(i).plays + 1,
        None => 1,
    };
    let mut track = t.clone();
    track.album = String::new();
    list.insert(0, Played { track, plays, last: crate::updates::now() });
    list.truncate(KEEP);
    save(&list);
}

/// The songs, newest first.
pub fn tracks() -> Vec<Track> {
    load().into_iter().map(|p| p.track).collect()
}

/// The most played songs (ties: the more recent first).
pub fn favourites(n: usize) -> Vec<Track> {
    let mut list = load();
    list.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.last.cmp(&a.last)));
    list.into_iter().filter(|p| p.plays > 1).take(n).map(|p| p.track).collect()
}
