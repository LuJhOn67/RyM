//! Long talks (backlog 48): a recording over 20 minutes (a talk, a lecture, a long Quran
//! recitation) keeps where it was stopped and starts there next time, and plays at its own speed
//! (Settings > "Long talks' speed"), which doesn't touch the songs' speed. Kept in
//! `talk-places.json`, the last 200.

use std::collections::HashMap;
use std::sync::Mutex;

/// Longer than this (seconds) counts as a long talk.
pub const LONG_SECS: f32 = 20.0 * 60.0;
const KEPT: usize = 200;
/// Under this from either end, nothing is kept (it starts again from the top).
const EDGE_SECS: f32 = 30.0;

/// The speeds to pick from in the settings.
pub const SPEEDS: [f32; 6] = [1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

pub fn is_long(seconds: f32) -> bool {
    seconds > LONG_SECS
}

#[derive(Default)]
struct Store {
    loaded: bool,
    /// Song id -> (the second it was at, when).
    map: HashMap<String, (f32, u64)>,
    /// The second last written for the playing one (writes every 10 s, not every tick).
    written: Option<(String, f32)>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| Mutex::new(Store::default()));

fn path() -> std::path::PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("rym-talk-places-test-{}.json", std::process::id()));
    }
    crate::data_dir().join("talk-places.json")
}

fn ready(s: &mut Store) {
    if !s.loaded {
        s.loaded = true;
        s.map = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    }
}

fn save(s: &mut Store) {
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

/// Where `video_id` (`seconds` long) was stopped, to start there; None for a song or from the top.
pub fn place(video_id: &str, seconds: f32) -> Option<f32> {
    if !is_long(seconds) {
        return None;
    }
    let mut s = STORE.lock().ok()?;
    ready(&mut s);
    s.map.get(video_id).map(|v| v.0).filter(|at| *at > EDGE_SECS && *at < seconds - EDGE_SECS)
}

/// The player is at `at` of `video_id` (`seconds` long): kept every 10 s; near the end it's
/// forgotten, so a finished talk starts from the top.
pub fn note(video_id: &str, at: f32, seconds: f32) {
    if !is_long(seconds) || video_id.is_empty() {
        return;
    }
    let Ok(mut s) = STORE.lock() else { return };
    ready(&mut s);
    let near_end = at >= seconds - EDGE_SECS;
    let moved = s.written.as_ref().is_none_or(|(id, was)| id != video_id || (at - was).abs() >= 10.0);
    if near_end {
        if s.map.remove(video_id).is_some() {
            save(&mut s);
        }
        s.written = Some((video_id.to_string(), at));
    } else if moved && at > EDGE_SECS {
        s.map.insert(video_id.to_string(), (at, crate::updates::now()));
        s.written = Some((video_id.to_string(), at));
        save(&mut s);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_long_talk_starts_where_it_stopped() {
        let long = 45.0 * 60.0;
        assert_eq!(super::place("talk-test", long), None);
        super::note("talk-test", 600.0, long);
        assert_eq!(super::place("talk-test", long), Some(600.0));
        super::note("talk-test", 605.0, long);
        assert_eq!(super::place("talk-test", long), Some(600.0), "written every 10 s");
        super::note("talk-test", 900.0, long);
        assert_eq!(super::place("talk-test", long), Some(900.0));
        super::note("talk-test", long - 5.0, long);
        assert_eq!(super::place("talk-test", long), None, "finished: from the top next time");
        // A song isn't kept.
        super::note("song-test", 120.0, 200.0);
        assert_eq!(super::place("song-test", 200.0), None);
    }
}
