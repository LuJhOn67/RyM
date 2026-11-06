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

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

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
    const ALL: [Kind; 5] = [Kind::Songs, Kind::Husseini, Kind::Quran, Kind::Nasheed, Kind::Talk];

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
fn telling_words() -> &'static [(Kind, HashSet<String>, Vec<String>)] {
    static WORDS: OnceLock<Vec<(Kind, HashSet<String>, Vec<String>)>> = OnceLock::new();
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
}

/// Artists kept, at most (the least seen go first).
const ARTISTS_KEPT: usize = 2000;

impl Notes {
    fn path() -> std::path::PathBuf {
        crate::data_dir().join("ram-points.json")
    }

    pub fn load() -> Notes {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&mut self) {
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

    /// The song's kind: its words, else its artist's; `None` when nothing tells.
    pub fn kind_of(&self, t: &Track) -> Option<Kind> {
        word_kind(t).or_else(|| self.artist_kind(&first_artist(&t.artist)))
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
        if t.link.is_some() || t.video_id.is_empty() || skip.contains(&t.video_id) {
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
        let mut points = base + fit;
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
        if t.link.is_some() || t.video_id.is_empty() || skip.contains(&t.video_id) {
            continue;
        }
        let k = notes.kind_of(t).or(c.radio_kind).unwrap_or(Kind::Songs);
        if k != kind || !ids.insert(t.video_id.clone()) || !keys.insert(same_song_key(t)) {
            continue;
        }
        let artist = first_artist(&t.artist);
        let mut points = c.base;
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
}
