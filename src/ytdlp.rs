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
