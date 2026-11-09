//! yt-dlp kept up to date from the settings: its own updater (stable or nightly builds), or pip
//! when it was installed with pip (its updater refuses then).

use anyhow::{Result, anyhow};

fn command(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

fn last_line(out: &std::process::Output) -> String {
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or_default().to_string()
}

/// Updates yt-dlp; what it said in the end ("yt-dlp is up to date"...).
pub async fn update(nightly: bool) -> Result<String> {
    let channel = if nightly { "nightly" } else { "stable" };
    let out = command("yt-dlp").args(["--update-to", channel]).output().await.map_err(|_| anyhow!("yt-dlp isn't installed"))?;
    let all = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if all.contains("pip") {
        return pip(nightly).await;
    }
    Ok(last_line(&out))
}

async fn pip(nightly: bool) -> Result<String> {
    let mut args = vec!["-m", "pip", "install", "-U"];
    if nightly {
        args.push("--pre");
    }
    args.push("yt-dlp[default]");
    for python in ["python", "py", "python3"] {
        if let Ok(out) = command(python).args(&args).output().await {
            return if out.status.success() { Ok("yt-dlp updated (pip)".into()) } else { Err(anyhow!("pip: {}", last_line(&out))) };
        }
    }
    Err(anyhow!("yt-dlp was installed with pip, and Python wasn't found"))
}

/// What's installed, for the settings: "yt-dlp 2026.10.26 (pip) · ejs 0.8.0 · Node 24.20.0", with
/// what's missing said plainly.
pub async fn status() -> String {
    // A search for nothing: yt-dlp prints its versions and goes nowhere.
    let out = command("yt-dlp").args(["-v", "--js-runtimes", "node", "-s", "--no-warnings", "ytsearch0:rym"]).output().await;
    let Ok(out) = out else { return "yt-dlp isn't installed (pip install -U \"yt-dlp[default]\")".into() };
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    parse_status(&text)
}

fn parse_status(text: &str) -> String {
    let line = |key: &str| text.lines().find_map(|l| l.split_once(key).map(|(_, rest)| rest.trim().to_string()));
    let version = line("yt-dlp version").map(|v| {
        let mut words = v.split_whitespace();
        let ver = words.next().unwrap_or_default().rsplit('@').next().unwrap_or_default().to_string();
        let how = v.rsplit_once('(').map(|(_, h)| h.trim_end_matches(')').to_string()).unwrap_or_default();
        if how.is_empty() { ver } else { format!("{ver} ({how})") }
    });
    let ejs = line("Optional libraries:").and_then(|l| l.split(',').map(str::trim).find_map(|p| p.strip_prefix("yt_dlp_ejs-").map(str::to_string)));
    let node = line("JS runtimes:").and_then(|l| l.split(',').map(str::trim).find_map(|p| p.strip_prefix("node-").map(str::to_string)));
    let mut parts = vec![version.map_or("yt-dlp: version unknown".to_string(), |v| format!("yt-dlp {v}"))];
    parts.push(ejs.map_or("ejs missing (pip install -U yt-dlp-ejs)".to_string(), |v| format!("ejs {v}")));
    parts.push(node.map_or("Node.js missing (nodejs.org)".to_string(), |v| format!("Node {v}")));
    parts.join(" · ")
}

/// A week since yt-dlp was last updated (the automatic weekly update).
pub fn update_due(last: u64, now: u64) -> bool {
    now.saturating_sub(last) >= 7 * 24 * 3600
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_what_is_installed() {
        let text = "[debug] yt-dlp version stable@2026.10.26 from yt-dlp/yt-dlp [c91f8de0b] (pip)\n[debug] Optional libraries: sqlite3-3.49.1, yt_dlp_ejs-0.8.0\n[debug] JS runtimes: node-24.20.0\n";
        assert_eq!(super::parse_status(text), "yt-dlp 2026.10.26 (pip) · ejs 0.8.0 · Node 24.20.0");
        let bare = "[debug] yt-dlp version stable@2026.10.26 from yt-dlp/yt-dlp [c91f8de0b] (win_exe)\n[debug] JS runtimes: none\n";
        assert_eq!(super::parse_status(bare), "yt-dlp 2026.10.26 (win_exe) · ejs missing (pip install -U yt-dlp-ejs) · Node.js missing (nodejs.org)");
    }

    #[test]
    fn weekly() {
        assert!(super::update_due(0, 8 * 24 * 3600));
        assert!(!super::update_due(100, 100 + 3600));
    }
}
