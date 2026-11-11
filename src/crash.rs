//! Crash reports. When an app crashes, a short report (the version, the system, where and what
//! went wrong) is kept beside its settings; the next start offers to open a GitHub issue filled
//! in with it, and you decide whether to send it. Nothing is ever sent on its own. File paths in
//! the report have your user folder replaced by `~`.

use std::path::PathBuf;

fn path() -> PathBuf {
    crate::data_dir().join("crash-report.txt")
}

/// Your user folder replaced by `~` (and your user name, wherever else it shows).
fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var)
            && home.len() > 3
        {
            out = out.replace(&home, "~").replace(&home.replace('\\', "/"), "~");
        }
    }
    for var in ["USERNAME", "USER"] {
        if let Ok(name) = std::env::var(var)
            && name.len() > 2
        {
            out = out.replace(&name, "<user>");
        }
    }
    out
}

/// Keeps a report of a crash (called from the app's panic hook; it must not panic itself).
pub fn record(app: &str, at: &str, what: &str) {
    let when = crate::updates::now();
    let text = format!(
        "App: {app} {}\nSystem: {} {}\nWhere: {at}\nWhat: {what}\nWhen: {when} (seconds since 1970)",
        crate::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let _ = std::fs::create_dir_all(crate::data_dir());
    let _ = std::fs::write(path(), scrub(&text));
}

/// The report of the last crash, if one is waiting.
pub fn pending() -> Option<String> {
    std::fs::read_to_string(path()).ok().filter(|t| !t.trim().is_empty())
}

/// The waiting report is dealt with (sent or not): it isn't offered again.
pub fn dismiss() {
    let _ = std::fs::remove_file(path());
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A new GitHub issue on RyM's page, filled in with `report` (you press "Submit").
pub fn issue_url(report: &str) -> String {
    let what = report.lines().find_map(|l| l.strip_prefix("What: ")).unwrap_or("a crash");
    let title: String = format!("Crash: {what}").chars().take(100).collect();
    let report: String = report.chars().take(5000).collect();
    let body = format!("RyM closed unexpectedly. What it noted:\n\n```\n{report}\n```\n\nWhat were you doing when it happened? (optional)\n");
    format!("https://github.com/{}/issues/new?title={}&body={}", crate::updates::REPO, encode(&title), encode(&body))
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_link_and_scrubbing() {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        let text = super::scrub(&format!("Where: {home}/RyM/src/main.rs:10"));
        if home.len() > 3 {
            assert!(!text.contains(&home) && text.contains("~/RyM"), "{text}");
        }
        let url = super::issue_url("App: RyM Slint 7.4.1\nWhat: index out of bounds\n");
        assert!(url.starts_with("https://github.com/LuJhOn67/RyM/issues/new?title=Crash%3A%20index%20out%20of%20bounds&body="));
        assert!(!url.contains(' ') && !url.contains('\n'));
    }
}
