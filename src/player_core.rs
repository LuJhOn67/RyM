//! What happens around playing, the same in every PC app (RyM Slint, RyM GTK, the terminal):
//! each app keeps these and calls them; only showing the result is the app's own. A feature here
//! is written once for all three.
//!
//! - [`NonMusic`]: SponsorBlock's non-music parts of the playing song, each jumped over once.
//! - [`Scrobbler`]: Last.fm's "now playing" and the scrobble once half the song played.
//! - [`alarm_rings`] and [`shuffled`]: the alarm, once a day at its time.
//! - [`PeerWatch`]: Seamless Music, when a phone's music should move here.
//! - [`song_with_radio`] and [`found_song`]: "Play in RyM", a song from a link.
//! - [`preview_start`]: Preview Drop, 15 seconds from a song's best part.
//! - [`Focus`]: the focus timer (25 minutes of music, a 5-minute break softer).
//! - [`OutputWatch`]: headphones or Bluetooth connected with nothing playing (ask what to play).
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
    /// The phone took the music: none taken back while it keeps playing there (it used to come
    /// back here every half hour), until something is played here or a new listening starts.
    phone_kept: bool,
    /// When the phone was last heard playing: a pause of [`NEW_LISTENING`] or more, then music
    /// again, is a new listening, which may move here again.
    phone_played_at: Option<Instant>,
}

/// A phone silent (paused, or RyM closed on it) this long, then playing again: a new listening.
const NEW_LISTENING: Duration = Duration::from_secs(5 * 60);

impl PeerWatch {
    /// A phone's note: true when its music should move here: Seamless Music on (`on`), the same
    /// account (`key`), the phone playing, this app idle, not taken back from the phone lately,
    /// and not the song already taken.
    pub fn wants(&mut self, note: &crate::link::Announce, key: Option<&str>, idle: bool, on: bool) -> bool {
        self.wants_at(note, key, idle, on, Instant::now())
    }

    fn wants_at(&mut self, note: &crate::link::Announce, key: Option<&str>, idle: bool, on: bool, now: Instant) -> bool {
        if note.kind != "phone" || key != Some(note.account.as_str()) {
            return false;
        }
        if note.playing {
            if self.phone_played_at.is_some_and(|t| now.duration_since(t) >= NEW_LISTENING) {
                self.phone_kept = false;
            }
            self.phone_played_at = Some(now);
        }
        if !on || !idle || !note.playing || note.video_id.is_empty() || self.phone_kept {
            return false;
        }
        let seen = (note.device.clone(), note.video_id.clone());
        if self.seen.as_ref() == Some(&seen) {
            return false;
        }
        self.seen = Some(seen);
        true
    }

    /// The phone took the music from here: it stays there while it plays.
    pub fn phone_took(&mut self) {
        self.phone_kept = true;
        self.phone_played_at = Some(Instant::now());
    }

    /// Something was played here by hand: the phone's music may move here again.
    pub fn played_here(&mut self) {
        self.phone_kept = false;
    }
}

/// The EQ of each output device: the speakers, the headphones and a Bluetooth set each keep their
/// own; when the sound moves to another device, its EQ comes back.
#[derive(Default)]
pub struct DeviceEq {
    /// The device the sound went to at the last look.
    device: Option<String>,
    looked: Option<Instant>,
}

impl DeviceEq {
    /// A look every few seconds (each tick is fine): true when the sound moved to a device with
    /// an EQ of its own and `cfg`'s EQ changed to it (the app puts the sound settings in place).
    pub fn check(&mut self, cfg: &mut Config) -> bool {
        if self.looked.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return false;
        }
        self.looked = Some(Instant::now());
        let now = crate::audio::output_name();
        if now == self.device {
            return false;
        }
        let first_look = self.device.is_none();
        self.device = now.clone();
        let Some((preset, custom)) = now.and_then(|d| cfg.eq_by_device.get(&d).copied()) else { return false };
        if first_look && preset == cfg.eq_preset && custom == cfg.eq_custom {
            return false;
        }
        cfg.eq_preset = preset;
        cfg.eq_custom = custom;
        true
    }

    /// The device the sound goes to (at the last look).
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// The EQ was changed by hand: it's this device's from now on.
    pub fn changed(&mut self, cfg: &mut Config) {
        let device = self.device.clone().or_else(crate::audio::output_name);
        if let Some(d) = device {
            cfg.eq_by_device.insert(d, (cfg.eq_preset, cfg.eq_custom));
        }
    }
}

/// Three quick skips in a row: Up next takes a new direction (another artist).
#[derive(Default)]
pub struct SkipStreak {
    /// The artists skipped in a row.
    artists: Vec<String>,
}

impl SkipStreak {
    /// A song was left (`left`: the song, seconds heard, its length) for another. A quick skip
    /// adds to the streak, a song heard ends it; the third skip in a row gives the new
    /// direction's seed: a liked song by an artist not skipped (None when there's none).
    pub fn left(&mut self, left: Option<&(Track, f32, f32)>, liked: &[Track]) -> Option<Track> {
        let (t, listened, length) = left?;
        if !crate::ram_points::is_skip(*listened, *length) {
            self.artists.clear();
            return None;
        }
        self.artists.push(crate::radio::first_artist(&t.artist));
        if self.artists.len() < 3 {
            return None;
        }
        let avoid = std::mem::take(&mut self.artists);
        let fits: Vec<&Track> = liked.iter().filter(|l| crate::is_youtube(&l.video_id) && !avoid.contains(&crate::radio::first_artist(&l.artist))).collect();
        (!fits.is_empty()).then(|| fits[crate::updates::now() as usize % fits.len()].clone())
    }
}

/// The new direction's songs: the seed's radio (hand them, ranked for the seed, to Up next in
/// place of the songs that were coming).
pub async fn new_direction(api: Api, seed: Track) -> anyhow::Result<(Track, Vec<Track>)> {
    let radio = crate::radio::with_artist(api, seed.clone()).await?;
    Ok((seed, radio))
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
    fn three_skips_turn_to_another_artist() {
        let song = |id: &str, artist: &str| Track { video_id: id.into(), title: id.into(), artist: artist.into(), album: String::new(), duration: String::new(), thumb: None, album_id: None, artist_id: None, link: None };
        let liked = vec![song("l1aaaaaaaaa", "Skipped One"), song("l2aaaaaaaaa", "Someone Else")];
        let mut st = SkipStreak::default();
        assert!(st.left(Some(&(song("a", "Skipped One"), 5.0, 200.0)), &liked).is_none());
        assert!(st.left(Some(&(song("b", "Skipped One"), 5.0, 200.0)), &liked).is_none());
        let seed = st.left(Some(&(song("c", "Skipped Two"), 5.0, 200.0)), &liked).expect("a new direction");
        assert_eq!(seed.artist, "Someone Else");
        // A song heard ends the streak.
        let mut st = SkipStreak::default();
        let _ = st.left(Some(&(song("a", "X"), 5.0, 200.0)), &liked);
        let _ = st.left(Some(&(song("b", "X"), 150.0, 200.0)), &liked);
        let _ = st.left(Some(&(song("c", "X"), 5.0, 200.0)), &liked);
        assert!(st.left(Some(&(song("d", "X"), 5.0, 200.0)), &liked).is_none());
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
    fn the_phone_keeps_the_music_while_it_plays() {
        let mut w = PeerWatch::default();
        let t0 = Instant::now();
        w.phone_took();
        // An hour of songs on the phone, a note every minute: never taken back (was: half an hour).
        for m in 1..=60u64 {
            let at = t0 + Duration::from_secs(m * 60);
            assert!(!w.wants_at(&note("P", &format!("s{m}"), "k", true), Some("k"), true, true, at), "minute {m}");
        }
        // Paused on the phone for ten minutes, then music again: a new listening moves here.
        let later = t0 + Duration::from_secs(70 * 60);
        assert!(!w.wants_at(&note("P", "s60", "k", false), Some("k"), true, true, later - Duration::from_secs(5)));
        assert!(w.wants_at(&note("P", "new", "k", true), Some("k"), true, true, later));
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

// ---------------------------------------------------------------- Preview Drop

/// How long Preview Drop plays.
pub const PREVIEW_SECS: f32 = 15.0;

/// Where Preview Drop starts: the song's SponsorBlock highlight, else about a minute in (where the
/// chorus usually is; a third of the way in for a short song). `length` in seconds.
pub fn preview_start(highlight: Option<f32>, length: Option<f32>) -> f32 {
    let length = length.filter(|l| *l > 0.0);
    if let Some(h) = highlight.filter(|h| *h >= 0.0 && length.is_none_or(|l| *h < l - 3.0)) {
        return h;
    }
    match length {
        Some(l) => (l / 3.0).min(60.0).min((l - PREVIEW_SECS).max(0.0)),
        None => 60.0,
    }
}

// ---------------------------------------------------------------- the focus timer

/// The focus timer's parts: music to work to, then a break with the music softer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusPart {
    Work,
    Break,
}

impl FocusPart {
    pub fn length(self) -> Duration {
        match self {
            FocusPart::Work => Duration::from_secs(25 * 60),
            FocusPart::Break => Duration::from_secs(5 * 60),
        }
    }

    /// The music's volume in this part (1: as set).
    pub fn level(self) -> f32 {
        match self {
            FocusPart::Work => 1.0,
            FocusPart::Break => 0.4,
        }
    }
}

/// The focus timer (Pomodoro): 25 minutes of calm music (Lo-Fi, the rain look), then a 5-minute
/// break with the music softer, again and again until it's stopped.
#[derive(Default)]
pub struct Focus {
    part: Option<(FocusPart, Instant)>,
}

/// What a focus session plays while working.
pub const FOCUS_SEARCH: &str = "lofi hip hop beats to study";

impl Focus {
    /// Starts it (working) or stops it; the part it's in now (None: stopped).
    pub fn toggle(&mut self, now: Instant) -> Option<FocusPart> {
        self.part = match self.part {
            Some(_) => None,
            None => Some((FocusPart::Work, now)),
        };
        self.part.map(|p| p.0)
    }

    pub fn on(&self) -> bool {
        self.part.is_some()
    }

    /// The part it's in and the time left of it.
    pub fn left(&self, now: Instant) -> Option<(FocusPart, Duration)> {
        self.part.map(|(p, began)| (p, p.length().saturating_sub(now.saturating_duration_since(began))))
    }

    /// Each tick: the part it moved into, when the last one ran out.
    pub fn tick(&mut self, now: Instant) -> Option<FocusPart> {
        let (p, began) = self.part?;
        if now.saturating_duration_since(began) < p.length() {
            return None;
        }
        let next = match p {
            FocusPart::Work => FocusPart::Break,
            FocusPart::Break => FocusPart::Work,
        };
        self.part = Some((next, began + p.length()));
        Some(next)
    }

    /// The music's volume now (1 when off or working).
    pub fn level(&self) -> f32 {
        self.part.map_or(1.0, |p| p.0.level())
    }
}

/// "24:10": the time left, as minutes and seconds.
pub fn mm_ss(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

// ---------------------------------------------------------------- headphones connected

/// Headphones or a Bluetooth set by its name (Windows, Linux and macOS name them so).
pub fn is_personal_output(name: &str) -> bool {
    let n = name.to_lowercase();
    ["headphone", "headset", "earphone", "earbud", "airpods", "buds", "bluetooth", "hands-free", "handsfree", "a2dp", "سماع"].iter().any(|w| n.contains(w))
}

/// Headphones or Bluetooth connected with nothing playing: RyM asks what to play (the last
/// songs, or a Road Radio mix), at most every 10 minutes.
#[derive(Default)]
pub struct OutputWatch {
    /// The device at the last look (None before the first).
    device: Option<String>,
    asked: Option<Instant>,
}

impl OutputWatch {
    /// The sound goes to `now` (from [`DeviceEq::device`]): true when it just moved to headphones
    /// or Bluetooth, nothing plays and RyM didn't ask in the last 10 minutes.
    pub fn moved(&mut self, now: Option<&str>, playing: bool, at: Instant) -> bool {
        let Some(now) = now else { return false };
        let Some(before) = self.device.replace(now.to_string()) else { return false };
        if before == now || playing || !is_personal_output(now) {
            return false;
        }
        if self.asked.is_some_and(|t| at.saturating_duration_since(t) < Duration::from_secs(600)) {
            return false;
        }
        self.asked = Some(at);
        true
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[test]
    fn preview_starts_at_the_best_part() {
        assert_eq!(preview_start(Some(72.0), Some(200.0)), 72.0);
        // No highlight: a minute in, a third in for a short song, never past its last 15 s.
        assert_eq!(preview_start(None, Some(240.0)), 60.0);
        assert_eq!(preview_start(None, Some(90.0)), 30.0);
        assert_eq!(preview_start(None, Some(20.0)), 5.0);
        assert_eq!(preview_start(None, None), 60.0);
        // A highlight past the end is ignored.
        assert_eq!(preview_start(Some(300.0), Some(200.0)), 60.0);
    }

    #[test]
    fn focus_works_then_breaks() {
        let t0 = Instant::now();
        let mut f = Focus::default();
        assert_eq!(f.level(), 1.0);
        assert_eq!(f.toggle(t0), Some(FocusPart::Work));
        assert_eq!(f.tick(t0 + Duration::from_secs(60)), None);
        assert_eq!(f.left(t0 + Duration::from_secs(60)).map(|l| l.1), Some(Duration::from_secs(24 * 60)));
        assert_eq!(f.tick(t0 + Duration::from_secs(25 * 60)), Some(FocusPart::Break));
        assert_eq!(f.level(), 0.4);
        assert_eq!(f.tick(t0 + Duration::from_secs(30 * 60)), Some(FocusPart::Work));
        assert_eq!(f.level(), 1.0);
        assert_eq!(f.toggle(t0), None);
        assert!(!f.on());
        assert_eq!(mm_ss(Duration::from_secs(24 * 60 + 5)), "24:05");
    }

    #[test]
    fn headphones_connecting_asks_once() {
        let t0 = Instant::now();
        let mut w = OutputWatch::default();
        // The first look only learns the device.
        assert!(!w.moved(Some("Speakers (Realtek(R) Audio)"), false, t0));
        assert!(w.moved(Some("Headphones (WH-1000XM4)"), false, t0));
        assert!(!w.moved(Some("Headphones (WH-1000XM4)"), false, t0));
        // Back and again within 10 minutes: not again.
        assert!(!w.moved(Some("Speakers (Realtek(R) Audio)"), false, t0));
        assert!(!w.moved(Some("Headphones (WH-1000XM4)"), false, t0 + Duration::from_secs(60)));
        // Playing already, or speakers: nothing to ask.
        let mut w = OutputWatch::default();
        w.moved(Some("Speakers"), false, t0);
        assert!(!w.moved(Some("Galaxy Buds2 Pro"), true, t0));
        assert!(!w.moved(Some("Monitor (HDMI)"), false, t0));
        assert!(is_personal_output("سماعات الرأس"));
    }
}

/// The rain's wind at `t` seconds, as a share of the fall speed: it turns slowly from one side to
/// the other (about a minute and a half for a full turn), so the rain doesn't lean one way.
pub fn wind(t: f32) -> f32 {
    0.05 * (t * 0.07).sin() + 0.015 * (t * 0.23).sin()
}

/// Drop `i`'s own small lean (±1% of the fall speed), always the same for that drop.
pub fn lean(i: usize) -> f32 {
    ((i * 37 % 17) as f32 / 16.0 - 0.5) * 0.02
}

#[cfg(test)]
mod wind_tests {
    #[test]
    fn the_wind_turns_both_ways() {
        let blows: Vec<f32> = (0..2000).map(|i| super::wind(i as f32 * 0.1)).collect();
        assert!(blows.iter().any(|w| *w > 0.03) && blows.iter().any(|w| *w < -0.03), "both sides");
        let mean = blows.iter().sum::<f32>() / blows.len() as f32;
        assert!(mean.abs() < 0.02, "no side wins: {mean}");
        let leans: Vec<f32> = (0..75).map(super::lean).collect();
        assert!(leans.iter().any(|l| *l > 0.0) && leans.iter().any(|l| *l < 0.0));
    }
}
