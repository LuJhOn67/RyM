//! A home feed that changes on every refresh. YouTube Music sends much the same page each time, so:
//! - items shown twice without being opened rest for three days, and never-shown ones come first;
//! - a "New for you" shelf on top comes from the radio of a few songs the user liked or played
//!   lately (different songs each refresh), leaving out songs they already know or saw lately.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::{Api, LIKED_ID, Track};

pub const NEW_FOR_YOU: &str = "New for you";
const TIRED_AFTER: u32 = 2;
const REST_SECS: u64 = 3 * 24 * 60 * 60;
/// Forgotten after this long, so the file stays small.
const FORGET_SECS: u64 = 30 * 24 * 60 * 60;
const SEEDS: usize = 3;
const PICKS: usize = 20;

/// A fresh home feed, and the liked songs fetched on the way (for the hearts).
pub struct Refreshed {
    pub home: Vec<Track>,
    pub liked: Option<Vec<Track>>,
}

/// Home feed, liked songs and history at once, then the radios for "New for you".
pub async fn refresh(api: Api) -> Result<Refreshed> {
    let user = api.logged_in();
    let (home, liked, history) = tokio::join!(
        api.clone().home(),
        async { if user { api.clone().playlist_tracks(LIKED_ID.into()).await.ok() } else { None } },
        async { if user { api.clone().history().await.ok() } else { None } },
    );
    let home = home?;
    let mut memory = Memory::load();
    let now = now();
    let arranged = memory.arrange(home, now);

    // Seeds: the newest liked songs and history first, then the feed's own songs (guests).
    let liked_list = liked.clone().unwrap_or_default();
    let history = history.unwrap_or_default();
    let pool: Vec<&Track> = liked_list
        .iter()
        .take(60)
        .chain(history.iter().take(40))
        .chain(arranged.iter().filter(|t| t.link.is_none()))
        .collect();
    let seeds = memory.pick_seeds(&pool, SEEDS, now);
    let mut radios = tokio::task::JoinSet::new();
    for seed in &seeds {
        radios.spawn(api.clone().radio(seed.clone()));
    }
    let mut lists = Vec::new();
    while let Some(r) = radios.join_next().await {
        if let Ok(Ok(list)) = r {
            lists.push(list);
        }
    }
    let known: HashSet<&str> = liked_list
        .iter()
        .chain(history.iter())
        .chain(arranged.iter())
        .map(|t| t.video_id.as_str())
        .chain(seeds.iter().map(String::as_str))
        .collect();
    let picks = memory.pick_new(lists, &known, now);
    memory.save();

    let mut out = picks;
    out.extend(arranged);
    Ok(Refreshed { home: out, liked })
}

/// Opened or played from the feed: never rested.
pub fn opened(t: &Track) {
    let mut m = Memory::load();
    m.items.entry(key(t)).or_default().opened = true;
    m.save();
}

fn key(t: &Track) -> String {
    match &t.link {
        Some(crate::api::Link::Playlist(id) | crate::api::Link::Album(id) | crate::api::Link::Mix(id)) => id.clone(),
        None => t.video_id.clone(),
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Default, Serialize, Deserialize)]
struct Seen {
    shown: u32,
    last: u64,
    opened: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Memory {
    items: HashMap<String, Seen>,
    /// Songs used as seeds lately, newest last; they wait before being picked again.
    seeds: Vec<String>,
    #[serde(skip)]
    random: u64,
}

impl Memory {
    fn path() -> PathBuf {
        crate::data_dir().join("feed-memory.json")
    }

    fn load() -> Memory {
        let mut m: Memory = std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        m.random = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
        m
    }

    fn save(&mut self) {
        let now = now();
        self.items.retain(|_, s| s.opened || now.saturating_sub(s.last) < FORGET_SECS);
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::create_dir_all(crate::data_dir());
            let _ = std::fs::write(Self::path(), json);
        }
    }

    /// xorshift: good enough to pick songs.
    fn next(&mut self, below: usize) -> usize {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random % below.max(1) as u64) as usize
    }

    /// The feed to show: rested items left out, unseen ones first in each section.
    fn arrange(&mut self, items: Vec<Track>, now: u64) -> Vec<Track> {
        let fresh: Vec<&Track> = items
            .iter()
            .filter(|t| {
                let s = self.items.entry(key(t)).or_default();
                if s.opened || s.shown < TIRED_AFTER {
                    true
                } else if now.saturating_sub(s.last) > REST_SECS {
                    // Rested long enough: comes back as if new.
                    s.shown = 0;
                    true
                } else {
                    false
                }
            })
            .collect();
        // Sections stay in YouTube's order; inside each, never-shown items first.
        let mut sections: Vec<&str> = Vec::new();
        for t in &fresh {
            if !sections.contains(&t.album.as_str()) {
                sections.push(&t.album);
            }
        }
        let shown = |t: &Track| self.items.get(&key(t)).map_or(0, |s| s.shown);
        let mut arranged: Vec<Track> = Vec::new();
        for section in sections {
            let mut part: Vec<&Track> = fresh.iter().copied().filter(|t| t.album == section).collect();
            part.sort_by_key(|t| (shown(t) > 0) as u8);
            arranged.extend(part.into_iter().cloned());
        }
        // Everything rested at once: show what YouTube sent rather than an empty page.
        let result = if arranged.len() < 8 { items } else { arranged };
        for t in &result {
            let s = self.items.entry(key(t)).or_default();
            s.shown += 1;
            s.last = now;
        }
        result
    }

    /// Up to `n` seed songs, at random, skipping ones used for the last few refreshes.
    fn pick_seeds(&mut self, pool: &[&Track], n: usize, _now: u64) -> Vec<String> {
        let mut candidates: Vec<&str> = Vec::new();
        for t in pool {
            if !self.seeds.contains(&t.video_id) && !candidates.contains(&t.video_id.as_str()) {
                candidates.push(&t.video_id);
            }
        }
        if candidates.len() < n {
            // Everything was used lately: start over.
            self.seeds.clear();
            candidates.clear();
            for t in pool {
                if !candidates.contains(&t.video_id.as_str()) {
                    candidates.push(&t.video_id);
                }
            }
        }
        let mut picked = Vec::new();
        while picked.len() < n && !candidates.is_empty() {
            let i = self.next(candidates.len());
            picked.push(candidates.swap_remove(i).to_string());
        }
        self.seeds.extend(picked.iter().cloned());
        let extra = self.seeds.len().saturating_sub(60);
        self.seeds.drain(..extra);
        picked
    }

    /// Songs from the radios, taken in turns, that the user doesn't know and wasn't shown lately.
    fn pick_new(&mut self, lists: Vec<Vec<Track>>, known: &HashSet<&str>, now: u64) -> Vec<Track> {
        let mut lists: Vec<std::vec::IntoIter<Track>> = lists.into_iter().map(Vec::into_iter).collect();
        let mut out: Vec<Track> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();
        while out.len() < PICKS && !lists.is_empty() {
            let mut i = 0;
            while i < lists.len() && out.len() < PICKS {
                match lists[i].next() {
                    Some(mut t) => {
                        let recent = self.items.get(&t.video_id).is_some_and(|s| s.shown > 0 && now.saturating_sub(s.last) < REST_SECS);
                        if !known.contains(t.video_id.as_str()) && !recent && taken.insert(t.video_id.clone()) {
                            t.album = NEW_FOR_YOU.into();
                            out.push(t);
                        }
                        i += 1;
                    }
                    None => {
                        lists.remove(i);
                    }
                }
            }
        }
        for t in &out {
            let s = self.items.entry(t.video_id.clone()).or_default();
            s.shown += 1;
            s.last = now;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, section: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artist: String::new(),
            album: section.into(),
            duration: String::new(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    fn memory() -> Memory {
        Memory { random: 0x9E37_79B9_7F4A_7C15, ..Memory::default() }
    }

    #[test]
    fn items_rest_after_two_showings_and_come_back_later() {
        let mut m = memory();
        let feed: Vec<Track> = (0..20).map(|i| song(&format!("a{i}"), "Quick picks")).collect();
        let mut more = feed.clone();
        more.extend((0..10).map(|i| song(&format!("b{i}"), "Mixes")));
        m.arrange(feed.clone(), 1000);
        m.arrange(feed.clone(), 2000);
        // Third time: the 20 shown twice rest; the 10 new ones are shown.
        let third = m.arrange(more.clone(), 3000);
        assert_eq!(third.iter().map(|t| t.video_id.as_str()).collect::<Vec<_>>(), (0..10).map(|i| format!("b{i}")).collect::<Vec<_>>());
        // Three days later they're back.
        let later = m.arrange(more, 3000 + REST_SECS + 1);
        assert_eq!(later.len(), 30);
    }

    #[test]
    fn opened_items_never_rest() {
        let mut m = memory();
        let feed: Vec<Track> = (0..10).map(|i| song(&format!("a{i}"), "S")).collect();
        m.items.entry("a3".into()).or_default().opened = true;
        for _ in 0..3 {
            m.arrange(feed.clone(), 100);
        }
        // Everything else rested: fewer than 8 left, so the whole feed shows instead of one item.
        assert_eq!(m.arrange(feed.clone(), 100).len(), 10);
    }

    #[test]
    fn new_picks_skip_known_songs_and_change_each_time() {
        let mut m = memory();
        let radio: Vec<Track> = (0..60).map(|i| song(&format!("r{i}"), "")).collect();
        let known: HashSet<&str> = ["r0", "r1", "r2"].into();
        let first = m.pick_new(vec![radio.clone()], &known, 100);
        assert_eq!(first.len(), PICKS);
        assert!(first.iter().all(|t| !known.contains(t.video_id.as_str()) && t.album == NEW_FOR_YOU));
        // The same radio again: none of the songs shown a moment ago.
        let second = m.pick_new(vec![radio], &known, 200);
        assert_eq!(second.len(), PICKS);
        assert!(second.iter().all(|t| !first.iter().any(|f| f.video_id == t.video_id)));
    }

    #[test]
    fn seeds_differ_between_refreshes() {
        let mut m = memory();
        let songs: Vec<Track> = (0..9).map(|i| song(&format!("s{i}"), "")).collect();
        let pool: Vec<&Track> = songs.iter().collect();
        let a = m.pick_seeds(&pool, 3, 0);
        let b = m.pick_seeds(&pool, 3, 0);
        let c = m.pick_seeds(&pool, 3, 0);
        let all: HashSet<&String> = a.iter().chain(&b).chain(&c).collect();
        assert_eq!(all.len(), 9, "{a:?} {b:?} {c:?}");
        // Pool used up: starts over instead of giving nothing.
        assert_eq!(m.pick_seeds(&pool, 3, 0).len(), 3);
    }
}
