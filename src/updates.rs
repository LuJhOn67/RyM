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
