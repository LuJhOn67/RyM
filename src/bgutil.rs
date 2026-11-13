//! Proof-of-origin tokens (PO Tokens) from bgutil-ytdlp-pot-provider
//! (github.com/Brainicism/bgutil-ytdlp-pot-provider, GPL-3.0), run as a separate program: never
//! built into RyM. YouTube sometimes wants one before it gives an audio link (more so without
//! Premium).
//!
//! - Script mode: RyM runs bgutil's script with Node.js only when it needs a token, and keeps
//!   each token for five hours (bgutil keeps them six), so Node isn't started for every song.
//!   Nothing stays running.
//! - Its folder is bgutil's own default (`~/bgutil-ytdlp-pot-provider`), where yt-dlp's bgutil
//!   plugin finds it too.
//! - Installing (from the settings, when you press it): git clone, `npm ci`, `npx tsc`, and the
//!   yt-dlp plugin with pip. Needs Node.js 22 or newer and git.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};

/// The bgutil release RyM installs.
const VERSION: &str = "2.0.2";
const REPO: &str = "https://github.com/Brainicism/bgutil-ytdlp-pot-provider.git";
/// How long a token is used before a new one is made (bgutil's own cache keeps them 6 h).
const KEEP: Duration = Duration::from_secs(5 * 60 * 60);
/// A token that couldn't be made: no new try for this long (so a missing Node doesn't slow every song).
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

fn home() -> PathBuf {
    let base = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    base.join("bgutil-ytdlp-pot-provider")
}

fn script() -> PathBuf {
    home().join("server").join("build").join("generate_once.js")
}

/// bgutil's script is built and ready.
pub fn installed() -> bool {
    script().is_file()
}

fn command(program: &str) -> tokio::process::Command {
    // npm and npx are .cmd files on Windows.
    let program = if cfg!(windows) && (program == "npm" || program == "npx") { format!("{program}.cmd") } else { program.to_string() };
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Tokens made, by what they're bound to (a video id, or the visitor data), and when.
fn tokens() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static T: std::sync::OnceLock<Mutex<HashMap<String, (String, Instant)>>> = std::sync::OnceLock::new();
    T.get_or_init(Default::default)
}

static FAILED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// The token in the script's answer (its last line: `{"poToken": "...", ...}`).
fn read_token(out: &str) -> Option<String> {
    let line = out.lines().rev().find(|l| l.trim_start().starts_with('{'))?;
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v["poToken"].as_str().filter(|t| !t.is_empty()).map(str::to_string)
}

/// A PO Token bound to `binding`, from bgutil's script; None when bgutil isn't installed or
/// couldn't make one.
pub async fn token(binding: &str) -> Option<String> {
    if !installed() || binding.is_empty() {
        return None;
    }
    if let Some((t, at)) = tokens().lock().ok()?.get(binding).cloned()
        && at.elapsed() < KEEP
    {
        return Some(t);
    }
    if FAILED_AT.lock().ok()?.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
        return None;
    }
    let run = command("node").arg(script()).args(["-c", binding]).kill_on_drop(true).output();
    let made = match tokio::time::timeout(Duration::from_secs(40), run).await {
        Ok(Ok(out)) if out.status.success() => read_token(&String::from_utf8_lossy(&out.stdout)),
        _ => None,
    };
    match &made {
        Some(t) => {
            if let Ok(mut m) = tokens().lock() {
                m.insert(binding.to_string(), (t.clone(), Instant::now()));
            }
        }
        None => {
            if let Ok(mut f) = FAILED_AT.lock() {
                *f = Some(Instant::now());
            }
        }
    }
    made
}

async fn run(cmd: &mut tokio::process::Command, what: &str) -> Result<()> {
    let out = cmd.output().await.map_err(|e| anyhow!("{what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Err(anyhow!("{what}: {}", why(&text)))
}

/// The line that says what went wrong: an "Error: ..." line, not npm's "a complete log of this
/// run can be found in ..." at the end.
fn why(text: &str) -> &str {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let useful = |l: &&&str| !l.to_lowercase().contains("complete log") && !l.to_lowercase().contains("verbose");
    lines
        .iter()
        .filter(useful)
        .rfind(|l| l.contains("Error:") || l.contains("ERR!") || l.starts_with("npm error") && !l.ends_with("error"))
        .or_else(|| lines.iter().rfind(useful))
        .copied()
        .unwrap_or("failed")
}

/// Installs bgutil (or brings it to RyM's version) and the yt-dlp plugin; what happened, to show.
pub async fn install() -> Result<String> {
    let dir = home();
    if dir.join(".git").is_dir() {
        run(command("git").current_dir(&dir).args(["fetch", "--depth", "1", "origin", "tag", VERSION]), "git fetch").await?;
        run(command("git").current_dir(&dir).args(["checkout", "-q", VERSION]), "git checkout").await?;
    } else {
        run(command("git").args(["clone", "-q", "--depth", "1", "--branch", VERSION, REPO]).arg(&dir), "git clone").await?;
    }
    let server = dir.join("server");
    // A clean start: a half-done earlier install (a library without its own helpers) made the
    // next `npm ci` fail. If it fails anyway, `npm install` works the lock file out again.
    let _ = std::fs::remove_dir_all(server.join("node_modules"));
    if let Err(first) = run(command("npm").current_dir(&server).args(["ci", "--no-audit", "--no-fund"]), "npm ci").await {
        let _ = std::fs::remove_dir_all(server.join("node_modules"));
        run(command("npm").current_dir(&server).args(["install", "--no-audit", "--no-fund"]), "npm install").await.map_err(|e| anyhow!("{first}; {e}"))?;
    }
    run(command("npx").current_dir(&server).arg("tsc"), "npx tsc").await?;
    // The plugin lets yt-dlp use it too (it looks in the same folder).
    let mut plugin = Err(anyhow!("Python wasn't found"));
    for python in if cfg!(windows) { ["python", "py", "python3"] } else { ["python3", "python", "py"] } {
        if command(python).arg("--version").output().await.is_ok_and(|o| o.status.success()) {
            plugin = run(command(python).args(["-m", "pip", "install", "-U", "bgutil-ytdlp-pot-provider"]), "pip").await;
            break;
        }
    }
    if let Ok(mut f) = FAILED_AT.lock() {
        *f = None;
    }
    Ok(match plugin {
        Ok(()) => format!("bgutil {VERSION} installed, with yt-dlp's plugin"),
        Err(e) => format!("bgutil {VERSION} installed (yt-dlp's plugin: {e})"),
    })
}

/// What's there, for the settings.
pub fn status() -> String {
    if installed() { format!("bgutil {VERSION} (script; used when YouTube asks for a token)") } else { "Not installed (needs Node.js 22+ and git)".into() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn says_what_went_wrong() {
        let npm = "npm error code 1\nnpm error path C:\\x\\node_modules\\canvas\nnpm error Error: Cannot find module 'prebuild-install\\bin.js'\nnpm error A complete log of this run can be found in: C:\\logs\\debug-0.log\n";
        assert_eq!(super::why(npm), "npm error Error: Cannot find module 'prebuild-install\\bin.js'");
        assert_eq!(super::why("one\ntwo\n"), "two");
        assert_eq!(super::why(""), "failed");
    }

    #[test]
    fn reads_the_scripts_answer() {
        let out = "Generating POT for abc\n{\"poToken\":\"MnQ_tok\",\"contentBinding\":\"abc\",\"expiresAt\":\"2026-11-12T03:00:00.000Z\"}\n";
        assert_eq!(super::read_token(out).as_deref(), Some("MnQ_tok"));
        assert_eq!(super::read_token("{}\n"), None, "a failed run prints {{}}");
        assert_eq!(super::read_token("nothing"), None);
    }
}
