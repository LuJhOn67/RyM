//! AutoEq (github.com/jaakkopasanen/AutoEq): measured corrections for thousands of headphones,
//! fitted to RyM's 5-band equalizer. Type a model ("WH-1000XM4", "AirPods Pro"), RyM finds it in
//! AutoEq's index, reads its correction curve and sets the custom EQ from it.
//!
//! RyM's 5 bands can't follow the whole curve: each band takes the curve's average within half an
//! octave of it, centred so the overall loudness stays about the same.

use anyhow::{Result, anyhow};

const BASE: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";

/// A headphone in AutoEq's index: its name, who measured it, and where its files are.
#[derive(Debug, Clone, PartialEq)]
pub struct Headphone {
    pub name: String,
    pub by: String,
    /// The folder under `results`, URL-encoded as the index has it ("oratory1990/over-ear/Sony%20WH-1000XM4").
    path: String,
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().expect("http client"))
}

/// AutoEq's index (about 850 KB), read once a run.
async fn index() -> Result<std::sync::Arc<Vec<Headphone>>> {
    static KNOWN: std::sync::OnceLock<tokio::sync::Mutex<Option<std::sync::Arc<Vec<Headphone>>>>> = std::sync::OnceLock::new();
    let mut known = KNOWN.get_or_init(Default::default).lock().await;
    if let Some(list) = known.as_ref() {
        return Ok(list.clone());
    }
    let text = http().get(format!("{BASE}/INDEX.md")).send().await?.error_for_status()?.text().await?;
    let list = std::sync::Arc::new(parse_index(&text));
    *known = Some(list.clone());
    Ok(list)
}

/// "- [Sony WH-1000XM4](./oratory1990/over-ear/Sony%20WH-1000XM4) by oratory1990" lines.
fn parse_index(text: &str) -> Vec<Headphone> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("- [")?;
            let (name, rest) = l.split_once("](./")?;
            // The path can hold brackets itself ("Sony%20WH-1000XM4%20(ANC%20on)"): it ends at ") by".
            let (path, by) = match rest.rfind(") by ") {
                Some(at) => (&rest[..at], &rest[at + 5..]),
                None => (rest.trim_end().strip_suffix(')')?, ""),
            };
            Some(Headphone { name: name.to_string(), by: by.split(" on ").next().unwrap_or(by).trim().to_string(), path: path.to_string() })
        })
        .collect()
}

/// The headphones whose name has every word typed, oratory1990's measurements first (the most
/// trusted), each name once; the first `n`.
pub async fn search(query: &str, n: usize) -> Result<Vec<Headphone>> {
    Ok(find(&index().await?, query, n))
}

fn find(list: &[Headphone], query: &str, n: usize) -> Vec<Headphone> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<&Headphone> = list.iter().filter(|h| {
        let name = h.name.to_lowercase();
        words.iter().all(|w| name.contains(w.as_str()))
    }).collect();
    hits.sort_by_key(|h| (h.by != "oratory1990", h.name.len()));
    let mut seen = std::collections::HashSet::new();
    hits.into_iter().filter(|h| seen.insert(h.name.clone())).take(n).cloned().collect()
}

/// The headphone's correction as RyM's 5 band gains (dB).
pub async fn bands(h: &Headphone) -> Result<[f32; 5]> {
    let file = h.path.rsplit('/').next().unwrap_or_default();
    let url = format!("{BASE}/{}/{file}%20GraphicEQ.txt", h.path);
    let text = http().get(url).send().await?.error_for_status()?.text().await?;
    let points = parse_graphic_eq(&text);
    if points.is_empty() {
        return Err(anyhow!("no correction for {}", h.name));
    }
    Ok(fit(&points))
}

/// "GraphicEQ: 20 -9.9; 21 -10.0; ..." as (Hz, dB) points.
fn parse_graphic_eq(text: &str) -> Vec<(f32, f32)> {
    let body = text.trim().strip_prefix("GraphicEQ:").unwrap_or(text);
    body.split(';')
        .filter_map(|p| {
            let mut it = p.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect()
}

/// The curve fitted to RyM's bands: each band's average within half an octave, centred (the
/// average gain taken off), within ±12 dB, to half a dB.
fn fit(points: &[(f32, f32)]) -> [f32; 5] {
    let mut out = [0f32; 5];
    for (band, centre) in crate::fx::BANDS.iter().enumerate() {
        let near: Vec<f32> = points.iter().filter(|(f, _)| (f / centre).log2().abs() <= 0.5).map(|(_, g)| *g).collect();
        out[band] = if near.is_empty() { 0.0 } else { near.iter().sum::<f32>() / near.len() as f32 };
    }
    let mean = out.iter().sum::<f32>() / out.len() as f32;
    out.map(|g| ((g - mean).clamp(-12.0, 12.0) * 2.0).round() / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_lines() {
        let list = parse_index("# Index\n- [Sony WH-1000XM4](./oratory1990/over-ear/Sony%20WH-1000XM4) by oratory1990\n- [Sony WH-1000XM4](./crinacle/GRAS%2043AG-7%20over-ear/Sony%20WH-1000XM4) by crinacle on GRAS 43AG-7\n- [AirPods Pro](./crinacle/711%20in-ear/AirPods%20Pro) by crinacle on 711\n- [Sony WH-1000XM4 (ANC on)](./HypetheSonics/over-ear/Sony%20WH-1000XM4%20(ANC%20on)) by HypetheSonics on GRAS\n");
        assert_eq!(list.len(), 4);
        assert_eq!(list[3].path, "HypetheSonics/over-ear/Sony%20WH-1000XM4%20(ANC%20on)");
        assert_eq!(list[1].by, "crinacle");
        let hits = find(&list, "wh-1000xm4", 5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].by, "oratory1990");
        assert_eq!(find(&list, "airpods pro", 5)[0].name, "AirPods Pro");
        assert!(find(&list, "  ", 5).is_empty());
    }

    #[test]
    fn a_curve_fitted_to_five_bands() {
        // Flat at -10 dB (the file's preamp): nothing to change.
        let flat: Vec<(f32, f32)> = (20..20000).step_by(50).map(|f| (f as f32, -10.0)).collect();
        assert_eq!(fit(&flat), [0.0; 5]);
        // More bass, less treble: the low bands up, the high ones down.
        let tilt = parse_graphic_eq("GraphicEQ: 50 4; 60 4; 70 4; 200 2; 230 2; 260 2; 800 0; 910 0; 1000 0; 3500 -2; 3600 -2; 3700 -2; 13000 -4; 14000 -4; 15000 -4");
        assert_eq!(fit(&tilt), [4.0, 2.0, 0.0, -2.0, -4.0]);
    }

    /// The real index and a real file: `cargo test --lib autoeq_live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn autoeq_live() {
        let hits = search("WH-1000XM4", 3).await.unwrap();
        println!("{hits:?}");
        let gains = bands(&hits[0]).await.unwrap();
        println!("{gains:?}");
        assert!(gains.iter().any(|g| *g != 0.0));
    }
}
