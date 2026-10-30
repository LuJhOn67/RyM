//! VLC as an outside player: when it's installed and "Play songs in" is set to VLC, songs open in
//! it with their direct link; otherwise the built-in player plays them.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result};

/// VLC's program, from its usual install folders or the PATH.
pub fn find() -> Option<PathBuf> {
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(var) {
            let p = Path::new(&base).join("VideoLAN").join("VLC").join("vlc.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let exe = if cfg!(windows) { "vlc.exe" } else { "vlc" };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(exe)).find(|p| p.is_file())
}

/// Starts VLC on a song's link; it closes itself when the song ends.
pub fn play(exe: &Path, url: &str, headers: &[(String, String)], title: &str) -> Result<Child> {
    command(exe, url, headers, title).spawn().with_context(|| format!("start {}", exe.display()))
}

fn command(exe: &Path, url: &str, headers: &[(String, String)], title: &str) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg("--play-and-exit").arg(format!("--meta-title={title}"));
    for (k, v) in headers {
        match k.to_ascii_lowercase().as_str() {
            "user-agent" => {
                cmd.arg(format!("--http-user-agent={v}"));
            }
            "referer" => {
                cmd.arg(format!("--http-referrer={v}"));
            }
            _ => {}
        }
    }
    cmd.arg(url);
    cmd
}

/// Closes a VLC this app started (when another song starts or the app stops using it).
pub fn stop(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlc_gets_the_link_title_and_user_agent() {
        let headers = [("User-Agent".to_string(), "UA/1".to_string()), ("Accept".to_string(), "*/*".to_string())];
        let cmd = command(Path::new("vlc"), "https://x/y", &headers, "A - B");
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["--play-and-exit", "--meta-title=A - B", "--http-user-agent=UA/1", "https://x/y"]);
    }
}
