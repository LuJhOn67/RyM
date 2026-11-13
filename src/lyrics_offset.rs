//! Each song's lyrics timing shift (backlog 35): lyrics that come early or late are moved by hand
//! (a quarter second at a time) and the shift is kept for that song. Kept in
//! `lyrics-offset.json`, the last 500 songs.

use std::collections::HashMap;
use std::sync::Mutex;

const KEPT: usize = 500;

#[derive(Default)]
struct Store {
    loaded: bool,
    /// Song id -> (seconds the lyrics move later (+) or earlier (-), when it was set).
    map: HashMap<String, (f32, u64)>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| Mutex::new(Store::default()));

fn path() -> std::path::PathBuf {
    // The tests keep to a file of their own.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("rym-lyrics-offset-test-{}.json", std::process::id()));
    }
    crate::data_dir().join("lyrics-offset.json")
}

fn ready(s: &mut Store) {
    if !s.loaded {
        s.loaded = true;
        s.map = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    }
}

/// The shift for `video_id` (seconds; 0 when none): add it to the song's position before
/// finding the sung line.
pub fn get(video_id: &str) -> f32 {
    let Ok(mut s) = STORE.lock() else { return 0.0 };
    ready(&mut s);
    s.map.get(video_id).map_or(0.0, |v| v.0)
}

/// Moves `video_id`'s lyrics by `by` seconds (+: the lines come sooner, -: later); the new shift.
pub fn shift(video_id: &str, by: f32) -> f32 {
    if video_id.is_empty() {
        return 0.0;
    }
    let Ok(mut s) = STORE.lock() else { return 0.0 };
    ready(&mut s);
    let now = (s.map.get(video_id).map_or(0.0, |v| v.0) + by).clamp(-10.0, 10.0);
    let now = (now * 4.0).round() / 4.0;
    if now == 0.0 {
        s.map.remove(video_id);
    } else {
        s.map.insert(video_id.to_string(), (now, crate::updates::now()));
    }
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
    now
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_songs_shift_is_kept() {
        assert_eq!(super::get("offset-test-song"), 0.0);
        assert_eq!(super::shift("offset-test-song", 0.25), 0.25);
        assert_eq!(super::shift("offset-test-song", 0.25), 0.5);
        assert_eq!(super::get("offset-test-song"), 0.5);
        assert_eq!(super::shift("offset-test-song", -0.5), 0.0);
        assert_eq!(super::shift("offset-test-song", 30.0), 10.0, "at most 10 s");
    }
}
