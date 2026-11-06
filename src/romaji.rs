//! Japanese lyrics in Latin letters (romaji), shown small under each line: Google Translate's
//! reading of the lines (it reads kanji too); when that fails, kana are turned by a table
//! (kanji stay as they are).

/// Each line in romaji (empty for a line without Japanese): one request for them all.
pub async fn romanize(lines: &[String]) -> Vec<String> {
    match reading(lines).await {
        Some(r) => r,
        None => lines.iter().map(|l| if has_kana(l) { kana(l) } else { String::new() }).collect(),
    }
}

pub fn has_kana(s: &str) -> bool {
    s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
}

fn is_japanese_char(c: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Google Translate's reading (dt=rm) of every line at once; lines stay lines.
async fn reading(lines: &[String]) -> Option<Vec<String>> {
    // Lines joined by " | ": the reading keeps that mark (new lines it turns into spaces).
    let text = lines.iter().map(|l| if l.chars().any(is_japanese_char) { l.replace(['\n', '|'], " ") } else { "-".to_string() }).collect::<Vec<_>>().join(" | ");
    let resp: serde_json::Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?
        .post("https://translate.googleapis.com/translate_a/single?client=gtx&sl=ja&tl=en&dt=rm")
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
    // [[[null, null, null, "the reading"], ...], ...]: the source's reading is the fourth item.
    let joined: String = resp.get(0)?.as_array()?.iter().filter_map(|seg| seg.get(3)?.as_str()).collect();
    let out: Vec<String> = joined.split('|').map(|s| s.trim().to_string()).collect();
    (out.len() == lines.len()).then(|| out.into_iter().zip(lines).map(|(r, orig)| if orig.chars().any(is_japanese_char) && r != "-" { r } else { String::new() }).collect())
}

/// Kana to romaji by table (Hepburn); anything else is kept.
pub fn kana(s: &str) -> String {
    let chars: Vec<char> = s.chars().map(to_hiragana).collect();
    let mut out = String::new();
    let mut i = 0;
    let mut double = false;
    while i < chars.len() {
        let c = chars[i];
        if c == 'っ' {
            double = true;
            i += 1;
            continue;
        }
        if c == 'ー' {
            if let Some(v) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                out.push(v);
            }
            i += 1;
            continue;
        }
        // A syllable with a small ya / yu / yo after it (きゃ -> kya).
        let pair: String = chars[i..chars.len().min(i + 2)].iter().collect();
        let (romaji, used) = match combo(&pair) {
            Some(r) => (r.to_string(), 2),
            None => match single(c) {
                Some(r) => (r.to_string(), 1),
                None => (c.to_string(), 1),
            },
        };
        if double && romaji.chars().next().is_some_and(|f| f.is_ascii_alphabetic() && !"aeiou".contains(f)) {
            out.push(if romaji.starts_with("ch") { 't' } else { romaji.chars().next().unwrap_or('t') });
        }
        double = false;
        // Words split at spaces the way they were written; kana between other letters joined.
        out.push_str(&romaji);
        i += used;
    }
    out
}

fn to_hiragana(c: char) -> char {
    match c {
        '\u{30a1}'..='\u{30f6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

fn combo(pair: &str) -> Option<&'static str> {
    Some(match pair {
        "きゃ" => "kya", "きゅ" => "kyu", "きょ" => "kyo", "しゃ" => "sha", "しゅ" => "shu", "しょ" => "sho",
        "ちゃ" => "cha", "ちゅ" => "chu", "ちょ" => "cho", "にゃ" => "nya", "にゅ" => "nyu", "にょ" => "nyo",
        "ひゃ" => "hya", "ひゅ" => "hyu", "ひょ" => "hyo", "みゃ" => "mya", "みゅ" => "myu", "みょ" => "myo",
        "りゃ" => "rya", "りゅ" => "ryu", "りょ" => "ryo", "ぎゃ" => "gya", "ぎゅ" => "gyu", "ぎょ" => "gyo",
        "じゃ" => "ja", "じゅ" => "ju", "じょ" => "jo", "びゃ" => "bya", "びゅ" => "byu", "びょ" => "byo",
        "ぴゃ" => "pya", "ぴゅ" => "pyu", "ぴょ" => "pyo", "てぃ" => "ti", "でぃ" => "di", "ふぁ" => "fa",
        "ふぃ" => "fi", "ふぇ" => "fe", "ふぉ" => "fo", "うぃ" => "wi", "うぇ" => "we", "しぇ" => "she",
        "ちぇ" => "che", "じぇ" => "je",
        _ => return None,
    })
}

fn single(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' => "a", 'い' => "i", 'う' => "u", 'え' => "e", 'お' => "o",
        'か' => "ka", 'き' => "ki", 'く' => "ku", 'け' => "ke", 'こ' => "ko",
        'さ' => "sa", 'し' => "shi", 'す' => "su", 'せ' => "se", 'そ' => "so",
        'た' => "ta", 'ち' => "chi", 'つ' => "tsu", 'て' => "te", 'と' => "to",
        'な' => "na", 'に' => "ni", 'ぬ' => "nu", 'ね' => "ne", 'の' => "no",
        'は' => "ha", 'ひ' => "hi", 'ふ' => "fu", 'へ' => "he", 'ほ' => "ho",
        'ま' => "ma", 'み' => "mi", 'む' => "mu", 'め' => "me", 'も' => "mo",
        'や' => "ya", 'ゆ' => "yu", 'よ' => "yo",
        'ら' => "ra", 'り' => "ri", 'る' => "ru", 'れ' => "re", 'ろ' => "ro",
        'わ' => "wa", 'を' => "wo", 'ん' => "n",
        'が' => "ga", 'ぎ' => "gi", 'ぐ' => "gu", 'げ' => "ge", 'ご' => "go",
        'ざ' => "za", 'じ' => "ji", 'ず' => "zu", 'ぜ' => "ze", 'ぞ' => "zo",
        'だ' => "da", 'ぢ' => "ji", 'づ' => "zu", 'で' => "de", 'ど' => "do",
        'ば' => "ba", 'び' => "bi", 'ぶ' => "bu", 'べ' => "be", 'ぼ' => "bo",
        'ぱ' => "pa", 'ぴ' => "pi", 'ぷ' => "pu", 'ぺ' => "pe", 'ぽ' => "po",
        'ぁ' => "a", 'ぃ' => "i", 'ぅ' => "u", 'ぇ' => "e", 'ぉ' => "o",
        'ゃ' => "ya", 'ゅ' => "yu", 'ょ' => "yo", 'ゔ' => "vu",
        '　' => " ", '、' => ", ", '。' => ". ", '「' => "\"", '」' => "\"",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana_by_table() {
        assert_eq!(kana("きみのなまえ"), "kiminonamae");
        assert_eq!(kana("トーキョー"), "tookyoo");
        assert_eq!(kana("きって"), "kitte");
        assert_eq!(kana("まっちゃ"), "matcha");
        assert_eq!(kana("夜にかける"), "夜nikakeru");
        assert!(has_kana("夜に駆ける") && !has_kana("夜駆"));
    }

    /// Live: Google's reading of a line with kanji (`cargo test --lib live_romaji -- --ignored --nocapture`).
    #[tokio::test]
    #[ignore]
    async fn live_romaji() {
        let lines = vec!["夜に駆ける".to_string(), "Oh yeah".to_string(), "沈むように溶けてゆくように".to_string()];
        let r = romanize(&lines).await;
        println!("{r:?}");
        assert_eq!(r.len(), 3);
        assert!(r[1].is_empty());
        assert!(r[0].to_lowercase().contains("yoru"), "{r:?}");
    }
}
