//! Each song's own volume: changed while the song played, it's used again the next time that song
//! starts (backlog 26). Kept in `song-volume.json`, the last 500 songs; written when the next song
//! starts (so dragging the volume doesn't write the file at every step).

use std::collections::HashMap;
use std::sync::Mutex;

const KEPT: usize = 500;

#[derive(Default)]
struct Store {
    loaded: bool,
    dirty: bool,
    /// Song id -> (volume 0 to 1, when it was set).
    map: HashMap<String, (f32, u64)>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| Mutex::new(Store::default()));

fn path() -> std::path::PathBuf {
    // The tests keep to a file of their own.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("rym-song-volume-test-{}.json", std::process::id()));
    }
    crate::data_dir().join("song-volume.json")
}

fn ready(s: &mut Store) {
    if !s.loaded {
        s.loaded = true;
        s.map = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    }
}

fn save(s: &mut Store) {
    if !s.dirty {
        return;
    }
    s.dirty = false;
    if s.map.len() > KEPT {
        let mut all: Vec<(String, (f32, u64))> = s.map.drain().collect();
        all.sort_by_key(|e| std::cmp::Reverse(e.1.1));
        all.truncate(KEPT);
        s.map = all.into_iter().collect();
    }
    if let Ok(json) = serde_json::to_string(&s.map) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        let _ = std::fs::write(path(), json);
    }
}

/// The volume was changed to `volume` while `video_id` played.
pub fn set(video_id: &str, volume: f32) {
    if video_id.is_empty() {
        return;
    }
    let Ok(mut s) = STORE.lock() else { return };
    ready(&mut s);
    s.map.insert(video_id.to_string(), (volume.clamp(0.0, 1.0), crate::updates::now()));
    s.dirty = true;
}

/// The volume `video_id` had last time, if it was changed while it played (and the changes so
/// far are written).
pub fn get(video_id: &str) -> Option<f32> {
    let Ok(mut s) = STORE.lock() else { return None };
    ready(&mut s);
    save(&mut s);
    s.map.get(video_id).map(|v| v.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_songs_volume_comes_back() {
        super::set("vol-test-song", 0.35);
        assert_eq!(super::get("vol-test-song"), Some(0.35));
        assert_eq!(super::get("never-set-song"), None);
        super::set("vol-test-song", 1.7);
        assert_eq!(super::get("vol-test-song"), Some(1.0), "kept within 0 to 1");
    }
}
