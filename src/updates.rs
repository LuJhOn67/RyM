//! Update notices: the newest release of RyM on GitHub, compared with this build. Pre-releases
//! count only on the beta channel (Settings > Beta updates).

use std::time::Duration;

use anyhow::{Result, anyhow};
use serde_json::Value;

pub const REPO: &str = "LuJhOn67/RyM";
/// How often the apps look on their own (they also look at start when it's been this long).
pub const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);

/// A release newer than this build.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// "6.8"
    pub version: String,
    pub title: String,
    /// The patch notes (Markdown).
    pub notes: String,
    /// A pre-release (beta channel).
    pub beta: bool,
    /// The release's page.
    pub page: String,
    /// Its files: (name, download link).
    pub files: Vec<(String, String)>,
}

impl Release {
    /// The file this program would update from: the GTK zip, the terminal exe, the older window
    /// app, or the APK; the page when there's no such file.
    pub fn download_for(&self, file: &str) -> String {
        self.files.iter().find(|(name, _)| name == file).map(|(_, url)| url.clone()).unwrap_or_else(|| self.page.clone())
    }

    /// The notes as plain lines for places without Markdown (the terminal): marks dropped.
    pub fn plain_notes(&self) -> Vec<String> {
        self.notes
            .lines()
            .map(|l| {
                let l = l.trim_end();
                let l = l.trim_start_matches('#').trim_start_matches('>').trim_start();
                l.replace("**", "").replace('`', "")
            })
            .filter(|l| !l.starts_with("|---"))
            .collect()
    }
}

/// "v6.7", "6.7.0", "v0.7.0-preview" -> [6, 7], [6, 7, 0], [0, 7, 0]. Not a version: empty.
pub fn parse_version(s: &str) -> Vec<u32> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("");
    let parts: Vec<u32> = core.split('.').map_while(|p| p.parse().ok()).collect();
    // Trailing zeros don't count: 6.7 and 6.7.0 are the same.
    let mut parts = parts;
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    parts
}

/// `a` is a later version than `b`.
pub fn newer(a: &str, b: &str) -> bool {
    let (a, b) = (parse_version(a), parse_version(b));
    !a.is_empty() && a > b
}

/// The newest release in GitHub's list that is newer than `current` (pre-releases only on the
/// beta channel; drafts and tags that aren't versions are skipped).
pub fn pick(releases: &[Value], beta: bool, current: &str) -> Option<Release> {
    releases
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| beta || !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            // Only the app's own "vX.Y" tags (old android-v… pages are something else).
            tag.starts_with('v').then_some(())?;
            let version = tag.trim_start_matches('v').to_string();
            Some((parse_version(&version), r, version))
        })
        .filter(|(v, _, _)| !v.is_empty())
        .max_by(|a, b| a.0.cmp(&b.0))
        .filter(|(_, _, version)| newer(version, current))
        .map(|(_, r, version)| Release {
            version,
            title: r["name"].as_str().unwrap_or_default().to_string(),
            notes: r["body"].as_str().unwrap_or_default().to_string(),
            beta: r["prerelease"].as_bool().unwrap_or(false),
            page: r["html_url"].as_str().unwrap_or_default().to_string(),
            files: r["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| Some((a["name"].as_str()?.to_string(), a["browser_download_url"].as_str()?.to_string())))
                .collect(),
        })
}

pub(crate) fn github() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("RyM/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("http client")
    })
}

/// A release newer than this build, if there is one.
pub async fn check(beta: bool) -> Result<Option<Release>> {
    let list: Value = github()
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=30"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow!("GitHub: {e}"))?
        .json()
        .await?;
    Ok(pick(list.as_array().map(Vec::as_slice).unwrap_or_default(), beta, crate::VERSION))
}

/// Seconds since 1970, for "last checked" times kept in the settings.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Time for the regular look (auto checks on, and the last one long enough ago).
pub fn due(auto: bool, last_checked: u64) -> bool {
    auto && now().saturating_sub(last_checked) >= CHECK_EVERY.as_secs()
}

// ---------------------------------------------------------------- updating in place

/// Downloads a release file (an app's .exe, or the GTK app's zip) and puts it in place of this
/// program and the files beside it. A running program (and the libraries it loaded) can be
/// renamed on Windows, not overwritten: each file replaced is renamed to `<name>.old` first, and
/// those go at the next start ([`clean_old`]). `progress` gets 0 to 1 as it downloads. Returns
/// the program to start again.
pub async fn install(url: &str, progress: impl Fn(f32) + Send + 'static) -> Result<std::path::PathBuf> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("RyM/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()?;
    let mut resp = http.get(url).send().await?.error_for_status().map_err(|e| anyhow!("GitHub: {e}"))?;
    let total = resp.content_length().unwrap_or(0) as f32;
    let mut data = Vec::with_capacity(total as usize);
    while let Some(chunk) = resp.chunk().await? {
        data.extend_from_slice(&chunk);
        if total > 0.0 {
            progress((data.len() as f32 / total).min(1.0));
        }
    }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| anyhow!("no folder for the program"))?.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf> {
        if data.starts_with(b"PK\x03\x04") {
            let files = unzip(&data)?;
            // The zip holds a folder ("RyM-GTK/..."): its insides go beside this program.
            let top = files.first().and_then(|(n, _)| n.split('/').next()).map(str::to_string);
            let strip = top.filter(|t| files.iter().all(|(n, _)| n.starts_with(&format!("{t}/"))));
            for (name, bytes) in &files {
                let rel = match &strip {
                    Some(t) => name[t.len() + 1..].to_string(),
                    None => name.clone(),
                };
                if rel.is_empty() || rel.split('/').any(|p| p == "..") {
                    continue;
                }
                let target = dir.join(&rel);
                // Unchanged files (the GTK runtime) are left alone.
                if std::fs::read(&target).is_ok_and(|old| old == *bytes) {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                replace(&target, bytes)?;
            }
        } else if data.starts_with(b"MZ") {
            replace(&exe, &data)?;
        } else {
            return Err(anyhow!("the download isn't a program or a zip"));
        }
        Ok(exe)
    })
    .await?
}

/// Puts `bytes` at `path`, the old file (maybe running) renamed out of the way first.
fn replace(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let fresh = path.with_extension("rym-new");
    std::fs::write(&fresh, bytes)?;
    if path.exists() {
        let mut n = 0;
        loop {
            let old = std::path::PathBuf::from(format!("{}.old{}", path.display(), if n == 0 { String::new() } else { n.to_string() }));
            let _ = std::fs::remove_file(&old);
            match std::fs::rename(path, &old) {
                Ok(()) => break,
                Err(_) if n < 9 => n += 1,
                Err(e) => return Err(e.into()),
            }
        }
    }
    std::fs::rename(&fresh, path)?;
    Ok(())
}

/// The files an update put aside (`*.old`, `*.old1`...), deleted once nothing runs them.
pub fn clean_old() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return };
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                stack.push(p);
            } else if let Some(tail) = name.rsplit_once(".old").map(|(_, t)| t)
                && tail.chars().all(|c| c.is_ascii_digit())
            {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// The files in a zip (stored or deflated), read from its central directory.
fn unzip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let u16_at = |i: usize| -> Result<usize> { data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(|| anyhow!("broken zip")) };
    let u32_at = |i: usize| -> Result<usize> { data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(|| anyhow!("broken zip")) };
    let end = (0..data.len().saturating_sub(21)).rev().find(|&i| data[i..].starts_with(b"PK\x05\x06")).ok_or_else(|| anyhow!("not a zip"))?;
    let count = u16_at(end + 10)?;
    let mut at = u32_at(end + 16)?;
    let mut out = Vec::new();
    for _ in 0..count {
        if !data.get(at..).is_some_and(|d| d.starts_with(b"PK\x01\x02")) {
            return Err(anyhow!("broken zip directory"));
        }
        let method = u16_at(at + 10)?;
        let packed_len = u32_at(at + 20)?;
        let size = u32_at(at + 24)?;
        let name_len = u16_at(at + 28)?;
        let extra_len = u16_at(at + 30)?;
        let comment_len = u16_at(at + 32)?;
        let local = u32_at(at + 42)?;
        let name = String::from_utf8_lossy(data.get(at + 46..at + 46 + name_len).ok_or_else(|| anyhow!("broken zip"))?).replace('\\', "/");
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let packed = data.get(start..start + packed_len).ok_or_else(|| anyhow!("broken zip entry"))?;
        let bytes = match method {
            0 => packed.to_vec(),
            8 => {
                use std::io::Read;
                let mut v = Vec::with_capacity(size);
                flate2::read::DeflateDecoder::new(packed).read_to_end(&mut v)?;
                v
            }
            m => return Err(anyhow!("zip method {m} isn't supported")),
        };
        out.push((name, bytes));
    }
    Ok(out)
}

/// Opens a link in the default browser (downloads, release pages).
pub fn open_link(url: &str) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // rundll32 takes the link as is (`start` would cut it at the first &).
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).creation_flags(0x0800_0000).spawn()?;
    }
    #[cfg(not(windows))]
    std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(url).spawn()?;
    Ok(())
}

#[cfg(test)]
mod live {
    /// GitHub's real list, read by a 6.6 app and by this one: prints what each would be offered.
    #[tokio::test]
    #[ignore]
    async fn offers_from_github() {
        let list: serde_json::Value = super::github()
            .get(format!("https://api.github.com/repos/{}/releases?per_page=30", super::REPO))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let list = list.as_array().unwrap();
        let old = super::pick(list, false, "6.6").expect("6.6 should be offered an update");
        println!("6.6 is offered {} ({} files, notes {} lines, zip {})", old.version, old.files.len(), old.plain_notes().len(), old.download_for("RyM-GTK-Windows.zip"));
        println!("{} is offered {:?}", crate::VERSION, super::pick(list, false, crate::VERSION).map(|r| r.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(parse_version("v6.7"), vec![6, 7]);
        assert_eq!(parse_version("6.7.0"), vec![6, 7]);
        assert_eq!(parse_version("v0.7.0-preview"), vec![0, 7]);
        assert!(newer("6.10", "6.9"));
        assert!(newer("v6.8", "6.7"));
        assert!(!newer("6.7.0", "6.7"));
        assert!(!newer("6.5", "6.7"));
        assert!(!newer("android-v9.0", "6.7"));
    }

    #[test]
    fn betas_only_on_the_beta_channel() {
        let list = vec![
            json!({ "tag_name": "v6.9", "prerelease": true, "draft": false, "name": "RyM 6.9", "body": "beta", "html_url": "p9", "assets": [] }),
            json!({ "tag_name": "v6.8", "prerelease": false, "draft": false, "name": "RyM 6.8", "body": "## RyM 6.8\n- **New:** `x`", "html_url": "p8",
                     "assets": [{ "name": "RyM.exe", "browser_download_url": "d8" }] }),
            json!({ "tag_name": "v7.0", "prerelease": false, "draft": true, "name": "draft", "body": "", "html_url": "p", "assets": [] }),
            json!({ "tag_name": "android-v9.0", "prerelease": false, "draft": false, "name": "old", "body": "", "html_url": "p", "assets": [] }),
        ];
        let stable = pick(&list, false, "6.7").unwrap();
        assert_eq!((stable.version.as_str(), stable.beta), ("6.8", false));
        assert_eq!(stable.download_for("RyM.exe"), "d8");
        assert_eq!(stable.download_for("missing.zip"), "p8");
        assert_eq!(stable.plain_notes(), vec!["RyM 6.8".to_string(), "- New: x".to_string()]);
        let beta = pick(&list, true, "6.7").unwrap();
        assert_eq!((beta.version.as_str(), beta.beta), ("6.9", true));
        // Already up to date.
        assert!(pick(&list, true, "6.9").is_none());
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    /// A zip like Compress-Archive makes: a folder, one stored file, one deflated.
    fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use std::io::Write;
        let (mut body, mut dir) = (Vec::new(), Vec::new());
        for (name, data, deflate) in files {
            let packed = if *deflate {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = body.len() as u32;
            body.extend_from_slice(b"PK\x03\x04");
            body.extend_from_slice(&[20, 0, 0, 0]);
            body.extend_from_slice(&method.to_le_bytes());
            body.extend_from_slice(&[0; 8]);
            body.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&(name.len() as u16).to_le_bytes());
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&packed);
            dir.extend_from_slice(b"PK\x01\x02");
            dir.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            dir.extend_from_slice(&method.to_le_bytes());
            dir.extend_from_slice(&[0; 8]);
            dir.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            dir.extend_from_slice(&(data.len() as u32).to_le_bytes());
            dir.extend_from_slice(&(name.len() as u16).to_le_bytes());
            dir.extend_from_slice(&[0; 12]);
            dir.extend_from_slice(&offset.to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
        }
        let (dir_at, dir_len, n) = (body.len() as u32, dir.len() as u32, files.len() as u16);
        body.extend_from_slice(&dir);
        body.extend_from_slice(b"PK\x05\x06");
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&n.to_le_bytes());
        body.extend_from_slice(&n.to_le_bytes());
        body.extend_from_slice(&dir_len.to_le_bytes());
        body.extend_from_slice(&dir_at.to_le_bytes());
        body.extend_from_slice(&[0, 0]);
        body
    }

    #[test]
    fn unzips_and_replaces_a_running_file() {
        let text = b"hello hello hello hello, the GTK runtime".repeat(20);
        let files = unzip(&zip(&[("RyM-GTK/RymPlayer-GTK.exe", b"MZ new program", false), ("RyM-GTK/lib/x.dll", &text, true)])).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], ("RyM-GTK/RymPlayer-GTK.exe".to_string(), b"MZ new program".to_vec()));
        assert_eq!(files[1].1, text);
        // Replacing: the old file is put aside, the new one takes its place.
        let dir = std::env::temp_dir().join(format!("rym-install-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("app.exe");
        std::fs::write(&target, b"MZ old").unwrap();
        replace(&target, b"MZ new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"MZ new");
        assert_eq!(std::fs::read(dir.join("app.exe.old")).unwrap(), b"MZ old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real GTK zip of the latest release unzips to the same files as on disk:
    /// `cargo test --lib real_release_zip -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_release_zip() {
        let r = pick(&github().get(format!("https://api.github.com/repos/{REPO}/releases?per_page=5")).send().await.unwrap().json::<Value>().await.unwrap().as_array().unwrap().clone(), false, "0.0").unwrap();
        let url = r.download_for("RyM-GTK-Windows.zip");
        let data = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
        let files = unzip(&data).unwrap();
        let exe = files.iter().find(|(n, _)| n.ends_with("RymPlayer-GTK.exe")).unwrap();
        println!("{} files from {}; the program is {} bytes and starts with MZ: {}", files.len(), r.version, exe.1.len(), exe.1.starts_with(b"MZ"));
    }
}
