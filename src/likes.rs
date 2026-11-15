//! Liked songs, kept in step with the account: likes made on the phone (or the website) show here
//! after the next sync, and a like made here a moment ago isn't undone by a sync that started
//! before YouTube Music saved it.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How often the apps re-read the liked songs while open.
pub const SYNC_EVERY: Duration = Duration::from_secs(120);
/// A like or unlike made here wins over synced lists until the account's list agrees, or this
/// long at most (YouTube Music's liked list can take minutes to show a new like).
const LOCAL_WINS_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// The whole liked list is read again this often (and at the start); between, only its first page.
const FULL_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Reading the liked songs: the whole list (every page, however many) at the start and once a
/// day; between, only the first page (new likes come at the top), merged into the list read
/// before. One request every sync instead of one per hundred likes.
#[derive(Debug, Default)]
pub struct Reading {
    full_at: Option<Instant>,
}

impl Reading {
    /// The read to do now (hand it the list read before): the whole list when it's due or nothing
    /// was read yet.
    pub fn read(&mut self, api: crate::api::Api, before: Vec<crate::api::Track>) -> impl std::future::Future<Output = anyhow::Result<Vec<crate::api::Track>>> + Send + use<> {
        let full = before.is_empty() || self.full_at.is_none_or(|t| t.elapsed() >= FULL_EVERY);
        if full {
            self.full_at = Some(Instant::now());
        }
        async move {
            if full {
                return api.playlist_all(crate::api::LIKED_ID.into(), usize::MAX).await;
            }
            let top = api.playlist_tracks(crate::api::LIKED_ID.into()).await?;
            Ok(merge_top(top, &before, |t| t.video_id.as_str()))
        }
    }

    /// The next read is the whole list (a full read failed).
    pub fn again(&mut self) {
        self.full_at = None;
    }
}

/// The liked list's first page (newest first) over the list read before: the songs above the
/// page's last known song that aren't on it any more were unliked; the rest of the old list
/// follows. Unlikes further down show at the next whole read.
pub fn merge_top<T: Clone>(top: Vec<T>, before: &[T], id: impl Fn(&T) -> &str) -> Vec<T> {
    let on_top: HashSet<&str> = top.iter().map(&id).collect();
    // The page's last song the old list has: everything above it in the old list is on the page,
    // or was unliked.
    let anchor = top.iter().rev().find_map(|t| before.iter().position(|b| id(b) == id(t)));
    let rest = match anchor {
        Some(k) => &before[k + 1..],
        None => before,
    };
    let mut out = top.clone();
    out.extend(rest.iter().filter(|b| !on_top.contains(id(b))).cloned());
    out
}

#[derive(Debug, Default, Clone)]
pub struct Likes {
    ids: HashSet<String>,
    local: HashMap<String, (bool, Instant)>,
}

impl Likes {
    pub fn contains(&self, video_id: &str) -> bool {
        self.ids.contains(video_id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// A like (true) or unlike made in this app.
    pub fn set(&mut self, video_id: &str, liked: bool) {
        if liked {
            self.ids.insert(video_id.to_string());
        } else {
            self.ids.remove(video_id);
        }
        self.local.insert(video_id.to_string(), (liked, Instant::now()));
    }

    /// The account's liked songs as YouTube Music has them now.
    pub fn sync(&mut self, ids: impl IntoIterator<Item = String>) {
        self.ids = ids.into_iter().collect();
        // A change the account's list shows now is done; one it doesn't show yet still wins.
        let ids = &self.ids;
        self.local.retain(|id, (liked, at)| ids.contains(id) != *liked && at.elapsed() < LOCAL_WINS_FOR);
        for (id, (liked, _)) in &self.local {
            if *liked {
                self.ids.insert(id.clone());
            } else {
                self.ids.remove(id);
            }
        }
    }

    /// The liked songs to show after a sync: songs liked here that YouTube's list doesn't show
    /// yet stay (first, as the newest), and songs unliked here go, so the list matches the hearts.
    pub fn shown(&self, synced: Vec<crate::api::Track>, before: &[crate::api::Track]) -> Vec<crate::api::Track> {
        let ids: HashSet<&str> = synced.iter().map(|t| t.video_id.as_str()).collect();
        let mut out: Vec<crate::api::Track> = before.iter().filter(|t| self.contains(&t.video_id) && !ids.contains(t.video_id.as_str())).cloned().collect();
        out.extend(synced.into_iter().filter(|t| self.contains(&t.video_id)));
        out
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.local.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_takes_the_account_but_keeps_fresh_local_changes() {
        let mut l = Likes::default();
        l.sync(["a".to_string(), "b".to_string()]);
        // Liked "c" here and unliked "a"; the account doesn't know yet.
        l.set("c", true);
        l.set("a", false);
        // Phone unliked "b" and liked "d".
        l.sync(["a".to_string(), "d".to_string()]);
        assert!(!l.contains("a") && !l.contains("b") && l.contains("c") && l.contains("d"));
        assert_eq!(l.len(), 2);
    }

    fn merged(top: &[&str], before: &[&str]) -> Vec<String> {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        merge_top(v(top), &v(before), |s: &String| s.as_str())
    }

    #[test]
    fn the_first_page_merges_into_the_list() {
        let before = ["e", "d", "c", "b", "a"];
        // Two new likes on top, "d" unliked; the page ends at "c".
        assert_eq!(merged(&["g", "f", "e", "c"], &before), ["g", "f", "e", "c", "b", "a"]);
        // Only new songs on the page: the old list follows whole.
        assert_eq!(merged(&["x", "y"], &before), ["x", "y", "e", "d", "c", "b", "a"]);
        // Nothing read before.
        assert_eq!(merged(&["x"], &[]), ["x"]);
    }

    fn song(id: &str) -> crate::api::Track {
        crate::api::Track { video_id: id.into(), album_id: None, artist_id: None, title: id.into(), artist: String::new(), album: String::new(), duration: String::new(), link: None, thumb: None }
    }

    #[test]
    fn the_list_matches_the_hearts() {
        let mut l = Likes::default();
        l.sync(["a".to_string(), "b".to_string()]);
        let before = vec![song("a"), song("b")];
        l.set("new", true);
        l.set("b", false);
        let mut with_new = vec![song("new")];
        with_new.extend(before);
        // YouTube's list doesn't show either change yet.
        l.sync(["a".to_string(), "b".to_string()]);
        let ids: Vec<String> = l.shown(vec![song("a"), song("b")], &with_new).into_iter().map(|t| t.video_id).collect();
        assert_eq!(ids, ["new", "a"]);
    }

    #[test]
    fn a_like_stays_until_the_account_shows_it() {
        let mut l = Likes::default();
        l.set("new", true);
        // Many syncs before YouTube's list has it: still liked.
        for _ in 0..5 {
            l.sync(["old".to_string()]);
            assert!(l.contains("new"));
        }
        // The list has it now: from here the account decides (an unlike elsewhere shows).
        l.sync(["old".to_string(), "new".to_string()]);
        l.sync(["old".to_string()]);
        assert!(!l.contains("new"));
    }
}
