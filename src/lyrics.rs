//! Lyrics: synced from LRCLIB, NetEase or KuGou (see `lyric_sources`); plain ones from LRCLIB,
//! YouTube Music or Genius as the last resort.
//!
//! Fast on purpose: all LRCLIB queries and YouTube Music are asked at once (the best answer wins
//! as soon as the better ones are in), answers are kept on disk so a song heard before shows its
//! lyrics at once, and the apps fetch the next song's lyrics while the current one plays.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::{Api, Track};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lyrics {
    /// (start seconds, line). Empty when only plain lyrics exist.
    pub synced: Vec<(f32, String)>,
    pub plain: Vec<String>,
    pub source: String,
    /// English under each line (Japanese songs), the same count as the lines shown; empty: none.
    #[serde(default)]
    pub translation: Vec<String>,
    /// Each synced line's words with when they're sung (the same order as `synced`; a line's list
    /// is empty when the source gave no word timing). Empty: no word timing at all.
    #[serde(default)]
    pub words: Vec<Words>,
}

/// One line's words: (start seconds, the word with the space after it).
pub type Words = Vec<(f32, String)>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrcLibEntry {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    duration: Option<f32>,
}

impl LrcLibEntry {
    fn into_lyrics(self) -> Lyrics {
        let (synced, words) = self.synced_lyrics.as_deref().map(parse_lrc_words).unwrap_or_default();
        let plain = self.plain_lyrics.unwrap_or_default().lines().map(String::from).collect();
        Lyrics { synced, plain, source: "LRCLIB".into(), translation: Vec::new(), words }
    }
}

impl Lyrics {
    /// LRCLIB is user-submitted; reject vandalized or stub entries.
    fn is_usable(&self) -> bool {
        self.synced.len() >= 4 || self.plain.len() >= 4
    }
}

const UA: &str = concat!("RymPlayer/", env!("CARGO_PKG_VERSION"));

/// GET with one retry: LRCLIB answers 503 when busy. `None` when there's no usable answer.
async fn get_json<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: reqwest::Url) -> Result<Option<T>> {
    for attempt in 0..2 {
        let resp = client.get(url.clone()).send().await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(Some(resp.json().await?));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if attempt == 0 {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }
    Ok(None)
}

/// "Song (feat. X) [Live]" -> "Song"
fn clean_title(title: &str) -> String {
    let mut t = title.to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let Some(a) = t.find(open) {
            let Some(b) = t[a..].find(close) else { break };
            t.replace_range(a..a + b + 1, "");
        }
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "A, B & C featuring D" -> "A"
fn first_artist(artist: &str) -> &str {
    let lower = artist.to_lowercase();
    let cut = [",", " & ", " featuring ", " feat.", " ft.", " x ", " and "]
        .iter()
        .filter_map(|sep| lower.find(sep))
        .min()
        .unwrap_or(artist.len());
    artist[..cut].trim()
}

/// One client for every lookup: its connection to LRCLIB stays open between songs.
fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().user_agent(UA).timeout(Duration::from_secs(10)).build().expect("http client"))
}

/// A song's lyrics: from the disk if found before, else the first synced lyrics in the order
/// LRCLIB, NetEase, KuGou (all asked at once; a later one is waited for only while the earlier
/// ones had nothing synced), then plain ones: LRCLIB's, YouTube Music's, Genius's. `None` when
/// none has any.
pub async fn for_track(api: Option<Api>, t: Track) -> Option<Lyrics> {
    if let Some(saved) = cached(&t.video_id) {
        return saved;
    }
    let duration = crate::parse_duration(&t.duration);
    let ytm = {
        let (api, vid) = (api.clone(), t.video_id.clone());
        tokio::spawn(async move {
            match api {
                Some(api) if crate::is_youtube(&vid) => api.lyrics(vid).await.ok().flatten(),
                _ => None,
            }
        })
    };
    let netease = tokio::spawn(crate::lyric_sources::netease(t.clone()));
    let kugou = tokio::spawn(crate::lyric_sources::kugou(t.clone()));
    let lrc = lrclib(t.title.clone(), t.artist.clone(), t.album.clone(), duration).await;
    let mut plain: Option<Lyrics> = None;
    let mut failed = lrc.is_err();
    match lrc {
        Ok(Some(l)) if !l.synced.is_empty() => {
            netease.abort();
            kugou.abort();
            ytm.abort();
            let found = Some(l);
            keep(&t.video_id, &found);
            return found;
        }
        Ok(Some(l)) => plain = Some(l),
        _ => {}
    }
    let found = 'found: {
        for answer in [netease.await, kugou.await] {
            match answer {
                Ok(Ok(Some(l))) if !l.synced.is_empty() => {
                    ytm.abort();
                    break 'found Some(l);
                }
                Ok(Ok(Some(l))) => plain = plain.or(Some(l)),
                Ok(Err(_)) => failed = true,
                _ => {}
            }
        }
        // Nothing synced anywhere: the plain ones, Genius last.
        if let Some(l) = plain.or(ytm.await.ok().flatten()) {
            break 'found Some(l);
        }
        match crate::lyric_sources::genius(t.clone()).await {
            Ok(l) => l,
            Err(_) => {
                failed = true;
                None
            }
        }
    };
    // A provider that couldn't be reached may have them later: "no lyrics" isn't remembered then.
    if found.is_some() || !failed {
        keep(&t.video_id, &found);
    }
    found
}

/// Fetches a song's lyrics into the disk cache (the next song, while this one plays).
pub async fn prefetch(api: Option<Api>, t: Track) {
    if cached(&t.video_id).is_none() {
        let _ = for_track(api, t).await;
    }
}

fn cache_path(video_id: &str) -> PathBuf {
    // Songs on this device have a path for an id: a hash of it makes the file name.
    let name = if video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        video_id.to_string()
    } else {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        video_id.hash(&mut h);
        format!("local-{:016x}", h.finish())
    };
    crate::data_dir().join("lyrics").join(format!("{name}.json"))
}

/// `Some(Some)`: saved lyrics; `Some(None)`: looked up lately and there were none; `None`: not known.
pub fn cached(video_id: &str) -> Option<Option<Lyrics>> {
    let path = cache_path(video_id);
    let text = std::fs::read_to_string(&path).ok()?;
    let saved: Option<Lyrics> = serde_json::from_str(&text).ok()?;
    if saved.is_none() {
        // "None" is asked again after a day: lyrics get added to LRCLIB all the time.
        let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?.elapsed().ok()?;
        if age > Duration::from_secs(24 * 60 * 60) {
            return None;
        }
    }
    Some(saved)
}

fn keep(video_id: &str, found: &Option<Lyrics>) {
    let path = cache_path(video_id);
    if let (Some(dir), Ok(json)) = (path.parent(), serde_json::to_string(found)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(path, json);
    }
}

/// LRCLIB: the exact match and three looser searches, all asked at once. Answers are taken in
/// that order of preference, and the first synced one wins as soon as the better queries are in.
pub async fn lrclib(title: String, artist: String, album: String, duration: Option<f32>) -> Result<Option<Lyrics>> {
    let client = client();
    let mut url = reqwest::Url::parse("https://lrclib.net/api/get")?;
    url.query_pairs_mut()
        .append_pair("track_name", &title)
        .append_pair("artist_name", &artist);
    if !album.is_empty() {
        url.query_pairs_mut().append_pair("album_name", &album);
    }
    if let Some(d) = duration {
        url.query_pairs_mut().append_pair("duration", &format!("{}", d.round() as u32));
    }
    let (clean, first) = (clean_title(&title), first_artist(&artist).to_string());
    let mut queries = vec![
        vec![("track_name", title.clone()), ("artist_name", artist.clone())],
        vec![("track_name", clean.clone()), ("artist_name", first.clone())],
        vec![("q", format!("{clean} {first}"))],
    ];
    queries.dedup();

    let mut jobs = tokio::task::JoinSet::new();
    jobs.spawn(async move {
        let l = get_json::<LrcLibEntry>(client, url).await?.map(LrcLibEntry::into_lyrics).filter(Lyrics::is_usable);
        Ok::<_, anyhow::Error>((0, l))
    });
    for (i, q) in queries.into_iter().enumerate() {
        let mut url = reqwest::Url::parse("https://lrclib.net/api/search")?;
        for (k, v) in &q {
            url.query_pairs_mut().append_pair(k, v);
        }
        jobs.spawn(async move { Ok((i + 1, get_json::<Vec<LrcLibEntry>>(client, url).await?.and_then(|hits| best_hit(hits, duration)))) });
    }
    let total = jobs.len();
    // answers[i]: None = still waiting, Some(x) = query i answered x.
    let mut answers: Vec<Option<Option<Lyrics>>> = vec![None; total];
    let mut failed = 0;
    while let Some(done) = jobs.join_next().await {
        match done {
            Ok(Ok((i, l))) => answers[i] = Some(l),
            _ => failed += 1,
        }
        // The best synced answer whose better-placed queries have all answered.
        for a in &answers {
            match a {
                Some(Some(l)) if !l.synced.is_empty() => {
                    jobs.abort_all();
                    return Ok(Some(l.clone()));
                }
                Some(_) => continue,
                None => break,
            }
        }
    }
    if failed == total {
        anyhow::bail!("LRCLIB didn't answer");
    }
    // No synced lyrics anywhere: the best plain ones.
    Ok(answers.into_iter().flatten().flatten().next())
}

/// Every source's lyrics for a song, for the lyrics view's source switcher: LRCLIB's matches
/// (synced first, then the closest length; different texts only), NetEase, KuGou, Genius and
/// YouTube Music's own, all asked at once. Each is labelled with where it came from ("LRCLIB 2" for
/// LRCLIB's second match). The lyrics `shown` now come first, so they can be picked again.
pub async fn sources(api: Option<Api>, t: Track, shown: Option<Lyrics>) -> Vec<Lyrics> {
    use crate::lyric_sources as ls;
    let duration = crate::parse_duration(&t.duration);
    let youtube = crate::is_youtube(&t.video_id);
    let (lrc, netease, kugou, genius, ytm) = tokio::join!(
        lrclib_all(t.title.clone(), t.artist.clone(), duration),
        ls::netease(t.clone()),
        ls::kugou(t.clone()),
        ls::genius(t.clone()),
        async {
            match api {
                Some(api) if youtube => api.lyrics(t.video_id.clone()).await.ok().flatten(),
                _ => None,
            }
        },
    );
    let others = [netease, kugou, genius].into_iter().filter_map(|r| r.ok().flatten());
    distinct(shown.into_iter().chain(lrc).chain(others).chain(ytm))
}

/// The same words with the same timing: the same lyrics, whatever the source.
fn key(l: &Lyrics) -> String {
    let words: String = l.plain.iter().chain(l.synced.iter().map(|(_, s)| s)).flat_map(|s| s.chars()).filter(|c| c.is_alphanumeric()).take(400).collect::<String>().to_lowercase();
    let timing: String = l.synced.iter().take(6).map(|(at, _)| format!("{:.0},", at * 2.0)).collect();
    format!("{timing}|{words}")
}

/// The lines shown: the synced ones, or the plain ones.
pub fn shown_lines(l: &Lyrics) -> Vec<String> {
    if l.synced.is_empty() { l.plain.clone() } else { l.synced.iter().map(|(_, s)| s.clone()).collect() }
}

/// Japanese lyrics: kana (hiragana or katakana) in a fifth of the lines or more.
pub fn is_japanese(l: &Lyrics) -> bool {
    let lines = shown_lines(l);
    let kana = lines.iter().filter(|s| s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))).count();
    !lines.is_empty() && kana * 5 >= lines.len()
}

/// Mostly Latin letters (a translation in English, not Chinese).
pub fn is_english<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    let (mut latin, mut other) = (0usize, 0usize);
    for c in lines.flat_map(str::chars).filter(|c| c.is_alphabetic()) {
        if c.is_ascii_alphabetic() { latin += 1 } else { other += 1 }
    }
    latin > 20 && latin > other * 4
}

/// A timed translation lined up with timed lines: each line gets the one starting within half a
/// second of it (else nothing).
pub fn by_time(lines: &[(f32, String)], tr: &[(f32, String)]) -> Vec<String> {
    let out: Vec<String> = lines.iter().map(|(at, _)| tr.iter().find(|(t, _)| (t - at).abs() < 0.5).map(|(_, s)| s.clone()).unwrap_or_default()).collect();
    if out.iter().filter(|s| !s.is_empty()).count() * 2 >= out.len() { out } else { Vec::new() }
}

/// English under Japanese lyrics: NetEase's own English translation when it lines up with these
/// lines, else a machine translation of every line at once (Google Translate). Empty: none.
/// Mostly Arabic letters (an Arabic song: nothing to translate for an Arabic reader).
pub fn is_arabic(l: &Lyrics) -> bool {
    let (mut arabic, mut other) = (0usize, 0usize);
    for c in shown_lines(l).iter().flat_map(|s| s.chars()).filter(|c| c.is_alphabetic()) {
        if ('\u{0600}'..='\u{06ff}').contains(&c) {
            arabic += 1;
        } else {
            other += 1;
        }
    }
    arabic > other
}

/// These lyrics get a translation under each line: with the app in Arabic, any song not in
/// Arabic (to Arabic); in English, a Japanese song (to English).
pub fn wants_translation(l: &Lyrics) -> bool {
    if shown_lines(l).is_empty() {
        return false;
    }
    if crate::i18n::is_arabic() { !is_arabic(l) } else { is_japanese(l) }
}

pub async fn translate(t: &Track, l: &Lyrics) -> Vec<String> {
    if !l.translation.is_empty() || !wants_translation(l) {
        return l.translation.clone();
    }
    // In Arabic: a machine translation to Arabic from whatever language the song is in.
    if crate::i18n::is_arabic() {
        return machine_translation_to(&shown_lines(l), "auto", "ar").await.unwrap_or_default();
    }
    if !l.synced.is_empty()
        && let Ok(Some(n)) = crate::lyric_sources::netease(t.clone()).await
        && !n.translation.is_empty()
    {
        let timed: Vec<(f32, String)> = n.synced.iter().map(|(at, _)| *at).zip(n.translation.iter().cloned()).collect();
        let lined = by_time(&l.synced, &timed);
        if !lined.is_empty() {
            return lined;
        }
    }
    machine_translation(&shown_lines(l)).await.unwrap_or_default()
}

/// Every line translated from Japanese to English in one request (Google Translate's free
/// endpoint); lines stay lines. None when it fails or the lines don't come back one for one.
pub async fn machine_translation(lines: &[String]) -> Option<Vec<String>> {
    machine_translation_to(lines, "ja", "en").await
}

/// Every line translated at once from `from` ("auto": found out) to `to`, a line for a line.
pub async fn machine_translation_to(lines: &[String], from: &str, to: &str) -> Option<Vec<String>> {
    let text = lines.iter().map(|l| if l.trim().is_empty() || l.trim() == "♪" { "-".to_string() } else { l.replace('\n', " ") }).collect::<Vec<_>>().join("\n");
    let resp: serde_json::Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?
        .post(format!("https://translate.googleapis.com/translate_a/single?client=gtx&sl={from}&tl={to}&dt=t"))
        .header("Content-Type", "application/x-www-form-urlencoded;charset=utf-8")
        .body({
            let mut u = reqwest::Url::parse("http://x/").ok()?;
            u.query_pairs_mut().append_pair("q", &text);
            u.query().unwrap_or_default().to_string()
        })
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let joined: String = resp.get(0)?.as_array()?.iter().filter_map(|seg| seg.get(0)?.as_str()).collect();
    let out: Vec<String> = joined.split('\n').map(|s| s.trim().to_string()).collect();
    (out.len() == lines.len()).then(|| out.into_iter().zip(lines).map(|(tr, orig)| if orig.trim().is_empty() || orig.trim() == "♪" || tr == "-" { String::new() } else { tr }).collect())
}

/// Keeps lyrics found later (a translation added) in place of the saved ones.
pub fn remember(video_id: &str, l: &Lyrics) {
    keep(video_id, &Some(l.clone()));
}

pub fn same(a: &Lyrics, b: &Lyrics) -> bool {
    key(a) == key(b)
}

/// The same lyrics under another source only once (another timing stays: that's what switching
/// is for when the lines run early or late); LRCLIB's matches numbered "LRCLIB", "LRCLIB 2", ...
pub fn distinct(list: impl IntoIterator<Item = Lyrics>) -> Vec<Lyrics> {
    let mut seen = std::collections::HashSet::new();
    let mut n = 0;
    let mut out = Vec::new();
    for mut l in list {
        if !seen.insert(key(&l)) {
            continue;
        }
        if l.source.starts_with("LRCLIB") {
            n += 1;
            l.source = if n == 1 { "LRCLIB".into() } else { format!("LRCLIB {n}") };
        }
        out.push(l);
    }
    out
}

/// The lyrics picked in the switcher are the song's lyrics from now on.
pub fn choose(video_id: &str, l: &Lyrics) {
    keep(video_id, &Some(l.clone()));
}

/// LRCLIB: every usable match of the exact lookup and the searches, synced first.
async fn lrclib_all(title: String, artist: String, duration: Option<f32>) -> Vec<Lyrics> {
    let client = client();
    let (clean, first) = (clean_title(&title), first_artist(&artist).to_string());
    let mut urls = Vec::new();
    if let Ok(mut u) = reqwest::Url::parse("https://lrclib.net/api/search") {
        u.query_pairs_mut().append_pair("track_name", &title).append_pair("artist_name", &artist);
        urls.push(u);
    }
    if let Ok(mut u) = reqwest::Url::parse("https://lrclib.net/api/search") {
        u.query_pairs_mut().append_pair("q", &format!("{clean} {first}"));
        urls.push(u);
    }
    let mut jobs = tokio::task::JoinSet::new();
    for u in urls {
        jobs.spawn(async move { get_json::<Vec<LrcLibEntry>>(client, u).await.ok().flatten().unwrap_or_default() });
    }
    let mut hits: Vec<(bool, f32, Lyrics)> = Vec::new();
    while let Some(Ok(list)) = jobs.join_next().await {
        for h in list {
            let off = match (duration, h.duration) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => 0.0,
            };
            if off > 15.0 {
                continue;
            }
            let l = h.into_lyrics();
            if l.is_usable() {
                hits.push((l.synced.is_empty(), off, l));
            }
        }
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    // Both searches often find the same entries: four different ones.
    let mut all = distinct(hits.into_iter().map(|(_, _, l)| l));
    all.truncate(4);
    all
}


/// Prefers synced lyrics, then the closest duration (within 10 s).
fn best_hit(hits: Vec<LrcLibEntry>, duration: Option<f32>) -> Option<Lyrics> {
    let off = |h: &LrcLibEntry| match (duration, h.duration) {
        (Some(a), Some(b)) => (a - b).abs(),
        _ => 0.0,
    };
    let mut cands: Vec<(f32, Lyrics)> = hits
        .into_iter()
        .filter(|h| off(h) <= 10.0)
        .map(|h| (off(&h), h.into_lyrics()))
        .filter(|(_, l)| l.is_usable())
        .collect();
    cands.sort_by(|(da, a), (db, b)| a.synced.is_empty().cmp(&b.synced.is_empty()).then(da.total_cmp(db)));
    cands.into_iter().next().map(|(_, l)| l)
}

/// Lyrics YouTube Music shows itself (plain text, no timing).
pub fn from_youtube_music(text: &str, source: &str) -> Option<Lyrics> {
    let plain: Vec<String> = text.lines().map(String::from).collect();
    let source = if source.is_empty() { "YouTube Music".to_string() } else { source.replace("Source: ", "") };
    let l = Lyrics { synced: Vec::new(), plain, source, translation: Vec::new(), words: Vec::new() };
    l.is_usable().then_some(l)
}

/// Parses `[mm:ss.xx] text` lines; lines can carry several timestamps.
pub fn parse_lrc(src: &str) -> Vec<(f32, String)> {
    parse_lrc_words(src).0
}

/// "mm:ss.xx" as seconds.
fn clock_secs(tag: &str) -> Option<f32> {
    let (m, s) = tag.split_once(':')?;
    Some(m.trim().parse::<f32>().ok()? * 60.0 + s.trim().parse::<f32>().ok()?)
}

/// LRC, with Enhanced LRC's word times when it has them (`[00:12.30]<00:12.30>Word <00:12.80>next`):
/// the lines (word tags taken out of their text) and each line's words (empty without tags).
pub fn parse_lrc_words(src: &str) -> (Vec<(f32, String)>, Vec<Words>) {
    let mut out: Vec<(f32, String, Words)> = Vec::new();
    for line in src.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            if let Some(t) = clock_secs(&stripped[..end]) {
                times.push(t);
            }
            rest = &stripped[end + 1..];
        }
        // Word tags: <mm:ss.xx> before each word.
        let mut words = Words::new();
        let mut text = String::new();
        let mut cur = rest;
        while let Some(open) = cur.find('<') {
            let Some(close) = cur[open..].find('>').map(|c| open + c) else { break };
            let Some(t) = clock_secs(&cur[open + 1..close]) else { break };
            text.push_str(&cur[..open]);
            if let Some(last) = words.last_mut() {
                last.1.push_str(&cur[..open]);
            }
            words.push((t, String::new()));
            cur = &cur[close + 1..];
        }
        text.push_str(cur);
        if let Some(last) = words.last_mut() {
            last.1.push_str(cur);
        }
        words.retain(|(_, w)| !w.is_empty());
        let text = if words.is_empty() { rest.trim().to_string() } else { text.trim().to_string() };
        for t in times {
            out.push((t, text.clone(), words.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    let words = if out.iter().any(|l| !l.2.is_empty()) { out.iter().map(|l| l.2.clone()).collect() } else { Vec::new() };
    (out.into_iter().map(|(t, s, _)| (t, s)).collect(), words)
}

/// NetEase's YRC (`[start_ms,dur_ms](word_ms,dur_ms,0)word(…)word`) and KuGou's KRC
/// (`[start_ms,dur_ms]<offset_ms,dur_ms,0>word…`, offsets from the line's start): the lines and
/// their words. Other lines (JSON credits, `[ar:…]` tags) are skipped.
pub fn parse_timed_words(src: &str) -> (Vec<(f32, String)>, Vec<Words>) {
    let mut lines = Vec::new();
    let mut words = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some(end) = rest.find(']') else { continue };
        let Some((start, _)) = rest[..end].split_once(',') else { continue };
        let Ok(start) = start.trim().parse::<f32>() else { continue };
        let mut cur = &rest[end + 1..];
        let mut ws = Words::new();
        // A word: (…) in YRC, <…> in KRC, then its text up to the next one.
        while let Some(open) = cur.find(['(', '<']) {
            let close_ch = if cur.as_bytes()[open] == b'(' { ')' } else { '>' };
            let Some(close) = cur[open..].find(close_ch).map(|c| open + c) else { break };
            let mut nums = cur[open + 1..close].split(',').map(|n| n.trim().parse::<f32>());
            let Some(Ok(at)) = nums.next() else { break };
            let at = if close_ch == '>' { start + at } else { at };
            let after = &cur[close + 1..];
            let next = after.find(['(', '<']).unwrap_or(after.len());
            let text = &after[..next];
            if !text.is_empty() {
                ws.push((at / 1000.0, text.to_string()));
            }
            cur = &after[next..];
        }
        if ws.is_empty() {
            continue;
        }
        lines.push((start / 1000.0, ws.iter().map(|(_, w)| w.as_str()).collect::<String>().trim().to_string()));
        words.push(ws);
    }
    (lines, words)
}

/// KuGou's KRC file as KuGou sends it (after base64): "krc1", then XOR-scrambled zlib text.
pub fn decode_krc(raw: &[u8]) -> Option<String> {
    const KEY: [u8; 16] = [64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105];
    let body = raw.strip_prefix(b"krc1")?;
    let plain: Vec<u8> = body.iter().enumerate().map(|(i, b)| b ^ KEY[i % KEY.len()]).collect();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::ZlibDecoder::new(&plain[..]), &mut text).ok()?;
    Some(text.trim_start_matches('\u{feff}').to_string())
}

/// The sung line split at `pos` seconds for the word-by-word fill: what's been sung (whole words,
/// then the letters of the word being sung, as far as its time has gone) and what's left.
/// `line_end`: when the next line starts.
pub fn sung_split(words: &[(f32, String)], line_end: f32, pos: f32) -> (String, String) {
    let mut sung = String::new();
    let mut left = String::new();
    for (i, (start, word)) in words.iter().enumerate() {
        let end = words.get(i + 1).map_or(line_end, |w| w.0).max(*start + 0.05);
        if pos >= end {
            sung.push_str(word);
        } else if pos <= *start {
            left.push_str(word);
        } else {
            let chars: Vec<char> = word.chars().collect();
            let lit = ((pos - start) / (end - start) * chars.len() as f32).round() as usize;
            sung.extend(&chars[..lit.min(chars.len())]);
            left.extend(&chars[lit.min(chars.len())..]);
        }
    }
    (sung, left)
}

/// Index of the line being sung at `pos` seconds.
pub fn current_line(synced: &[(f32, String)], pos: f32) -> Option<usize> {
    let idx = synced.partition_point(|(t, _)| *t <= pos);
    idx.checked_sub(1)
}

#[cfg(test)]
mod word_tests {
    use super::*;

    #[test]
    fn enhanced_lrc_gives_words_and_clean_lines() {
        let (lines, words) = parse_lrc_words("[00:01.00]<00:01.00>Hello <00:01.50>dear <00:02.00>world\n[00:03.00]plain line");
        assert_eq!(lines, [(1.0, "Hello dear world".to_string()), (3.0, "plain line".to_string())]);
        assert_eq!(words[0], [(1.0, "Hello ".to_string()), (1.5, "dear ".to_string()), (2.0, "world".to_string())]);
        assert!(words[1].is_empty());
        // No word tags: no words at all.
        assert!(parse_lrc_words("[00:01.00]just a line").1.is_empty());
    }

    #[test]
    fn yrc_and_krc_give_words() {
        let (l, w) = parse_timed_words("{\"t\":0,\"c\":[]}\n[16210,3460](16210,670,0)Hello (16880,410,0)world");
        assert_eq!(l, [(16.21, "Hello world".to_string())]);
        assert_eq!(w[0], [(16.21, "Hello ".to_string()), (16.88, "world".to_string())]);
        let (l, w) = parse_timed_words("[ar:Someone]\n[1000,2000]<0,500,0>こん<500,500,0>にちは");
        assert_eq!(l, [(1.0, "こんにちは".to_string())]);
        assert_eq!(w[0], [(1.0, "こん".to_string()), (1.5, "にちは".to_string())]);
    }

    #[test]
    fn the_sung_part_grows_letter_by_letter() {
        let words = vec![(1.0, "Hello ".to_string()), (2.0, "world".to_string())];
        assert_eq!(sung_split(&words, 3.0, 0.5), (String::new(), "Hello world".to_string()));
        assert_eq!(sung_split(&words, 3.0, 2.0), ("Hello ".to_string(), "world".to_string()));
        assert_eq!(sung_split(&words, 3.0, 2.6), ("Hello wor".to_string(), "ld".to_string()));
        assert_eq!(sung_split(&words, 3.0, 9.0), ("Hello world".to_string(), String::new()));
    }

    #[test]
    fn krc_unscrambles() {
        use std::io::Write;
        const KEY: [u8; 16] = [64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105];
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all("[1000,2000]<0,500,0>hi".as_bytes()).unwrap();
        let packed = z.finish().unwrap();
        let mut raw = b"krc1".to_vec();
        raw.extend(packed.iter().enumerate().map(|(i, b)| b ^ KEY[i % KEY.len()]));
        assert_eq!(decode_krc(&raw).as_deref(), Some("[1000,2000]<0,500,0>hi"));
        assert_eq!(decode_krc(b"nope"), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let l = parse_lrc("[00:01.50]hello\n[ar:x]\n[00:10.00][00:20.00]again");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], (1.5, "hello".into()));
        assert_eq!(current_line(&l, 0.5), None);
        assert_eq!(current_line(&l, 12.0), Some(1));
    }

    #[test]
    fn loosens_queries() {
        assert_eq!(clean_title("Nothing Ever (feat. Kasane Teto) [Live]"), "Nothing Ever");
        assert_eq!(first_artist("Nino, ROUND TABLE featuring Nino"), "Nino");
        assert_eq!(first_artist("Vacations"), "Vacations");
    }
}

#[cfg(test)]
mod distinct_tests {
    use super::*;

    #[test]
    fn other_timings_stay_and_lrclib_is_numbered() {
        let words: Vec<String> = ["one", "two", "three", "four"].map(String::from).to_vec();
        let timed = |shift: f32| Lyrics { synced: words.iter().enumerate().map(|(i, w)| (i as f32 * 2.0 + shift, w.clone())).collect(), plain: words.clone(), source: "LRCLIB".into(), translation: Vec::new(), words: Vec::new() };
        let plain = Lyrics { synced: Vec::new(), plain: words.clone(), source: "lyrics.ovh".into(), translation: Vec::new(), words: Vec::new() };
        let ytm = Lyrics { source: "YouTube Music".into(), ..plain.clone() };
        let out = distinct([timed(0.0), timed(0.0), timed(1.5), plain, ytm]);
        assert_eq!(out.iter().map(|l| l.source.as_str()).collect::<Vec<_>>(), ["LRCLIB", "LRCLIB 2", "lyrics.ovh"]);
        assert!(same(&out[0], &timed(0.0)) && !same(&out[0], &out[1]));
    }
}

#[cfg(test)]
mod sources_probe {
    /// Every source for a well-known song; prints sources and line counts only.
    #[tokio::test]
    #[ignore]
    async fn sources_for_a_known_song() {
        let t = crate::api::Track {
            video_id: "fJ9rUzIMcZQ".into(),
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            album: String::new(),
            duration: "5:55".into(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        };
        for l in super::sources(None, t, None).await {
            println!("{}: {} synced, {} plain", l.source, l.synced.len(), l.plain.len());
        }
    }
}

#[cfg(test)]
mod translation_tests {
    use super::*;

    #[test]
    fn japanese_english_and_timing() {
        let jp = Lyrics { synced: vec![(1.0, "君の名前を".into()), (5.0, "呼んでいる".into()), (9.0, "Oh yeah".into())], ..Default::default() };
        assert!(is_japanese(&jp));
        assert!(!is_japanese(&Lyrics { synced: vec![(1.0, "我爱你中国".into()), (2.0, "hello".into())], ..Default::default() }), "Chinese has no kana");
        assert!(is_english(["I am calling your name", "Calling out to you"].into_iter()));
        assert!(!is_english(["我在呼唤你的名字", "呼唤你"].into_iter()));
        let tr = vec![(1.1, "Your name".to_string()), (5.0, "I'm calling".to_string())];
        assert_eq!(by_time(&jp.synced, &tr), vec!["Your name".to_string(), "I'm calling".to_string(), String::new()]);
        assert!(by_time(&jp.synced, &[(30.0, "far".to_string())]).is_empty(), "not lined up: none");
    }

    /// `cargo test --lib live_machine_translation -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_machine_translation() {
        let lines: Vec<String> = ["君の名前を呼んでいる", "", "夜明けまで歌おう"].iter().map(|s| s.to_string()).collect();
        let out = machine_translation(&lines).await.expect("translated");
        println!("{out:?}");
        assert_eq!(out.len(), 3);
        assert!(out[1].is_empty() && !out[0].is_empty() && !out[2].is_empty());
    }

    #[test]
    fn which_lyrics_get_translated() {
        let lyrics = |lines: &[&str]| Lyrics { synced: Vec::new(), plain: lines.iter().map(|s| s.to_string()).collect(), source: String::new(), translation: Vec::new(), words: Vec::new() };
        let arabic = lyrics(&["يا ليل يا عين", "سهرت الليل"]);
        let english = lyrics(&["I walk alone", "through the night"]);
        assert!(super::is_arabic(&arabic) && !super::is_arabic(&english));
        crate::i18n::set_arabic(true);
        assert!(super::wants_translation(&english), "in Arabic: an English song is translated");
        assert!(!super::wants_translation(&arabic), "an Arabic song isn't");
        crate::i18n::set_arabic(false);
        assert!(!super::wants_translation(&english), "in English: only Japanese");
    }

}

#[cfg(test)]
mod arabic_live_tests {
    /// Live: an English line to Arabic (`cargo test --lib arabic_live -- --ignored --nocapture`).
    #[tokio::test]
    #[ignore]
    async fn arabic_live() {
        let out = super::machine_translation_to(&["I walk alone through the night".to_string(), "♪".to_string()], "auto", "ar").await;
        println!("{out:?}");
        assert!(out.is_some_and(|o| o.len() == 2 && o[0].chars().any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)) && o[1].is_empty()));
    }
}
