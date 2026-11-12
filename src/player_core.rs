//! What happens around playing, the same in every PC app (RyM Slint, RyM GTK, the terminal):
//! each app keeps these and calls them; only showing the result is the app's own. A feature here
//! is written once for all three.
//!
//! - [`NonMusic`]: SponsorBlock's non-music parts of the playing song, each jumped over once.
//! - [`Scrobbler`]: Last.fm's "now playing" and the scrobble once half the song played.
//! - [`alarm_rings`] and [`shuffled`]: the alarm, once a day at its time.
//! - [`PeerWatch`]: Seamless Music, when a phone's music should move here.
//! - [`song_with_radio`] and [`found_song`]: "Play in RyM", a song from a link.
//!
//! Network work comes back as a future for the app to run on its own runtime.

use std::future::Future;
use std::time::{Duration, Instant};

use crate::api::{Api, Track};
use crate::config::Config;
use crate::sponsorblock::Segments;

/// SponsorBlock: the playing song's non-music parts (intro, talk, outro), each jumped over once.
#[derive(Default)]
pub struct NonMusic {
    song: String,
    parts: Segments,
}

impl NonMusic {
    /// A new song plays: its parts are forgotten, and the future asking for them is given when
    /// skipping is on and it's a YouTube song (hand its answer to [`got`](Self::got)).
    pub fn song(&mut self, video_id: &str, on: bool) -> Option<impl Future<Output = (String, Segments)> + Send + use<>> {
        self.song = video_id.to_string();
        self.parts.clear();
        (on && crate::is_youtube(video_id)).then(|| {
            let v = video_id.to_string();
            async move {
                let parts = crate::sponsorblock::segments(&v).await;
                (v, parts)
            }
        })
    }

    /// The parts came (kept only for the song still playing).
    pub fn got(&mut self, video_id: &str, parts: Segments) {
        if self.song == video_id {
            self.parts = parts;
        }
    }

    /// Where to jump now, when `pos` (seconds) is in one of the playing song's parts.
    pub fn jump(&mut self, video_id: &str, pos: f32, on: bool) -> Option<f32> {
        if !on || self.song != video_id {
            return None;
        }
        let to = crate::sponsorblock::skip_to(&self.parts, pos)?;
        // Once per part: a jump that doesn't land isn't tried again and again.
        self.parts.retain(|p| p.1 != to);
        Some(to)
    }
}

/// Last.fm (signed in from RyM Slint's settings): "now playing" when a song starts, and the
/// scrobble once half of it (or 4 minutes) was heard.
#[derive(Default)]
pub struct Scrobbler {
    /// The song, when it started (seconds since 1970), seconds heard, and whether it was sent.
    song: Option<(String, u64, f32, bool)>,
}

impl Scrobbler {
    fn scrobble(t: &Track, duration: f32, started: u64) -> crate::lastfm::Scrobble {
        crate::lastfm::Scrobble { artist: t.artist.clone(), title: t.title.clone(), album: t.album.clone(), duration: duration as u32, started }
    }

    /// A song started: its listening counts from now, and Last.fm is told it plays (the future;
    /// None when not signed in to Last.fm).
    pub fn start(&mut self, t: &Track, duration: f32, cfg: &Config) -> Option<impl Future<Output = ()> + Send + use<>> {
        if cfg.lastfm_session.is_empty() {
            self.song = None;
            return None;
        }
        let started = crate::updates::now();
        self.song = Some((t.video_id.clone(), started, 0.0, false));
        let sc = Self::scrobble(t, duration, started);
        let (key, secret, session) = (cfg.lastfm_key.clone(), cfg.lastfm_secret.clone(), cfg.lastfm_session.clone());
        Some(async move {
            let _ = crate::lastfm::now_playing(sc, key, secret, session).await;
        })
    }

    /// `secs` more heard of `t`: the scrobble (the future) once it counts.
    pub fn heard(&mut self, t: Option<&Track>, secs: f32, duration: f32, cfg: &Config) -> Option<impl Future<Output = ()> + Send + use<>> {
        let t = t?;
        let (sid, started, heard, done) = self.song.clone()?;
        let heard = heard + secs;
        let counts = !done && sid == t.video_id && crate::lastfm::counts(heard, duration);
        self.song = Some((sid, started, heard, done || counts));
        counts.then(|| {
            let sc = Self::scrobble(t, duration, started);
            let (key, secret, session) = (cfg.lastfm_key.clone(), cfg.lastfm_secret.clone(), cfg.lastfm_session.clone());
            async move {
                let _ = crate::lastfm::scrobble(sc, key, secret, session).await;
            }
        })
    }
}

/// The alarm's time has come (once a day, RyM open): the day is marked in `cfg` and saved.
pub fn alarm_rings(cfg: &mut Config) -> bool {
    if !cfg.alarm_on {
        return false;
    }
    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    if now.format("%H:%M").to_string() != cfg.alarm_time || cfg.alarm_rang == today {
        return false;
    }
    cfg.alarm_rang = today;
    cfg.save();
    true
}

/// Songs in a new order (for the alarm's liked songs).
pub fn shuffled(mut songs: Vec<Track>) -> Vec<Track> {
    let n = songs.len();
    for i in (1..n).rev() {
        let j = (crate::updates::now() as usize).wrapping_mul(2_654_435_761).wrapping_add(i * 40503) % (i + 1);
        songs.swap(i, j);
    }
    songs
}

/// Seamless Music: when a phone's music should move here.
#[derive(Default)]
pub struct PeerWatch {
    /// The phone song last taken (or asked about): not again.
    seen: Option<(String, String)>,
    /// The phone took the music lately: none taken back until then (or something played here).
    quiet_until: Option<Instant>,
}

impl PeerWatch {
    /// A phone's note: true when its music should move here: Seamless Music on (`on`), the same
    /// account (`key`), the phone playing, this app idle, not taken back from the phone lately,
    /// and not the song already taken.
    pub fn wants(&mut self, note: &crate::link::Announce, key: Option<&str>, idle: bool, on: bool) -> bool {
        if !on || !idle || note.kind != "phone" || !note.playing || note.video_id.is_empty() || key != Some(note.account.as_str()) {
            return false;
        }
        if self.quiet_until.is_some_and(|t| Instant::now() < t) {
            return false;
        }
        let seen = (note.device.clone(), note.video_id.clone());
        if self.seen.as_ref() == Some(&seen) {
            return false;
        }
        self.seen = Some(seen);
        true
    }

    /// The phone took the music from here: it stays there for half an hour.
    pub fn phone_took(&mut self) {
        self.quiet_until = Some(Instant::now() + Duration::from_secs(30 * 60));
    }

    /// Something was played here by hand: the phone's music may move here again.
    pub fn played_here(&mut self) {
        self.quiet_until = None;
    }
}

/// "Play in RyM": a song by its id, its radio after it (the song first; a bare song when the
/// radio can't be had).
pub async fn song_with_radio(api: Api, video_id: String) -> Vec<Track> {
    let mut radio = api.radio(video_id.clone()).await.unwrap_or_default();
    let song = match radio.iter().position(|t| t.video_id == video_id) {
        Some(i) => radio.remove(i),
        None => Track { video_id: video_id.clone(), title: crate::i18n::tr("Song").into(), artist: String::new(), album: String::new(), duration: String::new(), thumb: None, album_id: None, artist_id: None, link: None },
    };
    radio.retain(|t| t.video_id != video_id);
    radio.insert(0, song);
    radio
}

/// "Play in RyM": a Spotify or SoundCloud song's link, found (Spotify: on YouTube Music).
pub async fn found_song(api: Api, target: crate::links::Target) -> anyhow::Result<Vec<Track>> {
    match target {
        crate::links::Target::Spotify { link, .. } => crate::spotify::import(api, link).await,
        crate::links::Target::SoundCloud { link, .. } => crate::soundcloud::open_link(link).await,
        _ => Err(anyhow::anyhow!("not a Spotify or SoundCloud song")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(device: &str, vid: &str, account: &str, playing: bool) -> crate::link::Announce {
        crate::link::Announce { rym: 1, kind: "phone".into(), device: device.into(), account: account.into(), port: 1, playing, video_id: vid.into(), title: String::new(), artist: String::new(), thumb: None, position: 0.0 }
    }

    #[test]
    fn a_phone_song_moves_here_once() {
        let mut w = PeerWatch::default();
        assert!(!w.wants(&note("P", "a", "k", true), Some("k"), true, false), "Seamless Music off");
        assert!(!w.wants(&note("P", "a", "x", true), Some("k"), true, true), "another account");
        assert!(!w.wants(&note("P", "a", "k", true), Some("k"), false, true), "playing here");
        assert!(w.wants(&note("P", "a", "k", true), Some("k"), true, true));
        assert!(!w.wants(&note("P", "a", "k", true), Some("k"), true, true), "the same song again");
        w.phone_took();
        assert!(!w.wants(&note("P", "b", "k", true), Some("k"), true, true), "the phone took it lately");
        w.played_here();
        assert!(w.wants(&note("P", "b", "k", true), Some("k"), true, true));
    }

    #[test]
    fn each_part_jumped_once() {
        let mut nm = NonMusic::default();
        let _ = nm.song("dQw4w9WgXcQ", true);
        nm.got("dQw4w9WgXcQ", vec![(10.0, 20.0)]);
        nm.got("otherSong01", vec![(0.0, 99.0)]);
        assert_eq!(nm.jump("dQw4w9WgXcQ", 12.0, true), Some(20.0));
        assert_eq!(nm.jump("dQw4w9WgXcQ", 12.0, true), None, "once");
        assert_eq!(nm.jump("otherSong01", 5.0, true), None, "not the playing song");
    }

    #[test]
    fn last_fm_counts_half_the_song() {
        let cfg = Config { lastfm_session: "s".into(), ..Config::default() };
        let t = Track { video_id: "a".into(), title: "T".into(), artist: "A".into(), album: String::new(), duration: String::new(), thumb: None, album_id: None, artist_id: None, link: None };
        let mut sc = Scrobbler::default();
        let _ = sc.start(&t, 200.0, &cfg);
        assert!(sc.heard(Some(&t), 90.0, 200.0, &cfg).is_none());
        assert!(sc.heard(Some(&t), 15.0, 200.0, &cfg).is_some(), "past half");
        assert!(sc.heard(Some(&t), 50.0, 200.0, &cfg).is_none(), "sent once");
        let mut off = Scrobbler::default();
        assert!(off.start(&t, 200.0, &Config::default()).is_none(), "not signed in to Last.fm");
    }
}
