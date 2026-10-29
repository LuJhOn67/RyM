//! Themes written as CSS: `%LOCALAPPDATA%\RymPlayer\data\themes\*.css`.
//!
//! A theme sets colors as CSS variables in a `:root { ... }` block, the same file works in the
//! PC apps and on the phone. Anything after that block is real CSS for the GTK app (it can style
//! any GTK widget); the other apps read only the variables.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every variable a theme can set; all are optional (missing ones come from a built-in theme).
pub const VARIABLES: &[(&str, &str)] = &[
    ("--name", "Name shown in Settings (else the file name)"),
    ("--light", "1 for a light theme, 0 for dark (else guessed from --bg)"),
    ("--bg", "Window and side rail"),
    ("--surface", "Main content area"),
    ("--bar", "Player bar"),
    ("--text", "Main text"),
    ("--dim", "Secondary text"),
    ("--accent", "Highlights, progress bar, the glow around sung lyrics"),
    ("--pill-bg", "Big buttons (Play all, Sign in)"),
    ("--pill-text", "Text on big buttons"),
    ("--hover", "Rows under the mouse"),
    ("--selected", "The playing song's row and the current page"),
    ("--input-bg", "Search box and fields"),
    ("--divider", "Thin lines"),
    ("--menu", "Right-click menu and dialogs"),
    ("--radius", "Rounded content panel in px (0 = flat)"),
    ("--lyrics-glow", "Glow around the sung lyrics line (else the cover's color)"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct CssTheme {
    pub name: String,
    pub vars: BTreeMap<String, String>,
    /// CSS after the `:root` block, applied as-is by the GTK app.
    pub rules: String,
}

pub fn themes_dir() -> PathBuf {
    crate::data_dir().join("themes")
}

const EXAMPLE: &str = r#"/* RymPlayer theme. Copy this file, rename it and change the colors.
   It works in RymPlayer on the PC (both versions) and on the phone (Settings > Themes > Import).
   Colors: #rgb, #rrggbb, #rrggbbaa, rgb(r, g, b) or rgba(r, g, b, a). Every line is optional. */
:root {
  --name: "Rose Night";
  --light: 0;
  --bg: #120d10;
  --surface: #1a1217;
  --bar: #160f13;
  --text: #fbeef3;
  --dim: #b79aa6;
  --accent: #ff7aa8;
  --pill-bg: #ff7aa8;
  --pill-text: #1a0d12;
  --hover: rgba(255, 255, 255, 0.06);
  --selected: rgba(255, 122, 168, 0.16);
  --input-bg: rgba(255, 255, 255, 0.07);
  --divider: rgba(255, 255, 255, 0.08);
  --menu: #2a1c23;
  --radius: 0;
}

/* Extra CSS for the GTK version, for example:
button.accent { border-radius: 999px; }
*/
"#;

/// Puts an example theme in the folder the first time, so the format is easy to find.
pub fn ensure_example() {
    let dir = themes_dir();
    let path = dir.join("rose-night.css");
    if !dir.exists() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(path, EXAMPLE);
    }
}

/// All themes in the folder, sorted by file name. Unreadable files are skipped.
pub fn load_all() -> Vec<CssTheme> {
    ensure_example();
    let Ok(dir) = std::fs::read_dir(themes_dir()) else { return Vec::new() };
    let mut files: Vec<PathBuf> =
        dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("css"))).collect();
    files.sort();
    files.iter().filter_map(|p| load(p)).collect()
}

pub fn load(path: &Path) -> Option<CssTheme> {
    let src = std::fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_string_lossy().replace(['-', '_'], " ");
    Some(parse(&src, &stem))
}

/// Reads the `:root { --x: y; }` variables; the rest is kept as GTK rules.
pub fn parse(src: &str, fallback_name: &str) -> CssTheme {
    let src = strip_comments(src);
    let mut vars = BTreeMap::new();
    let mut rules = src.clone();
    if let Some(start) = src.find(":root")
        && let Some(open) = src[start..].find('{').map(|i| start + i)
        && let Some(close) = src[open..].find('}').map(|i| open + i)
    {
        for decl in src[open + 1..close].split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"').trim_matches('\''));
                if k.starts_with("--") && !v.is_empty() {
                    vars.insert(k.to_string(), v.to_string());
                }
            }
        }
        rules = format!("{}{}", &src[..start], &src[close + 1..]);
    }
    let name = vars.get("--name").cloned().unwrap_or_else(|| title_case(fallback_name));
    CssTheme { name, vars, rules: rules.trim().to_string() }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl CssTheme {
    /// A color variable as `[r, g, b, a]` in 0..1.
    pub fn color(&self, key: &str) -> Option<[f32; 4]> {
        parse_color(self.vars.get(key)?)
    }

    pub fn number(&self, key: &str) -> Option<f32> {
        self.vars.get(key)?.trim_end_matches("px").trim().parse().ok()
    }

    /// From `--light`, else from how bright `--bg` is.
    pub fn is_light(&self) -> Option<bool> {
        if let Some(n) = self.number("--light") {
            return Some(n != 0.0);
        }
        self.color("--bg").map(|[r, g, b, _]| 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5)
    }
}

pub fn parse_color(v: &str) -> Option<[f32; 4]> {
    let v = v.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
        let byte = |hi: u8, lo: u8| (hi * 16 + lo) as f32 / 255.0;
        return match digits.as_slice() {
            [r, g, b] => Some([byte(*r, *r), byte(*g, *g), byte(*b, *b), 1.0]),
            [r, g, b, a] => Some([byte(*r, *r), byte(*g, *g), byte(*b, *b), byte(*a, *a)]),
            [r1, r2, g1, g2, b1, b2] => Some([byte(*r1, *r2), byte(*g1, *g2), byte(*b1, *b2), 1.0]),
            [r1, r2, g1, g2, b1, b2, a1, a2] => Some([byte(*r1, *r2), byte(*g1, *g2), byte(*b1, *b2), byte(*a1, *a2)]),
            _ => None,
        };
    }
    let inner = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<f32> = inner.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    match parts.as_slice() {
        [r, g, b] => Some([r / 255.0, g / 255.0, b / 255.0, 1.0]),
        [r, g, b, a] => Some([r / 255.0, g / 255.0, b / 255.0, *a]),
        _ => None,
    }
}

/// `#rrggbb` (or `rgba(...)` with transparency), for writing CSS.
pub fn to_css([r, g, b, a]: [f32; 4]) -> String {
    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 0.999 {
        format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
    } else {
        format!("rgba({}, {}, {}, {:.3})", c(r), c(g), c(b), a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_example() {
        let t = parse(EXAMPLE, "rose night");
        assert_eq!(t.name, "Rose Night");
        assert_eq!(t.color("--accent"), Some([1.0, 122.0 / 255.0, 168.0 / 255.0, 1.0]));
        assert_eq!(t.color("--hover").map(|c| c[3]), Some(0.06));
        assert_eq!(t.is_light(), Some(false));
        assert_eq!(t.number("--radius"), Some(0.0));
        assert!(t.rules.is_empty(), "commented-out GTK rules are dropped: {:?}", t.rules);
    }

    #[test]
    fn keeps_gtk_rules_and_guesses_light() {
        let t = parse("/* x */ :root { --bg: #fafafa; --accent:#e91e63 } button { color: red; }", "my_theme");
        assert_eq!(t.name, "My_theme");
        assert_eq!(t.is_light(), Some(true));
        assert_eq!(t.rules, "button { color: red; }");
        assert_eq!(parse_color("#e91e6380").map(|c| (c[3] * 255.0).round()), Some(128.0));
        assert_eq!(to_css([1.0, 0.0, 0.0, 1.0]), "#ff0000");
        assert_eq!(parse_color("nonsense"), None);
    }
}
