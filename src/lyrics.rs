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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrcLibEntry {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    duration: Option<f32>,
}

impl LrcLibEntry {
    fn into_lyrics(self) -> Lyrics {
        let synced = self.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default();
        let plain = self.plain_lyrics.unwrap_or_default().lines().map(String::from).collect();
        Lyrics { synced, plain, source: "LRCLIB".into(), translation: Vec::new() }
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
pub async fn translate(t: &Track, l: &Lyrics) -> Vec<String> {
    if !l.translation.is_empty() || !is_japanese(l) {
        return l.translation.clone();
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
    let text = lines.iter().map(|l| if l.trim().is_empty() || l.trim() == "♪" { "-".to_string() } else { l.replace('\n', " ") }).collect::<Vec<_>>().join("\n");
    let resp: serde_json::Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?
        .post("https://translate.googleapis.com/translate_a/single?client=gtx&sl=ja&tl=en&dt=t")
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
    let l = Lyrics { synced: Vec::new(), plain, source, translation: Vec::new() };
    l.is_usable().then_some(l)
}

/// Parses `[mm:ss.xx] text` lines; lines can carry several timestamps.
pub fn parse_lrc(src: &str) -> Vec<(f32, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some((m, s)) = tag.split_once(':')
                && let (Ok(m), Ok(s)) = (m.parse::<f32>(), s.parse::<f32>())
            {
                times.push(m * 60.0 + s);
            }
            rest = &stripped[end + 1..];
        }
        for t in times {
            out.push((t, rest.trim().to_string()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Index of the line being sung at `pos` seconds.
pub fn current_line(synced: &[(f32, String)], pos: f32) -> Option<usize> {
    let idx = synced.partition_point(|(t, _)| *t <= pos);
    idx.checked_sub(1)
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
        let timed = |shift: f32| Lyrics { synced: words.iter().enumerate().map(|(i, w)| (i as f32 * 2.0 + shift, w.clone())).collect(), plain: words.clone(), source: "LRCLIB".into(), translation: Vec::new() };
        let plain = Lyrics { synced: Vec::new(), plain: words.clone(), source: "lyrics.ovh".into(), translation: Vec::new() };
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
}
