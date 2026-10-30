//! Liked songs, kept in step with the account: likes made on the phone (or the website) show here
//! after the next sync, and a like made here a moment ago isn't undone by a sync that started
//! before YouTube Music saved it.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How often the apps re-read the liked songs while open.
pub const SYNC_EVERY: Duration = Duration::from_secs(120);
/// A like or unlike made here wins over synced lists for this long.
const LOCAL_WINS_FOR: Duration = Duration::from_secs(60);

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
        self.local.retain(|_, (_, at)| at.elapsed() < LOCAL_WINS_FOR);
        for (id, (liked, _)) in &self.local {
            if *liked {
                self.ids.insert(id.clone());
            } else {
                self.ids.remove(id);
            }
        }
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
}
