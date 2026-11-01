//! The play queue's rules, shared by the apps:
//! - "Add to queue" puts a song right after the playing one, after any added before it (so
//!   several added songs keep their order); "Play next" puts it right after the playing one.
//! - Starting something new keeps what was already played above it, so Previous goes back to it.
//! - Songs can be moved; the playing one stays the playing one.

use crate::api::Track;

/// Songs kept above a new start, at most (the oldest go first).
pub const HISTORY: usize = 100;

/// The queue: songs, the playing one, and how many songs added with "Add to queue" wait right
/// after it.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    pub songs: Vec<Track>,
    pub current: Option<usize>,
    pub added: usize,
}

impl Queue {
    pub fn now(&self) -> Option<&Track> {
        self.current.and_then(|i| self.songs.get(i))
    }

    /// Starts `new` from its row `start`, keeping the songs played so far (up to the playing
    /// one) above it; the songs that were coming next are dropped. Returns the new playing index.
    pub fn start(&mut self, new: Vec<Track>, start: usize) -> usize {
        let keep = self.current.map_or(0, |c| c + 1).min(self.songs.len());
        self.songs.truncate(keep);
        let extra = self.songs.len().saturating_sub(HISTORY);
        self.songs.drain(..extra);
        let at = self.songs.len() + start.min(new.len().saturating_sub(1));
        self.songs.extend(new);
        self.current = Some(at);
        self.added = 0;
        at
    }

    /// Right after the playing song (before songs added with "Add to queue"; it joins them, so
    /// songs added later still go after it).
    pub fn play_next(&mut self, t: Track) {
        match self.current {
            Some(c) => {
                self.songs.insert(c + 1, t);
                self.added += 1;
            }
            None => {
                self.songs.push(t);
                self.current = Some(self.songs.len() - 1);
            }
        }
    }

    /// After the playing song and the songs added before this one. Returns true when nothing
    /// was playing (the caller starts it).
    pub fn add(&mut self, t: Track) -> bool {
        match self.current {
            Some(c) => {
                let at = (c + 1 + self.added).min(self.songs.len());
                self.songs.insert(at, t);
                self.added += 1;
                false
            }
            None => {
                self.songs.push(t);
                self.current = Some(self.songs.len() - 1);
                true
            }
        }
    }

    /// The playing song changes to row `i`: moving on to the next song uses up one added song;
    /// jumping anywhere else forgets them (they're just songs in the queue now).
    pub fn went_to(&mut self, i: usize) {
        self.added = match self.current {
            Some(c) if i == c + 1 => self.added.saturating_sub(1),
            _ => 0,
        };
        self.current = Some(i);
    }

    /// Moves row `from` to `to`; the playing song stays the playing one.
    pub fn move_song(&mut self, from: usize, to: usize) {
        if from >= self.songs.len() || to >= self.songs.len() || from == to {
            return;
        }
        let t = self.songs.remove(from);
        self.songs.insert(to, t);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
        self.added = 0;
    }

    /// Removes row `i` (not the playing one).
    pub fn remove(&mut self, i: usize) {
        if i >= self.songs.len() || Some(i) == self.current {
            return;
        }
        self.songs.remove(i);
        match self.current {
            Some(c) if i < c => self.current = Some(c - 1),
            // One of the added songs waiting after the playing one.
            Some(c) if i <= c + self.added => self.added -= 1,
            _ => {}
        }
    }
}

/// A song's link for sharing.
pub fn share_link(t: &Track) -> String {
    crate::soundcloud::share_link(&t.video_id).unwrap_or_else(|| format!("https://music.youtube.com/watch?v={}", t.video_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artist: String::new(),
            album: String::new(),
            duration: String::new(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    fn ids(q: &Queue) -> Vec<&str> {
        q.songs.iter().map(|t| t.video_id.as_str()).collect()
    }

    #[test]
    fn added_songs_come_right_after_in_order_and_play_next_goes_first() {
        let mut q = Queue::default();
        q.start(vec![t("a"), t("b"), t("c")], 0);
        q.add(t("x"));
        q.add(t("y"));
        assert_eq!(ids(&q), ["a", "x", "y", "b", "c"]);
        q.play_next(t("n"));
        assert_eq!(ids(&q), ["a", "n", "x", "y", "b", "c"]);
        // Moving on uses up an added song: the next one still goes after the rest.
        q.went_to(1);
        q.went_to(2);
        q.add(t("z"));
        assert_eq!(ids(&q), ["a", "n", "x", "y", "z", "b", "c"]);
    }

    #[test]
    fn a_new_start_keeps_what_was_played() {
        let mut q = Queue::default();
        q.start(vec![t("a"), t("b"), t("c")], 0);
        q.went_to(1);
        let at = q.start(vec![t("s"), t("r1"), t("r2")], 0);
        // a and b stay above; c (still to come) is replaced by the new song and its radio.
        assert_eq!(ids(&q), ["a", "b", "s", "r1", "r2"]);
        assert_eq!(at, 2);
        assert_eq!(q.now().unwrap().video_id, "s");
    }

    #[test]
    fn moving_keeps_the_playing_song() {
        let mut q = Queue::default();
        q.start(vec![t("a"), t("b"), t("c"), t("d")], 1);
        q.move_song(3, 0);
        assert_eq!(ids(&q), ["d", "a", "b", "c"]);
        assert_eq!(q.now().unwrap().video_id, "b");
        q.move_song(2, 3);
        assert_eq!(ids(&q), ["d", "a", "c", "b"]);
        assert_eq!(q.now().unwrap().video_id, "b");
        q.remove(0);
        assert_eq!(q.now().unwrap().video_id, "b");
    }

    #[test]
    fn removing_a_later_song_keeps_the_added_ones_in_order() {
        let mut q = Queue::default();
        q.start(vec![t("a"), t("b"), t("c")], 0);
        q.add(t("x"));
        q.remove(3);
        q.add(t("y"));
        assert_eq!(ids(&q), ["a", "x", "y", "b"]);
        q.remove(1);
        q.add(t("z"));
        assert_eq!(ids(&q), ["a", "y", "z", "b"]);
    }

    #[test]
    fn play_next_then_add_to_queue_then_up_next() {
        // Blue Hair plays with its Up next; "Play next" one song, "Add to queue" another.
        let mut q = Queue::default();
        q.start(vec![t("blue"), t("up1"), t("up2")], 0);
        q.play_next(t("river"));
        q.add(t("telephones"));
        assert_eq!(ids(&q), ["blue", "river", "telephones", "up1", "up2"]);
        // The other way round gives the same order: Play next always comes first.
        let mut q = Queue::default();
        q.start(vec![t("blue"), t("up1")], 0);
        q.add(t("telephones"));
        q.play_next(t("river"));
        assert_eq!(ids(&q), ["blue", "river", "telephones", "up1"]);
    }
}
