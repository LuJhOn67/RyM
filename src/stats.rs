//! Listening stats kept on this computer: time listened (in all, and per month per artist), and
//! with the play counts of `recent`, the "Your recap" page: top songs, top artists this month,
//! hours listened. Time is added up in memory and written once a minute (and when the app
//! closes), never on every tick. Saved as `stats.json` in the data folder.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::api::Track;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Month {
    ms: u64,
    /// Artist -> milliseconds listened.
    artists: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Stats {
    total_ms: u64,
    /// "2026-11" -> that month.
    months: BTreeMap<String, Month>,
}

/// Time listened since the last write: artist -> ms.
static PENDING: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

fn path() -> std::path::PathBuf {
    crate::data_dir().join("stats.json")
}

fn load() -> Stats {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// "2026-11" for a time (seconds since 1970, UTC).
pub fn month_of(secs: u64) -> String {
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}")
}

fn artist_of(t: &Track) -> String {
    t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string()
}

/// `ms` of `t` were listened to (added up; written by [`flush`]).
pub fn listened(t: &Track, ms: u64) {
    if ms == 0 {
        return;
    }
    if let Ok(mut p) = PENDING.lock() {
        *p.entry(artist_of(t)).or_default() += ms;
    }
}

/// Writes the time added up since the last write.
pub fn flush() {
    let pending = match PENDING.lock() {
        Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
        _ => return,
    };
    let mut s = load();
    let month = s.months.entry(month_of(crate::updates::now())).or_default();
    for (artist, ms) in pending {
        s.total_ms += ms;
        month.ms += ms;
        if !artist.is_empty() {
            *month.artists.entry(artist).or_default() += ms;
        }
    }
    if let Ok(json) = serde_json::to_string(&s) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

/// The "Your recap" page.
#[derive(Debug, Clone, Default)]
pub struct Recap {
    pub hours_total: f32,
    pub hours_month: f32,
    /// Most played songs, with their plays.
    pub top_songs: Vec<(Track, u32)>,
    /// Artists listened to most this month, with hours.
    pub top_artists: Vec<(String, f32)>,
}

pub fn recap() -> Recap {
    flush();
    let s = load();
    let month = s.months.get(&month_of(crate::updates::now())).cloned().unwrap_or_default();
    let mut artists: Vec<(String, u64)> = month.artists.into_iter().collect();
    artists.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut played = crate::recent::load();
    played.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.last.cmp(&a.last)));
    Recap {
        hours_total: s.total_ms as f32 / 3_600_000.0,
        hours_month: month.ms as f32 / 3_600_000.0,
        top_songs: played.into_iter().take(10).map(|p| (p.track, p.plays)).collect(),
        top_artists: artists.into_iter().take(10).map(|(a, ms)| (a, ms as f32 / 3_600_000.0)).collect(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn months() {
        assert_eq!(super::month_of(0), "1970-01");
        assert_eq!(super::month_of(1_793_000_000), "2026-10");
        assert_eq!(super::month_of(1_793_600_000), "2026-11");
    }
}
