//! Lyrics: LRCLIB (synced when available), YouTube Music's own as a fallback.

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Default)]
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
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
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

pub async fn lrclib(title: String, artist: String, album: String, duration: Option<f32>) -> Result<Option<Lyrics>> {
    let client = reqwest::Client::builder().user_agent(UA).build()?;
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
    let mut fallback = None;
    if let Some(entry) = get_json::<LrcLibEntry>(&client, url).await? {
        let l = entry.into_lyrics();
        if l.is_usable() && !l.synced.is_empty() {
            return Ok(Some(l));
        }
        fallback = l.is_usable().then_some(l);
    }

    // No good exact match: search with looser and looser queries, stop at the first synced hit.
    let (clean, first) = (clean_title(&title), first_artist(&artist).to_string());
    let mut queries = vec![
        vec![("track_name", title.clone()), ("artist_name", artist.clone())],
        vec![("track_name", clean.clone()), ("artist_name", first.clone())],
        vec![("q", format!("{clean} {first}"))],
    ];
    queries.dedup();
    for q in queries {
        let mut url = reqwest::Url::parse("https://lrclib.net/api/search")?;
        for (k, v) in &q {
            url.query_pairs_mut().append_pair(k, v);
        }
        let Some(hits) = get_json::<Vec<LrcLibEntry>>(&client, url).await? else { continue };
        match best_hit(hits, duration) {
            Some(l) if !l.synced.is_empty() => return Ok(Some(l)),
            Some(l) if fallback.is_none() => fallback = Some(l),
            _ => {}
        }
    }
    Ok(fallback)
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
