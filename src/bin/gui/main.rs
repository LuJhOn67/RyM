#![windows_subsystem = "windows"]
//! RymPlayer with a window: YouTube Music / Spotify inspired, software-rendered to stay light.

mod icons;
mod right_click;
mod style;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::image;
use iced::{Color, Point, Size, Subscription, Task, Theme, window};

use rymplayer::api::{self, Api, Link, Playlist, Track};
use rymplayer::audio::Audio;
use rymplayer::config::{Config, Renderer};
use rymplayer::lyrics::{self, Lyrics};
use rymplayer::{cookie_path, login, parse_duration, stream};

fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    choose_renderer(Config::load().gui_renderer);
    iced::application(Gui::boot, Gui::update, Gui::view)
        .title(Gui::title)
        .subscription(Gui::subscription)
        .theme(Gui::theme)
        .default_font(view::FONT)
        .window(window::Settings {
            // The pear on the title bar and the taskbar too (the .exe carries it for Explorer).
            icon: window::icon::from_file_data(include_bytes!("../../../assets/icons/rym-gui.png"), None).ok(),
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(960.0, 620.0)),
            ..window::Settings::default()
        })
        .centered()
        .run()
}

/// Measured on Windows: the graphics card scrolls at about 180 fps either way; Direct3D 12 uses
/// about 50 MB less than Vulkan, while Vulkan also runs where Direct3D 12 doesn't (older Windows
/// and GPUs). Each GPU choice falls back to the other APIs, then iced falls back to software.
/// The processor alone uses about half the RAM but scrolls around 20 fps on large screens.
/// Variables the user already set win.
fn choose_renderer(renderer: Renderer) {
    let set = |k: &str, v: &str| {
        if std::env::var_os(k).is_none() {
            // SAFETY: runs first in main, before any other thread exists.
            unsafe { std::env::set_var(k, v) };
        }
    };
    match renderer {
        Renderer::Vulkan => set("WGPU_BACKEND", "vulkan,dx12,gl"),
        Renderer::Dx12 => set("WGPU_BACKEND", if cfg!(windows) { "dx12,vulkan,gl" } else { "vulkan,gl" }),
        Renderer::Cpu => set("ICED_BACKEND", "tiny-skia"),
    }
    if renderer != Renderer::Cpu {
        set("WGPU_POWER_PREF", "high");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Home,
    Search(String),
    History,
    Playlist(String),
    Album(String),
    Mix(String),
    Radio(String),
    Artist(String),
    /// The user's playlists as cards.
    Library,
    Settings,
}

/// What a right-click menu is about.
#[derive(Debug, Clone)]
pub struct MenuFor {
    pub track: Track,
    /// Set when opened on the "Up next" list.
    pub queue_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    UpNext,
    Lyrics,
}

/// Popups over the main window.
#[derive(Debug, Clone)]
pub enum Dialog {
    /// Name for a new playlist, and the song to put in it.
    NewPlaylist(String, Option<Track>),
    /// Pick a playlist for this song.
    AddTo(Track),
}

#[derive(Debug, Clone)]
pub enum Message {
    SignIn,
    Guest,
    LoggedIn(Result<Api, String>),
    SignOut,
    Playlists(Result<Vec<Playlist>, String>),

    Open(Page, String),
    Loaded(Page, Result<Vec<Track>, String>),
    SearchChanged(String),
    SearchSubmit,

    /// Play row `i` of the visible list.
    PlayRow(usize),
    /// Jump to item `i` of the queue.
    PlayQueue(usize),
    /// Play a single song and let YouTube Music's radio fill the queue after it.
    PlaySong(Track),
    PlayAll,
    Fetched(String, Result<PathBuf, String>),
    Toggle,
    Next,
    Prev,
    Seek(f32),
    SeekDone,
    Volume(f32),
    Tick,
    Radio(Result<Vec<Track>, String>),
    StartRadio(Track),
    Lyrics(String, Option<Lyrics>),
    Like(Track),
    Rated(String, bool, Result<(), String>),

    Image(String, Option<Thumb>),
    ToggleNowPlaying,
    ShowTab(Tab),
    /// Open the player page on its lyrics (or close it if it's already showing them).
    ShowLyrics,
    SetTheme(usize),
    ToggleTint,
    ToggleGpu,
    /// Switch the audio source between yt-dlp and InnerTube.
    ToggleSource,
    /// Download quality: best (yt-dlp) or fast (the playback file).
    ToggleDownload,
    /// Download connections up or down by this many.
    Connections(i32),
    /// Next download piece size.
    NextPiece,

    NewPlaylist(Option<Track>),
    NameChanged(String),
    CreatePlaylist,
    Created(Result<Playlist, String>, Option<String>),
    AddTo(Track),
    Pick(Playlist),
    Added(String, Result<(), String>),
    CloseDialog,

    Menu(MenuFor),
    CloseMenu,
    Cursor(Point),
    Resized(Size),
    Frame(Instant),
    PlayNext(Track),
    AddToQueue(Track),
    RemoveFromQueue(usize),
    DismissQueue,
    Download(Track),
    Downloaded(Result<PathBuf, String>),
    Share(Track),
    GoAlbum(Track),
    GoArtist(Track),
    Dislike(Track),
    Disliked(String, bool, Result<(), String>),
    Ignore,
}

fn s<T>(r: anyhow::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

/// Thumbnail sizes requested from Google (pixels; drawn at about half for sharp HiDPI).
pub const ROW_PX: u32 = 96;
pub const CARD_PX: u32 = 360;
pub const ART_PX: u32 = 720;

#[derive(Debug, Clone)]
pub struct Thumb {
    pub handle: image::Handle,
    pub width: u32,
    pub height: u32,
    /// When it arrived, for the fade-in.
    pub loaded: Instant,
    /// The cover's main color (small thumbnails only), for the background wash.
    pub tint: Option<Color>,
}

/// The cover's most colorful shade, brightened to a usable level. Greys count a little so
/// black-and-white covers still give a neutral wash instead of nothing.
fn cover_tint(img: &::image::RgbaImage) -> Option<Color> {
    let (mut r, mut g, mut b, mut w) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for px in img.pixels().step_by(3) {
        let [pr, pg, pb, _] = px.0.map(|c| c as f32 / 255.0);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        let weight = sat * sat * max + 0.02;
        r += pr * weight;
        g += pg * weight;
        b += pb * weight;
        w += weight;
    }
    if w == 0.0 {
        return None;
    }
    let (r, g, b) = (r / w, g / w, b / w);
    let max = r.max(g).max(b);
    if max < 0.04 {
        return None;
    }
    Some(Color::from_rgb(r / max * 0.85, g / max * 0.85, b / max * 0.85))
}

impl Thumb {
    /// Square covers are kept encoded (iced decodes them when drawn). Video thumbnails are
    /// 16:9 or letterboxed, so they're decoded once here and cut to their centered square.
    pub fn new(bytes: Vec<u8>, with_tint: bool) -> Option<Thumb> {
        let reader = || ::image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok();
        let (width, height) = reader()?.into_dimensions().ok()?;
        if width == height && !with_tint {
            return Some(Thumb { handle: image::Handle::from_bytes(bytes), width, height, loaded: Instant::now(), tint: None });
        }
        let img = reader()?.decode().ok()?;
        let tint = if with_tint { cover_tint(&img.to_rgba8()) } else { None };
        if width == height {
            return Some(Thumb { handle: image::Handle::from_bytes(bytes), width, height, loaded: Instant::now(), tint });
        }
        let side = width.min(height);
        let square = img.crop_imm((width - side) / 2, (height - side) / 2, side, side).to_rgba8();
        Some(Thumb {
            handle: image::Handle::from_rgba(side, side, square.into_raw()),
            width: side,
            height: side,
            loaded: Instant::now(),
            tint,
        })
    }
}

pub struct Gui {
    pub signed_in_screen: bool,
    pub busy: bool,
    pub status: String,
    pub api: Option<Api>,
    pub config: Config,

    pub page: Page,
    pub page_title: String,
    pub tracks: Vec<Track>,
    pub search: String,
    pub playlists: Vec<Playlist>,
    pub liked: HashSet<String>,

    pub queue: Vec<Track>,
    pub current: Option<usize>,
    pub loading: bool,
    pub audio: Option<Audio>,
    pub pos: f32,
    pub seeking: Option<f32>,
    pub volume: f32,
    pub radio_pending: bool,
    /// Songs in a row that couldn't be fetched; playback stops at 3.
    failures: u32,

    pub lyrics: Option<Lyrics>,
    pub lyrics_status: String,
    pub now_playing_open: bool,
    pub tab: Tab,
    pub dialog: Option<Dialog>,

    /// Thumbnails with their pixel size (needed to crop 16:9 video thumbnails square).
    pub images: HashMap<String, Thumb>,
    requested: HashSet<String>,

    pub disliked: HashSet<String>,
    pub menu: Option<MenuFor>,
    pub menu_pos: Point,
    cursor: Point,
    pub window: Size,
    /// Redraw every frame until then (fades, slides).
    anim_until: Instant,
    /// When the current page appeared, for its slide-in.
    pub page_shown: Instant,
    /// Lyrics line being sung, and when it changed.
    pub lyric_line: Option<usize>,
    pub lyric_changed: Instant,
    /// Background wash: fades from one cover's color to the next.
    tint_from: Option<Color>,
    tint_to: Option<Color>,
    tint_changed: Instant,
    /// `RYM_BENCH=<file>`: scroll the home page by itself and write frame times there.
    bench: Option<Bench>,
}

pub struct Bench {
    out: PathBuf,
    start: Option<Instant>,
    frames: Vec<Instant>,
}

pub const HOME_SCROLL: &str = "home-scroll";

/// 0 to 1 over `ms` since `since`, easing out (fast start, soft landing).
pub fn ease(since: Instant, ms: f32) -> f32 {
    let t = (since.elapsed().as_secs_f32() * 1000.0 / ms).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn long_ago() -> Instant {
    Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now)
}

impl Gui {
    /// State with no audio device and no network, for drawing tests.
    pub fn empty() -> Self {
        Gui {
            signed_in_screen: true,
            busy: false,
            status: String::new(),
            api: None,
            config: Config::default(),
            page: Page::Home,
            page_title: "For you".into(),
            tracks: Vec::new(),
            search: String::new(),
            playlists: Vec::new(),
            liked: HashSet::new(),
            queue: Vec::new(),
            current: None,
            loading: false,
            audio: None,
            pos: 0.0,
            seeking: None,
            volume: 0.8,
            radio_pending: false,
            failures: 0,
            lyrics: None,
            lyrics_status: String::new(),
            now_playing_open: false,
            tab: Tab::Lyrics,
            dialog: None,
            images: HashMap::new(),
            requested: HashSet::new(),
            disliked: HashSet::new(),
            menu: None,
            menu_pos: Point::ORIGIN,
            cursor: Point::ORIGIN,
            window: Size::new(1280.0, 800.0),
            anim_until: Instant::now(),
            page_shown: long_ago(),
            lyric_line: None,
            lyric_changed: long_ago(),
            tint_from: None,
            tint_to: None,
            tint_changed: long_ago(),
            bench: std::env::var_os("RYM_BENCH").map(|out| Bench { out: out.into(), start: None, frames: Vec::new() }),
        }
    }

    fn boot() -> (Self, Task<Message>) {
        let mut gui = Gui::empty();
        gui.signed_in_screen = false;
        gui.config = Config::load();
        stream::use_source(gui.config.source);
        stream::download_best(gui.config.download_best);
        stream::download_parallel(gui.config.download_connections, gui.config.download_piece_mb);
        match Audio::new() {
            Ok(a) => {
                a.set_volume(gui.volume);
                gui.audio = Some(a);
            }
            Err(e) => gui.status = format!("No audio device: {e}"),
        }
        let task = match std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty()) {
            Some(cookie) => {
                gui.busy = true;
                gui.status = "Signing in...".into();
                Task::perform(Api::with_cookie(cookie), |r| Message::LoggedIn(s(r)))
            }
            None => Task::none(),
        };
        (gui, task)
    }

    fn title(&self) -> String {
        match self.now_playing() {
            Some(t) => format!("{} · {} — RymPlayer", t.title, t.artist),
            None => "RymPlayer".into(),
        }
    }

    pub fn palette(&self) -> style::Palette {
        style::palette(self.config.gui_theme)
    }

    fn theme(&self) -> Theme {
        style::theme(self.palette())
    }

    fn subscription(&self) -> Subscription<Message> {
        // Only window resizes: listening to every mouse move would rebuild the whole interface
        // each time the mouse moves. Right-click positions come from `right_click::tracker`.
        let mut subs = vec![iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Window(window::Event::Resized(size)) => Some(Message::Resized(size)),
            _ => None,
        })];
        if self.current.is_some() {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick));
        }
        if Instant::now() < self.anim_until {
            subs.push(window::frames().map(Message::Frame));
        }
        Subscription::batch(subs)
    }

    /// The background wash color right now and how strong it is (fades between songs).
    pub fn tint(&self) -> (Option<Color>, f32) {
        let k = ease(self.tint_changed, 900.0);
        match (self.tint_from, self.tint_to) {
            (Some(a), Some(b)) => (Some(style::mix(a, b, k)), 1.0),
            (None, Some(b)) => (Some(b), k),
            (Some(a), None) => (Some(a), 1.0 - k),
            (None, None) => (None, 0.0),
        }
    }

    /// Starts a fade when the playing song's cover color is known (or changes).
    fn refresh_tint(&mut self) {
        let target = if self.config.gui_tint {
            self.now_playing().and_then(|t| self.image(t.thumb.as_ref(), ROW_PX)).and_then(|t| t.tint)
        } else {
            None
        };
        if target != self.tint_to {
            self.tint_from = self.tint().0;
            self.tint_to = target;
            self.tint_changed = Instant::now();
            self.animate(950);
        }
    }

    /// Bench mode: once the home page has had time to load its covers, scroll down and up for a
    /// few seconds, then write the frame times and quit.
    fn bench_frame(&mut self, now: Instant) -> Option<Task<Message>> {
        let loaded = self.page == Page::Home && !self.tracks.is_empty();
        let b = self.bench.as_mut()?;
        if !loaded {
            self.anim_until = Instant::now() + Duration::from_secs(1);
            return None;
        }
        let start = *b.start.get_or_insert(now + Duration::from_secs(8));
        self.anim_until = Instant::now() + Duration::from_secs(1);
        if now < start {
            return None;
        }
        b.frames.push(now);
        let t = now.duration_since(start).as_secs_f32();
        if t > 6.0 {
            let gaps: Vec<f32> = b.frames.windows(2).map(|w| w[1].duration_since(w[0]).as_secs_f32() * 1000.0).collect();
            let avg = gaps.iter().sum::<f32>() / gaps.len().max(1) as f32;
            let worst = gaps.iter().cloned().fold(0.0, f32::max);
            let _ = std::fs::write(&b.out, format!("frames={} avg_ms={avg:.1} fps={:.1} worst_ms={worst:.1}
", gaps.len(), 1000.0 / avg));
            return Some(iced::exit());
        }
        let dy = if t < 3.0 { 24.0 } else { -24.0 };
        Some(iced::widget::operation::scroll_by(HOME_SCROLL, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: dy }))
    }

    /// Keeps redrawing for `ms` so an animation can play.
    fn animate(&mut self, ms: u64) {
        self.anim_until = self.anim_until.max(Instant::now() + Duration::from_millis(ms));
    }

    /// Notices when the sung lyrics line changes, to animate it.
    fn track_lyrics(&mut self) {
        let line = self.lyrics.as_ref().and_then(|l| lyrics::current_line(&l.synced, self.pos));
        if line != self.lyric_line {
            self.lyric_line = line;
            self.lyric_changed = Instant::now();
            self.animate(450);
        }
    }

    pub fn logged_in(&self) -> bool {
        self.api.as_ref().is_some_and(Api::logged_in)
    }

    pub fn now_playing(&self) -> Option<&Track> {
        self.current.and_then(|i| self.queue.get(i))
    }

    pub fn is_playing(&self) -> bool {
        !self.loading && self.audio.as_ref().is_some_and(Audio::is_playing)
    }

    pub fn duration(&self) -> f32 {
        let from_audio = self.audio.as_ref().and_then(Audio::duration);
        from_audio.or_else(|| self.now_playing().and_then(|t| parse_duration(&t.duration))).unwrap_or(0.0)
    }

    /// Cached image for a thumbnail at `px`, if it has arrived.
    pub fn image(&self, url: Option<&String>, px: u32) -> Option<Thumb> {
        self.images.get(&api::thumb_sized(url?, px)).cloned()
    }

    /// Starts downloading thumbnails that aren't cached or on their way.
    fn want_images<'a>(&mut self, items: impl IntoIterator<Item = (&'a Option<String>, u32)>) -> Task<Message> {
        let mut tasks = Vec::new();
        for (url, px) in items {
            let Some(url) = url else { continue };
            let url = api::thumb_sized(url, px);
            if !self.requested.insert(url.clone()) {
                continue;
            }
            tasks.push(Task::perform(
                {
                    let url = url.clone();
                    async move {
                        let r = reqwest::get(&url).await.ok()?.error_for_status().ok()?;
                        let bytes = r.bytes().await.ok()?.to_vec();
                        let tint = px == ROW_PX;
                        tokio::task::spawn_blocking(move || Thumb::new(bytes, tint)).await.ok().flatten()
                    }
                },
                move |thumb| Message::Image(url.clone(), thumb),
            ));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        // Anything the user does closes an open right-click menu.
        let passive = matches!(
            message,
            Message::Cursor(_) | Message::Resized(_) | Message::Frame(_) | Message::Tick | Message::Image(..) | Message::Menu(_)
        );
        if !passive {
            self.menu = None;
        }
        match message {
            Message::SignIn => {
                self.busy = true;
                self.status = "Finish signing in in the Google window...".into();
                return Task::perform(
                    async {
                        let cookie = login::sign_in().await?;
                        Api::with_cookie(cookie).await
                    },
                    |r| Message::LoggedIn(s(r)),
                );
            }
            Message::Guest => {
                self.busy = true;
                return Task::perform(Api::guest(), |r| Message::LoggedIn(s(r)));
            }
            Message::LoggedIn(Ok(api)) => {
                self.busy = false;
                self.status.clear();
                self.signed_in_screen = true;
                let user = api.logged_in();
                self.api = Some(api.clone());
                let mut tasks = vec![self.open(Page::Home, "For you".into())];
                if user {
                    tasks.push(Task::perform(api.clone().library_playlists(), |r| Message::Playlists(s(r))));
                    // Liked songs, so hearts show everywhere.
                    tasks.push(Task::perform(api.playlist_tracks(api::LIKED_ID.into()), |r| {
                        Message::Loaded(Page::Playlist(api::LIKED_ID.into()), s(r))
                    }));
                }
                return Task::batch(tasks);
            }
            Message::LoggedIn(Err(e)) => {
                self.busy = false;
                self.signed_in_screen = false;
                self.status = format!("Sign-in failed: {e}");
            }
            Message::SignOut => {
                let _ = std::fs::remove_file(cookie_path());
                if let Some(a) = &mut self.audio {
                    a.stop();
                }
                let audio = self.audio.take();
                let config = self.config.clone();
                *self = Gui::empty();
                self.audio = audio;
                self.config = config;
                self.signed_in_screen = false;
            }
            Message::Playlists(Ok(p)) => {
                self.playlists = p;
                let wanted: Vec<_> = self.playlists.iter().map(|p| p.thumb.clone()).collect();
                return self.want_images(wanted.iter().map(|u| (u, CARD_PX)));
            }
            Message::Playlists(Err(e)) if e.contains("signed out") => {
                self.signed_in_screen = false;
                self.status = "Google ended your session. Sign in again.".into();
            }
            Message::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),

            Message::Open(page, title) => {
                self.now_playing_open = false;
                if matches!(page, Page::Library | Page::Settings) {
                    // Drawn from what's already loaded.
                    self.page = page;
                    self.page_title = title;
                    self.busy = false;
                    self.page_shown = Instant::now();
                    self.animate(400);
                    return Task::none();
                }
                return self.open(page, title);
            }
            Message::Loaded(page, result) => {
                if page == Page::Playlist(api::LIKED_ID.into())
                    && let Ok(t) = &result
                {
                    self.liked.extend(t.iter().map(|t| t.video_id.clone()));
                }
                if page != self.page {
                    return Task::none();
                }
                self.busy = false;
                match result {
                    Ok(t) => {
                        self.status.clear();
                        self.tracks = t;
                        self.page_shown = Instant::now();
                        self.animate(400);
                        let px = if self.page == Page::Home { CARD_PX } else { ROW_PX };
                        let wanted: Vec<_> = self.tracks.iter().map(|t| t.thumb.clone()).collect();
                        let header = self.tracks.first().map(|t| t.thumb.clone());
                        let mut tasks = vec![self.want_images(wanted.iter().map(|u| (u, px)))];
                        if let Some(h) = &header {
                            tasks.push(self.want_images([(h, CARD_PX)]));
                        }
                        if let Page::Radio(_) = self.page
                            && !self.tracks.is_empty()
                        {
                            tasks.push(self.play_list(0));
                        }
                        return Task::batch(tasks);
                    }
                    Err(e) => self.status = e,
                }
            }
            Message::SearchChanged(q) => self.search = q,
            Message::SearchSubmit => {
                let q = self.search.trim().to_string();
                if q.is_empty() {
                    return Task::none();
                }
                self.now_playing_open = false;
                return self.open(Page::Search(q.clone()), format!("Results for \"{q}\""));
            }

            Message::PlayRow(i) => {
                let Some(t) = self.tracks.get(i).cloned() else { return Task::none() };
                return match t.link {
                    Some(Link::Playlist(id)) => self.open(Page::Playlist(id), t.title),
                    Some(Link::Album(id)) => self.open(Page::Album(id), t.title),
                    Some(Link::Mix(id)) => self.open(Page::Mix(id), t.title),
                    None => self.play_list(i),
                };
            }
            Message::PlayAll => return self.play_list(0),
            Message::PlayQueue(i) => return self.play(i),
            Message::PlaySong(t) => {
                self.queue = vec![t];
                let play = self.play(0);
                return Task::batch([play, self.continue_with_radio()]);
            }
            Message::Fetched(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) != Some(&vid) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(path) => {
                        self.failures = 0;
                        let known = self
                            .now_playing()
                            .and_then(|t| parse_duration(&t.duration))
                            .or_else(|| stream::cached_duration(&vid));
                        if let Some(a) = &mut self.audio
                            && let Err(e) = a.play_file(&path, known)
                        {
                            self.status = format!("Can't play: {e}");
                        }
                        return self.prefetch_next();
                    }
                    Err(e) => {
                        self.failures += 1;
                        if self.failures >= 3 {
                            // Something is wrong for every song (YouTube, the connection, yt-dlp): stop and say why.
                            self.failures = 0;
                            self.status = format!("Stopped after 3 songs failed: {e}");
                            return Task::none();
                        }
                        self.status = format!("Skipped, download failed: {e}");
                        return self.next();
                    }
                }
            }
            Message::Toggle => {
                if let Some(a) = &self.audio {
                    a.toggle();
                }
            }
            Message::Next => return self.next(),
            Message::Prev => {
                let Some(i) = self.current else { return Task::none() };
                if self.pos > 3.0 || i == 0 {
                    if let Some(a) = &self.audio {
                        a.seek(0.0);
                    }
                } else {
                    return self.play(i - 1);
                }
            }
            Message::Seek(v) => self.seeking = Some(v),
            Message::SeekDone => {
                if let (Some(v), Some(a)) = (self.seeking.take(), &self.audio)
                    && a.is_loaded()
                {
                    a.seek(v.min(self.duration() - 0.5).max(0.0));
                    self.pos = v;
                }
            }
            Message::Volume(v) => {
                self.volume = v;
                if let Some(a) = &self.audio {
                    a.set_volume(v);
                }
            }
            Message::Tick => {
                let Some(a) = &mut self.audio else { return Task::none() };
                self.pos = a.position();
                if !self.loading && a.finished() {
                    a.stop();
                    return self.next();
                }
                self.track_lyrics();
            }
            Message::Radio(result) => {
                self.radio_pending = false;
                if let Ok(tracks) = result {
                    let fresh: Vec<Track> =
                        tracks.into_iter().filter(|t| !self.queue.iter().any(|q| q.video_id == t.video_id)).collect();
                    let wanted: Vec<_> = fresh.iter().map(|t| t.thumb.clone()).collect();
                    let was_done = self.current.is_some_and(|i| i + 1 >= self.queue.len()) && !self.is_playing() && !self.loading;
                    self.queue.extend(fresh);
                    let images = self.want_images(wanted.iter().map(|u| (u, ROW_PX)));
                    if was_done {
                        return Task::batch([images, self.next()]);
                    }
                    return images;
                }
            }
            Message::StartRadio(t) => {
                let page = Page::Radio(t.video_id.clone());
                return self.open(page, format!("Radio · {}", t.title));
            }
            Message::Lyrics(vid, found) => {
                if self.now_playing().map(|t| &t.video_id) == Some(&vid) {
                    self.lyrics_status = if found.is_some() { String::new() } else { "No lyrics for this song".into() };
                    self.lyrics = found;
                }
            }
            Message::Like(t) => {
                let Some(api) = self.api.clone() else { return Task::none() };
                let vid = t.video_id.clone();
                let like = !self.liked.contains(&vid);
                if like {
                    self.liked.insert(vid.clone());
                    self.disliked.remove(&vid);
                } else {
                    self.liked.remove(&vid);
                }
                return Task::perform(api.rate(vid.clone(), like), move |r| Message::Rated(vid.clone(), like, s(r)));
            }
            Message::Rated(vid, like, Err(e)) => {
                if like {
                    self.liked.remove(&vid);
                } else {
                    self.liked.insert(vid);
                }
                self.status = format!("Like failed: {e}");
            }
            Message::Rated(..) => {}

            Message::Image(url, Some(t)) => {
                self.images.insert(url, t);
                self.animate(320);
                self.refresh_tint();
            }
            Message::Image(url, None) => {
                // Allow a retry later.
                self.requested.remove(&url);
            }
            Message::ToggleNowPlaying => {
                self.now_playing_open = !self.now_playing_open && self.current.is_some();
                self.page_shown = Instant::now();
                self.animate(400);
            }
            Message::ShowTab(t) => self.tab = t,
            Message::ShowLyrics => {
                if self.now_playing_open && self.tab == Tab::Lyrics {
                    self.now_playing_open = false;
                } else if self.current.is_some() {
                    self.now_playing_open = true;
                    self.tab = Tab::Lyrics;
                }
                self.page_shown = Instant::now();
                self.animate(400);
            }
            Message::SetTheme(i) => {
                self.config.gui_theme = i % style::PALETTES.len();
                self.config.save();
            }
            Message::Connections(step) => {
                self.config.download_connections = (self.config.download_connections as i32 + step).clamp(1, 16) as u8;
                stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
                self.config.save();
            }
            Message::NextPiece => {
                let sizes = rymplayer::config::PIECE_SIZES;
                let i = sizes.iter().position(|s| *s == self.config.download_piece_mb).unwrap_or(0);
                self.config.download_piece_mb = sizes[(i + 1) % sizes.len()];
                stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
                self.config.save();
            }
            Message::ToggleDownload => {
                self.config.download_best = !self.config.download_best;
                stream::download_best(self.config.download_best);
                self.config.save();
            }
            Message::ToggleSource => {
                self.config.source = self.config.source.next();
                stream::use_source(self.config.source);
                self.config.save();
                self.status = format!("Songs now come through {}", self.config.source.label());
            }
            Message::ToggleGpu => {
                self.config.gui_renderer = self.config.gui_renderer.next();
                self.config.save();
                self.status = "Restart RymPlayer to switch the renderer".into();
            }
            Message::ToggleTint => {
                self.config.gui_tint = !self.config.gui_tint;
                self.config.save();
                self.refresh_tint();
            }

            Message::NewPlaylist(song) => self.dialog = Some(Dialog::NewPlaylist(String::new(), song)),
            Message::NameChanged(n) => {
                if let Some(Dialog::NewPlaylist(name, _)) = &mut self.dialog {
                    *name = n;
                }
            }
            Message::CreatePlaylist => {
                let Some(Dialog::NewPlaylist(name, song)) = self.dialog.take() else { return Task::none() };
                let (Some(api), name) = (self.api.clone(), name.trim().to_string()) else { return Task::none() };
                if name.is_empty() {
                    return Task::none();
                }
                self.status = format!("Creating \"{name}\"...");
                let vid = song.map(|t| t.video_id);
                return Task::perform(api.create_playlist(name), move |r| Message::Created(s(r), vid.clone()));
            }
            Message::Created(Ok(p), song) => {
                self.status = format!("Created \"{}\"", p.title);
                self.playlists.insert(0, p.clone());
                if let Some(vid) = song {
                    return self.add_to(p, vid);
                }
            }
            Message::Created(Err(e), _) => self.status = format!("Couldn't create the playlist: {e}"),
            Message::AddTo(t) => self.dialog = Some(Dialog::AddTo(t)),
            Message::Pick(p) => {
                let Some(Dialog::AddTo(t)) = self.dialog.take() else { return Task::none() };
                return self.add_to(p, t.video_id);
            }
            Message::Added(title, Ok(())) => self.status = format!("Added to \"{title}\""),
            Message::Added(title, Err(e)) => self.status = format!("Couldn't add to \"{title}\": {e}"),
            Message::CloseDialog => self.dialog = None,

            Message::Menu(m) => {
                // Keep the menu inside the window.
                let (w, h) = (view::MENU_W, view::menu_height(self, &m));
                let x = self.cursor.x.min(self.window.width - w - 8.0).max(8.0);
                let y = if self.cursor.y + h > self.window.height - 8.0 { (self.cursor.y - h).max(8.0) } else { self.cursor.y };
                self.menu_pos = Point::new(x, y);
                self.menu = Some(m);
            }
            Message::CloseMenu => {}
            Message::Cursor(p) => self.cursor = p,
            Message::Resized(s) => self.window = s,
            Message::Frame(now) => {
                if let Some(a) = &self.audio {
                    self.pos = a.position();
                }
                self.track_lyrics();
                if let Some(task) = self.bench_frame(now) {
                    return task;
                }
            }
            Message::PlayNext(t) => {
                let Some(i) = self.current else { return self.update(Message::PlaySong(t)) };
                self.queue.insert(i + 1, t);
                self.status = "Plays next".into();
            }
            Message::AddToQueue(t) => {
                self.queue.push(t);
                self.status = "Added to queue".into();
                if self.current.is_none() {
                    return self.play(self.queue.len() - 1);
                }
            }
            Message::RemoveFromQueue(i) => {
                if i >= self.queue.len() {
                    return Task::none();
                }
                self.queue.remove(i);
                match self.current {
                    Some(c) if i < c => self.current = Some(c - 1),
                    Some(c) if i == c => {
                        if c < self.queue.len() {
                            return self.play(c);
                        }
                        self.current = None;
                        if let Some(a) = &mut self.audio {
                            a.stop();
                        }
                    }
                    _ => {}
                }
            }
            Message::DismissQueue => {
                if let Some(c) = self.current {
                    let t = self.queue[c].clone();
                    self.queue = vec![t];
                    self.current = Some(0);
                }
            }
            Message::Download(t) => {
                self.status = format!("Downloading \"{}\"...", t.title);
                return Task::perform(download(t), |r| Message::Downloaded(s(r)));
            }
            Message::Downloaded(Ok(p)) => self.status = format!("Saved to {}", p.display()),
            Message::Downloaded(Err(e)) => self.status = format!("Download failed: {e}"),
            Message::Share(t) => {
                self.status = "Link copied".into();
                return iced::clipboard::write(format!("https://music.youtube.com/watch?v={}", t.video_id));
            }
            Message::GoAlbum(t) => {
                let Some(id) = t.album_id.clone() else { return Task::none() };
                self.now_playing_open = false;
                return self.open(Page::Album(id), t.album);
            }
            Message::GoArtist(t) => {
                let Some(id) = t.artist_id.clone() else { return Task::none() };
                self.now_playing_open = false;
                let name = t.artist.split(", ").next().unwrap_or(&t.artist).to_string();
                return self.open(Page::Artist(id), name);
            }
            Message::Dislike(t) => {
                let Some(api) = self.api.clone() else { return Task::none() };
                let vid = t.video_id.clone();
                let on = !self.disliked.contains(&vid);
                let mut tasks = Vec::new();
                if on {
                    self.disliked.insert(vid.clone());
                    self.liked.remove(&vid);
                    // Like YouTube Music: a thumbs-down on the playing song skips it.
                    if self.now_playing().map(|n| &n.video_id) == Some(&vid) {
                        tasks.push(self.next());
                    }
                } else {
                    self.disliked.remove(&vid);
                }
                tasks.push(Task::perform(api.dislike(vid.clone(), on), move |r| Message::Disliked(vid.clone(), on, s(r))));
                return Task::batch(tasks);
            }
            Message::Disliked(vid, on, Err(e)) => {
                if on {
                    self.disliked.remove(&vid);
                } else {
                    self.disliked.insert(vid);
                }
                self.status = format!("Couldn't rate: {e}");
            }
            Message::Disliked(..) => {}
            Message::Ignore => {}
        }
        Task::none()
    }

    fn open(&mut self, page: Page, title: String) -> Task<Message> {
        let Some(api) = self.api.clone() else { return Task::none() };
        self.page = page.clone();
        self.page_title = title;
        self.busy = true;
        self.tracks.clear();
        Task::perform(
            {
                let page = page.clone();
                async move {
                    match page {
                        Page::Home => api.home().await,
                        Page::Search(q) => api.search(q).await,
                        Page::History => api.history().await,
                        Page::Playlist(id) => api.playlist_tracks(id).await,
                        Page::Album(id) => api.album_tracks(id).await,
                        Page::Mix(id) => api.mix_tracks(id).await,
                        Page::Radio(vid) => api.radio(vid).await,
                        Page::Artist(id) => api.artist_songs(id).await,
                        Page::Library | Page::Settings => Ok(Vec::new()),
                    }
                }
            },
            move |r| Message::Loaded(page.clone(), s(r)),
        )
    }

    /// Plays row `i`; the queue becomes the list's songs.
    fn play_list(&mut self, i: usize) -> Task<Message> {
        let Some(vid) = self.tracks.get(i).map(|t| t.video_id.clone()) else { return Task::none() };
        self.queue = self.tracks.iter().filter(|t| t.link.is_none()).cloned().collect();
        match self.queue.iter().position(|t| t.video_id == vid) {
            Some(q) => self.play(q),
            None => Task::none(),
        }
    }

    fn play(&mut self, i: usize) -> Task<Message> {
        let Some(track) = self.queue.get(i).cloned() else { return Task::none() };
        self.current = Some(i);
        self.pos = 0.0;
        self.seeking = None;
        self.lyrics = None;
        self.lyrics_status = "Looking for lyrics...".into();
        self.loading = true;
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        let vid = track.video_id.clone();
        let fetch = Task::perform(stream::fetch(vid.clone()), {
            let vid = vid.clone();
            move |r| Message::Fetched(vid.clone(), s(r))
        });
        let api = self.api.clone();
        let lyr = Task::perform(
            {
                let t = track.clone();
                let api = api.clone();
                async move {
                    let dur = parse_duration(&t.duration);
                    match lyrics::lrclib(t.title, t.artist, t.album, dur).await {
                        Ok(Some(l)) => Some(l),
                        _ => match api {
                            Some(api) => api.lyrics(t.video_id).await.ok().flatten(),
                            None => None,
                        },
                    }
                }
            },
            {
                let vid = vid.clone();
                move |l| Message::Lyrics(vid.clone(), l)
            },
        );
        let report = match api {
            Some(api) => Task::perform(api.report_play(vid), |_| Message::Ignore),
            None => Task::none(),
        };
        let art = self.want_images([(&track.thumb, ART_PX), (&track.thumb, ROW_PX)]);
        self.refresh_tint();
        Task::batch([fetch, lyr, report, art])
    }

    fn next(&mut self) -> Task<Message> {
        let Some(i) = self.current else { return Task::none() };
        if i + 1 < self.queue.len() {
            self.play(i + 1)
        } else {
            self.continue_with_radio()
        }
    }

    fn continue_with_radio(&mut self) -> Task<Message> {
        let (Some(api), Some(vid)) = (self.api.clone(), self.now_playing().map(|t| t.video_id.clone())) else {
            return Task::none();
        };
        if self.radio_pending {
            return Task::none();
        }
        self.radio_pending = true;
        Task::perform(api.radio(vid), |r| Message::Radio(s(r)))
    }

    fn prefetch_next(&self) -> Task<Message> {
        match self.current.and_then(|i| self.queue.get(i + 1)) {
            Some(t) if stream::cached(&t.video_id).is_none() => {
                Task::perform(stream::fetch(t.video_id.clone()), |_| Message::Ignore)
            }
            _ => Task::none(),
        }
    }

    fn add_to(&mut self, p: Playlist, video_id: String) -> Task<Message> {
        let Some(api) = self.api.clone() else { return Task::none() };
        self.status = format!("Adding to \"{}\"...", p.title);
        let title = p.title.clone();
        Task::perform(api.add_to_playlist(p.id, video_id), move |r| Message::Added(title.clone(), s(r)))
    }
}

async fn download(t: Track) -> anyhow::Result<PathBuf> {
    stream::save_to_music(&t.video_id, &t.artist, &t.title).await
}

#[cfg(test)]
mod shots {
    //! Renders screens to PNG without opening a window: `cargo test --bin RymPlayer-GUI -- --ignored`.
    //! Output lands in `target/gui-shots/`.

    use super::*;
    use iced_test::simulator::Simulator;

    fn shot(gui: &Gui, name: &str) {
        let theme = gui.theme();
        let settings = iced_test::core::Settings { default_font: view::FONT, ..Default::default() };
        let mut sim = Simulator::with_size(settings, Size::new(1280.0, 800.0), gui.view());
        let snap = sim.snapshot(&theme).expect("snapshot");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/gui-shots");
        let _ = std::fs::create_dir_all(&dir);
        for f in std::fs::read_dir(&dir).unwrap().flatten() {
            if f.file_name().to_string_lossy().starts_with(name) {
                let _ = std::fs::remove_file(f.path());
            }
        }
        assert!(snap.matches_image(dir.join(name)).expect("write png"));
    }

    async fn fetch_images(gui: &mut Gui, items: Vec<(Option<String>, u32)>) {
        let mut set = tokio::task::JoinSet::new();
        for (url, px) in items {
            let Some(url) = url else { continue };
            let url = api::thumb_sized(&url, px);
            set.spawn(async move {
                let bytes = reqwest::get(&url).await.ok()?.bytes().await.ok()?;
                Some((url, bytes.to_vec(), px == ROW_PX))
            });
        }
        while let Some(r) = set.join_next().await {
            if let Ok(Some((url, bytes, tint))) = r
                && let Some(mut t) = Thumb::new(bytes, tint)
            {
                // Already faded in by the time the shot is taken.
                t.loaded = long_ago();
                gui.images.insert(url, t);
            }
        }
    }

    /// Uses the signed-in session and real data; only counts are printed.
    #[tokio::test]
    #[ignore]
    async fn screens() {
        let cookie = std::fs::read_to_string(cookie_path()).expect("signed-in cookie");
        let api = Api::with_cookie(cookie).await.expect("login");
        let mut gui = Gui::empty();
        gui.api = Some(api.clone());
        gui.playlists = api.clone().library_playlists().await.unwrap_or_default();

        let home = api.clone().home().await.expect("home");
        println!("home items: {}", home.len());
        fetch_images(&mut gui, home.iter().map(|t| (t.thumb.clone(), CARD_PX)).collect()).await;
        gui.tracks = home;

        let liked = api.clone().playlist_tracks(api::LIKED_ID.into()).await.expect("liked");
        gui.liked = liked.iter().map(|t| t.video_id.clone()).collect();
        fetch_images(&mut gui, liked.iter().take(30).map(|t| (t.thumb.clone(), ROW_PX)).collect()).await;
        fetch_images(&mut gui, liked.iter().take(1).map(|t| (t.thumb.clone(), CARD_PX)).collect()).await;
        fetch_images(&mut gui, liked.iter().take(1).map(|t| (t.thumb.clone(), ART_PX)).collect()).await;

        // Something playing, for the bar and the player page.
        gui.queue = liked.iter().take(25).cloned().collect();
        gui.current = Some(0);
        gui.pos = 71.0;
        gui.lyrics = Some(Lyrics {
            synced: lyrics::parse_lrc(
                "[00:58.00]Under the neon, under the rain\n[01:04.00]Every street sings it again\n[01:10.00]I hear the city calling my name\n[01:16.00]Running till the morning light\n[01:22.00]Nothing ever feels the same\n[01:28.00]Hold on, hold on tonight",
            ),
            plain: Vec::new(),
            source: "LRCLIB".into(),
        });

        gui.refresh_tint();
        gui.tint_changed = long_ago();
        gui.playlists = api.clone().library_playlists().await.unwrap_or_default();
        let covers = gui.playlists.iter().map(|p| (p.thumb.clone(), CARD_PX)).collect();
        fetch_images(&mut gui, covers).await;
        for theme in [0, 2, 3, 4] {
            gui.config.gui_theme = theme;
            let tag = style::PALETTES[theme].name.to_lowercase().replace(' ', "-");
            gui.page = Page::Home;
            gui.page_title = "For you".into();
            shot(&gui, &format!("{tag}-home"));

            let home = std::mem::replace(&mut gui.tracks, liked.clone());
            gui.page = Page::Playlist(api::LIKED_ID.into());
            gui.page_title = "Liked Music".into();
            shot(&gui, &format!("{tag}-playlist"));

            gui.now_playing_open = true;
            gui.tab = Tab::Lyrics;
            shot(&gui, &format!("{tag}-lyrics"));
            gui.now_playing_open = false;

            gui.page = Page::Library;
            shot(&gui, &format!("{tag}-library"));
            gui.page = Page::Settings;
            shot(&gui, &format!("{tag}-settings"));
            gui.page = Page::Playlist(api::LIKED_ID.into());

            gui.menu = Some(MenuFor { track: liked[2].clone(), queue_index: None });
            gui.menu_pos = Point::new(900.0, 300.0);
            shot(&gui, &format!("{tag}-menu"));
            gui.menu = None;
            gui.tracks = home;
        }
        gui.signed_in_screen = false;
        gui.config.gui_theme = 0;
        shot(&gui, "login");
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    fn song(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artist: "A".into(),
            album: String::new(),
            duration: "3:00".into(),
            thumb: None,
            album_id: None,
            artist_id: None,
            link: None,
        }
    }

    fn ids(g: &Gui) -> Vec<&str> {
        g.queue.iter().map(|t| t.video_id.as_str()).collect()
    }

    #[test]
    fn queue_actions() {
        let mut g = Gui::empty();
        g.queue = vec![song("a"), song("b"), song("c")];
        g.current = Some(1);
        let _ = g.update(Message::PlayNext(song("x")));
        assert_eq!(ids(&g), ["a", "b", "x", "c"]);
        let _ = g.update(Message::AddToQueue(song("z")));
        assert_eq!(ids(&g), ["a", "b", "x", "c", "z"]);
        // Removing a song before the playing one keeps the same song playing.
        let _ = g.update(Message::RemoveFromQueue(0));
        assert_eq!(g.now_playing().map(|t| t.video_id.as_str()), Some("b"));
        let _ = g.update(Message::DismissQueue);
        assert_eq!(ids(&g), ["b"]);
        assert_eq!(g.current, Some(0));
    }

    #[test]
    fn stops_after_three_failed_songs() {
        let mut g = Gui::empty();
        g.queue = (0..6).map(|i| song(&format!("s{i}"))).collect();
        g.current = Some(0);
        for _ in 0..3 {
            let vid = g.now_playing().unwrap().video_id.clone();
            let _ = g.update(Message::Fetched(vid, Err("ERROR: refused".into())));
        }
        // Two skips, then it stays on the third song and says why instead of running on.
        assert_eq!(g.now_playing().map(|t| t.video_id.as_str()), Some("s2"));
        assert!(g.status.starts_with("Stopped after 3 songs failed"), "{}", g.status);
    }

    #[test]
    fn menu_stays_inside_the_window() {
        let mut g = Gui::empty();
        g.window = Size::new(1000.0, 700.0);
        g.cursor = Point::new(990.0, 690.0);
        let _ = g.update(Message::Menu(MenuFor { track: song("a"), queue_index: Some(0) }));
        let h = view::menu_height(&g, g.menu.as_ref().unwrap());
        assert!(g.menu_pos.x + view::MENU_W <= 1000.0);
        assert!(g.menu_pos.y + h <= 700.0);
        // Any action closes it.
        let _ = g.update(Message::Toggle);
        assert!(g.menu.is_none());
    }
}

#[cfg(test)]
mod frame_cost {
    //! How long one full software-rendered frame takes: `cargo test --release --bin RymPlayer-GUI frame_cost -- --ignored --nocapture`.

    use super::*;
    use iced_test::simulator::Simulator;

    fn cover(seed: u8) -> Thumb {
        let mut px = Vec::with_capacity(360 * 360 * 4);
        for y in 0..360u32 {
            for x in 0..360u32 {
                px.extend_from_slice(&[(x as u8).wrapping_add(seed), (y as u8).wrapping_mul(3), seed.wrapping_mul(7), 255]);
            }
        }
        Thumb { handle: image::Handle::from_rgba(360, 360, px), width: 360, height: 360, loaded: long_ago(), tint: None }
    }

    #[test]
    #[ignore]
    fn home_frame() {
        let mut gui = Gui::empty();
        gui.window = Size::new(1920.0, 1200.0);
        gui.tracks = (0..90)
            .map(|i| Track {
                video_id: format!("v{i}"),
                title: format!("Song title number {i} with a longer name"),
                artist: "Some Artist".into(),
                album: format!("Section {}", i / 15),
                duration: "3:30".into(),
                thumb: Some(format!("https://lh3.googleusercontent.com/x{i}=w120-h120")),
                album_id: None,
                artist_id: None,
                link: None,
            })
            .collect();
        for (i, t) in gui.tracks.clone().iter().enumerate() {
            gui.images.insert(api::thumb_sized(t.thumb.as_ref().unwrap(), CARD_PX), cover(i as u8));
        }
        for theme in [0usize, 4] {
            gui.config.gui_theme = theme;
            let settings = iced_test::core::Settings { default_font: view::FONT, ..Default::default() };
            let mut sim = Simulator::with_size(settings, Size::new(1920.0, 1200.0), gui.view());
            let t = Instant::now();
            for _ in 0..5 {
                sim.snapshot(&gui.theme()).unwrap();
            }
            println!("{}: {:.1} ms per full frame", style::PALETTES[theme].name, t.elapsed().as_secs_f64() * 1000.0 / 5.0);
        }
    }
}
