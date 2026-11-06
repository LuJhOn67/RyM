//! The "For you" home page:
//! - "Recently played" on top: the songs played on this computer, newest first;
//! - "For you": a mix of three pools fetched at once, taken in turns:
//!   fresh songs (the radios of the last few songs played), new songs (radios and home-page songs
//!   the user has never played or liked), and familiar ones (liked and most played, about a third);
//!   no repeats, no re-uploads, at most three songs by one artist;
//! - then YouTube Music's own shelves (playlists, albums, mixes), where items seen on three
//!   different days without being opened rest for two days (one showing per six hours), and the
//!   page keeps at least 40 of them;
//! - scrolled to the bottom, it goes on ([`more`]): another mix from new seeds, and YouTube's
//!   shelves further down, never a song already on the page.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::{Api, LIKED_ID, More, Paged, Token, Track};

pub const RECENTLY_PLAYED: &str = "Recently played";
pub const FOR_YOU: &str = "For you";
pub const MORE_FOR_YOU: &str = "More for you";
const TIRED_AFTER: u32 = 3;
const REST_SECS: u64 = 2 * 24 * 60 * 60;
/// Showings closer together than this count once.
const SHOW_GAP_SECS: u64 = 6 * 60 * 60;
/// The page keeps at least this many items (rested ones come back, longest-unseen first).
const AT_LEAST: usize = 40;
/// Songs by one artist in a mix, at most.
const PER_ARTIST: usize = 3;
/// Forgotten after this long, so the file stays small.
const FORGET_SECS: u64 = 30 * 24 * 60 * 60;
/// Songs in the "Recently played" row, in the first mix, and in each later one.
const RECENT_ROW: usize = 20;
const FIRST_MIX: usize = 40;
const NEXT_MIX: usize = 30;

/// A fresh home feed, the liked songs fetched on the way (for the hearts), and how it goes on.
pub struct Refreshed {
    pub home: Vec<Track>,
    pub liked: Option<Vec<Track>>,
    pub more: Option<More>,
}

/// Where the feed goes on from: YouTube's next home page, the seeds used so far, every song on
/// the page (never shown twice), and the user's liked songs and history to draw from.
#[derive(Debug, Clone)]
pub struct Cursor {
    home: Option<Token>,
    used: Vec<String>,
    shown: HashSet<String>,
    liked: Arc<Vec<Track>>,
    history: Arc<Vec<Track>>,
}

/// Recently played, the "For you" mix, then YouTube's shelves; and the cursor to go on.
pub async fn refresh(api: Api) -> Result<Refreshed> {
    let user = api.logged_in();
    let recent = crate::recent::tracks();
    let (home, liked, history) = tokio::join!(
        api.clone().home_pages(3),
        async { if user { api.clone().playlist_tracks(LIKED_ID.into()).await.ok() } else { None } },
        async { if user { api.clone().history().await.ok() } else { None } },
    );
    let home = home?;
    let liked_list = liked.clone().unwrap_or_default();
    let history = history.unwrap_or_default();
    let mut memory = Memory::load();
    let now = now();
    let arranged = memory.arrange(home.tracks, now);

    // Fresh: the radios of the last songs played (here, or on YouTube Music); new: the radios
    // of a couple of liked or played songs picked at random.
    let mut fresh_seeds: Vec<String> = Vec::new();
    for t in recent.iter().chain(history.iter()) {
        if crate::is_youtube(&t.video_id) && !fresh_seeds.contains(&t.video_id) {
            fresh_seeds.push(t.video_id.clone());
        }
        if fresh_seeds.len() == 4 {
            break;
        }
    }
    let pool: Vec<&Track> = liked_list.iter().take(80).chain(history.iter().take(40)).filter(|t| !fresh_seeds.contains(&t.video_id)).collect();
    let new_seeds = memory.pick_seeds(&pool, 2, now);
    let (fresh_radios, new_radios) = tokio::join!(radios(&api, &fresh_seeds), radios(&api, &new_seeds));

    let known: HashSet<String> = liked_list.iter().chain(history.iter()).chain(recent.iter()).map(|t| t.video_id.clone()).collect();
    let fresh = turns(fresh_radios);
    let mut new: Vec<Track> = turns(new_radios);
    new.extend(arranged.iter().filter(|t| t.link.is_none()).cloned());
    new.retain(|t| !known.contains(&t.video_id));
    let familiar = familiar(&liked_list, &mut memory);

    // Played here or on the phone: the account's history first (every device of the account
    // reports its plays there), then songs known only on this device.
    let mut seen_row = HashSet::new();
    let recent_row: Vec<Track> = history
        .iter()
        .cloned()
        .chain(recent)
        .filter(|t| t.link.is_none() && seen_row.insert(t.video_id.clone()))
        .take(RECENT_ROW)
        .map(|t| Track { album: RECENTLY_PLAYED.into(), ..t })
        .collect();
    let mut skip: HashSet<String> = recent_row.iter().map(|t| t.video_id.clone()).collect();
    skip.extend(fresh_seeds.iter().cloned());
    let for_you = mix(fresh, new, familiar, &skip, FIRST_MIX, FOR_YOU);
    for t in &for_you {
        memory.seen(&t.video_id, now);
    }
    memory.save();

    let in_mix: HashSet<&str> = for_you.iter().map(|t| t.video_id.as_str()).collect();
    let shelves: Vec<Track> = arranged.into_iter().filter(|t| t.link.is_some() || !in_mix.contains(t.video_id.as_str())).collect();
    let mut out = recent_row;
    out.extend(for_you);
    out.extend(shelves);
    let cursor = Cursor {
        home: match home.more {
            Some(More::Home(t)) => Some(t),
            _ => None,
        },
        used: fresh_seeds.into_iter().chain(new_seeds).collect(),
        shown: out.iter().map(key).collect(),
        liked: Arc::new(liked_list),
        history: Arc::new(history),
    };
    Ok(Refreshed { home: out, liked, more: Some(More::Feed(Box::new(cursor))) })
}

/// The next part of the feed: a mix from three new seeds (one of the songs played lately, two at
/// random), and YouTube's home page further down; nothing already on the page.
pub async fn more(api: Api, mut c: Cursor) -> Result<Paged> {
    let mut memory = Memory::load();
    let now = now();
    let recent = crate::recent::tracks();
    let unused = |t: &&Track| crate::is_youtube(&t.video_id) && !c.used.contains(&t.video_id);
    let mut seeds: Vec<String> = recent.iter().filter(unused).take(1).map(|t| t.video_id.clone()).collect();
    let pool: Vec<&Track> = recent.iter().chain(c.history.iter()).chain(c.liked.iter()).filter(unused).filter(|t| !seeds.contains(&t.video_id)).collect();
    for _ in 0..2 {
        if pool.is_empty() {
            break;
        }
        let t = pool[memory.next(pool.len().min(40))];
        if !seeds.contains(&t.video_id) {
            seeds.push(t.video_id.clone());
        }
    }
    let next_home = c.home.take();
    let (lists, page) = tokio::join!(radios(&api, &seeds), async {
        match next_home {
            Some(token) => api.clone().home_after(token).await.ok(),
            None => None,
        }
    });
    let known: HashSet<String> = c.liked.iter().chain(c.history.iter()).chain(recent.iter()).map(|t| t.video_id.clone()).collect();
    let fresh = turns(lists);
    let new: Vec<Track> = fresh.iter().filter(|t| !known.contains(&t.video_id)).cloned().collect();
    let familiar = familiar(&c.liked, &mut memory);
    let mut out = mix(fresh, new, familiar, &c.shown, NEXT_MIX, MORE_FOR_YOU);
    for t in &out {
        memory.seen(&t.video_id, now);
    }
    memory.save();
    if let Some(page) = page {
        c.home = match page.more {
            Some(More::Home(t)) => Some(t),
            _ => None,
        };
        let mixed: HashSet<String> = out.iter().map(key).collect();
        out.extend(page.tracks.into_iter().filter(|t| !c.shown.contains(&key(t)) && !mixed.contains(&key(t))));
    }
    c.used.extend(seeds);
    c.shown.extend(out.iter().map(key));
    // Nothing new and nowhere left to look: that was all.
    let more = (!out.is_empty() || c.home.is_some()).then(|| More::Feed(Box::new(c)));
    Ok(Paged { tracks: out, more })
}

/// Each seed's radio (the ones that answered).
async fn radios(api: &Api, seeds: &[String]) -> Vec<Vec<Track>> {
    let mut jobs = tokio::task::JoinSet::new();
    for seed in seeds {
        jobs.spawn(api.clone().radio(seed.clone()));
    }
    let mut lists = Vec::new();
    while let Some(r) = jobs.join_next().await {
        if let Ok(Ok(list)) = r {
            lists.push(list);
        }
    }
    lists
}

/// The lists taken in turns (first of each, then second of each...).
fn turns(lists: Vec<Vec<Track>>) -> Vec<Track> {
    let mut iters: Vec<_> = lists.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    while !iters.is_empty() {
        iters.retain_mut(|it| match it.next() {
            Some(t) => {
                out.push(t);
                true
            }
            None => false,
        });
    }
    out
}

/// Liked songs and the most played ones, in a shuffled order.
fn familiar(liked: &[Track], memory: &mut Memory) -> Vec<Track> {
    let mut pool: Vec<Track> = crate::recent::favourites(40);
    pool.extend(liked.iter().take(150).cloned());
    for i in (1..pool.len()).rev() {
        let j = memory.next(i + 1);
        pool.swap(i, j);
    }
    pool
}

/// The three pools taken in turns, about 35% fresh, 30% new and 35% familiar (a pool that runs
/// out leaves its turns to the others): no song in `skip`, no repeats or re-uploads, at most three
/// by one artist. Each song gets `section` as its shelf.
fn mix(fresh: Vec<Track>, new: Vec<Track>, familiar: Vec<Track>, skip: &HashSet<String>, n: usize, section: &str) -> Vec<Track> {
    let mut pools = [fresh.into_iter(), new.into_iter(), familiar.into_iter()];
    let weights = [35usize, 30, 35];
    let mut taken = [0usize; 3];
    let mut out: Vec<Track> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut songs: HashSet<String> = HashSet::new();
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut open = [true; 3];
    while out.len() < n && open.iter().any(|o| *o) {
        // The open pool furthest behind its share goes next.
        let Some(p) = (0..3).filter(|&p| open[p]).min_by_key(|&p| (taken[p] * 100) / weights[p]) else { break };
        let Some(t) = pools[p].next() else {
            open[p] = false;
            continue;
        };
        let artist = crate::radio::first_artist(&t.artist);
        let crowded = !artist.is_empty() && per_artist.get(&artist).is_some_and(|c| *c >= PER_ARTIST);
        if t.link.is_some() || skip.contains(&t.video_id) || crowded || !ids.insert(t.video_id.clone()) || !songs.insert(crate::radio::same_song_key(&t)) {
            continue;
        }
        *per_artist.entry(artist).or_default() += 1;
        taken[p] += 1;
        out.push(Track { album: section.into(), ..t });
    }
    out
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
        // Too much rests: the items seen longest ago come back (in YouTube's order), so the page
        // never runs thin.
        if arranged.len() < AT_LEAST {
            let have: HashSet<String> = arranged.iter().map(key).collect();
            let mut rested: Vec<&Track> = items.iter().filter(|t| !have.contains(&key(t))).collect();
            rested.sort_by_key(|t| self.items.get(&key(t)).map_or(0, |s| s.last));
            let back: HashSet<String> = rested.into_iter().take(AT_LEAST - arranged.len()).map(key).collect();
            arranged = items.iter().filter(|t| have.contains(&key(t)) || back.contains(&key(t))).cloned().collect();
        }
        for t in &arranged {
            let s = self.items.entry(key(t)).or_default();
            // Opening the app again and again soon after counts as one showing.
            if now.saturating_sub(s.last) >= SHOW_GAP_SECS || s.shown == 0 {
                s.shown += 1;
            }
            s.last = now;
        }
        arranged
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

    /// A song put in a mix: it waits a while before it's picked again.
    fn seen(&mut self, id: &str, now: u64) {
        let s = self.items.entry(id.to_string()).or_default();
        s.shown += 1;
        s.last = now;
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
    fn items_rest_after_three_days_seen_and_the_page_stays_full() {
        let mut m = memory();
        let feed: Vec<Track> = (0..50).map(|i| song(&format!("a{i}"), "Quick picks")).collect();
        let mut more = feed.clone();
        more.extend((0..10).map(|i| song(&format!("b{i}"), "Mixes")));
        // Opening the app many times within an hour counts as one showing.
        for t in 0..5 {
            m.arrange(feed.clone(), 1000 + t * 60);
        }
        assert_eq!(m.arrange(more.clone(), 2000).len(), 60);
        let day = 24 * 60 * 60;
        m.arrange(feed.clone(), 1000 + day);
        m.arrange(feed.clone(), 1000 + 2 * day);
        // Seen on three days: they rest; the new ones stay and the page is filled up to 40
        // with the ones seen longest ago.
        let rested = m.arrange(more.clone(), 1000 + 2 * day + 100);
        assert_eq!(rested.len(), AT_LEAST);
        assert!((0..10).all(|i| rested.iter().any(|t| t.video_id == format!("b{i}"))));
        // Two days later everything is back.
        assert_eq!(m.arrange(more, 1000 + 4 * day + 200).len(), 60);
    }

    #[test]
    fn mix_takes_the_pools_in_turns_without_repeats() {
        let pool = |p: &str, n: usize| -> Vec<Track> { (0..n).map(|i| Track { artist: format!("{p}{i}"), ..song(&format!("{p}{i}"), "") }).collect() };
        let skip: HashSet<String> = ["f0".to_string()].into();
        let got = mix(pool("f", 10), pool("n", 10), pool("a", 10), &skip, 20, FOR_YOU);
        assert_eq!(got.len(), 20);
        assert!(got.iter().all(|t| t.album == FOR_YOU && t.video_id != "f0"));
        let count = |p: char| got.iter().filter(|t| t.video_id.starts_with(p)).count();
        assert!((6..=8).contains(&count('f')) && (5..=7).contains(&count('n')) && (6..=8).contains(&count('a')), "{got:?}");
        // The first few already mix all three.
        assert!(['f', 'n', 'a'].iter().all(|p| got.iter().take(3).any(|t| t.video_id.starts_with(*p))));
        // A pool that runs out leaves its turns to the others; one artist at most three times.
        let same: Vec<Track> = (0..10).map(|i| Track { artist: "Band".into(), ..song(&format!("b{i}"), "") }).collect();
        let got = mix(same, Vec::new(), pool("a", 10), &HashSet::new(), 10, FOR_YOU);
        assert_eq!(got.iter().filter(|t| t.artist == "Band").count(), 3);
        assert_eq!(got.len(), 10);
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

    /// The real feed with the saved sign-in: sections and counts, then two more parts; and a
    /// search with its next page. Prints counts only.
    #[tokio::test]
    #[ignore]
    async fn live_feed_and_more() {
        let cookie = std::fs::read_to_string(crate::cookie_path()).unwrap();
        let api = Api::with_cookie(cookie).await.unwrap();
        let r = refresh(api.clone()).await.unwrap();
        let mut sections: Vec<(String, usize)> = Vec::new();
        for t in &r.home {
            match sections.iter_mut().find(|(n, _)| *n == t.album) {
                Some((_, c)) => *c += 1,
                None => sections.push((t.album.clone(), 1)),
            }
        }
        println!("first: {} items: {:?}", r.home.len(), sections.iter().take(6).collect::<Vec<_>>());
        let mut seen: HashSet<String> = r.home.iter().map(key).collect();
        let mut more = r.more;
        for round in 1..=2 {
            let Some(m) = more.take() else { break };
            let p = api.clone().more(m).await.unwrap();
            let repeats = p.tracks.iter().filter(|t| !seen.insert(key(t))).count();
            println!("more {round}: {} items, {repeats} repeats, goes on: {}", p.tracks.len(), p.more.is_some());
            more = p.more;
        }
        let s = api.clone().search_paged("tame impala".into()).await.unwrap();
        let first = s.tracks.len();
        let next = api.more(s.more.expect("search goes on")).await.unwrap();
        println!("search: {first} then {} more, goes on: {}", next.tracks.len(), next.more.is_some());
    }
}


#[cfg(test)]
mod history_check {
    /// Read only: the account history's newest songs (a phone's plays show here too):
    /// `cargo test --lib live_history_top -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_history_top() {
        let cookie = std::fs::read_to_string(crate::cookie_path()).expect("signed in");
        let api = crate::api::Api::with_cookie(cookie).await.unwrap();
        for t in api.history().await.unwrap().iter().take(3) {
            println!("history: {} - {} ({})", t.title, t.artist, t.album);
        }
    }
}
