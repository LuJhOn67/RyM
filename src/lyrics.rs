//! Lyrics: LRCLIB (synced when available), YouTube Music's own as a fallback.
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
        Lyrics { synced, plain, source: "LRCLIB".into() }
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

/// A song's lyrics: from the disk if found before, else LRCLIB and YouTube Music at once
/// (LRCLIB's preferred: it has timing). `None` when neither has any.
pub async fn for_track(api: Option<Api>, t: Track) -> Option<Lyrics> {
    if let Some(saved) = cached(&t.video_id) {
        return saved;
    }
    let duration = crate::parse_duration(&t.duration);
    let ytm = {
        let (api, vid) = (api.clone(), t.video_id.clone());
        tokio::spawn(async move {
            match api {
                Some(api) if !crate::local::is_local(&vid) => api.lyrics(vid).await.ok().flatten(),
                _ => None,
            }
        })
    };
    let found = match lrclib(t.title.clone(), t.artist.clone(), t.album.clone(), duration).await {
        Ok(Some(l)) => {
            ytm.abort();
            Some(l)
        }
        Ok(None) => ytm.await.ok().flatten(),
        // LRCLIB down: don't remember "no lyrics", it may have them later.
        Err(_) => return ytm.await.ok().flatten(),
    };
    keep(&t.video_id, &found);
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
fn cached(video_id: &str) -> Option<Option<Lyrics>> {
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
    let l = Lyrics { synced: Vec::new(), plain, source };
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
