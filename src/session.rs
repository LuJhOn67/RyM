//! What was playing when the app closed: the queue, the song, where in it, and the volume.
//! The next start picks up there (the song waits, paused, at the same second). Saved as
//! `session.json` in the data folder.

use serde::{Deserialize, Serialize};

use crate::api::Track;
use crate::queue::Queue;

/// Played songs kept before the current one, and songs kept after it.
const BEFORE: usize = 100;
const AFTER: usize = 400;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub songs: Vec<Track>,
    pub current: Option<usize>,
    /// Seconds into the current song.
    pub position: f32,
    pub volume: f32,
}

impl Session {
    /// The queue around the playing song (at most 100 before it and 400 after it).
    pub fn new(q: &Queue, position: f32, volume: f32) -> Self {
        let (start, current) = match q.current {
            Some(c) => (c.saturating_sub(BEFORE), Some(c.min(BEFORE))),
            None => (0, None),
        };
        let end = q.current.map_or(AFTER, |c| c + 1 + AFTER).min(q.songs.len());
        Session { songs: q.songs.get(start..end).unwrap_or_default().to_vec(), current, position: position.max(0.0), volume: volume.clamp(0.0, 1.0) }
    }

    /// The saved queue, with nothing added waiting.
    pub fn queue(&self) -> Queue {
        Queue { songs: self.songs.clone(), current: self.current.filter(|c| *c < self.songs.len()), added: 0 }
    }
}

fn path() -> std::path::PathBuf {
    crate::data_dir().join("session.json")
}

pub fn load() -> Option<Session> {
    serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()
}

/// Writes it (through a temporary file, so a crash never leaves half a file).
pub fn save(s: &Session) {
    let Ok(json) = serde_json::to_string(s) else { return };
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: usize) -> Track {
        Track {
            video_id: format!("v{id}"),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration: String::new(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    #[test]
    fn keeps_the_queue_around_the_song() {
        let q = Queue { songs: (0..700).map(t).collect(), current: Some(150), added: 2 };
        let s = Session::new(&q, 42.5, 1.4);
        assert_eq!(s.songs.len(), 100 + 1 + 400);
        assert_eq!(s.songs[s.current.unwrap()].video_id, "v150");
        assert_eq!((s.position, s.volume), (42.5, 1.0));
        let back: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let q2 = back.queue();
        assert_eq!(q2.now().unwrap().video_id, "v150");
        assert_eq!(q2.added, 0);
        assert_eq!(Session::new(&Queue::default(), 0.0, 0.5).current, None);
    }
}
