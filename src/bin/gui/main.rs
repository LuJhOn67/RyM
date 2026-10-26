#![windows_subsystem = "windows"]
//! RymPlayer with a window: YouTube Music / Spotify inspired, software-rendered to stay light.

mod icons;
mod style;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::image;
use iced::{Size, Subscription, Task, Theme, window};

use rymplayer::api::{self, Api, Link, Playlist, Track};
use rymplayer::audio::Audio;
use rymplayer::config::Config;
use rymplayer::lyrics::{self, Lyrics};
use rymplayer::{cookie_path, login, parse_duration, stream, ytdlp_cookie_path};

fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    iced::application(Gui::boot, Gui::update, Gui::view)
        .title(Gui::title)
        .subscription(Gui::subscription)
        .theme(Gui::theme)
        .default_font(view::FONT)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(960.0, 620.0)),
            ..window::Settings::default()
        })
        .centered()
        .run()
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
    CycleTheme,

    NewPlaylist(Option<Track>),
    NameChanged(String),
    CreatePlaylist,
    Created(Result<Playlist, String>, Option<String>),
    AddTo(Track),
    Pick(Playlist),
    Added(String, Result<(), String>),
    CloseDialog,
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
}

impl Thumb {
    /// Square covers are kept encoded (iced decodes them when drawn). Video thumbnails are
    /// 16:9 or letterboxed, so they're decoded once here and cut to their centered square.
    pub fn new(bytes: Vec<u8>) -> Option<Thumb> {
        let reader = || ::image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok();
        let (width, height) = reader()?.into_dimensions().ok()?;
        if width == height {
            return Some(Thumb { handle: image::Handle::from_bytes(bytes), width, height });
        }
        let img = reader()?.decode().ok()?;
        let side = width.min(height);
        let square = img.crop_imm((width - side) / 2, (height - side) / 2, side, side).to_rgba8();
        Some(Thumb { handle: image::Handle::from_rgba(side, side, square.into_raw()), width: side, height: side })
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

    pub lyrics: Option<Lyrics>,
    pub lyrics_status: String,
    pub now_playing_open: bool,
    pub tab: Tab,
    pub dialog: Option<Dialog>,

    /// Thumbnails with their pixel size (needed to crop 16:9 video thumbnails square).
    pub images: HashMap<String, Thumb>,
    requested: HashSet<String>,
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
            lyrics: None,
            lyrics_status: String::new(),
            now_playing_open: false,
            tab: Tab::UpNext,
            dialog: None,
            images: HashMap::new(),
            requested: HashSet::new(),
        }
    }

    fn boot() -> (Self, Task<Message>) {
        let mut gui = Gui::empty();
        gui.signed_in_screen = false;
        gui.config = Config::load();
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
        if self.current.is_some() {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
        } else {
            Subscription::none()
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

    fn cookie_file(&self) -> Option<PathBuf> {
        let p = ytdlp_cookie_path();
        (self.logged_in() && p.exists()).then_some(p)
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
                        tokio::task::spawn_blocking(move || Thumb::new(bytes)).await.ok().flatten()
                    }
                },
                move |thumb| Message::Image(url.clone(), thumb),
            ));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
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
                    if let Ok(c) = std::fs::read_to_string(cookie_path()) {
                        let _ = stream::write_cookie_file(&c, &ytdlp_cookie_path());
                    }
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
                let _ = std::fs::remove_file(ytdlp_cookie_path());
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
            Message::Playlists(Ok(p)) => self.playlists = p,
            Message::Playlists(Err(e)) if e.contains("signed out") => {
                self.signed_in_screen = false;
                self.status = "Google ended your session. Sign in again.".into();
            }
            Message::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),

            Message::Open(page, title) => {
                self.now_playing_open = false;
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
            }
            Message::Image(url, None) => {
                // Allow a retry later.
                self.requested.remove(&url);
            }
            Message::ToggleNowPlaying => self.now_playing_open = !self.now_playing_open && self.current.is_some(),
            Message::ShowTab(t) => self.tab = t,
            Message::CycleTheme => {
                self.config.gui_theme = (self.config.gui_theme + 1) % style::PALETTES.len();
                self.config.save();
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
        let fetch = Task::perform(stream::fetch(vid.clone(), self.cookie_file()), {
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
                Task::perform(stream::fetch(t.video_id.clone(), self.cookie_file()), |_| Message::Ignore)
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
                Some((url, bytes.to_vec()))
            });
        }
        while let Some(r) = set.join_next().await {
            if let Ok(Some((url, bytes))) = r
                && let Some(t) = Thumb::new(bytes)
            {
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

        for theme in 0..style::PALETTES.len() {
            gui.config.gui_theme = theme;
            let tag = if theme == 0 { "ytm" } else { "spotify" };
            gui.page = Page::Home;
            gui.page_title = "For you".into();
            shot(&gui, &format!("{tag}-home"));

            let home = std::mem::replace(&mut gui.tracks, liked.clone());
            gui.page = Page::Playlist(api::LIKED_ID.into());
            gui.page_title = "Liked Music".into();
            shot(&gui, &format!("{tag}-playlist"));

            gui.now_playing_open = true;
            gui.tab = Tab::UpNext;
            shot(&gui, &format!("{tag}-player"));
            gui.tab = Tab::Lyrics;
            shot(&gui, &format!("{tag}-lyrics"));
            gui.now_playing_open = false;

            gui.dialog = Some(Dialog::AddTo(liked[0].clone()));
            shot(&gui, &format!("{tag}-add-to"));
            gui.dialog = None;
            gui.tracks = home;
        }
        gui.signed_in_screen = false;
        gui.config.gui_theme = 0;
        shot(&gui, "ytm-login");
    }
}
