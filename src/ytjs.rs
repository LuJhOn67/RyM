//! YouTube's player script: the signature (`s`) and throttling (`n`) puzzles that the TV and web
//! clients' links need solved before they play, and the script's date (`signatureTimestamp`) that
//! those clients send with their request.
//!
//! The puzzles are solved by yt-dlp's own solver (github.com/yt-dlp/ejs, Unlicense; its bundled
//! parsers are ISC and MIT, see `assets/ejs/LICENSE`) run in Node.js. Without Node these clients are
//! skipped and the app clients (no puzzles) do the work as before.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

const LIB: &str = include_str!("../assets/ejs/lib.min.js");
const CORE: &str = include_str!("../assets/ejs/core.min.js");

/// The player script's id is looked up again after this long (YouTube changes it now and then).
const PLAYER_FOR: Duration = Duration::from_secs(6 * 3600);

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("http client"))
}

/// Node.js, when it's installed (on the PATH or in its usual folder).
pub fn node() -> Option<PathBuf> {
    static NODE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    NODE.get_or_init(|| {
        let name = if cfg!(windows) { "node.exe" } else { "node" };
        let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()));
        on_path.or_else(|| {
            let usual: &[&str] = if cfg!(windows) { &[r"C:\Program Files\nodejs\node.exe"] } else { &["/usr/bin/node", "/usr/local/bin/node"] };
            usual.iter().map(PathBuf::from).find(|f| f.is_file())
        })
    })
    .clone()
}

/// The player script now in use: its id, its text, its date, and what the solver made of it
/// (kept so the slow first reading happens once per script).
struct Player {
    id: String,
    looked: Instant,
    code: String,
    sts: u64,
    prepared: Option<String>,
}

fn state() -> &'static tokio::sync::Mutex<Option<Player>> {
    static P: std::sync::OnceLock<tokio::sync::Mutex<Option<Player>>> = std::sync::OnceLock::new();
    P.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn cache_dir() -> PathBuf {
    crate::data_dir().join("cache").join("player")
}

/// The player script's id, from the page every embedded player loads.
async fn player_id() -> Result<String> {
    let page = http().get("https://www.youtube.com/iframe_api").send().await?.error_for_status()?.text().await?;
    let at = page.find("player\\/").map(|i| i + 8).or_else(|| page.find("player/").map(|i| i + 7)).ok_or_else(|| anyhow!("no player id"))?;
    let id: String = page[at..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if id.len() < 8 {
        bail!("no player id");
    }
    Ok(id)
}

/// The script's date, which the TV and web clients send so the links match the script.
fn sts_of(code: &str) -> Option<u64> {
    ["signatureTimestamp", "sts"].iter().find_map(|key| {
        let mut rest = code;
        while let Some(i) = rest.find(key) {
            let after = rest[i + key.len()..].trim_start();
            if let Some(num) = after.strip_prefix(':') {
                let digits: String = num.trim_start().chars().take_while(char::is_ascii_digit).collect();
                if digits.len() == 5 {
                    return digits.parse().ok();
                }
            }
            rest = &rest[i + key.len()..];
        }
        None
    })
}

/// Makes sure the current player script is loaded (from the disk if it was fetched before).
async fn ready(p: &mut Option<Player>) -> Result<()> {
    if p.as_ref().is_some_and(|p| p.looked.elapsed() < PLAYER_FOR) {
        return Ok(());
    }
    let id = player_id().await?;
    if p.as_ref().is_some_and(|p| p.id == id) {
        if let Some(p) = p.as_mut() {
            p.looked = Instant::now();
        }
        return Ok(());
    }
    let dir = cache_dir();
    let file = dir.join(format!("{id}.js"));
    let code = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => {
            let url = format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js");
            let c = http().get(url).send().await?.error_for_status()?.text().await?;
            let _ = std::fs::create_dir_all(&dir);
            // Older scripts go: one is enough.
            if let Ok(old) = std::fs::read_dir(&dir) {
                for e in old.flatten() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            let _ = std::fs::write(&file, &c);
            c
        }
    };
    let sts = sts_of(&code).ok_or_else(|| anyhow!("no signature date in the player script"))?;
    let prepared = std::fs::read_to_string(dir.join(format!("{id}.prepared.js"))).ok();
    *p = Some(Player { id, looked: Instant::now(), code, sts, prepared });
    Ok(())
}

/// The player script's date (`signatureTimestamp`).
pub async fn signature_timestamp() -> Result<u64> {
    let mut p = state().lock().await;
    ready(&mut p).await?;
    Ok(p.as_ref().map(|p| p.sts).unwrap_or_default())
}

/// Answers by (player script, 's' or 'n', puzzle).
type Answers = HashMap<(String, char, String), String>;

/// Answers already found: the same puzzles come back for every song of a session.
fn answers() -> &'static Mutex<Answers> {
    static A: std::sync::OnceLock<Mutex<Answers>> = std::sync::OnceLock::new();
    A.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Solves the signature puzzles `sig` and the throttling puzzles `n`; the answers come back in
/// the same order.
pub async fn solve(sig: &[String], n: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let node = node().ok_or_else(|| anyhow!("Node.js isn't installed"))?;
    let mut p = state().lock().await;
    ready(&mut p).await?;
    let player = p.as_mut().ok_or_else(|| anyhow!("no player script"))?;
    let known = |kind: char, c: &String| answers().lock().ok().and_then(|a| a.get(&(player.id.clone(), kind, c.clone())).cloned());
    let need_sig: Vec<String> = sig.iter().filter(|c| known('s', c).is_none()).cloned().collect();
    let need_n: Vec<String> = n.iter().filter(|c| known('n', c).is_none()).cloned().collect();
    if !need_sig.is_empty() || !need_n.is_empty() {
        let mut requests = Vec::new();
        if !need_sig.is_empty() {
            requests.push(json!({ "type": "sig", "challenges": need_sig }));
        }
        if !need_n.is_empty() {
            requests.push(json!({ "type": "n", "challenges": need_n }));
        }
        let input = match &player.prepared {
            Some(pre) => json!({ "type": "preprocessed", "preprocessed_player": pre, "requests": requests }),
            None => json!({ "type": "player", "player": player.code, "requests": requests, "output_preprocessed": true }),
        };
        let script = format!("{LIB}\nObject.assign(globalThis, lib);\n{CORE}\nconsole.log(JSON.stringify(jsc({input})));\n");
        let out = run_node(&node, script).await?;
        let v: Value = serde_json::from_str(out.trim()).context("the solver's answer")?;
        if v["type"] == "error" {
            bail!("solver: {}", v["error"].as_str().unwrap_or("failed"));
        }
        if let Some(pre) = v["preprocessed_player"].as_str() {
            let _ = std::fs::write(cache_dir().join(format!("{}.prepared.js", player.id)), pre);
            player.prepared = Some(pre.to_string());
        }
        let responses = v["responses"].as_array().cloned().unwrap_or_default();
        for (req, resp) in requests.iter().zip(responses) {
            if resp["type"] != "result" {
                bail!("solver: {}", resp["error"].as_str().unwrap_or("failed"));
            }
            let kind = if req["type"] == "sig" { 's' } else { 'n' };
            if let (Some(data), Ok(mut a)) = (resp["data"].as_object(), answers().lock()) {
                for (c, answer) in data {
                    if let Some(answer) = answer.as_str() {
                        a.insert((player.id.clone(), kind, c.clone()), answer.to_string());
                    }
                }
            }
        }
    }
    let get = |kind: char, list: &[String]| -> Result<Vec<String>> { list.iter().map(|c| known(kind, c).ok_or_else(|| anyhow!("unsolved {kind} puzzle"))).collect() };
    Ok((get('s', sig)?, get('n', n)?))
}

async fn run_node(node: &PathBuf, script: String) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut cmd = tokio::process::Command::new(node);
    cmd.arg("-").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().context("starting Node.js")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).await?;
    }
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await.map_err(|_| anyhow!("Node.js took too long"))??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("Node.js: {}", err.lines().last().unwrap_or("failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A stream's final link: the signature put in (when the link came as a cipher) and the
/// throttling value replaced by its answer.
pub async fn unlock(format: &Value) -> Result<String> {
    let (mut url, sig) = match format["url"].as_str() {
        Some(u) => (u.to_string(), None),
        None => {
            let cipher = format["signatureCipher"].as_str().ok_or_else(|| anyhow!("no link"))?;
            let parts: HashMap<String, String> = url::form_urlencoded::parse(cipher.as_bytes()).into_owned().collect();
            let url = parts.get("url").cloned().ok_or_else(|| anyhow!("no link in the cipher"))?;
            let s = parts.get("s").cloned().ok_or_else(|| anyhow!("no signature in the cipher"))?;
            let sp = parts.get("sp").cloned().unwrap_or_else(|| "signature".into());
            (url, Some((s, sp)))
        }
    };
    let n = url::Url::parse(&url)?.query_pairs().find(|(k, _)| k == "n").map(|(_, v)| v.into_owned());
    let sigs: Vec<String> = sig.iter().map(|(s, _)| s.clone()).collect();
    let ns: Vec<String> = n.iter().cloned().collect();
    let (sig_answer, n_answer) = solve(&sigs, &ns).await?;
    let mut parsed = url::Url::parse(&url)?;
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| if k == "n" { (k.into_owned(), n_answer.first().cloned().unwrap_or_else(|| v.into_owned())) } else { (k.into_owned(), v.into_owned()) })
        .collect();
    {
        let mut q = parsed.query_pairs_mut();
        q.clear();
        for (k, v) in &pairs {
            q.append_pair(k, v);
        }
        if let (Some((_, sp)), Some(answer)) = (&sig, sig_answer.first()) {
            q.append_pair(sp, answer);
        }
    }
    url = parsed.to_string();
    Ok(url)
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_scripts_date() {
        assert_eq!(super::sts_of("x={a:1,signatureTimestamp:20391,b:2}"), Some(20391));
        assert_eq!(super::sts_of("var c={sts:20123};"), Some(20123));
        assert_eq!(super::sts_of("sts:1"), None);
    }

    /// Fetches the real player script and solves a made-up puzzle of each kind:
    /// `cargo test --lib ytjs -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_solve() {
        println!("sts {}", super::signature_timestamp().await.unwrap());
        let t = std::time::Instant::now();
        let (s, n) = super::solve(&["0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz".into()], &["Rrxi8ylaFdlr9lcwV".into()]).await.unwrap();
        println!("sig {} chars, n {:?}, {:?}", s[0].len(), n, t.elapsed());
        let t = std::time::Instant::now();
        let _ = super::solve(&[], &["AAAAAAAAAAAAAAAAB".into()]).await.unwrap();
        println!("second solve {:?}", t.elapsed());
    }
}
