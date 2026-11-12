//! Layout and theme settings, changed from the in-app settings screen and saved as JSON.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::i18n::tr;

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
    /// Folders with songs on this device, besides the Music folder.
    pub local_folders: Vec<String>,
    /// The apps' words in Arabic instead of English.
    pub arabic: bool,
    /// Look for a new RyM on GitHub every 12 hours (and at start when it's been that long).
    pub auto_update_check: bool,
    /// Beta channel: pre-releases count as updates too.
    pub beta_updates: bool,
    /// When updates were last looked for (seconds since 1970).
    pub last_update_check: u64,
    /// Installed plugins: id -> version.
    pub plugins: std::collections::BTreeMap<String, String>,
    /// Seconds two songs overlap as one fades into the next (0 = off).
    pub crossfade: u8,
    /// Smart automix: songs change like a DJ mix (see `automix`); off: the plain crossfade.
    pub automix: bool,
    /// Which engine finds songs (search, radio, mixes): 0 Auto (ytmusicapi, built-in when it
    /// fails), 1 ytmusicapi only, 2 built-in InnerTube only (the rollback). See `ytmusicapi`.
    pub music_engine: u8,
    /// Seamless Music (RyM Link) on: the phone's music can move to this PC.
    pub cross_ears: bool,
    /// Japanese lyrics with English under each line.
    pub lyrics_translation: bool,
    /// The window app's lyrics size (pixels).
    pub lyrics_font: u8,
    /// RyM Link: music playing on the phone (same Wi-Fi and account) moves here as the window
    /// app opens; off: it asks first.
    pub link_auto: bool,
    /// A quiet start is skipped and a long quiet end finishes the song.
    pub skip_silence: bool,
    /// Equalizer preset (index into `fx::PRESETS`; the last is "Custom").
    pub eq_preset: usize,
    /// The "Custom" preset's gains, dB per band.
    pub eq_custom: [f32; 5],
    /// Every song at about the same loudness.
    pub normalize: bool,
    /// Playback speed and pitch (1 = normal), and the reverb of the "Slowed" mode.
    pub speed: f32,
    pub pitch: f32,
    pub reverb: f32,
    /// Show the playing song on Discord (needs Discord running and an application id).
    pub discord: bool,
    pub discord_app_id: String,
    /// The window app's floating lyrics line over other windows.
    pub lyrics_overlay: bool,
    /// Lyrics overlay: clicks go through it (else it can be dragged).
    pub overlay_click_through: bool,
    /// The Slint app's size: its layout times this (0.5 to 1.5).
    pub ui_zoom: f32,
    /// RaM points: For you keeps to this kind ("Husseini"...); empty: every kind heard.
    pub only_kind: String,
    /// The accent color from the playing song's cover (Slint app).
    pub accent_from_cover: bool,
    /// The colors: a built-in theme's name, "Custom" (`custom_theme`), or empty (the layout's own).
    #[serde(default)]
    pub palette: String,
    /// The user's own theme (imported from a `.json` file).
    #[serde(default)]
    pub custom_theme: Option<crate::themes::Theme>,
    /// The ← → keys skip this many seconds (5 to 30).
    #[serde(default = "seek_step")]
    pub seek_step: u32,
    /// Windows 11's glass behind the window: 0 off, 1 Mica (the wallpaper's colors), 2 Acrylic
    /// (what's behind the window, blurred).
    #[serde(default)]
    pub window_glass: u8,
    /// "Prefer lossless (FLAC)": the Internet Archive's FLAC first, YouTube when it isn't there.
    #[serde(default)]
    pub prefer_lossless: bool,
    /// Animations' frames a second (24 to 240; 0: as fast as the screen).
    #[serde(default = "fps_cap")]
    pub fps_cap: u32,
    /// The performance badge (frames a second, frame time, memory) in a corner.
    #[serde(default)]
    pub perf_badge: bool,
    /// RaM points reset by themselves on this day ("2026-12-31"; empty: no day picked).
    #[serde(default)]
    pub ram_reset_date: String,
    /// RaM points reset every week (off unless the user turns it on, after a warning).
    #[serde(default)]
    pub ram_weekly_reset: bool,
    /// When the points last reset by themselves (seconds since 1970).
    #[serde(default)]
    pub ram_last_reset: u64,
    /// Liquid Aurora: the cover's colors move behind everything with the bass.
    #[serde(default)]
    pub aurora: bool,
    /// The cover as a spinning record.
    #[serde(default)]
    pub vinyl: bool,
    /// How blurred the cover is behind the glass themes (2 to 20).
    #[serde(default = "glass_blur")]
    pub glass_blur: f32,
    /// The wave under the song moves with the music (Slint app).
    pub visualizer: bool,
    /// Romaji under Japanese lyrics.
    pub romaji: bool,
    /// The sung line fills in as it's sung (Slint app).
    pub karaoke: bool,
    /// Alarm: on, the time ("07:30"), and the day it last rang ("2026-11-06").
    pub alarm_on: bool,
    pub alarm_time: String,
    pub alarm_rang: String,
    /// Last.fm: the user's API key and secret (last.fm/api), and the session once connected.
    pub lastfm_key: String,
    pub lastfm_secret: String,
    pub lastfm_session: String,
    pub lastfm_user: String,
    /// yt-dlp updates from the nightly builds instead of the stable ones.
    pub ytdlp_nightly: bool,
    /// Seamless Music the other way: this computer tells the phones what it plays.
    pub link_announce: bool,
    /// The window's X in the Slint app: "ask", "tray" (keeps playing in the background, by the
    /// clock) or "quit".
    pub close_action: String,
    /// yt-dlp updated by itself once a week, and when it last was.
    pub ytdlp_auto_update: bool,
    pub ytdlp_updated_at: u64,
    /// SponsorBlock: a music video's non-music parts (intro, talk, outro) are skipped.
    pub sponsorblock: bool,
    /// No crossfade: the next song starts on the very frame this one ends.
    pub gapless: bool,
    /// Downloaded songs kept under this many GB (0: no limit).
    pub cache_limit_gb: u32,
    /// The phone can control this computer's music (play, pause, skip, seek, volume).
    pub remote_control: bool,
}

/// A side or place ("Left", "Bottom"...) in the app's language.
fn place(name: &str) -> String {
    let word = match name {
        "Left" => "Left",
        "Right" => "Right",
        "Top" => "Top",
        "Bottom" => "Bottom",
        "Hidden" => "Hidden",
        other => return other.to_string(),
    };
    tr(word).to_string()
}

/// RyM's own Discord application (its name and picture on your profile); your own id in the
/// settings takes its place.
pub const DISCORD_APP_ID: &str = "1568047216743030784";

/// Piece sizes the settings cycle through (MB; 0 = open).
pub const PIECE_SIZES: [u8; 5] = [0, 1, 2, 4, 8];

impl Config {
    /// The sound settings for the players' effects.
    pub fn fx(&self) -> crate::fx::Settings {
        let presets = &crate::fx::PRESETS;
        let i = self.eq_preset.min(presets.len() - 1);
        let eq = if i == presets.len() - 1 { self.eq_custom } else { presets[i].1 };
        crate::fx::Settings { eq, normalize: self.normalize, skip_silence: self.skip_silence, pitch: crate::fx::shift_for(self.speed, self.pitch), reverb: self.reverb }
    }

    pub fn eq_name(&self) -> &'static str {
        crate::fx::PRESETS[self.eq_preset.min(crate::fx::PRESETS.len() - 1)].0
    }

    /// Speed and pitch set to a quick pick ("Normal", "Nightcore", "Slowed").
    pub fn set_mode(&mut self, name: &str) {
        if let Some((_, speed, pitch, reverb)) = crate::fx::MODES.iter().find(|m| m.0 == name) {
            self.speed = *speed;
            self.pitch = *pitch;
            self.reverb = *reverb;
        }
    }

    /// The quick pick the speed and pitch match, if any.
    pub fn mode_name(&self) -> &'static str {
        crate::fx::MODES.iter().find(|m| (m.1 - self.speed).abs() < 0.01 && (m.2 - self.pitch).abs() < 0.01).map_or("Custom", |m| m.0)
    }

    pub fn piece_label(&self) -> String {
        match self.download_piece_mb {
            0 => tr("Open (split evenly)").to_string(),
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
            Renderer::Cpu => tr("Processor"),
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
            local_folders: Vec::new(),
            arabic: false,
            auto_update_check: true,
            beta_updates: false,
            last_update_check: 0,
            plugins: Default::default(),
            crossfade: 4,
            automix: false,
            music_engine: 0,
            cross_ears: true,
            lyrics_translation: true,
            lyrics_font: 25,
            link_auto: true,
            skip_silence: true,
            eq_preset: 0,
            eq_custom: [0.0; 5],
            normalize: true,
            speed: 1.0,
            pitch: 1.0,
            reverb: 0.0,
            discord: false,
            discord_app_id: DISCORD_APP_ID.into(),
            lyrics_overlay: false,
            overlay_click_through: false,
            ui_zoom: 0.85,
            only_kind: String::new(),
            accent_from_cover: false,
            seek_step: seek_step(),
            window_glass: 0,
            prefer_lossless: false,
            fps_cap: fps_cap(),
            perf_badge: false,
            ram_reset_date: String::new(),
            ram_weekly_reset: false,
            ram_last_reset: 0,
            palette: String::new(),
            custom_theme: None,
            aurora: false,
            vinyl: false,
            glass_blur: glass_blur(),
            visualizer: false,
            romaji: true,
            karaoke: true,
            alarm_on: false,
            alarm_time: "07:30".into(),
            alarm_rang: String::new(),
            lastfm_key: String::new(),
            lastfm_secret: String::new(),
            lastfm_session: String::new(),
            lastfm_user: String::new(),
            ytdlp_nightly: false,
            link_announce: true,
            close_action: "ask".into(),
            ytdlp_auto_update: true,
            ytdlp_updated_at: 0,
            sponsorblock: true,
            gapless: true,
            cache_limit_gb: 5,
            remote_control: true,
        }
    }
}

#[derive(Clone)]
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

/// Every terminal theme: the ones below, then the 11 color themes of the window apps
/// (`themes.rs`) in terminal colors.
pub fn all_themes() -> &'static [Theme] {
    static ALL: std::sync::OnceLock<Vec<Theme>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut all = THEMES.to_vec();
        for t in crate::themes::presets() {
            let bg = crate::themes::rgba(&t.bg).map_or((0, 0, 0), |(r, g, b, _)| (r, g, b));
            // A see-through color laid over the background, as the terminal can't blend.
            let c = |hex: &str| {
                let (r, g, b, a) = crate::themes::rgba(hex).unwrap_or((128, 128, 128, 255));
                let (r, g, b) = crate::colors::mix(bg, (r, g, b), a as f32 / 255.0);
                Color::Rgb(r, g, b)
            };
            let accent = crate::themes::rgba(&t.accent).map_or((255, 0, 51), |(r, g, b, _)| (r, g, b));
            let (cr, cg, cb) = crate::colors::mix(bg, accent, 0.45);
            let raised = c(&t.raised);
            all.push(Theme {
                name: Box::leak(t.name.clone().into_boxed_str()),
                accent: c(&t.accent),
                focus: c(&t.accent2),
                bg: Some(Color::Rgb(bg.0, bg.1, bg.2)),
                cursor: Color::Rgb(cr, cg, cb),
                cursor_dim: raised,
                text: c(&t.text),
                dim: c(&t.dim),
                faint: c(&t.border),
            });
        }
        all
    })
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
    Language,
    /// Look for updates on GitHub on its own.
    AutoUpdate,
    /// Pre-releases count as updates.
    Beta,
    Crossfade,
    /// Smart automix on or off.
    Automix,
    SkipSilence,
    Equalizer,
    Normalize,
    /// Speed and pitch quick picks.
    Mode,
    Discord,
    /// SponsorBlock skips a music video's non-music parts.
    SponsorBlock,
    /// English under Japanese lyrics.
    Translate,
    Romaji,
    /// Music starts at the alarm's time (set in RyM Slint).
    Alarm,
    Reset,
}

pub const SETTINGS: [Setting; 28] = [
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
    Setting::Language,
    Setting::AutoUpdate,
    Setting::Beta,
    Setting::Crossfade,
    Setting::Automix,
    Setting::SkipSilence,
    Setting::Equalizer,
    Setting::Normalize,
    Setting::Mode,
    Setting::Discord,
    Setting::SponsorBlock,
    Setting::Translate,
    Setting::Romaji,
    Setting::Alarm,
    Setting::Reset,
];

fn cycle<T: Copy + PartialEq>(options: &[T], current: T, step: i32) -> T {
    let i = options.iter().position(|o| *o == current).unwrap_or(0) as i32;
    options[(i + step).rem_euclid(options.len() as i32) as usize]
}

impl Config {
    pub fn theme(&self) -> &'static Theme {
        let all = all_themes();
        &all[self.theme.min(all.len() - 1)]
    }

    pub fn load() -> Config {
        let mut c: Config = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // No id of your own: RyM's (so "Show on Discord" works with no setup).
        if c.discord_app_id.trim().is_empty() {
            c.discord_app_id = DISCORD_APP_ID.into();
        }
        c
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(crate::data_dir());
            let _ = std::fs::write(path(), s);
        }
    }

    pub fn label(s: Setting) -> &'static str {
        tr(match s {
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
            Setting::Language => "Language",
            Setting::AutoUpdate => "Updates",
            Setting::Beta => "Beta updates",
            Setting::Crossfade => "Crossfade",
            Setting::Automix => "Smart Automix",
            Setting::SkipSilence => "Skip silence",
            Setting::Equalizer => "Equalizer",
            Setting::Normalize => "Even loudness",
            Setting::Mode => "Speed and pitch",
            Setting::Discord => "Discord",
            Setting::SponsorBlock => "Skip non-music parts",
            Setting::Translate => "Translate Japanese lyrics",
            Setting::Romaji => "Romaji",
            Setting::Alarm => "Alarm",
            Setting::Reset => "Reset layout",
        })
    }

    pub fn value(&self, s: Setting) -> String {
        match s {
            Setting::Theme => self.theme().name.to_string(),
            Setting::Library => place(&format!("{:?}", self.library)),
            Setting::LibraryWidth => format!("{} {}", self.library_width, tr("columns")),
            Setting::Lyrics => place(&format!("{:?}", self.lyrics)),
            Setting::LyricsSize => format!("{}%", self.lyrics_size),
            Setting::LyricsAlign => tr(if self.lyrics_centered { "Centered" } else { "Left" }).to_string(),
            Setting::Player => place(&format!("{:?}", self.player)),
            Setting::Help => tr(if self.help { "Shown" } else { "Hidden" }).to_string(),
            Setting::Source => self.source.label().to_string(),
            Setting::Download => tr(if self.download_best { "Best (yt-dlp, slower)" } else { "Fast (~160 kbps)" }).to_string(),
            Setting::Connections => format!("{} {}", self.download_connections, tr("at once")),
            Setting::Piece => self.piece_label(),
            Setting::Vlc => tr(if self.play_in_vlc { "VLC (when installed)" } else { "RyM (built-in)" }).to_string(),
            Setting::Language => if self.arabic { "العربية" } else { "English" }.to_string(),
            Setting::AutoUpdate => tr(if self.auto_update_check { "Automatic" } else { "Off" }).to_string(),
            Setting::Beta => tr(if self.beta_updates { "On" } else { "Off" }).to_string(),
            Setting::Crossfade => if self.crossfade == 0 { tr("Off").to_string() } else { format!("{} s", self.crossfade) },
            Setting::Automix => tr(if self.automix { "On" } else { "Off" }).to_string(),
            Setting::SkipSilence => tr(if self.skip_silence { "On" } else { "Off" }).to_string(),
            Setting::Equalizer => tr(self.eq_name()).to_string(),
            Setting::Normalize => tr(if self.normalize { "On" } else { "Off" }).to_string(),
            Setting::Mode => format!("{} ({:.2}x, {} {:.2})", tr(self.mode_name()), self.speed, tr("pitch"), self.pitch),
            Setting::Discord => match (self.discord, self.discord_app_id.is_empty()) {
                (false, _) => tr("Off").to_string(),
                (true, true) => tr("On (needs an application id)").to_string(),
                (true, false) => tr("On").to_string(),
            },
            Setting::SponsorBlock => tr(if self.sponsorblock { "On" } else { "Off" }).to_string(),
            Setting::Translate => tr(if self.lyrics_translation { "On" } else { "Off" }).to_string(),
            Setting::Romaji => tr(if self.romaji { "On" } else { "Off" }).to_string(),
            Setting::Alarm => if self.alarm_on { format!("{} ({})", tr("On"), self.alarm_time) } else { tr("Off").to_string() },
            Setting::Reset => "Enter".to_string(),
        }
    }

    /// Moves a setting to its next (`step` 1) or previous (-1) value.
    pub fn change(&mut self, s: Setting, step: i32) {
        match s {
            Setting::Theme => self.theme = (self.theme as i32 + step).rem_euclid(all_themes().len() as i32) as usize,
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
            Setting::AutoUpdate => self.auto_update_check = !self.auto_update_check,
            Setting::Beta => self.beta_updates = !self.beta_updates,
            Setting::Crossfade => self.crossfade = (self.crossfade as i32 + step).clamp(0, 10) as u8,
            Setting::Automix => self.automix = !self.automix,
            Setting::SkipSilence => self.skip_silence = !self.skip_silence,
            Setting::Equalizer => self.eq_preset = (self.eq_preset as i32 + step).rem_euclid(crate::fx::PRESETS.len() as i32) as usize,
            Setting::Normalize => self.normalize = !self.normalize,
            Setting::Mode => {
                let names: Vec<&str> = crate::fx::MODES.iter().map(|m| m.0).collect();
                let i = names.iter().position(|n| *n == self.mode_name()).map_or(0, |i| (i as i32 + step).rem_euclid(names.len() as i32) as usize);
                self.set_mode(names[i]);
            }
            Setting::Discord => self.discord = !self.discord,
            Setting::SponsorBlock => self.sponsorblock = !self.sponsorblock,
            Setting::Translate => self.lyrics_translation = !self.lyrics_translation,
            Setting::Romaji => self.romaji = !self.romaji,
            Setting::Alarm => {
                self.alarm_on = !self.alarm_on;
                if self.alarm_time.trim().is_empty() {
                    self.alarm_time = "07:30".into();
                }
            }
            Setting::Language => {
                self.arabic = !self.arabic;
                crate::i18n::set_arabic(self.arabic);
            }
            // Keeps the theme, puts every panel back where it started.
            Setting::Reset => {
                *self = Config { theme: self.theme, gui_theme: self.gui_theme, gui_icon: self.gui_icon.take(), source: self.source, download_best: self.download_best, download_connections: self.download_connections, download_piece_mb: self.download_piece_mb, play_in_vlc: self.play_in_vlc, arabic: self.arabic, auto_update_check: self.auto_update_check, beta_updates: self.beta_updates, last_update_check: self.last_update_check, plugins: std::mem::take(&mut self.plugins), crossfade: self.crossfade, music_engine: self.music_engine, automix: self.automix, link_auto: self.link_auto, cross_ears: self.cross_ears, lyrics_translation: self.lyrics_translation, lyrics_font: self.lyrics_font, skip_silence: self.skip_silence, eq_preset: self.eq_preset, eq_custom: self.eq_custom, normalize: self.normalize, speed: self.speed, pitch: self.pitch, reverb: self.reverb, discord: self.discord, discord_app_id: std::mem::take(&mut self.discord_app_id), lyrics_overlay: self.lyrics_overlay, overlay_click_through: self.overlay_click_through, ui_zoom: self.ui_zoom, only_kind: std::mem::take(&mut self.only_kind), accent_from_cover: self.accent_from_cover, visualizer: self.visualizer, romaji: self.romaji, karaoke: self.karaoke, alarm_on: self.alarm_on, alarm_time: std::mem::take(&mut self.alarm_time), alarm_rang: std::mem::take(&mut self.alarm_rang), lastfm_key: std::mem::take(&mut self.lastfm_key), lastfm_secret: std::mem::take(&mut self.lastfm_secret), lastfm_session: std::mem::take(&mut self.lastfm_session), lastfm_user: std::mem::take(&mut self.lastfm_user), ytdlp_nightly: self.ytdlp_nightly, link_announce: self.link_announce, close_action: std::mem::take(&mut self.close_action), ytdlp_auto_update: self.ytdlp_auto_update, ytdlp_updated_at: self.ytdlp_updated_at, sponsorblock: self.sponsorblock, gapless: self.gapless, cache_limit_gb: self.cache_limit_gb, remote_control: self.remote_control, ..Config::default() }
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
        // Back from the first: the last one (the window apps' color themes come after "Mono").
        c.change(Setting::Theme, -1);
        assert_eq!(c.theme().name, "Remorse");
        assert_eq!(all_themes()[THEMES.len() - 1].name, "Mono");
        assert_eq!(all_themes().len(), THEMES.len() + crate::themes::presets().len());
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.lyrics, LyricsPlace::Hidden);
        // Old or partial files still load.
        let partial: Config = serde_json::from_str(r#"{"theme": 2}"#).unwrap();
        assert_eq!(partial.library_width, 28);
    }
}

fn glass_blur() -> f32 {
    7.0
}

fn fps_cap() -> u32 {
    60
}

fn seek_step() -> u32 {
    5
}
