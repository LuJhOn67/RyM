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
        // Kept as "from the top" (not removed), so the phone hears it was finished too.
        if s.map.get(video_id).is_some_and(|v| v.0 > 0.0) {
            s.map.insert(video_id.to_string(), (0.0, crate::updates::now()));
            save(&mut s);
        }
        s.written = Some((video_id.to_string(), at));
    } else if moved && at > EDGE_SECS {
        s.map.insert(video_id.to_string(), (at, crate::updates::now()));
        s.written = Some((video_id.to_string(), at));
        save(&mut s);
    }
}

/// Every place kept, for the phone of the same account (`GET /talks`): `{"id": [second, when]}`.
pub fn export() -> String {
    let Ok(mut s) = STORE.lock() else { return "{}".into() };
    ready(&mut s);
    serde_json::to_string(&s.map).unwrap_or_else(|_| "{}".into())
}

/// The phone's places merged in (the newer one of each talk wins, a finished one too): a talk
/// stopped on the phone goes on here from the same second. How many changed. Checked like an
/// import: odd ids, seconds and times are left out.
pub fn merge(text: &str) -> usize {
    let Ok(theirs) = serde_json::from_str::<HashMap<String, (f32, u64)>>(text) else { return 0 };
    let Ok(mut s) = STORE.lock() else { return 0 };
    ready(&mut s);
    let latest = crate::updates::now() + 24 * 60 * 60;
    let mut changed = 0;
    for (id, (at, when)) in theirs {
        let ok_id = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c));
        if !ok_id || !at.is_finite() || !(0.0..=24.0 * 3600.0).contains(&at) || when > latest {
            continue;
        }
        if s.map.get(&id).is_none_or(|mine| when > mine.1) {
            s.map.insert(id, (at, when));
            changed += 1;
        }
    }
    if changed > 0 {
        save(&mut s);
    }
    changed
}

#[cfg(test)]
mod tests {
    #[test]
    fn places_move_between_devices() {
        let long = 50.0 * 60.0;
        super::note("sync-test1", 700.0, long);
        let now = crate::updates::now();
        // The phone went further later; an older phone place doesn't undo this one.
        let phone = format!(r#"{{"sync-test1": [1500.0, {}], "sync-test2": [300.0, {}], "bad id!": [5.0, 1], "sync-test3": [1e9, 1]}}"#, now + 5, now);
        assert_eq!(super::merge(&phone), 2);
        assert_eq!(super::place("sync-test1", long), Some(1500.0));
        assert_eq!(super::place("sync-test2", long), Some(300.0));
        assert_eq!(super::merge(&format!(r#"{{"sync-test1": [100.0, {}]}}"#, now - 100)), 0);
        // Finished on the phone: from the top here too.
        assert_eq!(super::merge(&format!(r#"{{"sync-test1": [0.0, {}]}}"#, now + 10)), 1);
        assert_eq!(super::place("sync-test1", long), None);
        assert!(super::export().contains("sync-test2"));
    }

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
