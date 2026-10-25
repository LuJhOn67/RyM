mod api;
mod audio;
mod login;
mod lyrics;
mod stream;
mod ui;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use api::{Api, Playlist, Track};
use audio::Audio;
use lyrics::Lyrics;

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ytm-player")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cookie_path() -> PathBuf {
    data_dir().join("cookie.txt")
}

fn ytdlp_cookie_path() -> PathBuf {
    data_dir().join("cookies-ytdlp.txt")
}

fn parse_duration(d: &str) -> Option<f32> {
    let mut secs = 0.0;
    for part in d.split(':') {
        secs = secs * 60.0 + part.trim().parse::<f32>().ok()?;
    }
    Some(secs)
}

fn fmt_time(secs: f32) -> String {
    let t = secs.max(0.0) as u32;
    format!("{}:{:02}", t / 60, t % 60)
}

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
    let result = app.run(&mut terminal, rx);
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
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Search,
    Playlist(String),
}

#[derive(Debug, Clone)]
enum NavItem {
    Search,
    Playlist(Playlist),
}

/// Results of background work, sent back to the UI loop.
enum Event {
    LoggedIn(Result<Api, String>),
    Playlists(Result<Vec<Playlist>, String>),
    Tracks(Page, Result<Vec<Track>, String>),
    Fetched(String, Result<PathBuf, String>),
    Lyrics(String, Result<Option<Lyrics>, String>),
    Rated(String, bool, Result<(), String>),
}

fn s<T>(r: anyhow::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

struct App {
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
    liked: HashSet<String>,
}

impl App {
    fn new(tx: UnboundedSender<Event>) -> Self {
        App {
            tx,
            mode: Mode::Login,
            focus: Focus::Tracks,
            input: None,
            buffer: String::new(),
            busy: false,
            status: String::new(),
            api: None,
            nav: vec![NavItem::Search],
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
            liked: HashSet::new(),
        }
    }

    /// Opens the audio device and signs in with a saved session, if any.
    fn start(&mut self) {
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

    fn cookie_file(&self) -> Option<PathBuf> {
        let p = ytdlp_cookie_path();
        (self.logged_in() && p.exists()).then_some(p)
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
                    if let Ok(c) = std::fs::read_to_string(cookie_path()) {
                        let _ = stream::write_cookie_file(&c, &ytdlp_cookie_path());
                    }
                    self.spawn(async move { Event::Playlists(s(api.library_playlists().await)) });
                    self.open_playlist(Playlist { id: api::LIKED_ID.into(), title: "Liked songs".into() });
                } else {
                    self.input = Some(Input::Search);
                }
                self.rebuild_nav(Vec::new());
            }
            Event::LoggedIn(Err(e)) => {
                self.busy = false;
                self.mode = Mode::Login;
                self.status = format!("Sign-in failed: {e}");
            }
            Event::Playlists(Ok(p)) => self.rebuild_nav(p),
            Event::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),
            Event::Tracks(page, result) => {
                if page != self.page {
                    return;
                }
                self.busy = false;
                match result {
                    Ok(t) => {
                        if page == Page::Playlist(api::LIKED_ID.into()) {
                            self.liked.extend(t.iter().map(|t| t.video_id.clone()));
                        }
                        self.status.clear();
                        self.track_state.select((!t.is_empty()).then_some(0));
                        self.tracks = t;
                    }
                    Err(e) => self.status = e,
                }
            }
            Event::Fetched(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) != Some(&vid) {
                    return;
                }
                self.loading = false;
                match result {
                    Ok(path) => {
                        if let Some(a) = &mut self.audio {
                            match a.play_file(&path) {
                                Ok(()) => self.status.clear(),
                                Err(e) => self.status = format!("Can't play: {e}"),
                            }
                        }
                        self.prefetch_next();
                    }
                    Err(e) => {
                        self.status = format!("Skipped, download failed: {e}");
                        self.next();
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
                if like {
                    self.liked.remove(&vid);
                } else {
                    self.liked.insert(vid);
                }
                self.status = format!("Like failed: {e}");
            }
            Event::Rated(_, _, Ok(())) => {}
        }
    }

    fn rebuild_nav(&mut self, playlists: Vec<Playlist>) {
        let mut nav = vec![NavItem::Search];
        if self.logged_in() {
            nav.push(NavItem::Playlist(Playlist { id: api::LIKED_ID.into(), title: "Liked songs".into() }));
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
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Nav { Focus::Tracks } else { Focus::Nav };
            }
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
        let _ = std::fs::remove_file(ytdlp_cookie_path());
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.api = None;
        self.tracks.clear();
        self.queue.clear();
        self.current = None;
        self.lyrics = None;
        self.liked.clear();
        self.nav = vec![NavItem::Search];
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
                    NavItem::Search => {
                        self.page = Page::Search;
                        self.page_title = "Search".into();
                        self.tracks.clear();
                        self.input = Some(Input::Search);
                    }
                    NavItem::Playlist(p) => self.open_playlist(p),
                }
                self.focus = Focus::Tracks;
            }
            Focus::Tracks => {
                if let Some(i) = self.track_state.selected().filter(|&i| i < self.tracks.len()) {
                    self.queue = self.tracks.clone();
                    self.play(i);
                }
            }
        }
    }

    fn open_playlist(&mut self, p: Playlist) {
        let Some(api) = self.api.clone() else { return };
        let page = Page::Playlist(p.id.clone());
        self.page = page.clone();
        self.page_title = p.title;
        self.busy = true;
        self.tracks.clear();
        self.track_state.select(None);
        self.spawn(async move { Event::Tracks(page, s(api.playlist_tracks(p.id).await)) });
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
        let cookies = self.cookie_file();
        let v = vid.clone();
        self.spawn(async move { Event::Fetched(v.clone(), s(stream::fetch(v, cookies).await)) });

        let v = vid.clone();
        let dur = parse_duration(&track.duration);
        self.spawn(async move {
            Event::Lyrics(v, s(lyrics::lrclib(track.title, track.artist, track.album, dur).await))
        });

        if let Some(api) = self.api.clone() {
            // Fire and forget: keeps YouTube Music history in sync.
            tokio::spawn(api.report_play(vid));
        }
    }

    fn prefetch_next(&self) {
        let next = self.current.and_then(|i| self.queue.get(i + 1));
        if let Some(t) = next
            && stream::cached(&t.video_id).is_none()
        {
            tokio::spawn(stream::fetch(t.video_id.clone(), self.cookie_file()));
        }
    }

    fn next(&mut self) {
        if let Some(i) = self.current
            && i + 1 < self.queue.len()
        {
            self.play(i + 1);
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
        if let Some(a) = &self.audio
            && let Some(d) = a.duration()
        {
            let to = (self.pos + delta).clamp(0.0, d);
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
        let (Some(t), Some(api)) = (self.now_playing(), self.api.clone()) else { return };
        let vid = t.video_id.clone();
        let like = !self.liked.contains(&vid);
        if like {
            self.liked.insert(vid.clone());
        } else {
            self.liked.remove(&vid);
        }
        self.spawn(async move { Event::Rated(vid.clone(), like, s(api.rate(vid, like).await)) });
    }

    fn tick(&mut self) {
        let Some(a) = &mut self.audio else { return };
        self.pos = a.position();
        if self.loading || !a.finished() {
            return;
        }
        match self.current.map(|i| i + 1).filter(|&n| n < self.queue.len()) {
            Some(n) => self.play(n),
            None => a.stop(),
        }
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

        let path = stream::fetch(t.video_id.clone(), None).await.expect("fetch");
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
