//! RaM points: how RyM picks the songs it suggests, on the home page ("For you") and in Up next.
//!
//! - **Kinds never mix.** Every song has a kind: songs, Husseini (latmiyat, nawa'i, qasa'id),
//!   Quran, nasheeds, or talk (podcasts, lectures). The words in its title and artist tell; an
//!   artist's songs teach RyM the artist's kind (kept in `ram-points.json`), and a song nothing
//!   tells about takes the kind of the radio it came in. A Husseini song brings only Husseini, a
//!   song only songs.
//! - **Points.** Each song that may come gets points: its place in YouTube's radio, the same
//!   artist as the playing song (a lot), artists heard lately or liked, the same writing (Arabic,
//!   Latin, Japanese...), and new songs over ones played or shown lately. The best go first; the
//!   same artist never twice in a row, liked songs only now and then.
//! - **Song points.** Playing a song gives it points, hearing it to the end a few more, skipping
//!   it in its first seconds takes some away; 30% of every change goes to its artist too, so an
//!   artist's songs not heard yet come up when the artist is a favorite. A song just played counts
//!   a third for 6 hours (no same top songs every time), and points fade by 5% a day. A song
//!   picked by hand loses its minus points first (a skip by mistake doesn't keep it down).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::api::Track;
use crate::radio::{first_artist, same_song_key};

pub const NAME: &str = "RaM points";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Songs,
    Husseini,
    Quran,
    Nasheed,
    Talk,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Songs, Kind::Husseini, Kind::Quran, Kind::Nasheed, Kind::Talk];

    /// Its name in the settings ("Husseini"...), and back.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Songs => "Songs",
            Kind::Husseini => "Husseini",
            Kind::Quran => "Quran",
            Kind::Nasheed => "Nasheeds",
            Kind::Talk => "Talks",
        }
    }

    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The home page's own shelf for this kind, when it isn't the page's main one.
    pub fn shelf(self) -> &'static str {
        match self {
            Kind::Songs => "Songs for you",
            Kind::Husseini => "Husseini for you",
            Kind::Quran => "Quran for you",
            Kind::Nasheed => "Nasheeds for you",
            Kind::Talk => "Talks for you",
        }
    }
}

/// Every kind's shelf name (for the apps' shelf titles).
pub const SHELVES: [&str; 5] = ["Songs for you", "Husseini for you", "Quran for you", "Nasheeds for you", "Talks for you"];

// Words that tell a song's kind (written as usual: they're normalized before matching).
const HUSSEINI: &[&str] = &[
    "لطمية", "لطميات", "لطم", "نعي", "نعاوي", "عزاء", "رادود", "الرادود", "محرم", "عاشوراء", "كربلاء", "الطف", "الحسين",
    "حسيني", "حسينية", "حسينيات", "ياحسين", "يا حسين", "الزهراء", "يا زهراء", "يازهراء", "يا زينب", "السيدة زينب", "ام البنين",
    "ابا الفضل", "ابو الفضل", "يا عباس", "العباس", "السجاد", "علي الاكبر", "الامام", "الائمة", "اهل البيت", "يا مهدي",
    "صاحب الزمان", "الاربعين", "مشاية", "موكب", "مواكب", "مواليد", "الكربلائي", "latmiya", "latmiyat", "latmya", "latmia",
    "nauha", "nohay", "nohe", "majlis", "muharram", "ashura", "karbala", "karbalaei", "karbalai", "matam", "radood", "ya hussain", "ya hussein",
    "ya husayn", "imam hussain", "imam hussein", "ya abbas", "ya zahra",
];
const QURAN: &[&str] = &["قرآن", "القرآن", "سورة", "تلاوة", "ترتيل", "مرتل", "مجود", "quran", "surah", "surat", "tilawat", "tilawah"];
const NASHEED: &[&str] = &["نشيد", "اناشيد", "انشودة", "nasheed", "nasheeds", "anasheed"];
const TALK: &[&str] = &["بودكاست", "محاضرة", "خطبة", "podcast", "lecture", "sermon", "khutbah"];
/// Arabic word beginnings ("and", "the", "with the"...) a word may carry.
const PREFIXES: [&str; 8] = ["وال", "بال", "فال", "لل", "ال", "و", "ب", "ف"];

/// Letters only, lower case; Arabic without its vowel marks and with one form of alif, taa
/// marbuta and yaa.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}' => {}
            'أ' | 'إ' | 'آ' | 'ٱ' => out.push('ا'),
            'ة' => out.push('ه'),
            'ى' => out.push('ي'),
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => out.push(' '),
        }
    }
    out
}

/// Each kind's words and phrases, normalized once.
/// A kind, its single telling words, and its telling phrases.
type Telling = (Kind, HashSet<String>, Vec<String>);

fn telling_words() -> &'static [Telling] {
    static WORDS: OnceLock<Vec<Telling>> = OnceLock::new();
    WORDS.get_or_init(|| {
        // Quran first: a reciter's "imam" shouldn't make it Husseini.
        [(Kind::Quran, QURAN), (Kind::Husseini, HUSSEINI), (Kind::Nasheed, NASHEED), (Kind::Talk, TALK)]
            .into_iter()
            .map(|(kind, list)| {
                let mut single = HashSet::new();
                let mut phrases = Vec::new();
                for w in list {
                    let n = normalize(w).split_whitespace().collect::<Vec<_>>().join(" ");
                    if n.contains(' ') {
                        phrases.push(format!(" {n} "));
                    } else {
                        single.insert(n);
                    }
                }
                (kind, single, phrases)
            })
            .collect()
    })
}

/// The kind the song's own words tell (title, artist, album), if any.
pub fn word_kind(t: &Track) -> Option<Kind> {
    let text = normalize(&format!("{} {} {}", t.title, t.artist, t.album));
    let words: Vec<&str> = text.split_whitespace().collect();
    let spaced = format!(" {} ", words.join(" "));
    telling_words().iter().find_map(|(kind, single, phrases)| {
        let word_hit = words.iter().any(|w| single.contains(*w) || PREFIXES.iter().any(|p| w.strip_prefix(p).is_some_and(|rest| single.contains(rest))));
        (word_hit || phrases.iter().any(|p| spaced.contains(p.as_str()))).then_some(*kind)
    })
}

/// The writing a song's title and artist are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Arabic,
    Latin,
    Japanese,
    Korean,
    Chinese,
    Cyrillic,
    Other,
}

/// The writing of the title (the artist's name only when the title has no letters): an
/// Arabic title by "Basim Al-Karbalaei" is Arabic.
pub fn script(t: &Track) -> Script {
    match script_of(&t.title) {
        Script::Other => script_of(&t.artist),
        s => s,
    }
}

fn script_of(text: &str) -> Script {
    let mut n = [0usize; 6];
    for c in text.chars() {
        match c as u32 {
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => n[0] += 1,
            0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F => n[1] += 1,
            0x3040..=0x30FF => n[2] += 1,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => n[3] += 1,
            0x4E00..=0x9FFF => n[4] += 1,
            0x0400..=0x04FF => n[5] += 1,
            _ => {}
        }
    }
    if n[2] > 0 {
        return Script::Japanese;
    }
    let (best, count) = n.iter().enumerate().max_by_key(|(_, c)| **c).unwrap_or((0, &0));
    if *count == 0 {
        return Script::Other;
    }
    [Script::Arabic, Script::Latin, Script::Japanese, Script::Korean, Script::Chinese, Script::Cyrillic][best]
}

/// What RyM has learned about artists: how much each kind was seen with them.
#[derive(Default, Serialize, Deserialize)]
pub struct Notes {
    artists: HashMap<String, [u32; 5]>,
    /// Artists the user put in a kind by hand ("Not this kind" in a song's menu): it wins.
    #[serde(default)]
    fixed: HashMap<String, Kind>,
    /// "Recommend more" / "Don't recommend": points added to (or taken from) an artist.
    #[serde(default)]
    bias: HashMap<String, i32>,
    /// Songs never suggested again ("Don't recommend").
    #[serde(default)]
    hidden: std::collections::HashSet<String>,
    /// Each song's points (from playing, finishing and skipping it), by id.
    #[serde(default)]
    tracks: HashMap<String, Score>,
    /// Each artist's share of their songs' points.
    #[serde(default)]
    affinity: HashMap<String, f32>,
    /// Artists as they're written (the keys above are lowercase), for the settings.
    #[serde(default)]
    names: HashMap<String, String>,
}

impl Notes {
    /// Artist keys saved before [`crate::radio::normalize_artist_key`] ("adelevevo", "adele -
    /// topic") moved onto the normalized key, their counts and points added together, so no
    /// earlier points are lost. Songs stay keyed by their id.
    fn rekey(&mut self) {
        use crate::radio::normalize_artist_key as key;
        fn merge<V>(map: &mut HashMap<String, V>, add: impl Fn(&mut V, V)) {
            if map.keys().all(|k| key(k) == *k) {
                return;
            }
            for (k, v) in std::mem::take(map) {
                let k = key(&k);
                match map.get_mut(&k) {
                    Some(have) => add(have, v),
                    None => {
                        map.insert(k, v);
                    }
                }
            }
        }
        merge(&mut self.artists, |a, b| a.iter_mut().zip(b).for_each(|(x, y)| *x += y));
        merge(&mut self.fixed, |_, _| {});
        merge(&mut self.bias, |a, b| *a += b);
        merge(&mut self.affinity, |a, b| *a += b);
        merge(&mut self.names, |_, _| {});
    }
}

/// A song's points, and when it last played (seconds since 1970).
#[derive(Default, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub points: f32,
    pub last: u64,
}

/// Points for how much of a song was heard before it ended or was left:
/// - skipped in its first 30 seconds (and before its half): [`SKIP`];
/// - left before 30% of it: up to [`EARLY`], the earlier the more;
/// - from 30% to 80%: from 0 up to [`FINISH`], little by little;
/// - 80% or more: [`FINISH`].
pub fn heard_points(listened: f32, length: f32) -> f32 {
    if listened < 30.0 && (length <= 0.0 || listened < length * 0.5) {
        return SKIP;
    }
    if length <= 0.0 {
        return 0.0;
    }
    let f = (listened / length).clamp(0.0, 1.0);
    if f < 0.3 {
        EARLY * (0.3 - f) / 0.3
    } else if f < 0.8 {
        FINISH * (f - 0.3) / 0.5
    } else {
        FINISH
    }
}

/// Left early (past the first 30 s, before 30% of the song), at most.
const EARLY: f32 = -6.0;
/// A song played.
const PLAY: f32 = 10.0;
/// Heard to the end (80% or more).
const FINISH: f32 = 6.0;
/// Skipped in its first 30 seconds (and before its half).
const SKIP: f32 = -15.0;
/// The share of a song's points its artist gets.
const TO_ARTIST: f32 = 0.3;
/// A song just played counts this much ...
const COOLDOWN: f32 = 0.35;
/// ... for this long.
const COOLDOWN_SECS: u64 = 6 * 60 * 60;
/// Points kept each day since the song last played.
const DECAY: f32 = 0.95;
/// Songs kept, at most (the weakest go first).
const TRACKS_KEPT: usize = 5000;

/// One change to the points file at a time (playing and the settings can come together).
static FILE: Mutex<()> = Mutex::new(());

/// One song gave way to another: the old one's listening time counts (`listened` of `length`
/// seconds), then the new one is played; `manual`: picked by hand. Done off the UI thread.
pub fn moved_on(left: Option<(Track, f32, f32)>, next: Track, manual: bool) {
    std::thread::spawn(move || {
        let _one = FILE.lock();
        let mut notes = Notes::load();
        let now = crate::updates::now();
        if let Some((t, listened, length)) = left.filter(|(t, ..)| !t.video_id.is_empty()) {
            notes.left(&t, listened, length);
        }
        notes.played(&next, manual, now);
        notes.save();
    });
}

/// What the settings show: songs with points, and the artists with the most.
pub struct Summary {
    pub tracked: usize,
    pub top_artists: Vec<String>,
}

pub fn summary() -> Summary {
    let _one = FILE.lock();
    Notes::load().summary()
}

/// "Reset all points": every song's and artist's points back to 0 (kinds and hidden songs stay).
pub fn reset_all() {
    let _one = FILE.lock();
    let mut notes = Notes::load();
    notes.reset_all();
    notes.save();
}

/// A week, for the weekly reset.
const WEEK: u64 = 7 * 24 * 60 * 60;

/// A reset date as written in the settings ("2026-12-31", spaces around are fine); `None` when it
/// isn't a real date.
pub fn reset_date(text: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok().map(|d| d.format("%Y-%m-%d").to_string())
}

/// Today's date where the user is, as the settings write dates.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Why the points reset by themselves now, if they do: the day the user picked has come, or a
/// week has passed since the last reset with the weekly reset on (it's off unless turned on).
#[derive(Debug, PartialEq, Eq)]
pub enum Due {
    Date,
    Week,
}

pub fn reset_due(date: &str, weekly: bool, last_reset: u64, now: u64, today: &str) -> Option<Due> {
    if !date.is_empty() && today >= date {
        return Some(Due::Date);
    }
    (weekly && now.saturating_sub(last_reset) >= WEEK).then_some(Due::Week)
}

/// "Reset negative points only": skipped songs and artists back to 0; favorites stay.
pub fn reset_negative() {
    let _one = FILE.lock();
    let mut notes = Notes::load();
    notes.reset_negative();
    notes.save();
}

/// "Recommend more" in a song's menu: its artist comes up more in Up next and For you.
pub fn recommend_more(t: &Track) {
    let mut notes = Notes::load();
    notes.hidden.remove(&t.video_id);
    let artist = first_artist(&t.artist);
    if !artist.is_empty() {
        let b = notes.bias.entry(artist).or_default();
        *b = (*b + 25).min(50);
    }
    notes.save();
}

/// "Don't recommend": the song is never suggested again, and its artist a little less.
pub fn dont_recommend(t: &Track) {
    let mut notes = Notes::load();
    notes.hidden.insert(t.video_id.clone());
    let artist = first_artist(&t.artist);
    if !artist.is_empty() {
        let b = notes.bias.entry(artist).or_default();
        *b = (*b - 20).max(-60);
    }
    notes.save();
}

/// "Only this kind" (Settings): For you shows only it. None: every kind heard.
pub fn only_kind() -> Option<Kind> {
    Kind::from_name(&crate::config::Config::load().only_kind)
}

/// The kind of a song by its artist and words, with what's learned (for the menus).
pub fn kind_of_track(t: &Track) -> Kind {
    Notes::load().kind_of(t).unwrap_or(Kind::Songs)
}

/// The user says the song's artist is of `kind`: kept, and it wins over words and radios.
pub fn correct(t: &Track, kind: Kind) {
    let mut notes = Notes::load();
    let artist = first_artist(&t.artist);
    if artist.is_empty() {
        return;
    }
    notes.fixed.insert(artist.clone(), kind);
    let mut seen = [0u32; 5];
    seen[kind as usize] = 12;
    notes.artists.insert(artist, seen);
    notes.save();
}

/// How much of each kind is in `tracks` (the songs played lately, for the stats page).
pub fn kind_counts(tracks: &[Track]) -> [usize; 5] {
    let notes = Notes::load();
    let mut out = [0usize; 5];
    for t in tracks {
        out[notes.kind_of(t).unwrap_or(Kind::Songs) as usize] += 1;
    }
    out
}

/// Artists kept, at most (the least seen go first).
const ARTISTS_KEPT: usize = 2000;

impl Notes {
    fn path() -> std::path::PathBuf {
        crate::data_dir().join("ram-points.json")
    }

    pub fn load() -> Notes {
        let mut notes: Notes = std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        notes.rekey();
        notes
    }

    pub fn save(&mut self) {
        self.names.retain(|a, _| self.affinity.contains_key(a));
        if self.tracks.len() > TRACKS_KEPT {
            let mut all: Vec<(String, f32)> = self.tracks.iter().map(|(id, s)| (id.clone(), s.points.abs())).collect();
            all.sort_by(|a, b| b.1.total_cmp(&a.1));
            let keep: HashSet<String> = all.into_iter().take(TRACKS_KEPT).map(|(id, _)| id).collect();
            self.tracks.retain(|id, _| keep.contains(id));
        }
        if self.artists.len() > ARTISTS_KEPT {
            let mut totals: Vec<(String, u32)> = self.artists.iter().map(|(a, c)| (a.clone(), c.iter().sum())).collect();
            totals.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let keep: HashSet<String> = totals.into_iter().take(ARTISTS_KEPT).map(|(a, _)| a).collect();
            self.artists.retain(|a, _| keep.contains(a));
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::create_dir_all(crate::data_dir());
            let _ = std::fs::write(Self::path(), json);
        }
    }

    /// `delta` points to the song, and 30% of it to its artist.
    fn give(&mut self, t: &Track, delta: f32) {
        if t.video_id.is_empty() || delta == 0.0 {
            return;
        }
        let s = self.tracks.entry(t.video_id.clone()).or_default();
        s.points = (s.points + delta).clamp(-100.0, 100.0);
        let artist = first_artist(&t.artist);
        if !artist.is_empty() {
            let written = t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string();
            self.names.entry(artist.clone()).or_insert(written);
            let a = self.affinity.entry(artist).or_default();
            *a = (*a + delta * TO_ARTIST).clamp(-60.0, 60.0);
        }
    }

    /// The song starts. Picked by hand, its minus points go first (back to 0).
    pub fn played(&mut self, t: &Track, manual: bool, now: u64) {
        if t.video_id.is_empty() {
            return;
        }
        if manual && let Some(below) = self.tracks.get(&t.video_id).map(|s| s.points).filter(|p| *p < 0.0) {
            self.give(t, -below);
        }
        self.give(t, PLAY);
        self.tracks.entry(t.video_id.clone()).or_default().last = now;
    }

    /// The song stopped after `listened` of its `length` seconds: a quick skip loses points,
    /// hearing it to the end gives some.
    pub fn left(&mut self, t: &Track, listened: f32, length: f32) {
        let points = heard_points(listened, length);
        if points != 0.0 {
            self.give(t, points);
        }
    }

    /// The song's points now: 5% less each day since it played, a third for 6 hours after.
    pub fn track_points(&self, id: &str, now: u64) -> f32 {
        let Some(s) = self.tracks.get(id) else { return 0.0 };
        let ago = now.saturating_sub(s.last);
        let mut p = s.points * DECAY.powf(ago as f32 / DAY as f32);
        if p > 0.0 && ago < COOLDOWN_SECS {
            p *= COOLDOWN;
        }
        p
    }

    /// The artist's points: from their songs, and "Recommend more" / "Don't recommend".
    pub fn artist_points(&self, artist: &str) -> f32 {
        self.affinity.get(artist).copied().unwrap_or(0.0)
    }

    /// What songs and artists add to a song's place in a list.
    fn liking(&self, t: &Track, artist: &str, now: u64) -> i32 {
        self.track_points(&t.video_id, now).round().clamp(-40.0, 40.0) as i32 + self.artist_points(artist).round().clamp(-30.0, 30.0) as i32
    }

    pub fn summary(&self) -> Summary {
        let mut artists: Vec<(&String, f32)> = self.affinity.iter().map(|(a, n)| (a, n + self.bias.get(a).copied().unwrap_or(0) as f32)).filter(|(_, n)| *n > 0.0).collect();
        artists.sort_by(|a, b| b.1.total_cmp(&a.1));
        Summary { tracked: self.tracks.len(), top_artists: artists.into_iter().take(5).map(|(a, _)| self.names.get(a).unwrap_or(a).clone()).collect() }
    }

    pub fn reset_all(&mut self) {
        self.tracks.clear();
        self.affinity.clear();
        self.bias.clear();
        self.names.clear();
    }

    pub fn reset_negative(&mut self) {
        self.tracks.retain(|_, s| s.points >= 0.0);
        self.affinity.retain(|_, n| *n >= 0.0);
        self.bias.retain(|_, n| *n >= 0);
    }

    fn add(&mut self, artist: &str, kind: Kind, weight: u32) {
        if artist.is_empty() {
            return;
        }
        let seen = self.artists.entry(artist.to_string()).or_default();
        seen[kind as usize] += weight;
        // Halved now and then, so what's seen lately counts most.
        if seen.iter().sum::<u32>() > 120 {
            seen.iter_mut().for_each(|n| *n /= 2);
        }
    }

    /// The artist's kind, once it's clear (seen enough, and mostly with one kind).
    fn artist_kind(&self, artist: &str) -> Option<Kind> {
        let seen = self.artists.get(artist)?;
        let total: u32 = seen.iter().sum();
        let (best, n) = seen.iter().enumerate().max_by_key(|(_, n)| **n)?;
        (total >= 3 && n * 10 >= total * 7).then_some(Kind::ALL[best])
    }

    /// The song's kind: the user's word for its artist, else its words, else its artist's;
    /// `None` when nothing tells.
    pub fn kind_of(&self, t: &Track) -> Option<Kind> {
        let artist = first_artist(&t.artist);
        self.fixed.get(&artist).copied().or_else(|| word_kind(t)).or_else(|| self.artist_kind(&artist))
    }

    /// The kind of a radio's song: its own, else the kind known for most of its radio (a few
    /// that aren't songs are enough: a latmiya's radio is mostly latmiyat), else songs.
    pub fn seed_kind(&self, seed: &Track, radio: &[Track]) -> Kind {
        if let Some(k) = self.kind_of(seed) {
            return k;
        }
        let mut count = [0usize; 5];
        for t in radio {
            if let Some(k) = self.kind_of(t) {
                count[k as usize] += 1;
            }
        }
        let (best, n) = (1..5).map(|i| (i, count[i])).max_by_key(|(_, n)| *n).unwrap_or((0, 0));
        if n >= 2 && n * 4 >= radio.len() { Kind::ALL[best] } else { Kind::Songs }
    }

    /// What a radio teaches: an artist with telling words gets their kind (counts three), the
    /// others the radio's kind (counts one) when it doesn't go against what's known of them
    /// (and, for a radio that isn't songs, when they're in the seed's writing).
    pub fn learn(&mut self, kind: Kind, seed: &Track, radio: &[Track]) {
        let seed_script = script(seed);
        for t in std::iter::once(seed).chain(radio) {
            let artist = first_artist(&t.artist);
            if self.fixed.contains_key(&artist) {
                continue;
            }
            match word_kind(t) {
                Some(k) => self.add(&artist, k, 3),
                None if self.artist_kind(&artist).is_none_or(|k| k == kind) && (kind == Kind::Songs || script(t) == seed_script) => {
                    self.add(&artist, kind, 1)
                }
                None => {}
            }
        }
    }
}

/// xorshift: enough to stir equal points a little.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> i32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as i32
    }
}

struct Scored {
    t: Track,
    points: i32,
    artist: String,
    liked: bool,
}

/// The best first, `n` at most: the same artist as the last song loses 20 points (8 for the
/// one before), a liked song only after `liked_every` others, `per_artist` songs per artist.
fn order(mut cands: Vec<Scored>, n: usize, liked_every: usize, per_artist: Option<usize>) -> Vec<Track> {
    let mut out = Vec::new();
    let mut last: [String; 2] = Default::default();
    let mut since_liked = 0usize;
    let mut per: HashMap<String, usize> = HashMap::new();
    while out.len() < n {
        let best = cands
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.liked || since_liked >= liked_every)
            .filter(|(_, s)| s.artist.is_empty() || per_artist.is_none_or(|m| per.get(&s.artist).copied().unwrap_or(0) < m))
            .min_by_key(|(_, s)| {
                let repeat = if s.artist.is_empty() {
                    0
                } else if s.artist == last[0] {
                    20
                } else if s.artist == last[1] {
                    8
                } else {
                    0
                };
                -(s.points - repeat)
            })
            .map(|(i, _)| i);
        let Some(i) = best else { break };
        let s = cands.remove(i);
        since_liked = if s.liked { 0 } else { since_liked + 1 };
        *per.entry(s.artist.clone()).or_default() += 1;
        last = [s.artist.clone(), std::mem::take(&mut last[0])];
        out.push(s.t);
    }
    out
}

/// What was played lately: when each song last played (seconds since 1970), and now.
pub struct Lately {
    pub played_at: HashMap<String, u64>,
    pub now: u64,
}

impl Lately {
    /// From the songs played on this computer.
    pub fn load() -> Lately {
        Lately { played_at: crate::recent::load().into_iter().map(|p| (p.track.video_id, p.last)).collect(), now: crate::updates::now() }
    }

    fn played_ago(&self, id: &str) -> Option<u64> {
        self.played_at.get(id).map(|at| self.now.saturating_sub(*at))
    }
}

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;

/// Up next for the playing song (`seed`) from its radio: only its kind, songs by its artist and
/// ones like it first, liked songs (same artist or an artist of the radio) one in four at
/// most, nothing in `skip` (played or queued), no song twice. `heard`: first artists heard this
/// session. Learns from the radio.
pub fn up_next(radio: Vec<Track>, seed: &Track, liked: &[Track], skip: &HashSet<String>, heard: &HashSet<String>, random: u64) -> Vec<Track> {
    let mut notes = Notes::load();
    let out = rank_up_next(&mut notes, radio, seed, liked, skip, heard, &Lately::load(), random);
    notes.save();
    out
}

#[allow(clippy::too_many_arguments)]
fn rank_up_next(notes: &mut Notes, radio: Vec<Track>, seed: &Track, liked: &[Track], skip: &HashSet<String>, heard: &HashSet<String>, lately: &Lately, random: u64) -> Vec<Track> {
    let kind = notes.seed_kind(seed, &radio);
    notes.learn(kind, seed, &radio);
    let seed_artist = first_artist(&seed.artist);
    let seed_script = script(seed);
    let liked_artists: HashSet<String> = liked.iter().map(|t| first_artist(&t.artist)).collect();
    let radio_artists: HashSet<String> = radio.iter().map(|t| first_artist(&t.artist)).collect();
    let mut rng = Rng(random | 1);
    let mut ids: HashSet<String> = HashSet::from([seed.video_id.clone()]);
    let mut keys: HashSet<String> = HashSet::from([same_song_key(seed)]);
    let len = radio.len().max(1) as i32;
    let liked_ones = liked.iter().filter(|t| {
        let a = first_artist(&t.artist);
        !a.is_empty() && (a == seed_artist || radio_artists.contains(&a))
    });
    let from_radio = radio.into_iter().enumerate().map(|(i, t)| (t, 30 - 30 * i as i32 / len, false));
    let mut cands = Vec::new();
    for (t, base, is_liked) in from_radio.chain(liked_ones.cloned().map(|t| (t, 0, true))) {
        if t.link.is_some() || t.video_id.is_empty() || skip.contains(&t.video_id) || notes.hidden.contains(&t.video_id) {
            continue;
        }
        // Only the seed's kind. A song nothing tells about may come with a radio (a little
        // lower when the seed isn't a song); a liked one must be known to fit.
        let fit = match notes.kind_of(&t) {
            Some(k) if k == kind => 0,
            Some(_) => continue,
            None if kind == Kind::Songs => 0,
            None if is_liked || script(&t) != seed_script => continue,
            None => -12,
        };
        if !ids.insert(t.video_id.clone()) || !keys.insert(same_song_key(&t)) {
            continue;
        }
        let artist = first_artist(&t.artist);
        let mut points = base + fit + notes.bias.get(&artist).copied().unwrap_or(0) + notes.liking(&t, &artist, lately.now);
        if !artist.is_empty() && artist == seed_artist {
            points += 35;
        } else if heard.contains(&artist) {
            points += 10;
        }
        if liked_artists.contains(&artist) {
            points += 6;
        }
        let s = script(&t);
        if s == seed_script {
            points += 12;
        } else if s != Script::Other && seed_script != Script::Other {
            points -= 25;
        }
        match lately.played_ago(&t.video_id) {
            Some(ago) if ago < 12 * HOUR => points -= 50,
            Some(ago) if ago < 3 * DAY => points -= 30,
            _ => {}
        }
        if is_liked {
            points -= 4;
        }
        points += rng.below(7);
        cands.push(Scored { t, points, artist, liked: is_liked });
    }
    order(cands, usize::MAX, 3, None)
}

/// A song that may go on the home page: the points its source gives, the kind of the radio it
/// came in (`None`: not from a radio), and whether it's a liked one.
pub struct Candidate {
    pub track: Track,
    pub base: i32,
    pub radio_kind: Option<Kind>,
    pub liked: bool,
}

impl Candidate {
    /// The songs of a seed's radio, YouTube's first ones worth the most (`bonus` on top).
    pub fn from_radio(radio: &[Track], kind: Kind, bonus: i32) -> Vec<Candidate> {
        let len = radio.len().max(1) as i32;
        radio.iter().enumerate().map(|(i, t)| Candidate { track: t.clone(), base: bonus + 20 - 20 * i as i32 / len, radio_kind: Some(kind), liked: false }).collect()
    }
}

/// The kinds the home page shows, from how much each was heard lately (`(kind, weight)`): the
/// main one, and a second one when it's at least a quarter. Songs when nothing was heard.
pub fn page_kinds(heard: &[(Kind, f32)]) -> Vec<Kind> {
    let mut sum = [0f32; 5];
    for (k, w) in heard {
        sum[*k as usize] += w;
    }
    let total: f32 = sum.iter().sum();
    if total <= 0.0 {
        return vec![Kind::Songs];
    }
    let mut kinds: Vec<usize> = (0..5).filter(|i| sum[*i] > 0.0).collect();
    kinds.sort_by(|a, b| sum[*b].total_cmp(&sum[*a]));
    kinds.into_iter().enumerate().filter(|(place, i)| *place == 0 || (*place == 1 && sum[*i] >= total * 0.25)).map(|(_, i)| Kind::ALL[i]).collect()
}

/// What the home page knows of each song: the songs the user knows (liked or played), the
/// artists they play most, and when and how often a song was shown on it.
pub struct Feed<'a> {
    pub known: &'a HashSet<String>,
    pub top_artists: &'a HashSet<String>,
    pub shown: &'a dyn Fn(&str) -> Option<(u32, u64)>,
    pub lately: &'a Lately,
}

/// `n` songs of one `kind` for the home page, by points: new songs first, nothing played or
/// shown lately (shown in the last day: only if nothing else is left), at most three by one
/// artist, a liked song after five others at most; nothing in `skip`.
pub fn for_you(notes: &Notes, cands: &[Candidate], kind: Kind, n: usize, skip: &HashSet<String>, feed: &Feed, random: u64) -> Vec<Track> {
    let mut rng = Rng(random | 1);
    let mut ids: HashSet<String> = HashSet::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut scored = Vec::new();
    for c in cands {
        let t = &c.track;
        if t.link.is_some() || t.video_id.is_empty() || skip.contains(&t.video_id) || notes.hidden.contains(&t.video_id) {
            continue;
        }
        let k = notes.kind_of(t).or(c.radio_kind).unwrap_or(Kind::Songs);
        if k != kind || !ids.insert(t.video_id.clone()) || !keys.insert(same_song_key(t)) {
            continue;
        }
        let artist = first_artist(&t.artist);
        let mut points = c.base + notes.bias.get(&artist).copied().unwrap_or(0) + notes.liking(t, &artist, feed.lately.now);
        if !feed.known.contains(&t.video_id) {
            points += 18;
        }
        match feed.lately.played_ago(&t.video_id) {
            Some(ago) if ago < 3 * DAY => points -= 40,
            Some(ago) if ago < 14 * DAY => points -= 10,
            _ => {}
        }
        if let Some((times, last)) = (feed.shown)(&t.video_id) {
            let ago = feed.lately.now.saturating_sub(last);
            points -= if ago < DAY {
                60
            } else if ago < 3 * DAY {
                25
            } else {
                0
            };
            if times >= 3 {
                points -= 10;
            }
        }
        if feed.top_artists.contains(&artist) {
            points += 6;
        }
        points += rng.below(9);
        scored.push(Scored { t: t.clone(), points, artist, liked: c.liked });
    }
    order(scored, n, 5, Some(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str, artist: &str) -> Track {
        Track {
            video_id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: String::new(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    fn quiet() -> Lately {
        Lately { played_at: HashMap::new(), now: 100 * DAY }
    }

    #[test]
    fn words_tell_the_kind() {
        assert_eq!(word_kind(&t("a", "لطمية يا حسين", "باسم الكربلائي")), Some(Kind::Husseini));
        assert_eq!(word_kind(&t("a", "قصيدة جديدة | محرم ١٤٤٧", "مسلم الوائلي")), Some(Kind::Husseini));
        assert_eq!(word_kind(&t("a", "وين", "الرادود علي")), Some(Kind::Husseini));
        assert_eq!(word_kind(&t("a", "سورة الكهف", "مشاري العفاسي")), Some(Kind::Quran));
        assert_eq!(word_kind(&t("a", "Ya Hussain (Latmiya)", "Ali")), Some(Kind::Husseini));
        // A singer named Hussein, a child (الطفل), a love song: songs.
        assert_eq!(word_kind(&t("a", "بشرة خير", "حسين الجسمي")), None);
        assert_eq!(word_kind(&t("a", "الطفل", "Someone")), None);
        assert_eq!(word_kind(&t("a", "The Less I Know The Better", "Tame Impala")), None);
        assert_eq!(script(&t("a", "夜に駆ける", "YOASOBI")), Script::Japanese);
        assert_eq!(script(&t("a", "بشرة خير", "حسين الجسمي")), Script::Arabic);
    }

    #[test]
    fn a_husseini_song_brings_only_husseini() {
        let mut notes = Notes::default();
        let seed = t("s", "لطمية يا حسين", "باسم الكربلائي");
        let radio = vec![
            t("a", "نعي الزهراء", "مسلم الوائلي"),
            t("pop", "بشرة خير", "حسين الجسمي"),
            t("b", "ونين", "باسم الكربلائي"),
            t("en", "Blinding Lights", "The Weeknd"),
            t("c", "قصيدة محرم", "محمد الحلفي"),
        ];
        // The pop singer is known from songs' radios.
        for _ in 0..3 {
            notes.learn(Kind::Songs, &t("x", "x", "حسين الجسمي"), &[]);
        }
        let liked = vec![t("l1", "Song", "باسم الكربلائي"), t("l2", "عزاء", "باسم الكربلائي")];
        let out = rank_up_next(&mut notes, radio, &seed, &liked, &HashSet::new(), &HashSet::new(), &quiet(), 7);
        let ids: Vec<&str> = out.iter().map(|t| t.video_id.as_str()).collect();
        assert!(!ids.contains(&"pop") && !ids.contains(&"en"), "{ids:?}");
        // The same artist first; one liked song by the radood joins after three others (the one
        // with no telling words fits because the artist is now known as Husseini).
        assert_eq!(ids[0], "b", "{ids:?}");
        assert!(ids.contains(&"a") && ids.contains(&"c"), "{ids:?}");
        assert_eq!(ids.iter().filter(|i| i.starts_with('l')).count(), 1, "{ids:?}");
        // What it learned: the radood is Husseini.
        assert_eq!(notes.artist_kind("باسم الكربلائي"), Some(Kind::Husseini));
    }

    #[test]
    fn a_song_brings_only_songs_same_artist_up_and_no_repeats() {
        let mut notes = Notes::default();
        let seed = t("s", "Let It Happen", "Tame Impala");
        let radio = vec![
            t("a", "Alpha", "Other"),
            t("lat", "لطمية يا حسين", "باسم الكربلائي"),
            t("b", "Borderline", "Tame Impala"),
            t("b2", "Borderline (Official Video)", "Tame Impala"),
            t("c", "Gamma", "Someone"),
            t("d", "Feels Like We Only Go Backwards", "Tame Impala"),
            t("p", "Played", "X"),
        ];
        let skip: HashSet<String> = ["p".to_string()].into();
        let out = rank_up_next(&mut notes, radio, &seed, &[], &skip, &HashSet::new(), &quiet(), 7);
        let ids: Vec<&str> = out.iter().map(|t| t.video_id.as_str()).collect();
        assert!(!ids.contains(&"lat") && !ids.contains(&"p") && !ids.contains(&"b2"), "{ids:?}");
        // Same artist first, never twice in a row while others are left.
        assert_eq!(ids[0], "b", "{ids:?}");
        assert_ne!(ids[1], "d", "{ids:?}");
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn recommend_more_and_dont_recommend() {
        let mut notes = Notes::default();
        notes.bias.insert("b".into(), 50);
        notes.hidden.insert("gone".into());
        let seed = t("s", "Seed", "Z");
        let radio = vec![t("a", "A", "A"), t("gone", "Gone", "C"), t("bb", "B", "B")];
        let out = rank_up_next(&mut notes, radio, &seed, &[], &HashSet::new(), &HashSet::new(), &quiet(), 7);
        let ids: Vec<&str> = out.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, ["bb", "a"]);
    }

    #[test]
    fn song_points_go_to_the_artist_and_bring_their_new_songs_up() {
        let mut notes = Notes::default();
        let fav = t("f1", "Old favorite", "Fav");
        for _ in 0..5 {
            notes.played(&fav, false, 0);
            notes.left(&fav, 200.0, 210.0);
        }
        // 5 × (10 + 6) = 80 points to the song, 30% of them to its artist.
        assert_eq!(notes.tracks["f1"].points, 80.0);
        assert!((notes.artist_points("fav") - 24.0).abs() < 0.01);
        let seed = t("s", "Seed", "Z");
        let radio = vec![t("a", "A", "Other"), t("f2", "Not heard yet", "Fav")];
        let mut lately = quiet();
        lately.now = 10 * DAY;
        let out = rank_up_next(&mut notes, radio, &seed, &[], &HashSet::new(), &HashSet::new(), &lately, 7);
        assert_eq!(out[0].video_id, "f2");
    }

    #[test]
    fn a_song_just_played_cools_down_and_points_fade_by_day() {
        let mut notes = Notes::default();
        let song = t("x", "X", "A");
        notes.played(&song, false, 1000);
        assert!((notes.track_points("x", 1000 + HOUR) - 3.5).abs() < 0.01);
        assert!((notes.track_points("x", 1000 + DAY) - 9.5).abs() < 0.01);
        assert!((notes.track_points("x", 1000 + 10 * DAY) - 10.0 * 0.95f32.powi(10)).abs() < 0.01);
    }

    #[test]
    fn a_quick_skip_takes_points_and_a_pick_by_hand_clears_them() {
        let mut notes = Notes::default();
        let song = t("x", "X", "A");
        notes.played(&song, false, 0);
        notes.left(&song, 5.0, 200.0);
        notes.left(&song, 5.0, 200.0);
        assert_eq!(notes.tracks["x"].points, -20.0);
        notes.played(&song, false, 10);
        assert_eq!(notes.tracks["x"].points, -10.0);
        notes.played(&song, true, 20);
        assert_eq!(notes.tracks["x"].points, 10.0);
        assert!((notes.artist_points("a") - 3.0).abs() < 0.01);
    }

    #[test]
    fn resets_wipe_everything_or_only_the_minus_points() {
        let mut notes = Notes::default();
        let good = t("g", "G", "Good");
        let bad = t("b", "B", "Bad");
        notes.played(&good, false, 0);
        notes.left(&bad, 1.0, 100.0);
        notes.bias.insert("bad".into(), -20);
        notes.bias.insert("good".into(), 25);
        assert_eq!(notes.summary().top_artists, ["Good"]);
        notes.reset_negative();
        assert!(notes.tracks.contains_key("g") && !notes.tracks.contains_key("b"));
        assert_eq!(notes.artist_points("bad"), 0.0);
        assert_eq!(notes.bias.get("bad"), None);
        assert_eq!(notes.bias["good"], 25);
        notes.reset_all();
        assert_eq!(notes.summary().tracked, 0);
        assert!(notes.bias.is_empty() && notes.affinity.is_empty());
    }

    #[test]
    fn resets_come_on_the_picked_day_or_every_week_only_when_asked() {
        let day = 24 * 60 * 60;
        // No date, weekly off: never by itself.
        assert_eq!(reset_due("", false, 0, 365 * day, "2027-01-01"), None);
        // The picked day: not before, on it and after.
        assert_eq!(reset_due("2026-12-31", false, 0, 0, "2026-12-30"), None);
        assert_eq!(reset_due("2026-12-31", false, 0, 0, "2026-12-31"), Some(Due::Date));
        assert_eq!(reset_due("2026-12-31", false, 0, 0, "2027-01-02"), Some(Due::Date));
        // Weekly: seven days after the last reset.
        assert_eq!(reset_due("", true, 10 * day, 16 * day, "2027-01-01"), None);
        assert_eq!(reset_due("", true, 10 * day, 17 * day, "2027-01-01"), Some(Due::Week));
        // Dates as written in the settings.
        assert_eq!(reset_date(" 2026-12-31 ").as_deref(), Some("2026-12-31"));
        assert_eq!(reset_date("2026-2-3").as_deref(), Some("2026-02-03"));
        assert_eq!(reset_date("2026-02-30"), None);
        assert_eq!(reset_date("tomorrow"), None);
    }

    #[test]
    fn songs_played_lately_go_down() {
        let mut notes = Notes::default();
        let seed = t("s", "Seed", "Band");
        let radio = vec![t("old", "Old", "A"), t("new", "New", "B")];
        let mut lately = quiet();
        lately.played_at.insert("old".into(), lately.now - HOUR);
        let out = rank_up_next(&mut notes, radio, &seed, &[], &HashSet::new(), &HashSet::new(), &lately, 7);
        assert_eq!(out[0].video_id, "new");
    }

    #[test]
    fn for_you_keeps_one_kind_new_songs_first_and_rests_shown_ones() {
        let notes = Notes::default();
        let husseini = vec![t("h1", "لطمية", "رادود"), t("h2", "نعي", "رادود2")];
        let songs: Vec<Track> = (0..8).map(|i| t(&format!("s{i}"), &format!("Song {i}"), &format!("Artist {i}"))).collect();
        let mut cands = Candidate::from_radio(&songs, Kind::Songs, 10);
        cands.extend(Candidate::from_radio(&husseini, Kind::Husseini, 10));
        let known: HashSet<String> = ["s0".to_string()].into();
        let lately = quiet();
        let now = lately.now;
        let shown = move |id: &str| (id == "s1").then_some((1u32, now - HOUR));
        let feed = Feed { known: &known, top_artists: &HashSet::new(), shown: &shown, lately: &lately };
        let got = for_you(&notes, &cands, Kind::Songs, 8, &HashSet::new(), &feed, 3);
        let ids: Vec<&str> = got.iter().map(|t| t.video_id.as_str()).collect();
        assert!(ids.iter().all(|id| id.starts_with('s')), "{ids:?}");
        // Shown an hour ago: last; known: after the new ones.
        assert_eq!(*ids.last().unwrap(), "s1", "{ids:?}");
        assert!(ids.iter().position(|i| *i == "s0").unwrap() >= 2, "{ids:?}");
        let h = for_you(&notes, &cands, Kind::Husseini, 8, &HashSet::new(), &feed, 3);
        assert_eq!(h.len(), 2);
    }

    /// Live: Up next for a latmiya and for a song, with what RaM points makes of each
    /// (`cargo test --lib live_up_next_kinds -- --ignored --nocapture`).
    #[tokio::test]
    #[ignore]
    async fn live_up_next_kinds() {
        let api = crate::api::Api::guest().await.unwrap();
        for query in ["لطمية باسم الكربلائي", "Tame Impala Let It Happen"] {
            let seed = api.clone().search(query.into()).await.unwrap().into_iter().next().unwrap();
            let radio = crate::radio::with_artist(api.clone(), seed.clone()).await.unwrap();
            let mut notes = Notes::load();
            let kind = notes.seed_kind(&seed, &radio);
            let out = rank_up_next(&mut notes, radio.clone(), &seed, &[], &HashSet::new(), &HashSet::new(), &Lately::load(), 7);
            println!("== {} - {} -> {kind:?}: radio {}, up next {}", seed.title, seed.artist, radio.len(), out.len());
            for t in out.iter().take(12) {
                println!("   {:?} | {} - {}", notes.kind_of(t), t.title, t.artist);
            }
            let left: Vec<String> = radio.iter().filter(|r| !out.iter().any(|o| o.video_id == r.video_id)).map(|t| format!("{} - {}", t.title, t.artist)).collect();
            println!("   left out: {left:?}");
        }
    }

    #[test]
    fn page_kinds_follow_what_was_heard() {
        assert_eq!(page_kinds(&[]), vec![Kind::Songs]);
        assert_eq!(page_kinds(&[(Kind::Husseini, 2.0), (Kind::Songs, 0.3)]), vec![Kind::Husseini]);
        assert_eq!(page_kinds(&[(Kind::Husseini, 2.0), (Kind::Songs, 1.0)]), vec![Kind::Husseini, Kind::Songs]);
    }

    #[test]
    fn liked_songs_come_now_and_then_at_most_three_per_artist() {
        let mk = |id: &str, a: &str, liked: bool, points: i32| Scored { t: t(id, id, a), points, artist: a.into(), liked };
        let cands = vec![mk("l1", "x", true, 100), mk("l2", "y", true, 99), mk("a", "a", false, 10), mk("b", "b", false, 9), mk("c", "c", false, 8), mk("d", "d", false, 7)];
        let ids: Vec<String> = order(cands, 10, 3, None).into_iter().map(|t| t.video_id).collect();
        assert_eq!(ids, ["a", "b", "c", "l1", "d"]);
        let same: Vec<Scored> = (0..6).map(|i| mk(&format!("s{i}"), "one", false, 10)).collect();
        assert_eq!(order(same, 10, 3, Some(3)).len(), 3);
    }

    #[test]
    fn artist_keys_merge_without_losing_points() {
        use crate::radio::normalize_artist_key as key;
        assert_eq!(key("Adele"), "adele");
        assert_eq!(key("AdeleVEVO"), "adele");
        assert_eq!(key("Adele - Topic"), "adele");
        assert_eq!(key("  Tame   Impala Official "), "tame impala");
        assert_eq!(key("VEVO"), "vevo", "a name that is only the ending stays");
        assert_eq!(crate::radio::first_artist("Bruno Mars, Lady Gaga"), "bruno mars");
        let mut notes = Notes::default();
        notes.affinity.insert("adelevevo".into(), 4.0);
        notes.affinity.insert("adele".into(), 2.0);
        notes.bias.insert("adele - topic".into(), 3);
        notes.artists.insert("adelevevo".into(), [1, 0, 0, 0, 0]);
        notes.artists.insert("adele".into(), [2, 0, 0, 0, 1]);
        notes.rekey();
        assert_eq!(notes.affinity.get("adele"), Some(&6.0));
        assert_eq!(notes.affinity.len(), 1);
        assert_eq!(notes.bias.get("adele"), Some(&3));
        assert_eq!(notes.artists.get("adele"), Some(&[3, 0, 0, 0, 1]));
    }


    #[test]
    fn points_follow_how_much_was_heard() {
        use super::heard_points as p;
        assert_eq!(p(10.0, 240.0), super::SKIP, "a skip in the first seconds");
        assert!(p(36.0, 240.0) < 0.0 && p(36.0, 240.0) > super::EARLY, "left at 15%: a little minus");
        assert!((p(72.0, 240.0) - 0.0).abs() < 1e-4, "left at 30%: nothing");
        assert!((p(132.0, 240.0) - 3.0).abs() < 1e-3, "left at 55%: halfway to finishing");
        assert_eq!(p(200.0, 240.0), super::FINISH, "80% and more: finished");
        assert_eq!(p(240.0, 240.0), super::FINISH);
        assert_eq!(p(45.0, 0.0), 0.0, "length unknown, past the skip: nothing");
        // More heard is never worth less.
        let mut last = f32::MIN;
        for s in (30..=240).step_by(5) {
            let now = p(s as f32, 240.0);
            assert!(now >= last, "at {s} s");
            last = now;
        }
    }

}
