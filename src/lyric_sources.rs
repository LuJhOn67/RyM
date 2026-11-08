//! More lyrics providers besides LRCLIB and YouTube Music:
//! - NetEase Cloud Music: search for the song, then its synced LRC;
//! - KuGou: search for the song, its lyrics candidates, then the LRC (sent as base64);
//! - Genius: plain lyrics only (the song's page), the last resort.
//!
//! A match must have the same title (without "(feat. ...)" and the like) and, when both lengths
//! are known, a length within 10 seconds. Credit lines ("作词 : ...", "Written by: ...") and the
//! "Title - Artist" line some files start with are left out.

use std::time::Duration;

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::api::Track;
use crate::lyrics::{Lyrics, parse_lrc};

pub const NETEASE: &str = "NetEase";
pub const KUGOU: &str = "KuGou";
pub const GENIUS: &str = "Genius";
pub const YOUTUBE: &str = "YouTube Music";
pub const LRCLIB: &str = "LRCLIB";

/// The providers in the order they are tried (and shown in the lyrics view's switcher).
pub const PROVIDERS: [&str; 5] = [LRCLIB, NETEASE, KUGOU, GENIUS, YOUTUBE];

/// Which provider some lyrics came from ("LRCLIB 2" is LRCLIB; YouTube Music names its own
/// sources, like "LyricFind").
pub fn provider(l: &Lyrics) -> &'static str {
    PROVIDERS.iter().copied().find(|p| *p != YOUTUBE && l.source.starts_with(p)).unwrap_or(YOUTUBE)
}

const BROWSER: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().user_agent(BROWSER).timeout(Duration::from_secs(10)).build().expect("http client"))
}

async fn json(url: reqwest::Url, referer: Option<&str>) -> Result<Value> {
    let mut req = client().get(url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let text = req.send().await?.error_for_status()?.text().await?;
    Ok(serde_json::from_str(&text)?)
}

/// Lower case letters and digits, without the "(feat. ...)" and "[...]" parts.
fn norm(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' | '（' | '【' => depth += 1,
            ')' | ']' | '）' | '】' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            _ if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out
}

fn first_artist(a: &str) -> String {
    a.split([',', '&', '、', '/']).next().unwrap_or(a).trim().to_string()
}

/// The same song: the title matches (one holds the other), and the lengths are within 10 s.
fn same_song(t: &Track, title: &str, secs: Option<f32>) -> bool {
    let (a, b) = (norm(&t.title), norm(title));
    let titles = !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a));
    let lengths = match (crate::parse_duration(&t.duration), secs) {
        (Some(x), Some(y)) => (x - y).abs() <= 10.0,
        _ => true,
    };
    titles && lengths
}

fn query(t: &Track) -> String {
    let mut title = t.title.clone();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let Some(a) = title.find(open) {
            let Some(b) = title[a..].find(close) else { break };
            title.replace_range(a..a + b + close.len_utf8(), "");
        }
    }
    format!("{} {}", title.split_whitespace().collect::<Vec<_>>().join(" "), first_artist(&t.artist))
}

/// Credit lines and the "Title - Artist" header line are not lyrics.
pub fn clean(mut l: Lyrics, t: &Track) -> Lyrics {
    const CREDITS: [&str; 22] = [
        "作词", "作曲", "编曲", "制作", "混音", "母带", "和声", "吉他", "贝斯", "鼓", "录音", "监制", "出品", "发行", "词", "曲", "written by", "composed by",
        "lyrics by", "lyricist", "composer", "producer",
    ];
    let (title, artist) = (norm(&t.title), norm(&first_artist(&t.artist)));
    let keep = |line: &str| {
        let lower = line.trim().to_lowercase();
        let credit = CREDITS.iter().any(|c| {
            lower.strip_prefix(c).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.starts_with(':') || rest.starts_with('：')
            })
        });
        let n = norm(line);
        let header = !title.is_empty() && !artist.is_empty() && n.contains(&title) && n.contains(&artist);
        !credit && !header
    };
    // The word times go with their lines.
    if l.words.len() == l.synced.len() && !l.words.is_empty() {
        let kept: Vec<bool> = l.synced.iter().map(|(_, s)| keep(s)).collect();
        let mut at = 0;
        l.words.retain(|_| {
            at += 1;
            kept[at - 1]
        });
    } else {
        l.words.clear();
    }
    l.synced.retain(|(_, s)| keep(s));
    l.plain.retain(|s| keep(s));
    // A first line naming the artist with a dash ("Artist -<Title>"): a header, title misspelled.
    let header = |s: &str| !artist.is_empty() && norm(s).contains(&artist) && s.contains('-');
    if l.synced.first().is_some_and(|(_, s)| header(s)) {
        l.synced.remove(0);
        if !l.words.is_empty() {
            l.words.remove(0);
        }
        if l.plain.first().is_some_and(|s| header(s)) {
            l.plain.remove(0);
        }
    }
    l
}

fn usable(l: Lyrics) -> Option<Lyrics> {
    (l.synced.len() >= 4 || l.plain.iter().filter(|s| !s.trim().is_empty()).count() >= 4).then_some(l)
}

/// NetEase Cloud Music's synced lyrics.
pub async fn netease(t: Track) -> Result<Option<Lyrics>> {
    let mut url = reqwest::Url::parse("https://music.163.com/api/search/get")?;
    url.query_pairs_mut().append_pair("s", &query(&t)).append_pair("type", "1").append_pair("limit", "10").append_pair("offset", "0");
    let found = json(url, Some("https://music.163.com/")).await?;
    let Some(id) = found["result"]["songs"].as_array().into_iter().flatten().find_map(|s| {
        let secs = s["duration"].as_f64().map(|ms| (ms / 1000.0) as f32);
        same_song(&t, s["name"].as_str()?, secs).then(|| s["id"].as_u64()).flatten()
    }) else {
        return Ok(None);
    };
    // v1 also sends the word-timed lyrics (YRC) when NetEase has them.
    let mut url = reqwest::Url::parse("https://music.163.com/api/song/lyric/v1")?;
    url.query_pairs_mut().append_pair("id", &id.to_string()).append_pair("lv", "1").append_pair("kv", "1").append_pair("tv", "-1").append_pair("yv", "1");
    let lrc = json(url, Some("https://music.163.com/")).await?;
    let mut synced = parse_lrc(lrc["lrc"]["lyric"].as_str().unwrap_or_default());
    let (timed, words) = crate::lyrics::parse_timed_words(lrc["yrc"]["lyric"].as_str().unwrap_or_default());
    // The word-timed lines when they're the whole song (not a few lines of it).
    let words = if !timed.is_empty() && timed.len() * 10 >= synced.len() * 6 {
        synced = timed;
        words
    } else {
        Vec::new()
    };
    let plain = synced.iter().map(|(_, s)| s.clone()).collect();
    let mut l = clean(Lyrics { synced, plain, source: NETEASE.into(), translation: Vec::new(), words }, &t);
    // Its own translation (often Chinese): kept when it's English, line by line by time.
    let tr = parse_lrc(lrc["tlyric"]["lyric"].as_str().unwrap_or_default());
    if crate::lyrics::is_english(tr.iter().map(|(_, s)| s.as_str())) {
        l.translation = crate::lyrics::by_time(&l.synced, &tr);
    }
    Ok(usable(l))
}

/// KuGou's synced lyrics.
pub async fn kugou(t: Track) -> Result<Option<Lyrics>> {
    let mut url = reqwest::Url::parse("http://mobilecdn.kugou.com/api/v3/search/song")?;
    url.query_pairs_mut()
        .append_pair("format", "json")
        .append_pair("keyword", &query(&t))
        .append_pair("page", "1")
        .append_pair("pagesize", "10")
        .append_pair("showtype", "1");
    let found = json(url, None).await?;
    let Some((hash, secs)) = found["data"]["info"].as_array().into_iter().flatten().find_map(|s| {
        let secs = s["duration"].as_f64().map(|d| d as f32);
        same_song(&t, s["songname"].as_str()?, secs).then(|| Some((s["hash"].as_str()?.to_string(), secs))).flatten()
    }) else {
        return Ok(None);
    };
    let mut url = reqwest::Url::parse("https://krcs.kugou.com/search")?;
    url.query_pairs_mut()
        .append_pair("ver", "1")
        .append_pair("man", "yes")
        .append_pair("client", "mobi")
        .append_pair("keyword", "")
        .append_pair("duration", &((secs.unwrap_or(0.0) * 1000.0) as u64).to_string())
        .append_pair("hash", &hash);
    let candidates = json(url, None).await?;
    let Some(c) = candidates["candidates"].as_array().and_then(|c| c.first()) else { return Ok(None) };
    let (Some(id), Some(key)) = (c["id"].as_str().map(String::from).or_else(|| c["id"].as_u64().map(|n| n.to_string())), c["accesskey"].as_str()) else {
        return Ok(None);
    };
    let download = |fmt: &str| {
        let mut url = reqwest::Url::parse("https://lyrics.kugou.com/download").expect("url");
        url.query_pairs_mut()
            .append_pair("ver", "1")
            .append_pair("client", "pc")
            .append_pair("id", &id)
            .append_pair("accesskey", key)
            .append_pair("fmt", fmt)
            .append_pair("charset", "utf8");
        json(url, None)
    };
    // KRC: word by word (scrambled and packed); plain LRC when it isn't there or can't be read.
    if let Ok(file) = download("krc").await
        && let Some(text) = base64(file["content"].as_str().unwrap_or_default()).and_then(|raw| crate::lyrics::decode_krc(&raw))
    {
        let (synced, words) = crate::lyrics::parse_timed_words(&text);
        if !synced.is_empty() {
            let plain = synced.iter().map(|(_, s)| s.clone()).collect();
            if let Some(l) = usable(clean(Lyrics { synced, plain, source: KUGOU.into(), translation: Vec::new(), words }, &t)) {
                return Ok(Some(l));
            }
        }
    }
    let file = download("lrc").await?;
    let text = String::from_utf8_lossy(&base64(file["content"].as_str().unwrap_or_default()).ok_or_else(|| anyhow!("KuGou: bad lyrics file"))?).into_owned();
    let synced = parse_lrc(&text);
    let plain = synced.iter().map(|(_, s)| s.clone()).collect();
    Ok(usable(clean(Lyrics { synced, plain, source: KUGOU.into(), translation: Vec::new(), words: Vec::new() }, &t)))
}

/// Genius's lyrics (plain: Genius has no timing), from the song's page.
pub async fn genius(t: Track) -> Result<Option<Lyrics>> {
    let mut url = reqwest::Url::parse("https://genius.com/api/search/song")?;
    url.query_pairs_mut().append_pair("q", &query(&t)).append_pair("per_page", "5");
    let found = json(url, None).await?;
    let artist = norm(&first_artist(&t.artist));
    let hit = found["response"]["sections"].as_array().into_iter().flatten().flat_map(|s| s["hits"].as_array().into_iter().flatten()).find_map(|h| {
        let r = &h["result"];
        let by = norm(r["primary_artist"]["name"].as_str().unwrap_or_default());
        // Translations come from "Genius Traducciones" and such: the artist must match.
        let artist_ok = artist.is_empty() || by.contains(&artist) || artist.contains(&by);
        (artist_ok && same_song(&t, r["title"].as_str()?, None)).then(|| r["url"].as_str().map(String::from)).flatten()
    });
    let Some(page) = hit else { return Ok(None) };
    let html = client().get(page).send().await?.error_for_status()?.text().await?;
    let plain = genius_lines(&html);
    Ok(usable(clean(Lyrics { synced: Vec::new(), plain, source: GENIUS.into(), translation: Vec::new(), words: Vec::new() }, &t)))
}

/// The text of the page's lyrics containers, line by line (their nested headers left out).
fn genius_lines(html: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut from = 0;
    while let Some(at) = html[from..].find("data-lyrics-container=\"true\"") {
        let tag = html[..from + at].rfind("<div").unwrap_or(from + at);
        let inner = div_inner(html, tag);
        from = from + at + 1;
        let mut inner = inner.to_string();
        while let Some(x) = inner.find("data-exclude-from-selection=\"true\"") {
            let tag = inner[..x].rfind("<div").unwrap_or(x);
            let len = div_inner(&inner, tag).len();
            let open_end = inner[tag..].find('>').map_or(inner.len(), |e| tag + e + 1);
            let close_end = (open_end + len + "</div>".len()).min(inner.len());
            inner.replace_range(tag..close_end, "");
        }
        parts.push(inner);
    }
    let text = parts.join("\n").replace("<br/>", "\n").replace("<br>", "\n").replace("<br />", "\n");
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let lines: Vec<String> = unescape(&out).lines().map(|l| l.trim().to_string()).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    lines[start..].to_vec()
}

/// What's between the `<div ...>` at `tag` and its own `</div>` (nested divs counted).
fn div_inner(html: &str, tag: usize) -> &str {
    let Some(open_end) = html[tag..].find('>').map(|e| tag + e + 1) else { return "" };
    let mut depth = 1;
    let mut at = open_end;
    while depth > 0 {
        let next_open = html[at..].find("<div").map(|i| at + i);
        let Some(next_close) = html[at..].find("</div>").map(|i| at + i) else { return &html[open_end..] };
        match next_open {
            Some(o) if o < next_close => {
                depth += 1;
                at = o + 4;
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    return &html[open_end..next_close];
                }
                at = next_close + 6;
            }
        }
    }
    ""
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(title: &str, artist: &str, duration: &str) -> Track {
        Track {
            video_id: "x".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: duration.into(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    #[test]
    fn credits_and_header_lines_go() {
        let song = t("The Less I Know The Better", "Tame Impala", "3:38");
        let lrc = "[00:00.00] 作词 : Kevin Parker\n[00:01.00]作曲：Kevin\n[00:02.00]The Less I Know The Better - Tame Impala\n[00:12.47]Written by：Kevin Parke\n[00:25.05]Someone said they left together\n[00:29.18]I ran out the door to get her";
        let l = clean(Lyrics { synced: parse_lrc(lrc), plain: Vec::new(), source: NETEASE.into(), translation: Vec::new(), words: Vec::new() }, &song);
        assert_eq!(l.synced.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["Someone said they left together", "I ran out the door to get her"]);
        assert!(same_song(&song, "The Less I Know The Better (Explicit)", Some(218.9)));
        assert!(!same_song(&song, "The Less I Know The Better", Some(370.0)));
        assert_eq!(provider(&Lyrics { source: "LRCLIB 2".into(), ..Default::default() }), LRCLIB);
        assert_eq!(provider(&Lyrics { source: "LyricFind".into(), ..Default::default() }), YOUTUBE);
    }

    #[test]
    fn genius_page_text_and_base64() {
        let html = r#"<div data-lyrics-container="true" class="a"><div data-exclude-from-selection="true">185 Contributors<div>x</div></div>[Verse 1]<br/>Someone said &amp; <i>left</i><br/>I ran</div><p>no</p><div data-lyrics-container="true">Last line</div>"#;
        assert_eq!(genius_lines(html), ["[Verse 1]", "Someone said & left", "I ran", "Last line"]);
        assert_eq!(base64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
    }

    /// Word times from NetEase (YRC) and KuGou (KRC), lined up with their lines.
    #[tokio::test]
    #[ignore]
    async fn live_word_times() {
        // NetEase (YRC) and KuGou (KRC) give word times for well-known songs.
        for song in [t("Blinding Lights", "The Weeknd", "3:20"), t("夜に駆ける", "YOASOBI", "4:21")] {
            for (name, l) in [("netease", netease(song.clone()).await), ("kugou", kugou(song.clone()).await)] {
                let l = l.ok().flatten();
                let lines = l.as_ref().map_or(0, |l| l.synced.len());
                let timed = l.as_ref().map_or(0, |l| l.words.iter().filter(|w| !w.is_empty()).count());
                let first = l.as_ref().and_then(|l| l.words.iter().find(|w| !w.is_empty()).cloned()).unwrap_or_default();
                println!("{} / {name}: {lines} lines, {timed} with word times, first: {first:?}", song.title);
                if let Some(l) = &l {
                    assert!(l.words.is_empty() || l.words.len() == l.synced.len(), "words and lines out of step");
                }
            }
        }
    }

    /// The three providers for a well-known song (line counts only).
    #[tokio::test]
    #[ignore]
    async fn live_providers() {
        let song = t("The Less I Know The Better", "Tame Impala", "3:38");
        for (name, l) in [("netease", netease(song.clone()).await), ("kugou", kugou(song.clone()).await), ("genius", genius(song.clone()).await)] {
            let l = l.unwrap().expect(name);
            println!("{name}: {} synced, {} plain, first: {:?}", l.synced.len(), l.plain.len(), l.synced.first().map(|x| &x.1).or(l.plain.first()));
        }
    }
}
