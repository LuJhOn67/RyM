//! Layout and theme settings, changed from the in-app settings screen and saved as JSON.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LyricsPlace {
    Right,
    Left,
    Bottom,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BarPlace {
    Bottom,
    Top,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: usize,
    pub library: Side,
    pub library_width: u16,
    pub lyrics: LyricsPlace,
    /// Percent of the width (or height, when lyrics sit at the bottom).
    pub lyrics_size: u16,
    pub lyrics_centered: bool,
    pub player: BarPlace,
    pub help: bool,
    /// Look of the window app (index into its palettes).
    pub gui_theme: usize,
    /// Window app: wash the background with the cover art's color.
    pub gui_tint: bool,
    /// Window app icon: a file name in the icons folder, or none for the default.
    pub gui_icon: Option<String>,
    /// How the window app draws.
    pub gui_renderer: Renderer,
    /// Where songs' audio comes from.
    pub source: Source,
    /// Downloads in the best quality (yt-dlp with your account, 256 kbps for Premium, slower)
    /// instead of the playback file (about 160 kbps, in seconds).
    pub download_best: bool,
    /// Connections a song downloads over at once (1-16).
    pub download_connections: u8,
    /// Size of each downloaded piece in MB; 0 = open: the song is split evenly over the connections.
    pub download_piece_mb: u8,
    /// Play songs in VLC (when it's installed) instead of the built-in player.
    pub play_in_vlc: bool,
}

/// Piece sizes the settings cycle through (MB; 0 = open).
pub const PIECE_SIZES: [u8; 5] = [0, 1, 2, 4, 8];

impl Config {
    pub fn piece_label(&self) -> String {
        match self.download_piece_mb {
            0 => "Open (split evenly)".to_string(),
            mb => format!("{mb} MB"),
        }
    }
}

/// Where songs' audio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// InnerTube first (fast), yt-dlp when YouTube refuses it.
    Auto,
    /// yt-dlp with your account: keeps up with YouTube, Premium gets 256 kbps. Needs yt-dlp and Node.js.
    YtDlp,
    /// YouTube's player API straight from the app, without yt-dlp.
    InnerTube,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Auto => "InnerTube + yt-dlp",
            Source::YtDlp => "yt-dlp",
            Source::InnerTube => "InnerTube",
        }
    }

    pub fn next(self) -> Source {
        match self {
            Source::Auto => Source::YtDlp,
            Source::YtDlp => Source::InnerTube,
            Source::InnerTube => Source::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Renderer {
    /// Graphics card, Vulkan first (widest support), then Direct3D 12, then OpenGL.
    Vulkan,
    /// Graphics card, Direct3D 12 first: same speed as Vulkan on Windows, less RAM.
    Dx12,
    /// Processor only: least RAM, choppy scrolling on large screens.
    Cpu,
}

impl Renderer {
    pub fn label(self) -> &'static str {
        match self {
            Renderer::Vulkan => "Vulkan",
            Renderer::Dx12 => "DirectX 12",
            Renderer::Cpu => "Processor",
        }
    }

    pub fn next(self) -> Renderer {
        match self {
            Renderer::Vulkan => Renderer::Dx12,
            Renderer::Dx12 => Renderer::Cpu,
            Renderer::Cpu => Renderer::Vulkan,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: 0,
            library: Side::Left,
            library_width: 28,
            lyrics: LyricsPlace::Right,
            lyrics_size: 30,
            lyrics_centered: true,
            player: BarPlace::Bottom,
            help: true,
            gui_theme: 0,
            gui_tint: true,
            gui_icon: None,
            gui_renderer: Renderer::Vulkan,
            source: Source::Auto,
            download_best: true,
            download_connections: 8,
            download_piece_mb: 0,
            play_in_vlc: false,
        }
    }
}

pub struct Theme {
    pub name: &'static str,
    pub accent: Color,
    /// Border of the pane you're in.
    pub focus: Color,
    /// Fills the whole screen; `None` keeps the terminal's own background.
    pub bg: Option<Color>,
    /// Selected row in the focused pane.
    pub cursor: Color,
    /// Selected row in the other pane.
    pub cursor_dim: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
}

/// The first one is the original look and stays the default.
pub const THEMES: &[Theme] = &[
    Theme {
        name: "Rym Red",
        accent: Color::Rgb(255, 0, 51),
        focus: Color::Rgb(255, 0, 51),
        bg: None,
        cursor: Color::Rgb(150, 0, 30),
        cursor_dim: Color::Rgb(50, 50, 50),
        text: Color::White,
        dim: Color::Rgb(140, 140, 140),
        faint: Color::Rgb(90, 90, 90),
    },
    // The YouTube Music web player: warm near-black, white text, grey rows, red only where
    // YouTube uses it (progress bar, logo).
    Theme {
        name: "YouTube Music",
        accent: Color::Rgb(255, 0, 0),
        focus: Color::Rgb(205, 205, 205),
        bg: Some(Color::Rgb(22, 20, 19)),
        cursor: Color::Rgb(64, 60, 58),
        cursor_dim: Color::Rgb(40, 37, 36),
        text: Color::Rgb(255, 255, 255),
        dim: Color::Rgb(170, 170, 170),
        faint: Color::Rgb(66, 63, 61),
    },
    Theme {
        name: "Ocean",
        accent: Color::Rgb(56, 189, 248),
        focus: Color::Rgb(56, 189, 248),
        bg: None,
        cursor: Color::Rgb(12, 74, 110),
        cursor_dim: Color::Rgb(40, 48, 60),
        text: Color::Rgb(226, 238, 248),
        dim: Color::Rgb(130, 150, 170),
        faint: Color::Rgb(70, 85, 100),
    },
    Theme {
        name: "Forest",
        accent: Color::Rgb(74, 222, 128),
        focus: Color::Rgb(74, 222, 128),
        bg: None,
        cursor: Color::Rgb(20, 83, 45),
        cursor_dim: Color::Rgb(42, 52, 44),
        text: Color::Rgb(230, 245, 232),
        dim: Color::Rgb(140, 165, 145),
        faint: Color::Rgb(75, 95, 80),
    },
    Theme {
        name: "Violet",
        accent: Color::Rgb(192, 132, 252),
        focus: Color::Rgb(192, 132, 252),
        bg: None,
        cursor: Color::Rgb(88, 28, 135),
        cursor_dim: Color::Rgb(52, 44, 62),
        text: Color::Rgb(243, 232, 255),
        dim: Color::Rgb(160, 145, 180),
        faint: Color::Rgb(90, 80, 105),
    },
    Theme {
        name: "Sunset",
        accent: Color::Rgb(251, 146, 60),
        focus: Color::Rgb(251, 146, 60),
        bg: None,
        cursor: Color::Rgb(124, 45, 18),
        cursor_dim: Color::Rgb(60, 48, 40),
        text: Color::Rgb(255, 244, 230),
        dim: Color::Rgb(175, 150, 130),
        faint: Color::Rgb(100, 85, 72),
    },
    Theme {
        name: "Mono",
        accent: Color::White,
        focus: Color::White,
        bg: None,
        cursor: Color::Rgb(80, 80, 80),
        cursor_dim: Color::Rgb(45, 45, 45),
        text: Color::Rgb(235, 235, 235),
        dim: Color::Rgb(150, 150, 150),
        faint: Color::Rgb(85, 85, 85),
    },
];

/// One row of the settings screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    Theme,
    Library,
    LibraryWidth,
    Lyrics,
    LyricsSize,
    LyricsAlign,
    Player,
    Help,
    Source,
    Download,
    Connections,
    Piece,
    Vlc,
    Reset,
}

pub const SETTINGS: [Setting; 14] = [
    Setting::Theme,
    Setting::Library,
    Setting::LibraryWidth,
    Setting::Lyrics,
    Setting::LyricsSize,
    Setting::LyricsAlign,
    Setting::Player,
    Setting::Help,
    Setting::Source,
    Setting::Download,
    Setting::Connections,
    Setting::Piece,
    Setting::Vlc,
    Setting::Reset,
];

fn cycle<T: Copy + PartialEq>(options: &[T], current: T, step: i32) -> T {
    let i = options.iter().position(|o| *o == current).unwrap_or(0) as i32;
    options[(i + step).rem_euclid(options.len() as i32) as usize]
}

impl Config {
    pub fn theme(&self) -> &'static Theme {
        &THEMES[self.theme.min(THEMES.len() - 1)]
    }

    pub fn load() -> Config {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(crate::data_dir());
            let _ = std::fs::write(path(), s);
        }
    }

    pub fn label(s: Setting) -> &'static str {
        match s {
            Setting::Theme => "Theme",
            Setting::Library => "Library panel",
            Setting::LibraryWidth => "Library width",
            Setting::Lyrics => "Lyrics panel",
            Setting::LyricsSize => "Lyrics size",
            Setting::LyricsAlign => "Lyrics text",
            Setting::Player => "Player bar",
            Setting::Help => "Key hints",
            Setting::Source => "Audio source",
            Setting::Download => "Download quality",
            Setting::Connections => "Download connections",
            Setting::Piece => "Download piece size",
            Setting::Vlc => "Play songs in",
            Setting::Reset => "Reset layout",
        }
    }

    pub fn value(&self, s: Setting) -> String {
        match s {
            Setting::Theme => self.theme().name.to_string(),
            Setting::Library => format!("{:?}", self.library),
            Setting::LibraryWidth => format!("{} columns", self.library_width),
            Setting::Lyrics => format!("{:?}", self.lyrics),
            Setting::LyricsSize => format!("{}%", self.lyrics_size),
            Setting::LyricsAlign => if self.lyrics_centered { "Centered" } else { "Left" }.to_string(),
            Setting::Player => format!("{:?}", self.player),
            Setting::Help => if self.help { "Shown" } else { "Hidden" }.to_string(),
            Setting::Source => self.source.label().to_string(),
            Setting::Download => if self.download_best { "Best (yt-dlp, slower)" } else { "Fast (~160 kbps)" }.to_string(),
            Setting::Connections => format!("{} at once", self.download_connections),
            Setting::Piece => self.piece_label(),
            Setting::Vlc => if self.play_in_vlc { "VLC (when installed)" } else { "RyM (built-in)" }.to_string(),
            Setting::Reset => "Enter".to_string(),
        }
    }

    /// Moves a setting to its next (`step` 1) or previous (-1) value.
    pub fn change(&mut self, s: Setting, step: i32) {
        match s {
            Setting::Theme => self.theme = (self.theme as i32 + step).rem_euclid(THEMES.len() as i32) as usize,
            Setting::Library => self.library = cycle(&[Side::Left, Side::Right, Side::Hidden], self.library, step),
            Setting::LibraryWidth => self.library_width = (self.library_width as i32 + 2 * step).clamp(18, 44) as u16,
            Setting::Lyrics => {
                let all = [LyricsPlace::Right, LyricsPlace::Left, LyricsPlace::Bottom, LyricsPlace::Hidden];
                self.lyrics = cycle(&all, self.lyrics, step);
            }
            Setting::LyricsSize => self.lyrics_size = (self.lyrics_size as i32 + 5 * step).clamp(20, 60) as u16,
            Setting::LyricsAlign => self.lyrics_centered = !self.lyrics_centered,
            Setting::Player => self.player = cycle(&[BarPlace::Bottom, BarPlace::Top], self.player, step),
            Setting::Help => self.help = !self.help,
            Setting::Source => self.source = self.source.next(),
            Setting::Download => self.download_best = !self.download_best,
            Setting::Connections => self.download_connections = (self.download_connections as i32 + step).clamp(1, 16) as u8,
            Setting::Piece => self.download_piece_mb = cycle(&PIECE_SIZES, self.download_piece_mb, step),
            Setting::Vlc => self.play_in_vlc = !self.play_in_vlc,
            // Keeps the theme, puts every panel back where it started.
            Setting::Reset => {
                *self = Config { theme: self.theme, gui_theme: self.gui_theme, gui_icon: self.gui_icon.take(), source: self.source, download_best: self.download_best, download_connections: self.download_connections, download_piece_mb: self.download_piece_mb, play_in_vlc: self.play_in_vlc, ..Config::default() }
            }
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::data_dir().join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_and_round_trips() {
        let mut c = Config::default();
        c.change(Setting::Lyrics, 1);
        assert_eq!(c.lyrics, LyricsPlace::Left);
        c.change(Setting::Lyrics, -1);
        c.change(Setting::Lyrics, -1);
        assert_eq!(c.lyrics, LyricsPlace::Hidden);
        c.change(Setting::Theme, -1);
        assert_eq!(c.theme().name, "Mono");
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.lyrics, LyricsPlace::Hidden);
        // Old or partial files still load.
        let partial: Config = serde_json::from_str(r#"{"theme": 2}"#).unwrap();
        assert_eq!(partial.library_width, 28);
    }
}
