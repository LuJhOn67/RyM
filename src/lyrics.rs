//! Lyrics from LRCLIB (synced when available).

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

const UA: &str = concat!("ytm-player/", env!("CARGO_PKG_VERSION"));

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
    let resp = client.get(url).send().await?;
    let mut fallback = None;
    if resp.status().is_success() {
        let l = resp.json::<LrcLibEntry>().await?.into_lyrics();
        if l.is_usable() && !l.synced.is_empty() {
            return Ok(Some(l));
        }
        fallback = l.is_usable().then_some(l);
    }

    // No good exact match: search, prefer synced lyrics with the closest duration.
    let mut url = reqwest::Url::parse("https://lrclib.net/api/search")?;
    url.query_pairs_mut()
        .append_pair("track_name", &title)
        .append_pair("artist_name", &artist);
    let hits: Vec<LrcLibEntry> = client.get(url).send().await?.json().await?;
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
    // Synced first, then closest duration.
    cands.sort_by(|(da, a), (db, b)| {
        a.synced.is_empty().cmp(&b.synced.is_empty()).then(da.total_cmp(db))
    });
    let best = cands.into_iter().next().map(|(_, l)| l);
    Ok(match best {
        Some(b) if !b.synced.is_empty() => Some(b),
        b => fallback.or(b),
    })
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
}
