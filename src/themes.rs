//! Color themes: 11 built-in looks and the user's own (a `.json` file), the same on RyM Slint,
//! RyM GTK and the phone. A theme is colors, how round the corners are, and a style that draws
//! a light pattern behind everything (a grid, dots, rain...).
//!
//! The JSON file is this struct as it is (`name`, the colors as `#rrggbb` or `#rrggbbaa`, and
//! the style's fields); unknown fields are ignored and missing ones take the defaults, so a file
//! from another version still opens.

use serde::{Deserialize, Serialize};

/// The pattern behind everything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Plain,
    /// Thin lines in squares (tactical screens).
    Grid,
    /// A dot matrix.
    Dots,
    /// Comic-print dots.
    Halftone,
    /// Rain running down.
    Rain,
    /// Old-screen lines.
    Scanlines,
    /// Brushed metal.
    Metal,
    /// Corner marks of a head-up display.
    Hud,
    /// Frosted glass over the cover.
    Glass,
    /// Glossy bubbles.
    Aqua,
    /// Stars.
    Stars,
    /// Grey ash falling slowly.
    Ash,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Light text on a dark background.
    pub dark: bool,
    pub bg: String,
    /// Panels, menus, cards.
    pub raised: String,
    pub text: String,
    /// Second text (artists, notes).
    pub dim: String,
    pub icon: String,
    /// Buttons, the played part of the wave.
    pub accent: String,
    /// The wave's other end, glows.
    pub accent2: String,
    /// Bars not filled yet (volume, the wave's rest).
    pub bar: String,
    pub border: String,
    pub style: Style,
    /// Corners: 0 sharp, 1 as round as the default look.
    pub round: f32,
    /// Accents glow.
    pub glow: bool,
    /// Borders take the playing cover's color (neon frames).
    pub cover_border: bool,
    /// Glass: how see-through the panels are (0 solid, 1 clear).
    pub glass: f32,
    /// Times in dot-matrix digits.
    pub dot_clock: bool,
    /// Two needle meters (left and right) by the song.
    pub vu: bool,
    /// The covers lose most of their color (Remorse).
    pub muted: bool,
    /// The screen's edges darken.
    pub vignette: bool,
    /// A slow, heavy glow in the accent that beats with the bass.
    pub heartbeat: bool,
    /// Corners cut at 45 degrees, with a glowing line along the cut (Cyber HUD).
    pub cut: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            name: "Custom".into(),
            dark: true,
            bg: "#343345".into(),
            raised: "#3d3c50".into(),
            text: "#ffffff".into(),
            dim: "#c6c6c6".into(),
            icon: "#e4e3e3".into(),
            accent: "#ea814a".into(),
            accent2: "#f9d19e".into(),
            bar: "#d9d9d9".into(),
            border: "#66647780".into(),
            style: Style::Plain,
            round: 1.0,
            glow: false,
            cover_border: false,
            glass: 0.0,
            dot_clock: false,
            vu: false,
            muted: false,
            vignette: false,
            heartbeat: false,
            cut: false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn theme(name: &str, dark: bool, [bg, raised, text, dim, icon, accent, accent2, bar, border]: [&str; 9], style: Style, round: f32) -> Theme {
    Theme {
        name: name.into(),
        dark,
        bg: bg.into(),
        raised: raised.into(),
        text: text.into(),
        dim: dim.into(),
        icon: icon.into(),
        accent: accent.into(),
        accent2: accent2.into(),
        bar: bar.into(),
        border: border.into(),
        style,
        round,
        ..Theme::default()
    }
}

/// The built-in themes, in the order the settings show them.
pub fn presets() -> Vec<Theme> {
    use Style::*;
    vec![
        Theme { glow: true, cover_border: true, ..theme("OLED Black", true, ["#000000", "#0b0b0b", "#ffffff", "#9a9a9a", "#e6e6e6", "#00e5ff", "#7cf9ff", "#262626", "#1f1f1f"], Plain, 1.0) },
        Theme { glass: 0.55, ..theme("Frosted Glass", true, ["#141826", "#ffffff1c", "#ffffff", "#d3d9ea", "#f2f4fa", "#9ad0ff", "#d4ecff", "#ffffff40", "#ffffff33"], Glass, 1.0) },
        theme("Tactical Beige", false, ["#dad4bb", "#cdc8b0", "#4e4b42", "#6b675b", "#4e4b42", "#4e4b42", "#8a8574", "#b5af98", "#4e4b4299"], Grid, 0.0),
        Theme { glow: true, ..theme("Stellar Neon", true, ["#070b1a", "#0e1530", "#e8f6ff", "#8fa3c7", "#bfefff", "#00e1ff", "#ffc857", "#1c2a4d", "#00e1ff40"], Stars, 0.8) },
        Theme { dot_clock: true, ..theme("Dot Matrix", true, ["#000000", "#111111", "#f2f2f2", "#8c8c8c", "#f2f2f2", "#d71921", "#ff5a60", "#2a2a2a", "#2a2a2a"], Dots, 1.0) },
        Theme { vu: true, ..theme("Hi-Fi Rack", true, ["#1a1b1d", "#2a2c2f", "#e9e4d8", "#a39d90", "#d8d2c4", "#ffb000", "#ffd27a", "#3a3c40", "#45484d"], Metal, 0.35) },
        Theme { glow: true, cut: true, ..theme("Cyber HUD", true, ["#0a0a12", "#14141f", "#f5f5f5", "#9aa0b4", "#00f0ff", "#fcee09", "#00f0ff", "#23233a", "#00f0ff55"], Hud, 0.0) },
        theme("Lo-Fi Rain", true, ["#1a1530", "#251e42", "#f3e9ff", "#b3a6d6", "#e8dcff", "#ffb347", "#ffd59a", "#3a2f63", "#4a3d7a80"], Rain, 1.0),
        Theme { glass: 0.35, ..theme("Aqua Glass", false, ["#dff6ff", "#ffffffb0", "#073b4c", "#2f6f80", "#0b6e8a", "#00b894", "#48dbfb", "#a8e6f0", "#ffffffcc"], Aqua, 1.0) },
        theme("Manga Ink", false, ["#ffffff", "#f2f2f2", "#000000", "#333333", "#000000", "#000000", "#555555", "#cccccc", "#000000"], Halftone, 0.0),
        theme("Tactical Olive", true, ["#1d2117", "#2a2f22", "#d9dcc6", "#9aa08a", "#c8cdb0", "#8fbf3f", "#c3e86a", "#3b4230", "#5b6448"], Scanlines, 0.3),
        // Sadness and guilt: an abyssal slate night, cold mist, dried crimson like an old wound, ash
        // falling slowly, darkened edges, a slow heavy heartbeat, and covers that lost their color.
        Theme {
            muted: true,
            vignette: true,
            heartbeat: true,
            ..theme("Remorse", true, ["#0b0d12", "#14171f", "#c3c8cf", "#5c6b73", "#8e9aa3", "#8c2d38", "#b0606a", "#232833", "#2a303c"], Ash, 0.6)
        },
    ]
}

/// A built-in theme by its name.
pub fn preset(name: &str) -> Option<Theme> {
    presets().into_iter().find(|t| t.name == name)
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as (r, g, b, a).
pub fn rgba(hex: &str) -> Option<(u8, u8, u8, u8)> {
    let h = hex.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
            Some((d(0)?, d(1)?, d(2)?, 255))
        }
        6 => Some((byte(0)?, byte(2)?, byte(4)?, 255)),
        8 => Some((byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

impl Theme {
    /// Every color readable; corners and glass within their range.
    pub fn check(mut self) -> Result<Theme, String> {
        for (field, value) in [
            ("bg", &self.bg),
            ("raised", &self.raised),
            ("text", &self.text),
            ("dim", &self.dim),
            ("icon", &self.icon),
            ("accent", &self.accent),
            ("accent2", &self.accent2),
            ("bar", &self.bar),
            ("border", &self.border),
        ] {
            if rgba(value).is_none() {
                return Err(format!("{field}: \"{value}\" is not a color like #12ab34"));
            }
        }
        self.round = self.round.clamp(0.0, 1.0);
        self.glass = self.glass.clamp(0.0, 1.0);
        if self.name.trim().is_empty() {
            self.name = "Custom".into();
        }
        Ok(self)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Theme, String> {
        serde_json::from_str::<Theme>(text).map_err(|e| e.to_string())?.check()
    }

    /// How fast the pattern falls while music plays (pixels a millisecond; 0 = it stays).
    pub fn fall(&self) -> f32 {
        match self.style {
            Style::Rain => 0.09,
            Style::Ash => 0.012,
            _ => 0.0,
        }
    }

    /// The pattern's small picture (RGBA, `w` × `h`), repeated over the window; `None`: plain.
    pub fn tile(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (r, g, b, _) = rgba(&self.text)?;
        let (ar, ag, ab, _) = rgba(&self.accent2)?;
        let mut t = Tile::new(match self.style {
            Style::Plain | Style::Glass => return None,
            Style::Grid => (40, 40),
            Style::Dots => (12, 12),
            Style::Halftone => (14, 14),
            Style::Rain => (60, 120),
            Style::Scanlines => (4, 4),
            Style::Metal => (256, 8),
            Style::Hud => (64, 64),
            Style::Aqua => (120, 120),
            Style::Stars => (160, 160),
            Style::Ash => (150, 150),
        });
        let mut seed = 0x9e37_79b9_u32;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        match self.style {
            Style::Grid => {
                for i in 0..40 {
                    t.put(i, 39, (r, g, b), 34);
                    t.put(39, i, (r, g, b), 34);
                }
                for i in 17..23 {
                    t.put(i, 20, (r, g, b), 60);
                    t.put(20, i, (r, g, b), 60);
                }
            }
            Style::Dots => t.disc(6.0, 6.0, 1.6, (r, g, b), 40),
            Style::Halftone => t.disc(7.0, 7.0, 3.2, (0, 0, 0), 46),
            Style::Rain => {
                for _ in 0..7 {
                    let x = rand() % 60;
                    let y = rand() % 120;
                    let len = 10 + rand() % 18;
                    for k in 0..len {
                        let a = (70 * (k + 1) / len) as u8;
                        t.put(x, (y + k) % 120, (ar, ag, ab), a.saturating_add(10));
                    }
                }
            }
            Style::Scanlines => {
                for x in 0..4 {
                    t.put(x, 0, (0, 0, 0), 70);
                }
            }
            Style::Metal => {
                for y in 0..8 {
                    for x in 0..256 {
                        let n = (rand() % 22) as u8;
                        t.put(x, y, if y % 2 == 0 { (255, 255, 255) } else { (0, 0, 0) }, n);
                    }
                }
            }
            Style::Hud => {
                for i in 0..6 {
                    for (x, y) in [(i, 0), (0, i), (63 - i, 0), (63, i), (i, 63), (0, 63 - i), (63 - i, 63), (63, 63 - i)] {
                        t.put(x, y, (ar, ag, ab), 90);
                    }
                }
                t.put(32, 32, (ar, ag, ab), 70);
            }
            Style::Aqua => {
                for _ in 0..5 {
                    let (x, y, rad) = ((rand() % 120) as f32, (rand() % 120) as f32, 3.0 + (rand() % 9) as f32);
                    t.ring(x, y, rad, (255, 255, 255), 90);
                    t.disc(x - rad * 0.35, y - rad * 0.35, rad * 0.25, (255, 255, 255), 120);
                }
            }
            Style::Stars => {
                for _ in 0..26 {
                    let (x, y) = (rand() % 160, rand() % 160);
                    let a = 50 + (rand() % 150) as u8;
                    let c = if rand() % 4 == 0 { (ar, ag, ab) } else { (r, g, b) };
                    t.put(x, y, c, a);
                    if a > 170 {
                        t.disc(x as f32, y as f32, 1.2, c, a / 2);
                    }
                }
            }
            Style::Ash => {
                for _ in 0..16 {
                    let (x, y) = ((rand() % 150) as f32, (rand() % 150) as f32);
                    let rad = 0.7 + (rand() % 10) as f32 / 8.0;
                    t.disc(x, y, rad, (r, g, b), 35 + (rand() % 55) as u8);
                }
            }
            Style::Plain | Style::Glass => {}
        }
        Some((t.w, t.h, t.px))
    }
}

struct Tile {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Tile {
    fn new((w, h): (u32, u32)) -> Tile {
        Tile { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    /// One pixel over what's there (the tile wraps around at its edges).
    fn put(&mut self, x: u32, y: u32, (r, g, b): (u8, u8, u8), a: u8) {
        let i = (((y % self.h) * self.w + (x % self.w)) * 4) as usize;
        if self.px[i + 3] < a {
            self.px[i..i + 4].copy_from_slice(&[r, g, b, a]);
        }
    }

    fn disc(&mut self, cx: f32, cy: f32, rad: f32, c: (u8, u8, u8), a: u8) {
        self.shape(cx, cy, rad + 1.0, c, |d| (rad + 0.5 - d).clamp(0.0, 1.0) * a as f32);
    }

    fn ring(&mut self, cx: f32, cy: f32, rad: f32, c: (u8, u8, u8), a: u8) {
        self.shape(cx, cy, rad + 1.5, c, |d| (1.0 - (d - rad).abs()).clamp(0.0, 1.0) * a as f32);
    }

    fn shape(&mut self, cx: f32, cy: f32, reach: f32, c: (u8, u8, u8), alpha: impl Fn(f32) -> f32) {
        let (x0, x1) = ((cx - reach).floor() as i64, (cx + reach).ceil() as i64);
        let (y0, y1) = ((cy - reach).floor() as i64, (cy + reach).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = alpha(d);
                if a >= 1.0 {
                    self.put(x.rem_euclid(self.w as i64) as u32, y.rem_euclid(self.h as i64) as u32, c, a as u8);
                }
            }
        }
    }
}

/// Takes 70% of the color out of a picture (RGBA), as Remorse shows the covers.
pub fn mute(rgba: &mut [u8]) {
    for p in rgba.as_chunks_mut::<4>().0 {
        let grey = 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
        for c in &mut p[..3] {
            *c = (*c as f32 * 0.3 + grey * 0.7) as u8;
        }
    }
}

/// The cover's `n` main colors (lively ones first), for the moving backdrop. Grey covers give
/// shades of their grey.
pub fn cover_colors(rgba: &[u8], n: usize) -> Vec<(u8, u8, u8)> {
    // Colors in 4×4×4 boxes; each box's count and its colors' sum.
    let mut boxes = vec![(0u32, 0u32, 0u32, 0u32); 64];
    for p in rgba.as_chunks::<4>().0.iter().step_by(7) {
        let i = (p[0] as usize >> 6) * 16 + (p[1] as usize >> 6) * 4 + (p[2] as usize >> 6);
        let b = &mut boxes[i];
        b.0 += 1;
        b.1 += p[0] as u32;
        b.2 += p[1] as u32;
        b.3 += p[2] as u32;
    }
    let mut found: Vec<((u8, u8, u8), f32)> = boxes
        .iter()
        .filter(|b| b.0 > 0)
        .map(|b| {
            let c = ((b.1 / b.0) as u8, (b.2 / b.0) as u8, (b.3 / b.0) as u8);
            let (max, min) = (c.0.max(c.1).max(c.2) as f32, c.0.min(c.1).min(c.2) as f32);
            let lively = if max > 0.0 { (max - min) / max } else { 0.0 };
            (c, b.0 as f32 * (0.35 + lively) * (0.3 + max / 255.0))
        })
        .collect();
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(u8, u8, u8)> = found.into_iter().map(|(c, _)| c).take(n).collect();
    while !out.is_empty() && out.len() < n {
        let c = out[out.len() - 1];
        out.push(crate::colors::mix(c, (0, 0, 0), 0.35));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleven_presets_with_good_colors_and_patterns() {
        let all = presets();
        assert_eq!(all.len(), 12);
        for t in &all {
            let checked = t.clone().check().unwrap_or_else(|e| panic!("{}: {e}", t.name));
            assert_eq!(&checked, t);
            if let Some((w, h, px)) = t.tile() {
                assert_eq!(px.len(), (w * h * 4) as usize);
                assert!(px.chunks_exact(4).any(|p| p[3] > 0), "{}: empty pattern", t.name);
            }
        }
    }

    #[test]
    fn json_round_trip_and_bad_files() {
        let t = preset("Cyber HUD").unwrap();
        assert_eq!(Theme::from_json(&t.to_json()).unwrap(), t);
        // Missing fields take the defaults; a bad color is refused with its name.
        let partial = Theme::from_json(r##"{"name": "Mine", "accent": "#ff0000", "round": 3}"##).unwrap();
        assert_eq!((partial.accent.as_str(), partial.round, partial.bg.as_str()), ("#ff0000", 1.0, "#343345"));
        assert!(Theme::from_json(r#"{"bg": "red"}"#).unwrap_err().contains("bg"));
        assert!(Theme::from_json("not json").is_err());
    }

    #[test]
    fn remorse_mutes_covers_and_its_ash_falls_slowly() {
        let t = preset("Remorse").unwrap();
        assert!(t.muted && t.vignette && t.heartbeat && t.style == Style::Ash);
        assert_eq!((t.bg.as_str(), t.raised.as_str(), t.accent.as_str(), t.dim.as_str()), ("#0b0d12", "#14171f", "#8c2d38", "#5c6b73"));
        assert!(t.fall() > 0.0 && t.fall() < preset("Lo-Fi Rain").unwrap().fall());
        let mut px = vec![220, 30, 30, 255];
        mute(&mut px);
        let spread = px[..3].iter().max().unwrap() - px[..3].iter().min().unwrap();
        // 70% of the color goes: red 220 → about 127, the rest about 70.
        assert!(spread < 70 && spread > 40, "{px:?}");
    }

    #[test]
    fn colors_read_in_every_form() {
        assert_eq!(rgba("#fff"), Some((255, 255, 255, 255)));
        assert_eq!(rgba("#12ab34"), Some((0x12, 0xab, 0x34, 255)));
        assert_eq!(rgba("#12ab3480"), Some((0x12, 0xab, 0x34, 0x80)));
        assert_eq!(rgba("12ab34"), None);
    }

    #[test]
    fn cover_colors_find_the_main_ones() {
        let mut px = Vec::new();
        for i in 0..400 {
            px.extend_from_slice(if i % 4 == 0 { &[20, 40, 230, 255] } else { &[230, 30, 30, 255] });
        }
        let c = cover_colors(&px, 4);
        assert_eq!(c.len(), 4);
        assert!(c[0].0 > 200 && c[0].2 < 60, "{c:?}");
        assert!(c.iter().any(|c| c.2 > 200));
    }
}
