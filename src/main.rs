mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::widgets::{ListState, TableState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use rymplayer::likes::{Likes, SYNC_EVERY};
use rymplayer::{api, audio, config, cookie_path, data_dir, feed, fmt_time, login, lyrics, parse_duration, stream};

use api::{Api, Link, Playlist, Track};
use audio::Audio;
use config::{Config, SETTINGS, Side};
use lyrics::Lyrics;

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();

    let (tx, rx) = unbounded_channel();
    let mut app = App::new(tx);
    app.start();
    let mut terminal = ratatui::init();
    // Optional: legacy Windows consoles reject it, and pasted text then arrives as key presses.
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    let result = app.run(&mut terminal, rx);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    ratatui::restore();
    result
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Login,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    Nav,
    Tracks,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Input {
    Search,
    Cookie,
    NewPlaylist,
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    ForYou,
    History,
    Search,
    Playlist(String),
    Album(String),
    Mix(String),
    /// YouTube Music radio started from a song.
    Radio(String),
}

#[derive(Debug, Clone)]
enum NavItem {
    ForYou,
    History,
    NewPlaylist,
    Playlist(Playlist),
}

/// Results of background work, sent back to the UI loop.
enum Event {
    LoggedIn(Result<Api, String>),
    /// Radio tracks to continue with after the queue ran out.
    Radio(Result<Vec<Track>, String>),
    Playlists(Result<Vec<Playlist>, String>),
    Tracks(Page, Result<Vec<Track>, String>),
    /// The song to play: from the cache, or still arriving (it plays while it downloads).
    Fetched(String, Result<stream::Playable, String>),
    /// The song's direct link for VLC.
    InVlc(String, Result<(String, Vec<(String, String)>), String>),
    Lyrics(String, Result<Option<Lyrics>, String>),
    Rated(String, bool, Result<(), String>),
    /// The account's liked songs, re-read now and then so likes made on the phone show here.
    Liked(Result<Vec<Track>, String>),
    /// A playlist was made; the song to put in it, if it was made from the picker.
    Created(Result<Playlist, String>, Option<String>),
    /// A song was added to the named playlist.
    Added(String, Result<(), String>),
    /// A song was saved to the Music folder.
    Downloaded(Result<PathBuf, String>),
}

fn s<T>(r: anyhow::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

struct App {
    /// VLC playing the current song, when "Play songs in" is VLC.
    vlc: Option<std::process::Child>,
    /// Songs in a row that couldn't be fetched; playback stops at 3.
    failures: u32,
    tx: UnboundedSender<Event>,
    mode: Mode,
    focus: Focus,
    input: Option<Input>,
    buffer: String,
    busy: bool,
    status: String,
    api: Option<Api>,

    nav: Vec<NavItem>,
    nav_state: ListState,
    page: Page,
    page_title: String,
    tracks: Vec<Track>,
    track_state: TableState,

    queue: Vec<Track>,
    current: Option<usize>,
    loading: bool,
    audio: Option<Audio>,
    pos: f32,
    volume: f32,

    lyrics: Option<Lyrics>,
    lyrics_status: String,
    liked: Likes,
    /// When the liked songs were last read from the account.
    likes_synced: Instant,
    /// Start playing this page as soon as it loads (radio started with `r`).
    play_when_loaded: Option<Page>,
    radio_pending: bool,
    /// Where things were drawn, for mouse clicks.
    areas: ui::Areas,
    config: Config,
    /// Selected row while the settings screen is open.
    settings: Option<usize>,
    /// Selected row while the add-to-playlist picker is open.
    picker: Option<usize>,
    /// Song the picker (or a new playlist) is for.
    pick_song: Option<Track>,
    /// The user's own playlists, for the picker.
    playlists: Vec<Playlist>,
}

impl App {
    fn new(tx: UnboundedSender<Event>) -> Self {
        App {
            failures: 0,
            vlc: None,
            tx,
            mode: Mode::Login,
            focus: Focus::Tracks,
            input: None,
            buffer: String::new(),
            busy: false,
            status: String::new(),
            api: None,
            nav: vec![NavItem::ForYou],
            nav_state: ListState::default().with_selected(Some(0)),
            page: Page::Search,
            page_title: "Search".into(),
            tracks: Vec::new(),
            track_state: TableState::default(),
            queue: Vec::new(),
            current: None,
            loading: false,
            audio: None,
            pos: 0.0,
            volume: 0.8,
            lyrics: None,
            lyrics_status: String::new(),
            liked: Likes::default(),
            likes_synced: Instant::now(),
            play_when_loaded: None,
            radio_pending: false,
            areas: ui::Areas::default(),
            config: Config::default(),
            settings: None,
            picker: None,
            pick_song: None,
            playlists: Vec::new(),
        }
    }

    /// Opens the audio device and signs in with a saved session, if any.
    fn start(&mut self) {
        self.config = Config::load();
        stream::use_source(self.config.source);
        stream::download_best(self.config.download_best);
        stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
        match Audio::new() {
            Ok(a) => {
                a.set_volume(self.volume);
                self.audio = Some(a);
            }
            Err(e) => self.status = format!("No audio device: {e}"),
        }
        if let Some(cookie) = std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty()) {
            self.busy = true;
            self.status = "Signing in...".into();
            // The last home feed shows at once while signing in; the fresh one replaces it.
            let saved = rymplayer::page_cache::load_home();
            if !saved.is_empty() {
                self.mode = Mode::Main;
                self.rebuild_nav(Vec::new());
                self.page = Page::ForYou;
                self.page_title = "For you".into();
                self.track_state.select(Some(0));
                self.tracks = saved;
            }
            self.spawn(async move { Event::LoggedIn(s(Api::with_cookie(cookie).await)) });
        }
    }

    fn spawn(&self, fut: impl Future<Output = Event> + Send + 'static) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(fut.await);
        });
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal, mut rx: UnboundedReceiver<Event>) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| ui::draw(f, self))?;
            while let Ok(ev) = rx.try_recv() {
                self.handle(ev);
            }
            // Drain everything pending before redrawing: a pasted cookie can be thousands of keys.
            let mut wait = Duration::from_millis(100);
            while event::poll(wait)? {
                wait = Duration::ZERO;
                match event::read()? {
                    TermEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        if !self.key(k) {
                            return Ok(());
                        }
                    }
                    TermEvent::Paste(text) if self.input.is_some() => {
                        self.buffer.push_str(text.trim_end_matches(['\r', '\n']));
                    }
                    TermEvent::Mouse(m) if self.mode == Mode::Main && self.input.is_none() => self.mouse(m),
                    _ => {}
                }
            }
            self.tick();
        }
    }

    fn logged_in(&self) -> bool {
        self.api.as_ref().is_some_and(Api::logged_in)
    }

    fn now_playing(&self) -> Option<&Track> {
        self.current.and_then(|i| self.queue.get(i))
    }

    // ------------------------------------------------------------ background results

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::LoggedIn(Ok(api)) => {
                self.busy = false;
                self.status.clear();
                let user = api.logged_in();
                self.api = Some(api.clone());
                self.mode = Mode::Main;
                if user {
                    self.spawn(async move { Event::Playlists(s(api.library_playlists().await)) });
                }
                self.rebuild_nav(Vec::new());
                self.likes_synced = Instant::now();
                self.open(Page::ForYou, "For you".into());
            }
            Event::Radio(result) => {
                self.radio_pending = false;
                match result {
                    Ok(tracks) => {
                        let fresh: Vec<Track> = tracks
                            .into_iter()
                            .filter(|t| !self.queue.iter().any(|q| q.video_id == t.video_id))
                            .collect();
                        if fresh.is_empty() {
                            self.status = "Radio has nothing new to play".into();
                            return;
                        }
                        self.queue.extend(fresh);
                        self.next();
                    }
                    Err(e) => self.status = format!("Radio: {e}"),
                }
            }
            Event::LoggedIn(Err(e)) => {
                self.busy = false;
                self.mode = Mode::Login;
                self.status = format!("Sign-in failed: {e}");
            }
            Event::Playlists(Ok(p)) => self.rebuild_nav(p),
            Event::Playlists(Err(e)) if e.contains("signed out") => {
                // The saved session is dead: send the user to sign in again.
                self.mode = Mode::Login;
                self.busy = false;
                self.status = "Google ended your session. Press g to sign in again.".into();
            }
            Event::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),
            Event::Tracks(page, result) => {
                if page != self.page {
                    return;
                }
                self.busy = false;
                match result {
                    Ok(t) => {
                        if page == Page::ForYou {
                            rymplayer::page_cache::save_home(&t);
                        }
                        if page == Page::Playlist(api::LIKED_ID.into()) {
                            self.liked.sync(t.iter().map(|t| t.video_id.clone()));
                            self.likes_synced = Instant::now();
                        }
                        self.status.clear();
                        self.track_state.select((!t.is_empty()).then_some(0));
                        // The first songs' links are looked up now, so a tap starts at once.
                        tokio::spawn(stream::look_ahead(t.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect()));
                        self.tracks = t;
                        if self.play_when_loaded.take() == Some(page) {
                            self.play_from_list(0);
                        }
                    }
                    Err(e) => self.status = e,
                }
            }
            Event::InVlc(vid, result) => {
                let Some(t) = self.now_playing().filter(|t| t.video_id == vid).cloned() else { return };
                self.loading = false;
                match (result, rymplayer::vlc::find()) {
                    (Ok((url, headers)), Some(exe)) => {
                        rymplayer::vlc::stop(&mut self.vlc);
                        match rymplayer::vlc::play(&exe, &url, &headers, &format!("{} - {}", t.artist, t.title)) {
                            Ok(child) => {
                                self.vlc = Some(child);
                                self.status = "Playing in VLC".into();
                            }
                            Err(e) => self.status = format!("VLC: {e}"),
                        }
                    }
                    (Err(e), _) => self.status = format!("Can't play: {e}"),
                    (_, None) => self.status = "VLC isn't installed".into(),
                }
            }
            Event::Fetched(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) != Some(&vid) {
                    return;
                }
                self.loading = false;
                match result {
                    Ok(song) => {
                        self.failures = 0;
                        let known = parse_duration(&self.now_playing().map(|t| t.duration.clone()).unwrap_or_default())
                            .or_else(|| stream::cached_duration(&vid));
                        if let Some(a) = &mut self.audio {
                            match a.play(&song, known) {
                                Ok(()) => self.status.clear(),
                                Err(e) => self.status = format!("Can't play: {e}"),
                            }
                        }
                        self.prefetch_next();
                    }
                    Err(e) => {
                        self.failures += 1;
                        if self.failures >= 3 {
                            // Something is wrong for every song (YouTube, the connection, yt-dlp): stop and say why.
                            self.failures = 0;
                            self.status = format!("Stopped after 3 songs failed: {e}");
                        } else {
                            self.status = format!("Skipped, download failed: {e}");
                            self.next();
                        }
                    }
                }
            }
            Event::Lyrics(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) != Some(&vid) {
                    return;
                }
                match result {
                    Ok(Some(l)) => {
                        self.lyrics = Some(l);
                        self.lyrics_status.clear();
                    }
                    Ok(None) => self.lyrics_status = "No lyrics found".into(),
                    Err(e) => self.lyrics_status = format!("Lyrics: {e}"),
                }
            }
            Event::Rated(vid, like, Err(e)) => {
                // Roll back the optimistic toggle.
                self.liked.set(&vid, !like);
                self.status = format!("Like failed: {e}");
            }
            Event::Liked(Ok(t)) => {
                self.liked.sync(t.iter().map(|t| t.video_id.clone()));
                if self.page == Page::Playlist(api::LIKED_ID.into()) && !self.busy {
                    self.tracks = t;
                    let last = self.tracks.len().saturating_sub(1);
                    self.track_state.select((!self.tracks.is_empty()).then(|| self.track_state.selected().unwrap_or(0).min(last)));
                }
            }
            Event::Liked(Err(_)) => {}
            Event::Rated(_, _, Ok(())) => {}
            Event::Created(Ok(p), song) => {
                self.status = format!("Created \"{}\"", p.title);
                if let Some(vid) = song {
                    self.add_to(p.clone(), vid);
                }
                self.playlists.insert(0, p);
                self.rebuild_nav(self.playlists.clone());
            }
            Event::Created(Err(e), _) => self.status = format!("Couldn't create the playlist: {e}"),
            Event::Added(title, Ok(())) => self.status = format!("Added to \"{title}\""),
            Event::Added(title, Err(e)) => self.status = format!("Couldn't add to \"{title}\": {e}"),
            Event::Downloaded(Ok(p)) => self.status = format!("Saved to {}", p.display()),
            Event::Downloaded(Err(e)) => self.status = format!("Download failed: {e}"),
        }
    }

    fn rebuild_nav(&mut self, playlists: Vec<Playlist>) {
        self.playlists = playlists.clone();
        let mut nav = vec![NavItem::ForYou];
        if self.logged_in() {
            nav.push(NavItem::History);
            nav.push(NavItem::NewPlaylist);
            nav.push(NavItem::Playlist(Playlist { id: api::LIKED_ID.into(), title: "Liked songs".into(), thumb: None }));
        }
        nav.extend(playlists.into_iter().map(NavItem::Playlist));
        self.nav = nav;
        let sel = self.nav_state.selected().unwrap_or(0).min(self.nav.len() - 1);
        self.nav_state.select(Some(sel));
    }

    // ------------------------------------------------------------ keys

    /// Returns false to quit.
    fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        if let Some(input) = self.input {
            match k.code {
                KeyCode::Esc => {
                    self.input = None;
                    self.buffer.clear();
                }
                KeyCode::Enter => self.submit(input),
                KeyCode::Backspace => {
                    self.buffer.pop();
                }
                KeyCode::Char(c) => self.buffer.push(c),
                _ => {}
            }
            return true;
        }
        if let Some(sel) = self.settings {
            self.settings_key(sel, k.code);
            return true;
        }
        if let Some(sel) = self.picker {
            self.picker_key(sel, k.code);
            return true;
        }
        if self.mode == Mode::Login {
            match k.code {
                KeyCode::Char('q') => return false,
                KeyCode::Char('g') if !self.busy => self.google_login(),
                KeyCode::Char('c') if !self.busy => self.input = Some(Input::Cookie),
                KeyCode::Char('s') if !self.busy => {
                    self.busy = true;
                    self.status = "Connecting...".into();
                    self.spawn(async { Event::LoggedIn(s(Api::guest().await)) });
                }
                _ => {}
            }
            return true;
        }

        match k.code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('/') => {
                self.input = Some(Input::Search);
                self.buffer.clear();
            }
            KeyCode::Tab | KeyCode::BackTab if self.config.library != Side::Hidden => {
                self.focus = if self.focus == Focus::Nav { Focus::Tracks } else { Focus::Nav };
            }
            KeyCode::Char(',') => self.settings = Some(0),
            KeyCode::Char('h') => self.open(Page::History, "History".into()),
            KeyCode::Char('a') => self.open_picker(),
            KeyCode::Char('N') => self.new_playlist(None),
            KeyCode::Char(' ') => {
                if let Some(a) = &self.audio {
                    a.toggle();
                }
            }
            KeyCode::Char('n') => self.next(),
            KeyCode::Char('p') => self.prev(),
            KeyCode::Left => self.seek_by(-5.0),
            KeyCode::Right => self.seek_by(5.0),
            KeyCode::Char('+') | KeyCode::Char('=') => self.set_volume(self.volume + 0.05),
            KeyCode::Char('-') => self.set_volume(self.volume - 0.05),
            KeyCode::Char('l') => self.toggle_like(),
            KeyCode::Char('O') => self.sign_out(),
            KeyCode::Char('g') if !self.busy => self.google_login(),
            KeyCode::Char('r') => self.start_radio(),
            KeyCode::Char('d') => self.download(),
            // Refresh: a new home feed (new songs on top) and the liked songs again.
            KeyCode::F(5) | KeyCode::Char('u') => {
                self.open(Page::ForYou, "For you".into());
                self.focus = Focus::Tracks;
            }
            // Like YouTube: 0-9 jump to 0%..90% of the song.
            KeyCode::Char(c @ '0'..='9') => {
                let tenth = c.to_digit(10).unwrap_or(0) as f32 / 10.0;
                self.seek_to(self.duration() * tenth);
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::Home => self.move_sel(i32::MIN / 2),
            KeyCode::End => self.move_sel(i32::MAX / 2),
            KeyCode::Enter => self.activate(),
            _ => {}
        }
        true
    }

    fn submit(&mut self, input: Input) {
        let text = std::mem::take(&mut self.buffer).trim().to_string();
        self.input = None;
        if text.is_empty() {
            return;
        }
        match input {
            Input::Search => {
                let Some(api) = self.api.clone() else { return };
                self.page = Page::Search;
                self.page_title = format!("Results for \"{text}\"");
                self.busy = true;
                self.tracks.clear();
                self.focus = Focus::Tracks;
                self.spawn(async move { Event::Tracks(Page::Search, s(api.search(text).await)) });
            }
            Input::NewPlaylist => {
                let Some(api) = self.api.clone() else { return };
                let song = self.pick_song.take().map(|t| t.video_id);
                self.status = format!("Creating \"{text}\"...");
                self.spawn(async move { Event::Created(s(api.create_playlist(text).await), song) });
            }
            Input::Cookie => {
                self.busy = true;
                self.status = "Signing in...".into();
                let _ = std::fs::create_dir_all(data_dir());
                let _ = std::fs::write(cookie_path(), &text);
                self.spawn(async move { Event::LoggedIn(s(Api::with_cookie(text).await)) });
            }
        }
    }

    fn google_login(&mut self) {
        self.busy = true;
        self.status = "Finish signing in in the Google window...".into();
        self.spawn(async {
            let r = async {
                let cookie = login::sign_in().await?;
                Api::with_cookie(cookie).await
            };
            Event::LoggedIn(s(r.await))
        });
    }

    fn sign_out(&mut self) {
        let _ = std::fs::remove_file(cookie_path());
        rymplayer::page_cache::clear();
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.api = None;
        self.tracks.clear();
        self.queue.clear();
        self.current = None;
        self.lyrics = None;
        self.liked.clear();
        self.nav = vec![NavItem::ForYou];
        self.playlists.clear();
        self.mode = Mode::Login;
        self.status.clear();
    }

    fn move_sel(&mut self, delta: i32) {
        let (len, sel) = match self.focus {
            Focus::Nav => (self.nav.len(), self.nav_state.selected()),
            Focus::Tracks => (self.tracks.len(), self.track_state.selected()),
        };
        if len == 0 {
            return;
        }
        let new = (sel.unwrap_or(0) as i64 + delta as i64).clamp(0, len as i64 - 1) as usize;
        match self.focus {
            Focus::Nav => self.nav_state.select(Some(new)),
            Focus::Tracks => self.track_state.select(Some(new)),
        }
    }

    fn activate(&mut self) {
        match self.focus {
            Focus::Nav => {
                let Some(item) = self.nav_state.selected().and_then(|i| self.nav.get(i)).cloned() else { return };
                match item {
                    NavItem::ForYou => self.open(Page::ForYou, "For you".into()),
                    NavItem::History => self.open(Page::History, "History".into()),
                    NavItem::NewPlaylist => {
                        self.new_playlist(None);
                        return;
                    }
                    NavItem::Playlist(p) => self.open(Page::Playlist(p.id), p.title),
                }
                self.focus = Focus::Tracks;
            }
            Focus::Tracks => {
                let Some(i) = self.track_state.selected() else { return };
                let Some(t) = self.tracks.get(i).cloned() else { return };
                if self.page == Page::ForYou {
                    feed::opened(&t);
                }
                match t.link {
                    Some(Link::Playlist(id)) => self.open(Page::Playlist(id), t.title),
                    Some(Link::Album(id)) => self.open(Page::Album(id), t.title),
                    Some(Link::Mix(id)) => self.open(Page::Mix(id), t.title),
                    None => self.play_from_list(i),
                }
            }
        }
    }

    /// Plays row `i` of the visible list; the queue is the list's songs (links skipped).
    fn play_from_list(&mut self, i: usize) {
        let Some(vid) = self.tracks.get(i).map(|t| t.video_id.clone()) else { return };
        self.queue = self.tracks.iter().filter(|t| t.link.is_none()).cloned().collect();
        if let Some(q) = self.queue.iter().position(|t| t.video_id == vid) {
            self.play(q);
        }
    }

    fn open(&mut self, page: Page, title: String) {
        let Some(api) = self.api.clone() else { return };
        // The saved home feed stays on screen until the fresh one arrives.
        let keep = page == Page::ForYou && self.page == Page::ForYou && !self.tracks.is_empty();
        self.page = page.clone();
        self.page_title = title;
        self.busy = true;
        if !keep {
            self.tracks.clear();
            self.track_state.select(None);
        }
        let tx = self.tx.clone();
        self.spawn(async move {
            let r = match page.clone() {
                Page::ForYou => feed::refresh(api).await.map(|r| {
                    if let Some(liked) = r.liked {
                        let _ = tx.send(Event::Liked(Ok(liked)));
                    }
                    r.home
                }),
                Page::History => api.history().await,
                Page::Playlist(id) => api.playlist_tracks(id).await,
                Page::Album(id) => api.album_tracks(id).await,
                Page::Mix(id) => api.mix_tracks(id).await,
                Page::Radio(vid) => api.radio(vid).await,
                Page::Search => Ok(Vec::new()),
            };
            Event::Tracks(page, s(r))
        });
    }

    /// `r`: radio from the selected song, or the playing one.
    /// Saves the selected song (or the playing one) to the Music folder.
    fn download(&mut self) {
        let selected = (self.focus == Focus::Tracks)
            .then(|| self.track_state.selected().and_then(|i| self.tracks.get(i)))
            .flatten()
            .filter(|t| t.link.is_none());
        let Some(t) = selected.or(self.now_playing()).cloned() else { return };
        self.status = format!("Downloading \"{}\"...", t.title);
        self.spawn(async move { Event::Downloaded(s(stream::save_to_music(&t.video_id, &t.artist, &t.title).await)) });
    }

    fn start_radio(&mut self) {
        let selected = (self.focus == Focus::Tracks)
            .then(|| self.track_state.selected().and_then(|i| self.tracks.get(i)))
            .flatten()
            .filter(|t| t.link.is_none());
        let Some(seed) = selected.or(self.now_playing()).cloned() else { return };
        let page = Page::Radio(seed.video_id);
        self.play_when_loaded = Some(page.clone());
        self.open(page, format!("Radio: {}", seed.title));
    }

    /// Queue ran out: keep going with YouTube Music's radio for the last song.
    fn continue_with_radio(&mut self) {
        let (Some(api), Some(vid)) = (self.api.clone(), self.now_playing().map(|t| t.video_id.clone())) else { return };
        if self.radio_pending {
            return;
        }
        self.radio_pending = true;
        self.status = "Queue finished, continuing with radio...".into();
        self.spawn(async move { Event::Radio(s(api.radio(vid).await)) });
    }

    // ------------------------------------------------------------ settings and playlists

    fn settings_key(&mut self, sel: usize, code: KeyCode) {
        let last = SETTINGS.len() - 1;
        match code {
            KeyCode::Esc | KeyCode::Char(',') | KeyCode::Char('q') => self.settings = None,
            KeyCode::Up | KeyCode::Char('k') => self.settings = Some(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.settings = Some((sel + 1).min(last)),
            KeyCode::Left => self.change_setting(sel, -1),
            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => self.change_setting(sel, 1),
            _ => {}
        }
    }

    fn change_setting(&mut self, i: usize, step: i32) {
        self.config.change(SETTINGS[i], step);
        stream::use_source(self.config.source);
        stream::download_best(self.config.download_best);
        stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
        self.config.save();
        if self.config.library == Side::Hidden {
            self.focus = Focus::Tracks;
        }
    }

    /// Picker rows: "new playlist", then the user's playlists.
    fn picker_rows(&self) -> Vec<String> {
        std::iter::once(ui::NEW_PLAYLIST_ROW.to_string())
            .chain(self.playlists.iter().map(|p| p.title.clone()))
            .collect()
    }

    /// The song the picker acts on.
    fn pick_target(&self) -> Option<&Track> {
        self.pick_song.as_ref()
    }

    /// `a`: add the selected song (or the playing one) to a playlist.
    fn open_picker(&mut self) {
        if !self.logged_in() {
            self.status = "Sign in to add songs to playlists".into();
            return;
        }
        let selected = (self.focus == Focus::Tracks)
            .then(|| self.track_state.selected().and_then(|i| self.tracks.get(i)))
            .flatten()
            .filter(|t| t.link.is_none());
        let Some(song) = selected.or(self.now_playing()).cloned() else { return };
        self.pick_song = Some(song);
        self.picker = Some(0);
    }

    fn picker_key(&mut self, sel: usize, code: KeyCode) {
        let last = self.playlists.len();
        match code {
            KeyCode::Esc | KeyCode::Char('a') | KeyCode::Char('q') => self.picker = None,
            KeyCode::Up | KeyCode::Char('k') => self.picker = Some(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.picker = Some((sel + 1).min(last)),
            KeyCode::Enter => self.pick(sel),
            _ => {}
        }
    }

    fn pick(&mut self, i: usize) {
        self.picker = None;
        let Some(song) = self.pick_song.clone() else { return };
        if i == 0 {
            self.new_playlist(Some(song));
        } else if let Some(p) = self.playlists.get(i - 1).cloned() {
            self.add_to(p, song.video_id);
        }
    }

    /// Asks for a name; the playlist is made on Enter, with `song` in it if given.
    fn new_playlist(&mut self, song: Option<Track>) {
        if !self.logged_in() {
            self.status = "Sign in to make playlists".into();
            return;
        }
        self.pick_song = song;
        self.buffer.clear();
        self.input = Some(Input::NewPlaylist);
    }

    fn add_to(&mut self, p: Playlist, video_id: String) {
        let Some(api) = self.api.clone() else { return };
        self.status = format!("Adding to \"{}\"...", p.title);
        self.spawn(async move { Event::Added(p.title, s(api.add_to_playlist(p.id, video_id).await)) });
    }

    fn button(&mut self, click: ui::Click) {
        match click {
            ui::Click::ForYou => self.open(Page::ForYou, "For you".into()),
            ui::Click::History => self.open(Page::History, "History".into()),
            ui::Click::Search => {
                self.input = Some(Input::Search);
                self.buffer.clear();
            }
            ui::Click::Settings => self.settings = if self.settings.is_some() { None } else { Some(0) },
            ui::Click::Setting(i) => {
                self.settings = Some(i);
                self.change_setting(i, 1);
            }
            ui::Click::Pick(i) => self.pick(i),
        }
    }

    fn mouse(&mut self, m: MouseEvent) {
        let a = self.areas.clone();
        let inside = |r: ratatui::layout::Rect| {
            m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height
        };
        if m.kind == MouseEventKind::Down(MouseButton::Left) {
            // Popups are drawn last, so their buttons are at the end and win.
            if let Some((_, click)) = a.buttons.iter().rev().find(|(r, _)| inside(*r)) {
                self.button(*click);
                return;
            }
            if self.settings.is_some() || self.picker.is_some() {
                if !inside(a.popup) {
                    self.settings = None;
                    self.picker = None;
                }
                return;
            }
        }
        if self.settings.is_some() || self.picker.is_some() {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                if a.nav.width > 0 && inside(a.nav) {
                    self.focus = Focus::Nav;
                } else if inside(a.tracks) {
                    self.focus = Focus::Tracks;
                } else {
                    return;
                }
                self.move_sel(delta);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if inside(a.gauge) && m.column >= a.gauge.x + a.gauge_label {
                    let width = a.gauge.width.saturating_sub(a.gauge_label).max(1) as f32;
                    let frac = (m.column - a.gauge.x - a.gauge_label) as f32 / width;
                    self.seek_to(self.duration() * frac);
                } else if inside(a.tracks) && m.row >= a.tracks.y + 2 {
                    // Border and header rows come first.
                    let i = self.track_state.offset() + (m.row - a.tracks.y - 2) as usize;
                    if i < self.tracks.len() {
                        self.click(Focus::Tracks, i);
                    }
                } else if a.nav.width > 0 && inside(a.nav) && m.row > a.nav.y {
                    let i = self.nav_state.offset() + (m.row - a.nav.y - 1) as usize;
                    if i < self.nav.len() {
                        self.click(Focus::Nav, i);
                    }
                }
            }
            _ => {}
        }
    }

    /// First click selects, clicking the selected row again opens/plays it.
    fn click(&mut self, pane: Focus, i: usize) {
        let state_sel = match pane {
            Focus::Nav => self.nav_state.selected(),
            Focus::Tracks => self.track_state.selected(),
        };
        let again = self.focus == pane && state_sel == Some(i);
        self.focus = pane;
        match pane {
            Focus::Nav => self.nav_state.select(Some(i)),
            Focus::Tracks => self.track_state.select(Some(i)),
        }
        if again {
            self.activate();
        }
    }

    // ------------------------------------------------------------ playback

    fn play(&mut self, i: usize) {
        let Some(track) = self.queue.get(i).cloned() else { return };
        self.current = Some(i);
        self.pos = 0.0;
        self.lyrics = None;
        self.lyrics_status = "Loading lyrics...".into();
        self.loading = true;
        if let Some(a) = &mut self.audio {
            a.stop();
        }

        let vid = track.video_id.clone();
        let v = vid.clone();
        // "Play songs in: VLC" with VLC installed: the song opens there instead.
        let in_vlc = self.config.play_in_vlc && rymplayer::vlc::find().is_some();
        if self.config.play_in_vlc && !in_vlc {
            self.status = "VLC isn't installed: playing here".into();
        }
        if in_vlc {
            self.spawn(async move { Event::InVlc(v.clone(), s(stream::link(&v).await)) });
        } else {
            rymplayer::vlc::stop(&mut self.vlc);
            self.spawn(async move { Event::Fetched(v.clone(), s(stream::open(v).await)) });
        }

        let v = vid.clone();
        let api = self.api.clone();
        self.spawn(async move { Event::Lyrics(v, Ok(lyrics::for_track(api, track).await)) });

        if let Some(api) = self.api.clone() {
            // Fire and forget: keeps YouTube Music history in sync.
            tokio::spawn(api.report_play(vid));
        }
    }

    /// The next song (and its lyrics) download while this one plays.
    fn prefetch_next(&self) {
        let next = self.current.and_then(|i| self.queue.get(i + 1));
        if let Some(t) = next {
            tokio::spawn(lyrics::prefetch(self.api.clone(), t.clone()));
        }
        if let Some(t) = next
            && stream::cached(&t.video_id).is_none()
        {
            tokio::spawn(stream::fetch(t.video_id.clone()));
        }
    }

    fn next(&mut self) {
        let Some(i) = self.current else { return };
        if i + 1 < self.queue.len() {
            self.play(i + 1);
        } else {
            self.continue_with_radio();
        }
    }

    fn prev(&mut self) {
        let Some(i) = self.current else { return };
        // Like most players: restart the song unless we're at its start.
        if self.pos > 3.0 || i == 0 {
            if let Some(a) = &self.audio {
                a.seek(0.0);
            }
        } else {
            self.play(i - 1);
        }
    }

    fn seek_by(&mut self, delta: f32) {
        self.seek_to(self.pos + delta);
    }

    fn seek_to(&mut self, secs: f32) {
        let dur = self.duration();
        if let Some(a) = &self.audio
            && a.is_loaded()
            && dur > 0.0
        {
            let to = secs.clamp(0.0, dur - 0.5);
            a.seek(to);
            self.pos = to;
        }
    }

    fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(a) = &self.audio {
            a.set_volume(self.volume);
        }
    }

    fn toggle_like(&mut self) {
        let (Some(t), Some(api)) = (self.now_playing().cloned(), self.api.clone()) else { return };
        let vid = t.video_id.clone();
        let like = !self.liked.contains(&vid);
        self.liked.set(&vid, like);
        // The Liked songs page follows at once.
        if self.page == Page::Playlist(api::LIKED_ID.into()) {
            if like {
                if !self.tracks.iter().any(|x| x.video_id == vid) {
                    self.tracks.insert(0, t.clone());
                }
            } else {
                self.tracks.retain(|x| x.video_id != vid);
            }
        }
        self.spawn(async move { Event::Rated(vid.clone(), like, s(api.rate(vid, like).await)) });
    }

    fn tick(&mut self) {
        if self.logged_in() && self.likes_synced.elapsed() > SYNC_EVERY {
            self.likes_synced = Instant::now();
            if let Some(api) = self.api.clone() {
                self.spawn(async move { Event::Liked(s(api.playlist_tracks(api::LIKED_ID.into()).await)) });
            }
        }
        let Some(a) = &mut self.audio else { return };
        self.pos = a.position();
        if self.loading || !a.finished() {
            return;
        }
        a.stop();
        self.next();
    }

    fn is_playing(&self) -> bool {
        !self.loading && self.audio.as_ref().is_some_and(Audio::is_playing)
    }

    fn duration(&self) -> f32 {
        let from_audio = self.audio.as_ref().and_then(Audio::duration);
        from_audio.or_else(|| self.now_playing().and_then(|t| parse_duration(&t.duration))).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the network: `cargo test -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn search_fetch_decode_lyrics() {
        let api = Api::guest().await.expect("guest api");
        let tracks = api.search("Rick Astley Never Gonna Give You Up".into()).await.expect("search");
        let t = tracks.first().expect("results").clone();
        println!("first hit: {} - {} [{}] {}", t.title, t.artist, t.album, t.duration);

        let path = stream::fetch(t.video_id.clone()).await.expect("fetch");
        println!("audio: {}", path.display());
        let secs = audio::decode_seconds(&path).expect("decode");
        println!("decoded {secs:.1}s of audio");
        assert!(secs > 60.0);

        let l = lyrics::lrclib(t.title, t.artist, t.album, parse_duration(&t.duration)).await.expect("lyrics");
        let l = l.expect("lyrics found");
        println!("lyrics: {} synced lines, first: {:?}", l.synced.len(), l.synced.first());
        assert!(!l.synced.is_empty());
    }

    /// Uses the signed-in session the app saved. Prints counts and ids only, never the cookie.
    #[tokio::test]
    #[ignore]
    async fn signed_in_library() {
        let cookie = std::fs::read_to_string(cookie_path()).expect("signed-in cookie");
        let api = Api::with_cookie(cookie).await.expect("login");
        let playlists = api.clone().library_playlists().await.expect("playlists");
        println!("playlists: {}", playlists.len());
        let liked = api.clone().playlist_tracks(api::LIKED_ID.into()).await.expect("liked");
        println!("liked: {} tracks", liked.len());
        if let Some(p) = playlists.first() {
            let t = api.clone().playlist_tracks(p.id.clone()).await.expect("playlist");
            println!("first playlist: {} tracks", t.len());
        }
    }
}

#[cfg(test)]
mod lyrics_probe {
    use super::*;

    /// How many of the user's liked songs LRCLIB finds lyrics for.
    #[tokio::test]
    #[ignore]
    async fn liked_lyrics_coverage() {
        let cookie = std::fs::read_to_string(cookie_path()).expect("signed-in cookie");
        let api = Api::with_cookie(cookie).await.expect("login");
        let liked = api.clone().playlist_tracks(api::LIKED_ID.into()).await.expect("liked");
        println!("liked: {}", liked.len());
        for t in liked.into_iter().take(15) {
            let r = lyrics::lrclib(t.title.clone(), t.artist.clone(), t.album.clone(), parse_duration(&t.duration)).await;
            let res = match r {
                Ok(Some(l)) => format!("LRCLIB synced={} plain={}", l.synced.len(), l.plain.len()),
                Ok(None) => match api.clone().lyrics(t.video_id.clone()).await {
                    Ok(Some(l)) => format!("{} plain={}", l.source, l.plain.len()),
                    _ => "NONE".into(),
                },
                Err(e) => format!("ERR {e}"),
            };
            println!("{:<45} | {:<25} | {res}", t.title, t.artist);
        }
    }
}

#[cfg(test)]
mod music_lyrics_probe {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn music_lyrics_fallback() {
        let api = Api::guest().await.unwrap();
        let l = api.lyrics("lYBUbBu4W08".into()).await.unwrap();
        println!("YouTube Music lyrics: {:?}", l.map(|l| (l.source, l.plain.len())));
    }
}

#[cfg(test)]
mod history_probe {
    use super::*;

    /// Read-only: counts only.
    #[tokio::test]
    #[ignore]
    async fn history_loads() {
        let cookie = std::fs::read_to_string(cookie_path()).expect("signed-in cookie");
        let api = Api::with_cookie(cookie).await.expect("login");
        match api.history().await {
            Ok(h) => {
                let periods: std::collections::BTreeSet<_> = h.iter().map(|t| t.album.clone()).collect();
                println!("history: {} songs, periods {:?}", h.len(), periods);
            }
            Err(e) => println!("history ERR: {e}"),
        }
    }
}
