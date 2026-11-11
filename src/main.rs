mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::widgets::{ListState, TableState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use rymplayer::i18n::tr;
use rymplayer::likes::{Likes, SYNC_EVERY};
use rymplayer::{api, audio, config, data_dir, feed, fmt_time, login, lyrics, parse_duration, stream};

use api::{Api, Link, Playlist, Track};
use audio::Audio;
use config::{Config, SETTINGS, Side};
use lyrics::Lyrics;

fn main() -> anyhow::Result<()> {
    // What the last update put aside goes now.
    rymplayer::updates::clean_old();
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    // "Play in RyM": a link given here goes to the window app when it's open (it plays it there).
    if let Some(link) = std::env::args().skip(1).find(|a| rymplayer::links::is_incoming(a))
        && rymplayer::link::hand_over(&link)
    {
        println!("{}", tr("Sent to RyM"));
        return Ok(());
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
    /// How many liked songs to download (empty = all).
    DownloadCount,
    /// Video plugin: a link from YouTube or any site to download.
    Link,
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    ForYou,
    /// Songs on this device (the Music folder).
    Local,
    History,
    Search,
    Playlist(String),
    Album(String),
    Mix(String),
    /// YouTube Music radio started from a song.
    Radio(String),
    /// An artist's songs, albums and singles.
    Artist(String),
    /// The queue ("Up next"): played songs, the playing one, what comes next.
    Queue,
    /// "Your recap": the most played songs (the title says the hours and top artists).
    Recap,
}

#[derive(Debug, Clone)]
enum NavItem {
    ForYou,
    Local,
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
    /// The lyrics pane's background, from the song's cover.
    LyricsBg(String, (u8, u8, u8)),
    Rated(String, bool, Result<(), String>),
    /// "Go to artist" for a song without its artist's id: found by name.
    ArtistFound(String, Result<Option<String>, String>),
    /// The liked songs batch download, after each song.
    Batch(stream::Batch),
    /// Every source's lyrics for a song (L switches between them).
    LyricsSources(String, Vec<Lyrics>),
    /// Where a page goes on as it's scrolled (the home feed, search results, a playlist).
    Goes(Page, Option<api::More>),
    /// A page's next part.
    More(Page, Result<api::Paged, String>),
    /// A newer RyM on GitHub (None: up to date); true when asked with U.
    Update(Result<Option<rymplayer::updates::Release>, String>, bool),
    /// The update downloaded and put in place.
    Installed(Result<std::path::PathBuf, String>),
    /// The plugin store's list.
    Store(Result<Vec<rymplayer::plugins::Plugin>, String>),
    /// A video's picture and sound links, to watch it.
    VideoLinks(Track, Result<(String, Option<String>), String>),
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

    /// What plays: the songs, the playing one, and the ones added to play after it.
    pub q: rymplayer::queue::Queue,
    /// The search box looks on SoundCloud (Tab in the box, with the SoundCloud plugin).
    pub search_sc: bool,
    /// The liked songs (mixed into Up next).
    liked_tracks: Vec<Track>,
    /// Sleep timer: minutes chosen and when it ends (`None` minutes: at the end of the song).
    sleep: Option<(Option<u64>, Instant)>,
    /// The next song starts while this one fades out; the song whose crossfade already started.
    crossfade_next: bool,
    crossfade_for: Option<String>,
    /// Listening stats: when time was last added, and last written.
    listened_at: Instant,
    stats_written: Instant,
    /// What Discord was last told: song, playing, position, when.
    discord_sent: Option<(String, bool, f32, Instant)>,
    /// Songs started since memory was last given back (every 5 it is), and the last limit check.
    songs_since_cleanup: u32,
    memory_checked: Instant,
    /// Where the open page goes on, and whether its next part is on the way.
    more: Option<api::More>,
    loading_more: bool,
    /// The song left off last time and the second to go on from, until it plays.
    resume_at: Option<(String, f32)>,
    /// What the session file last got (song, second, volume, queue length), and when it was checked.
    session_key: Option<(String, u32, u32, usize)>,
    session_checked: Instant,
    /// Songs played this session (Up next leaves them out).
    played: std::collections::HashSet<String>,
    /// Every source's lyrics for the playing song once looked up, and which one shows (L).
    alt_lyrics: Option<(String, Vec<Lyrics>, usize)>,
    loading: bool,
    audio: Option<Audio>,
    pos: f32,
    volume: f32,

    lyrics: Option<Lyrics>,
    lyrics_status: String,
    /// The lyrics pane's color (the playing song's cover).
    lyrics_bg: (u8, u8, u8),
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
    /// Selected row while the song menu is open, and the song it's for.
    song_menu: Option<usize>,
    menu_song: Option<Track>,
    /// A newer RyM, shown in a popup with its patch notes until closed.
    pub update: Option<rymplayer::updates::Release>,
    /// The plugin store: its list once loaded, and the selected row while open.
    pub store: Option<Vec<rymplayer::plugins::Plugin>>,
    pub store_sel: Option<usize>,
    /// When the hourly "is it time to look for updates" last ran.
    update_tick: Instant,
    /// When Previous was last pressed: twice in a row goes back even mid-song.
    last_prev: Instant,
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
            page_title: tr("Search").into(),
            tracks: Vec::new(),
            track_state: TableState::default(),
            q: Default::default(),
            search_sc: false,
            liked_tracks: Vec::new(),
            sleep: None,
            crossfade_next: false,
            crossfade_for: None,
            listened_at: Instant::now(),
            stats_written: Instant::now(),
            discord_sent: None,
            songs_since_cleanup: 0,
            memory_checked: Instant::now(),
            more: None,
            loading_more: false,
            resume_at: None,
            session_key: None,
            session_checked: Instant::now(),
            played: Default::default(),
            alt_lyrics: None,
            loading: false,
            audio: None,
            pos: 0.0,
            volume: 0.8,
            lyrics: None,
            lyrics_status: String::new(),
            lyrics_bg: rymplayer::colors::PLAIN,
            liked: Likes::default(),
            likes_synced: Instant::now(),
            play_when_loaded: None,
            radio_pending: false,
            areas: ui::Areas::default(),
            config: Config::default(),
            settings: None,
            picker: None,
            song_menu: None,
            menu_song: None,
            update: None,
            store: None,
            store_sel: None,
            update_tick: Instant::now(),
            last_prev: Instant::now() - Duration::from_secs(60),
            pick_song: None,
            playlists: Vec::new(),
        }
    }

    /// Opens the audio device and signs in with a saved session, if any.
    fn start(&mut self) {
        self.config = Config::load();
        rymplayer::i18n::set_arabic(self.config.arabic);
        self.auto_update_check();
        self.spawn(async { Event::Store(s(rymplayer::plugins::store().await)) });
        stream::use_source(self.config.source);
        rymplayer::ytmusicapi::use_engine(rymplayer::ytmusicapi::Engine::from_index(self.config.music_engine));
        rymplayer::ytmusicapi::use_language(self.config.arabic);
        stream::download_best(self.config.download_best);
        stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
        // The last session: its queue, its song paused where it was left, its volume.
        if let Some(saved) = rymplayer::session::load() {
            self.volume = saved.volume;
            self.q = saved.queue();
            if let Some(t) = self.q.now() {
                self.resume_at = Some((t.video_id.clone(), saved.position));
                self.pos = saved.position;
            }
        }
        rymplayer::fx::set(self.config.fx());
        match Audio::new() {
            Ok(a) => {
                a.set_volume(self.volume);
                a.set_speed(self.config.speed);
                self.audio = Some(a);
            }
            Err(e) => self.status = format!("{}: {e}", tr("No audio device")),
        }
        if let Some(cookie) = rymplayer::secret::read_cookie_io().ok().filter(|c| !c.trim().is_empty()) {
            self.busy = true;
            self.status = tr("Signing in...").into();
            // The last home feed shows at once while signing in; the fresh one replaces it.
            let saved = rymplayer::page_cache::load_home();
            if !saved.is_empty() {
                self.mode = Mode::Main;
                self.rebuild_nav(Vec::new());
                self.page = Page::ForYou;
                self.page_title = tr("For you").into();
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
                            self.save_session();
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
        self.q.current.and_then(|i| self.q.songs.get(i))
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
                self.open(Page::ForYou, tr("For you").into());
            }
            Event::Radio(result) => {
                self.radio_pending = false;
                match result {
                    Ok(tracks) => {
                        // No repeats, nothing played or queued already, same artist moved up.
                        let fresh: Vec<Track> = match self.q.now().cloned() {
                            Some(seed) => {
                                let queued: std::collections::HashSet<String> = self.q.songs.iter().map(|t| t.video_id.clone()).collect();
                                let heard = rymplayer::radio::artists_of(self.q.songs.iter().take(self.q.current.map_or(0, |c| c + 1)));
                                // RaM points: only the playing song's kind, its artist first.
                                let skip: std::collections::HashSet<String> = self.played.union(&queued).cloned().collect();
                                rymplayer::ram_points::up_next(tracks, &seed, &self.liked_tracks, &skip, &heard, rymplayer::updates::now() | 1)
                            }
                            None => tracks,
                        };
                        if fresh.is_empty() {
                            self.status = tr("Radio has nothing new to play").into();
                            return;
                        }
                        // Move on only when the queue had run out (not when the radio was asked
                        // for a song that just started).
                        let ended = self.q.current.is_some_and(|c| c + 1 >= self.q.songs.len())
                            && !self.loading
                            && self.audio.as_ref().is_some_and(|a| !a.is_loaded());
                        self.q.songs.extend(fresh);
                        self.sync_queue_page();
                        if ended {
                            self.next();
                        }
                    }
                    Err(e) => self.status = format!("Radio: {e}"),
                }
            }
            Event::LoggedIn(Err(e)) => {
                self.busy = false;
                self.mode = Mode::Login;
                self.status = format!("{}: {e}", tr("Sign-in failed"));
            }
            Event::Playlists(Ok(p)) => self.rebuild_nav(p),
            Event::Playlists(Err(e)) if e.contains("signed out") => {
                // The saved session is dead: send the user to sign in again.
                self.mode = Mode::Login;
                self.busy = false;
                self.status = tr("Google ended your session. Press g to sign in again.").into();
            }
            Event::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),
            Event::Goes(page, more) => {
                if page == self.page {
                    self.more = more;
                }
            }
            Event::More(page, result) => {
                self.loading_more = false;
                if page != self.page {
                    return;
                }
                match result {
                    Ok(part) => {
                        let mut seen: std::collections::HashSet<String> = self.tracks.iter().map(|t| format!("{}{:?}", t.video_id, t.link)).collect();
                        let fresh: Vec<Track> = part.tracks.into_iter().filter(|t| seen.insert(format!("{}{:?}", t.video_id, t.link))).collect();
                        tokio::spawn(stream::look_ahead(fresh.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect()));
                        self.tracks.extend(fresh);
                        self.more = part.more;
                        self.status.clear();
                    }
                    Err(e) => self.status = e,
                }
            }
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
                                self.status = tr("Playing in VLC").into();
                            }
                            Err(e) => self.status = format!("VLC: {e}"),
                        }
                    }
                    (Err(e), _) => self.status = format!("{}: {e}", tr("Can't play")),
                    (_, None) => self.status = tr("VLC isn't installed").into(),
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
                        let resume = self.resume_at.take().filter(|(id, at)| *id == vid && *at > 1.0).map(|(_, at)| at);
                        let crossfade = std::mem::take(&mut self.crossfade_next);
                        let automix = self.config.automix;
                        let secs = if automix { rymplayer::automix::mix_secs(self.config.crossfade) } else { self.config.crossfade as f32 };
                        if let Some(a) = &mut self.audio {
                            let started = match (crossfade, automix) {
                                (true, true) => a.automix_to(&song, known, secs),
                                (true, false) => a.crossfade_to(&song, known, secs),
                                _ => a.play(&song, known),
                            };
                            match started {
                                Ok(()) => {
                                    if let Some(at) = resume {
                                        a.seek(at);
                                    }
                                    self.status.clear();
                                }
                                Err(e) => self.status = format!("{}: {e}", tr("Can't play")),
                            }
                        }
                        self.prefetch_next();
                    }
                    Err(e) => {
                        self.failures += 1;
                        if self.failures >= 3 {
                            // Something is wrong for every song (YouTube, the connection, yt-dlp): stop and say why.
                            self.failures = 0;
                            self.status = format!("{}: {e}", tr("Stopped after 3 songs failed"));
                        } else {
                            self.status = format!("{}: {e}", tr("Skipped, download failed"));
                            self.next();
                        }
                    }
                }
            }
            Event::LyricsBg(vid, color) => {
                if self.now_playing().map(|t| &t.video_id) == Some(&vid) {
                    self.lyrics_bg = color;
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
                    Ok(None) => self.lyrics_status = tr("No lyrics found").into(),
                    Err(e) => self.lyrics_status = format!("{}: {e}", tr("Lyrics")),
                }
            }
            Event::Rated(vid, like, Err(e)) => {
                // Roll back the optimistic toggle.
                self.liked.set(&vid, !like);
                self.status = format!("{}: {e}", tr("Like failed"));
            }
            Event::Liked(Ok(t)) => {
                self.liked.sync(t.iter().map(|t| t.video_id.clone()));
                self.liked_tracks = t.clone();
                if self.page == Page::Playlist(api::LIKED_ID.into()) && !self.busy {
                    self.tracks = t;
                    let last = self.tracks.len().saturating_sub(1);
                    self.track_state.select((!self.tracks.is_empty()).then(|| self.track_state.selected().unwrap_or(0).min(last)));
                }
            }
            Event::Liked(Err(_)) => {}
            Event::Rated(_, _, Ok(())) => {}
            Event::ArtistFound(name, Ok(Some(id))) => {
                self.status.clear();
                self.open(Page::Artist(id), name);
                self.focus = Focus::Tracks;
            }
            Event::ArtistFound(..) => self.status = tr("Artist not found").into(),
            Event::Installed(Ok(_)) => self.status = tr("Updated: quit (q) and open RyM again to use the new version").into(),
            Event::Installed(Err(e)) => self.status = format!("{}: {e}", tr("Couldn't update")),
            Event::Update(result, asked) => match result {
                Ok(Some(r)) => self.update = Some(r),
                Ok(None) if asked => self.status = tr("You have the latest version").into(),
                Err(e) if asked => self.status = format!("{}: {e}", tr("Couldn't check for updates")),
                _ => {}
            },
            Event::Store(result) => match result {
                Ok(list) => {
                    let news: Vec<String> = rymplayer::plugins::updates(&self.config.plugins, &list).iter().map(|p| p.name().to_string()).collect();
                    if !news.is_empty() {
                        self.status = format!("{}: {}", tr("Plugin updates"), news.join(", "));
                    }
                    self.store = Some(list);
                }
                Err(e) => self.status = e,
            },
            Event::VideoLinks(t, result) => match (result, rymplayer::vlc::find()) {
                (Ok((video, audio)), Some(exe)) => {
                    if self.is_playing()
                        && let Some(a) = &self.audio
                    {
                        a.toggle();
                    }
                    match rymplayer::video::watch_in_vlc(&exe, &video, audio.as_deref(), &format!("{} - {}", t.artist, t.title)) {
                        Ok(child) => {
                            self.vlc = Some(child);
                            self.status.clear();
                        }
                        Err(e) => self.status = format!("{}: {e}", tr("Can't open the video")),
                    }
                }
                (_, None) => {
                    let _ = rymplayer::updates::open_link(&rymplayer::video::watch_url(&t.video_id));
                    self.status = tr("VLC isn't installed: opening YouTube in the browser").into();
                }
                (Err(e), _) => self.status = format!("{}: {e}", tr("Can't open the video")),
            },
            Event::LyricsSources(vid, list) => {
                if self.now_playing().is_some_and(|t| t.video_id == vid) {
                    // The ones showing are in the list: L shows the next source.
                    let at = match &self.lyrics {
                        Some(shown) => list.iter().position(|l| lyrics::same(l, shown)).map_or(0, |p| (p + 1) % list.len()),
                        None => 0,
                    };
                    self.alt_lyrics = Some((vid, list, at));
                    self.show_lyrics_source();
                }
            }
            Event::Batch(b) => {
                let mut words = format!("{} {} / {}", tr("Downloading songs"), b.done, b.total);
                for (n, what) in [(b.saved, tr("saved")), (b.skipped, tr("already there")), (b.failed, tr("failed"))] {
                    if n > 0 {
                        words.push_str(&format!(" · {n} {what}"));
                    }
                }
                self.status = words;
            }
            Event::Created(Ok(p), song) => {
                self.status = format!("Created \"{}\"", p.title);
                if let Some(vid) = song {
                    self.add_to(p.clone(), vid);
                }
                self.playlists.insert(0, p);
                self.rebuild_nav(self.playlists.clone());
            }
            Event::Created(Err(e), _) => self.status = format!("{}: {e}", tr("Couldn't create the playlist")),
            Event::Added(title, Ok(())) => self.status = format!("Added to \"{title}\""),
            Event::Added(title, Err(e)) => self.status = format!("Couldn't add to \"{title}\": {e}"),
            Event::Downloaded(Ok(p)) => self.status = format!("{} {}", tr("Saved to"), p.display()),
            Event::Downloaded(Err(e)) => self.status = format!("{}: {e}", tr("Download failed")),
        }
    }

    fn rebuild_nav(&mut self, playlists: Vec<Playlist>) {
        self.playlists = playlists.clone();
        let mut nav = vec![NavItem::ForYou, NavItem::Local];
        if self.logged_in() {
            nav.push(NavItem::History);
            nav.push(NavItem::NewPlaylist);
            nav.push(NavItem::Playlist(Playlist { id: api::LIKED_ID.into(), title: tr("Liked songs").into(), thumb: None }));
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
                KeyCode::Tab if input == Input::Search && self.has_plugin(rymplayer::plugins::SOUNDCLOUD) => self.search_sc = !self.search_sc,
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
        if let Some(r) = self.update.clone() {
            match k.code {
                KeyCode::Enter => {
                    let url = r.download_for(if cfg!(windows) { "RyM.exe" } else { "RyM-linux-x86_64.tar.gz" });
                    // Windows: the new RyM.exe downloads and goes in place here.
                    if cfg!(windows) && url.ends_with(".exe") {
                        self.status = tr("Downloading the update...").into();
                        self.spawn(async move { Event::Installed(s(rymplayer::updates::install(&url, |_| {}).await)) });
                    } else {
                        let _ = rymplayer::updates::open_link(&url);
                    }
                    self.update = None;
                }
                KeyCode::Esc | KeyCode::Char('q') => self.update = None,
                _ => {}
            }
            return true;
        }
        if let Some(sel) = self.store_sel {
            self.store_key(sel, k.code);
            return true;
        }
        if let Some(sel) = self.song_menu {
            self.song_menu_key(sel, k.code);
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
                KeyCode::Char('m') if !self.busy => {
                    // Songs on this device, no account or internet needed.
                    self.mode = Mode::Main;
                    self.rebuild_nav(Vec::new());
                    self.nav_state.select(Some(1));
                    self.open(Page::Local, tr("On this device").into());
                }
                KeyCode::Char('s') if !self.busy => {
                    self.busy = true;
                    self.status = tr("Connecting...").into();
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
            KeyCode::Char('h') => self.open(Page::History, tr("History").into()),
            KeyCode::Char('a') => self.open_picker(),
            KeyCode::Char('o') => self.open_song_menu(),
            KeyCode::Char('z') => self.next_sleep(),
            KeyCode::Char('R') => self.show_recap(),
            KeyCode::Char('[') => self.step_mode(-1),
            KeyCode::Char(']') => self.step_mode(1),
            KeyCode::Char('Q') => self.show_queue(),
            KeyCode::Char('L') => self.next_lyrics_source(),
            KeyCode::Char('s') => {
                if let Some(t) = self.target_song() {
                    self.share(&t);
                }
            }
            // Up next: move the selected song up or down, or take it out.
            KeyCode::Char('K') if self.page == Page::Queue => self.move_selected(-1),
            KeyCode::Char('J') if self.page == Page::Queue => self.move_selected(1),
            KeyCode::Up if self.page == Page::Queue && k.modifiers.contains(KeyModifiers::SHIFT) => self.move_selected(-1),
            KeyCode::Down if self.page == Page::Queue && k.modifiers.contains(KeyModifiers::SHIFT) => self.move_selected(1),
            KeyCode::Char('x') | KeyCode::Delete if self.page == Page::Queue => {
                if let Some(i) = self.track_state.selected() {
                    self.q.remove(i);
                    self.sync_queue_page();
                }
            }
            KeyCode::Char('U') => self.check_updates(true),
            KeyCode::Char('P') => {
                self.store_sel = Some(0);
                if self.store.is_none() {
                    self.spawn(async { Event::Store(s(rymplayer::plugins::store().await)) });
                }
            }
            KeyCode::Char('v') => {
                if let Some(t) = self.target_song() {
                    self.watch_video(t);
                }
            }
            KeyCode::Char('V') => {
                if self.has_video() {
                    self.buffer.clear();
                    self.input = Some(Input::Link);
                } else {
                    self.status = tr("Install the video plugin first (P)").into();
                }
            }
            KeyCode::Char('A') => {
                if let Some(t) = self.target_song() {
                    self.go_artist(t);
                }
            }
            KeyCode::Char('D') => {
                if self.page != Page::Playlist(api::LIKED_ID.into()) {
                    self.status = tr("Open the Liked songs page first").into();
                } else {
                    self.buffer.clear();
                    self.input = Some(Input::DownloadCount);
                }
            }
            KeyCode::Char('N') => self.new_playlist(None),
            KeyCode::Char(' ') => match &self.audio {
                Some(a) if a.is_loaded() => a.toggle(),
                _ => {
                    if let Some(i) = self.q.current {
                        self.play(i);
                    }
                }
            },
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
                self.open(Page::ForYou, tr("For you").into());
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
        if input == Input::Link {
            if !text.is_empty() {
                self.status = format!("{} {text}", tr("Downloading"));
                self.spawn(async move { Event::Downloaded(s(rymplayer::video::download(&text, true).await)) });
            }
            return;
        }
        if input == Input::DownloadCount {
            self.download_all(text.parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(5000));
            return;
        }
        if text.is_empty() {
            return;
        }
        match input {
            Input::Search => {
                use rymplayer::{plugins, soundcloud, spotify};
                let sc_link = soundcloud::is_link(&text);
                let sc = sc_link || (self.search_sc && self.has_plugin(plugins::SOUNDCLOUD));
                let sp = spotify::is_link(&text);
                if (sc_link && !self.has_plugin(plugins::SOUNDCLOUD)) || (sp && !self.has_plugin(plugins::SPOTIFY)) {
                    self.status = tr(if sp { "Install the Spotify plugin to open Spotify links" } else { "Install the SoundCloud plugin to open SoundCloud links" }).into();
                    return;
                }
                let api = self.api.clone();
                if api.is_none() && !sc {
                    return;
                }
                self.page = Page::Search;
                self.page_title = match (sp, sc_link, sc) {
                    (true, ..) => "Spotify".into(),
                    (_, true, _) => "SoundCloud".into(),
                    (_, _, true) => format!("SoundCloud: \"{text}\""),
                    _ => format!("Results for \"{text}\""),
                };
                self.busy = true;
                self.tracks.clear();
                self.focus = Focus::Tracks;
                if sp {
                    self.status = tr("Finding the Spotify songs on YouTube Music...").into();
                }
                self.more = None;
                let tx = self.tx.clone();
                self.spawn(async move {
                    let r = match (sp, sc_link, sc, api) {
                        (true, _, _, Some(api)) => spotify::import(api, text).await,
                        (_, true, ..) => soundcloud::open_link(text).await,
                        (_, _, true, _) => soundcloud::search(text).await,
                        (_, _, _, Some(api)) => api.search_paged(text).await.map(|p| {
                            let _ = tx.send(Event::Goes(Page::Search, p.more));
                            p.tracks
                        }),
                        _ => Ok(Vec::new()),
                    };
                    Event::Tracks(Page::Search, s(r))
                });
            }
            Input::DownloadCount | Input::Link => {}
            Input::NewPlaylist => {
                let Some(api) = self.api.clone() else { return };
                let song = self.pick_song.take().map(|t| t.video_id);
                self.status = format!("Creating \"{text}\"...");
                self.spawn(async move { Event::Created(s(api.create_playlist(text).await), song) });
            }
            Input::Cookie => {
                self.busy = true;
                self.status = tr("Signing in...").into();
                let _ = std::fs::create_dir_all(data_dir());
                let _ = rymplayer::secret::write_cookie(&text);
                self.spawn(async move { Event::LoggedIn(s(Api::with_cookie(text).await)) });
            }
        }
    }

    fn google_login(&mut self) {
        self.busy = true;
        self.status = tr("Finish signing in in the Google window...").into();
        self.spawn(async {
            let r = async {
                let cookie = login::sign_in().await?;
                Api::with_cookie(cookie).await
            };
            Event::LoggedIn(s(r.await))
        });
    }

    fn sign_out(&mut self) {
        rymplayer::secret::forget_cookie();
        rymplayer::page_cache::clear();
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.api = None;
        self.tracks.clear();
        self.q = Default::default();
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
                    NavItem::ForYou => self.open(Page::ForYou, tr("For you").into()),
                    NavItem::Local => self.open(Page::Local, tr("On this device").into()),
                    NavItem::History => self.open(Page::History, tr("History").into()),
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
        if self.page == Page::Search
            && let Some(t) = self.tracks.get(i).cloned()
        {
            // A search result: the song, then a radio of songs like it (not the other results).
            let at = self.q.start(vec![t], 0);
            self.play(at);
            self.continue_with_radio();
            return;
        }
        if self.page == Page::Queue {
            self.play(i);
            return;
        }
        // The list's songs after what was played (Previous goes back to it).
        let songs: Vec<Track> = self.tracks.iter().filter(|t| t.link.is_none()).cloned().collect();
        if let Some(pos) = songs.iter().position(|t| t.video_id == vid) {
            let at = self.q.start(songs, pos);
            self.play(at);
        }
    }

    fn open(&mut self, page: Page, title: String) {
        if page == Page::Local {
            // No account or internet needed: the files are read off the UI thread.
            self.page = Page::Local;
            self.page_title = title;
            self.busy = true;
            self.tracks.clear();
            self.track_state.select(None);
            self.status = tr("Looking for songs on this device...").into();
            let folders = rymplayer::local::folders(&self.config.local_folders);
            self.spawn(async move {
                let found = tokio::task::spawn_blocking(move || rymplayer::local::scan(&folders)).await;
                Event::Tracks(Page::Local, found.map_err(|e| e.to_string()).and_then(|t| {
                    if t.is_empty() { Err(tr("No songs found in your Music folder").to_string()) } else { Ok(t) }
                }))
            });
            return;
        }
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
        self.more = None;
        self.spawn(async move {
            let r = match page.clone() {
                Page::ForYou => feed::refresh(api).await.map(|r| {
                    if let Some(liked) = r.liked {
                        let _ = tx.send(Event::Liked(Ok(liked)));
                    }
                    let _ = tx.send(Event::Goes(Page::ForYou, r.more));
                    r.home
                }),
                Page::History => api.history().await,
                Page::Playlist(id) => api.playlist_paged(id.clone()).await.map(|p| {
                    let _ = tx.send(Event::Goes(Page::Playlist(id), p.more));
                    p.tracks
                }),
                Page::Album(id) => api.album_tracks(id).await,
                Page::Mix(id) => api.mix_tracks(id).await,
                Page::Radio(vid) => api.radio(vid).await,
                Page::Artist(id) => api.artist_page(id).await,
                Page::Search | Page::Local | Page::Queue | Page::Recap => Ok(Vec::new()),
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
        if rymplayer::local::is_local(&t.video_id) {
            self.status = tr("This song is already on your device").into();
            return;
        }
        self.status = format!("Downloading \"{}\"...", t.title);
        self.spawn(async move { Event::Downloaded(s(stream::save_to_music(&t.video_id, &t.artist, &t.title).await)) });
    }

    fn start_radio(&mut self) {
        let selected = (self.focus == Focus::Tracks)
            .then(|| self.track_state.selected().and_then(|i| self.tracks.get(i)))
            .flatten()
            .filter(|t| t.link.is_none());
        let Some(seed) = selected.or(self.now_playing()).cloned() else { return };
        let page = Page::Radio(seed.video_id.clone());
        self.play_when_loaded = Some(page.clone());
        if rymplayer::is_youtube(&seed.video_id) {
            self.open(page, format!("Radio: {}", seed.title));
            return;
        }
        // A SoundCloud song or a file: the radio of the same song found on YouTube Music.
        let Some(api) = self.api.clone() else { return };
        self.page = page.clone();
        self.page_title = format!("Radio: {}", seed.title);
        self.busy = true;
        self.tracks.clear();
        self.track_state.select(None);
        self.spawn(async move { Event::Tracks(page, s(rymplayer::radio::for_track(api, seed).await)) });
    }

    /// Queue ran out: keep going with YouTube Music's radio for the last song.
    fn continue_with_radio(&mut self) {
        let (Some(api), Some(t)) = (self.api.clone(), self.now_playing().cloned()) else { return };
        if self.radio_pending {
            return;
        }
        self.radio_pending = true;
        self.status = tr("Queue finished, continuing with radio...").into();
        self.spawn(async move { Event::Radio(s(rymplayer::radio::with_artist(api, t).await)) });
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
        self.apply_sound();
        stream::use_source(self.config.source);
        rymplayer::ytmusicapi::use_engine(rymplayer::ytmusicapi::Engine::from_index(self.config.music_engine));
        rymplayer::ytmusicapi::use_language(self.config.arabic);
        stream::download_best(self.config.download_best);
        stream::download_parallel(self.config.download_connections, self.config.download_piece_mb);
        self.config.save();
        if self.config.library == Side::Hidden {
            self.focus = Focus::Tracks;
        }
    }

    /// The selected song, or the playing one.
    fn target_song(&self) -> Option<Track> {
        let selected = (self.focus == Focus::Tracks)
            .then(|| self.track_state.selected().and_then(|i| self.tracks.get(i)))
            .flatten()
            .filter(|t| t.link.is_none());
        selected.or(self.now_playing()).cloned()
    }

    /// The song menu's rows: (text, what it does).
    pub fn song_menu_rows(&self) -> Vec<(&'static str, char)> {
        let Some(t) = &self.menu_song else { return Vec::new() };
        let local = rymplayer::local::is_local(&t.video_id);
        let mut rows = vec![(tr("Play next"), 'n'), (tr("Add to queue"), 'q')];
        if !local {
            rows.push((tr("Download"), 'd'));
            let youtube = rymplayer::is_youtube(&t.video_id);
            if self.logged_in() && youtube {
                rows.push((if self.liked.contains(&t.video_id) { tr("Remove from liked songs") } else { tr("Add to liked songs") }, 'l'));
            }
            if !t.artist.is_empty() && youtube {
                rows.push((tr("Go to artist"), 'a'));
            }
            rows.push((tr("Share"), 's'));
            if self.has_video() && youtube {
                rows.push((tr("Watch video"), 'v'));
                rows.push((tr("Download video"), 'V'));
            }
        }
        rows
    }

    /// `o` or a right-click: the song menu for the selected song (or the playing one).
    fn open_song_menu(&mut self) {
        let Some(t) = self.target_song() else { return };
        self.menu_song = Some(t);
        self.song_menu = Some(0);
    }

    fn song_menu_key(&mut self, sel: usize, code: KeyCode) {
        let last = self.song_menu_rows().len().saturating_sub(1);
        match code {
            KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('q') => self.song_menu = None,
            KeyCode::Up | KeyCode::Char('k') => self.song_menu = Some(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.song_menu = Some((sel + 1).min(last)),
            KeyCode::Enter => self.song_menu_pick(sel),
            _ => {}
        }
    }

    pub fn song_menu_pick(&mut self, i: usize) {
        self.song_menu = None;
        let Some(t) = self.menu_song.take() else { return };
        let Some(&(_, what)) = self.song_menu_rows_for(&t).get(i) else { return };
        match what {
            's' => self.share(&t),
            'n' => {
                let idle = self.q.current.is_none();
                self.q.play_next(t);
                if idle && let Some(c) = self.q.current {
                    self.play(c);
                }
                self.sync_queue_page();
            }
            'q' => {
                // Right after the playing song (after songs added before it).
                if self.q.add(t)
                    && let Some(c) = self.q.current
                {
                    self.play(c);
                }
                self.sync_queue_page();
            }
            'd' => {
                self.status = format!("{} \"{}\"...", tr("Downloading"), t.title);
                self.spawn(async move { Event::Downloaded(s(stream::save_to_music(&t.video_id, &t.artist, &t.title).await)) });
            }
            'l' => self.like(t),
            'a' => self.go_artist(t),
            'v' => self.watch_video(t),
            'V' => {
                let url = rymplayer::video::watch_url(&t.video_id);
                self.status = format!("{} \"{}\"...", tr("Downloading"), t.title);
                self.spawn(async move { Event::Downloaded(s(rymplayer::video::download(&url, true).await)) });
            }
            _ => {}
        }
    }

    fn song_menu_rows_for(&mut self, t: &Track) -> Vec<(&'static str, char)> {
        self.menu_song = Some(t.clone());
        let rows = self.song_menu_rows();
        self.menu_song = None;
        rows
    }

    /// The artist's page: every song, then albums and singles. Songs without the artist's id
    /// find it by name first.
    fn go_artist(&mut self, t: Track) {
        let name = t.artist.split(", ").next().unwrap_or(&t.artist).to_string();
        if let Some(id) = t.artist_id {
            self.open(Page::Artist(id), name);
            self.focus = Focus::Tracks;
            return;
        }
        let Some(api) = self.api.clone() else { return };
        self.status = tr("Finding the artist...").into();
        self.spawn(async move { Event::ArtistFound(name.clone(), s(api.artist_id_for(name).await)) });
    }

    /// Downloads the liked songs (the first `limit`), one after another, skipping ones already
    /// in Music/RymPlayer; the status line shows how it goes.
    fn download_all(&mut self, limit: usize) {
        let Some(api) = self.api.clone().filter(Api::logged_in) else { return };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let songs = api.playlist_all(api::LIKED_ID.into(), limit).await.unwrap_or_default();
            stream::download_many(songs, |b| {
                let _ = tx.send(Event::Batch(b));
            })
            .await;
        });
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
            self.status = tr("Sign in to add songs to playlists").into();
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
            self.status = tr("Sign in to make playlists").into();
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
            ui::Click::ForYou => self.open(Page::ForYou, tr("For you").into()),
            ui::Click::History => self.open(Page::History, tr("History").into()),
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
            ui::Click::MenuItem(i) => self.song_menu_pick(i),
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
            if self.settings.is_some() || self.picker.is_some() || self.song_menu.is_some() {
                if !inside(a.popup) {
                    self.settings = None;
                    self.picker = None;
                    self.song_menu = None;
                }
                return;
            }
        }
        if self.settings.is_some() || self.picker.is_some() || self.song_menu.is_some() {
            return;
        }
        if m.kind == MouseEventKind::Down(MouseButton::Right) && inside(a.tracks) && m.row >= a.tracks.y + 2 {
            let i = self.track_state.offset() + (m.row - a.tracks.y - 2) as usize;
            if self.tracks.get(i).is_some_and(|t| t.link.is_none()) {
                self.focus = Focus::Tracks;
                self.track_state.select(Some(i));
                self.open_song_menu();
            }
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
        let Some(track) = self.q.songs.get(i).cloned() else { return };
        self.q.went_to(i);
        self.played.insert(track.video_id.clone());
        rymplayer::recent::played(&track);
        self.songs_since_cleanup += 1;
        if self.songs_since_cleanup >= 5 {
            self.songs_since_cleanup = 0;
            rymplayer::memory::trim();
        }
        // Another song than the one left off last time: that one's second no longer counts.
        if self.resume_at.as_ref().is_some_and(|(id, _)| *id != track.video_id) {
            self.resume_at = None;
        }
        self.session_key = None;
        self.alt_lyrics = None;
        self.pos = 0.0;
        self.lyrics = None;
        self.lyrics_status = tr("Loading lyrics...").into();
        self.loading = true;
        // A crossfade keeps the old song playing while the new one loads.
        if !self.crossfade_next
            && let Some(a) = &mut self.audio
        {
            a.stop();
        }

        let vid = track.video_id.clone();
        let v = vid.clone();
        // "Play songs in: VLC" with VLC installed: the song opens there instead.
        let in_vlc = self.config.play_in_vlc && rymplayer::vlc::find().is_some() && !rymplayer::local::is_local(&vid);
        if self.config.play_in_vlc && !in_vlc {
            self.status = tr("VLC isn't installed: playing here").into();
        }
        if in_vlc {
            self.spawn(async move { Event::InVlc(v.clone(), s(stream::link(&v).await)) });
        } else {
            rymplayer::vlc::stop(&mut self.vlc);
            self.spawn(async move { Event::Fetched(v.clone(), s(stream::open(v).await)) });
        }

        let (v, thumb) = (vid.clone(), track.thumb.clone());
        self.spawn(async move { Event::LyricsBg(v.clone(), rymplayer::colors::for_cover(thumb).await) });
        let v = vid.clone();
        let api = self.api.clone();
        self.spawn(async move { Event::Lyrics(v, Ok(lyrics::for_track(api, track).await)) });

        if let Some(api) = self.api.clone()
            && !rymplayer::local::is_local(&vid)
        {
            // Fire and forget: keeps YouTube Music history in sync.
            tokio::spawn(api.report_play(vid));
        }
    }

    /// The next song (and its lyrics) download while this one plays.
    fn prefetch_next(&self) {
        let next = self.q.current.and_then(|i| self.q.songs.get(i + 1));
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
        let Some(i) = self.q.current else { return };
        if i + 1 < self.q.songs.len() {
            self.play(i + 1);
        } else {
            self.continue_with_radio();
        }
    }

    fn prev(&mut self) {
        let Some(i) = self.q.current else { return };
        // Like most players: restart the song unless we're at its start, or Previous was
        // pressed twice in a row. The player's own position, not the last drawn one.
        let pos = self.audio.as_ref().filter(|a| a.is_loaded()).map_or(0.0, Audio::position);
        let again = self.last_prev.elapsed() < Duration::from_millis(1500);
        self.last_prev = Instant::now();
        if (pos > 3.0 && !again) || i == 0 {
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
        if let Some(t) = self.now_playing().cloned() {
            self.like(t);
        }
    }

    fn like(&mut self, t: Track) {
        let Some(api) = self.api.clone() else { return };
        if !rymplayer::is_youtube(&t.video_id) {
            self.status = tr("Likes are for YouTube Music songs").into();
            return;
        }
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

    /// Up next as a page: played songs, the playing one, what comes next.
    fn show_queue(&mut self) {
        self.page = Page::Queue;
        self.page_title = tr("Up next").into();
        self.busy = false;
        self.focus = Focus::Tracks;
        self.tracks = self.q.songs.clone();
        self.track_state.select(self.q.current.or((!self.tracks.is_empty()).then_some(0)));
    }

    /// Keeps the Up next page in step after the queue changed.
    fn sync_queue_page(&mut self) {
        if self.page == Page::Queue {
            let sel = self.track_state.selected();
            self.tracks = self.q.songs.clone();
            let last = self.tracks.len().saturating_sub(1);
            self.track_state.select(sel.map(|s| s.min(last)));
        }
    }

    fn move_selected(&mut self, by: i32) {
        let Some(i) = self.track_state.selected() else { return };
        let to = i as i32 + by;
        if to < 0 || to as usize >= self.q.songs.len() {
            return;
        }
        self.q.move_song(i, to as usize);
        self.sync_queue_page();
        self.track_state.select(Some(to as usize));
    }

    /// L: the next lyrics source for the playing song (looked up the first time).
    fn next_lyrics_source(&mut self) {
        let Some(t) = self.now_playing().cloned() else { return };
        match &mut self.alt_lyrics {
            Some((id, list, at)) if *id == t.video_id && !list.is_empty() => {
                *at = (*at + 1) % list.len();
                self.show_lyrics_source();
            }
            Some((id, _, _)) if *id == t.video_id => self.lyrics_status = tr("No lyrics for this song").into(),
            _ => {
                self.status = tr("Looking for lyrics...").into();
                let (api, shown) = (self.api.clone(), self.lyrics.clone());
                self.spawn(async move {
                    let vid = t.video_id.clone();
                    Event::LyricsSources(vid, lyrics::sources(api, t, shown).await)
                });
            }
        }
    }

    fn show_lyrics_source(&mut self) {
        let Some((vid, list, at)) = &self.alt_lyrics else { return };
        match list.get(*at) {
            Some(l) => {
                self.status = format!("{}: {} ({}/{})", tr("Lyrics sources"), l.source, at + 1, list.len());
                lyrics::choose(vid, l);
                self.lyrics = Some(l.clone());
            }
            None => self.status = tr("No lyrics for this song").into(),
        }
    }

    /// Copies the song's link (with its name) to the clipboard.
    fn share(&mut self, t: &Track) {
        if rymplayer::local::is_local(&t.video_id) {
            return;
        }
        let text = format!("{} - {}\n{}", t.title, t.artist, rymplayer::queue::share_link(t));
        self.status = match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
            Ok(()) => tr("Link copied").into(),
            Err(e) => format!("{}: {e}", tr("Share")),
        };
    }

    fn has_video(&self) -> bool {
        self.has_plugin(rymplayer::plugins::VIDEO)
    }

    pub fn has_plugin(&self, id: &str) -> bool {
        self.config.plugins.contains_key(id)
    }

    /// Video plugin: the song's video in VLC (or on YouTube in the browser without VLC).
    fn watch_video(&mut self, t: Track) {
        if !self.has_video() {
            self.status = tr("Install the video plugin first (P)").into();
            return;
        }
        if !rymplayer::is_youtube(&t.video_id) {
            return;
        }
        self.status = tr("Opening the video...").into();
        self.spawn(async move {
            let links = rymplayer::video::links(&t.video_id).await;
            Event::VideoLinks(t, s(links))
        });
    }

    fn check_updates(&mut self, asked: bool) {
        self.config.last_update_check = rymplayer::updates::now();
        self.config.save();
        if asked {
            self.status = tr("Checking for updates...").into();
        }
        let beta = self.config.beta_updates;
        self.spawn(async move { Event::Update(s(rymplayer::updates::check(beta).await), asked) });
    }

    /// At start, then hourly: a look on GitHub when it's been 12 hours.
    fn auto_update_check(&mut self) {
        if rymplayer::updates::due(self.config.auto_update_check, self.config.last_update_check) {
            self.check_updates(false);
        }
    }

    /// The plugin store's rows: the store's plugins with whether they're installed.
    pub fn store_rows(&self) -> Vec<(String, String)> {
        let Some(list) = &self.store else { return Vec::new() };
        list.iter()
            .map(|p| {
                let state = match self.config.plugins.get(&p.id) {
                    Some(v) if rymplayer::updates::newer(&p.version, v) => format!("{} {} → {}", tr("Update"), v, p.version),
                    Some(v) => format!("{} {v}", tr("Installed")),
                    None => format!("{} {}", tr("Version"), p.version),
                };
                (format!("{} · {}", p.name(), p.description()), state)
            })
            .collect()
    }

    fn store_key(&mut self, sel: usize, code: KeyCode) {
        let last = self.store.as_ref().map_or(0, |l| l.len().saturating_sub(1));
        match code {
            KeyCode::Esc | KeyCode::Char('P') | KeyCode::Char('q') => self.store_sel = None,
            KeyCode::Up | KeyCode::Char('k') => self.store_sel = Some(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.store_sel = Some((sel + 1).min(last)),
            KeyCode::Enter => {
                let Some(p) = self.store.as_ref().and_then(|l| l.get(sel)).cloned() else { return };
                // Installed and current: remove; otherwise install (or update).
                let current = self.config.plugins.get(&p.id).is_some_and(|v| !rymplayer::updates::newer(&p.version, v));
                if current {
                    self.config.plugins.remove(&p.id);
                    self.status = tr("Plugin removed").into();
                } else {
                    self.config.plugins.insert(p.id.clone(), p.version.clone());
                    self.status = tr("Plugin installed").into();
                }
                self.config.save();
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        if self.memory_checked.elapsed() > Duration::from_secs(30) {
            self.memory_checked = Instant::now();
            rymplayer::memory::trim_if_over_limit();
        }
        self.load_more_near_the_end();
        // What plays and where, kept every 3 seconds (a crash loses at most that much).
        if self.session_checked.elapsed() > Duration::from_secs(3) {
            self.session_checked = Instant::now();
            self.save_session();
        }
        if self.update_tick.elapsed() > Duration::from_secs(3600) {
            self.update_tick = Instant::now();
            self.auto_update_check();
        }
        if self.logged_in() && self.likes_synced.elapsed() > SYNC_EVERY {
            self.likes_synced = Instant::now();
            if let Some(api) = self.api.clone() {
                self.spawn(async move { Event::Liked(s(api.playlist_tracks(api::LIKED_ID.into()).await)) });
            }
        }
        self.sound_tick();
        let Some(a) = &mut self.audio else { return };
        self.pos = a.position();
        if self.loading || !a.finished() {
            return;
        }
        a.stop();
        self.next();
    }

    /// Settings > Sound into the players.
    fn apply_sound(&mut self) {
        rymplayer::fx::set(self.config.fx());
        if let Some(a) = &self.audio {
            a.set_speed(self.config.speed);
        }
    }

    /// `[` and `]`: speed and pitch quick picks (Normal, Nightcore, Slowed).
    fn step_mode(&mut self, step: i32) {
        self.config.change(rymplayer::config::Setting::Mode, step);
        self.apply_sound();
        self.config.save();
        self.status = format!("{}: {}", tr("Speed and pitch"), self.config.value(rymplayer::config::Setting::Mode));
    }

    /// `z`: the sleep timer: 15, 30, 45, 60 minutes, end of the song, off.
    fn next_sleep(&mut self) {
        let next = match self.sleep.map(|(m, _)| m) {
            None => Some(Some(15)),
            Some(Some(m)) if m < 60 => Some(Some(m + 15)),
            Some(Some(_)) => Some(None),
            Some(None) => None,
        };
        self.sleep = next.map(|m| (m, Instant::now() + Duration::from_secs(m.unwrap_or(0) * 60)));
        if let Some(a) = &self.audio {
            a.set_fade(1.0);
        }
        self.status = match next {
            Some(Some(m)) => format!("{}: {m} {}", tr("Sleep timer"), tr("minutes")),
            Some(None) => tr("The music stops after this song").into(),
            None => tr("Sleep timer off").into(),
        };
    }

    /// `R`: "Your recap": the most played songs; hours and top artists in the title.
    fn show_recap(&mut self) {
        let r = rymplayer::stats::recap();
        let artists: Vec<&str> = r.top_artists.iter().take(3).map(|(a, _)| a.as_str()).collect();
        self.page = Page::Recap;
        self.page_title = format!("{}: {:.1} h {} · {:.1} h {} · {}", tr("Your recap"), r.hours_month, tr("this month"), r.hours_total, tr("in all"), artists.join(", "));
        self.busy = false;
        self.focus = Focus::Tracks;
        self.tracks = r.top_songs.into_iter().map(|(t, plays)| Track { album: format!("{plays} {}", tr("plays")), ..t }).collect();
        self.track_state.select((!self.tracks.is_empty()).then_some(0));
    }

    /// Crossfade, the sleep timer, listening stats and Discord, every tick.
    fn sound_tick(&mut self) {
        let Some(a) = &mut self.audio else { return };
        a.tick();
        let (playing, pos, dur) = (a.is_playing(), a.position(), a.duration().unwrap_or(0.0));
        let since = self.listened_at.elapsed();
        self.listened_at = Instant::now();
        if playing && let Some(t) = self.now_playing() {
            rymplayer::stats::listened(t, since.as_millis().min(1000) as u64);
        }
        if self.stats_written.elapsed() > Duration::from_secs(60) {
            self.stats_written = Instant::now();
            rymplayer::stats::flush();
        }
        if self.loading {
            return;
        }
        // Sleep timer: the last 10 seconds fade out, then the music stops.
        if let Some((minutes, when)) = self.sleep {
            let left = match minutes {
                Some(_) => when.saturating_duration_since(Instant::now()).as_secs_f32(),
                None if dur > 0.0 => dur - pos,
                None => f32::MAX,
            };
            if playing && left < 10.0 {
                if let Some(a) = &self.audio {
                    a.set_fade((left / 10.0).clamp(0.0, 1.0));
                    if left <= 0.25 {
                        a.pause();
                        a.set_fade(1.0);
                    }
                }
                if left <= 0.25 {
                    self.sleep = None;
                    self.status = tr("Sleep timer: music stopped").into();
                    return;
                }
            }
        }
        // Discord: told when the song, play/pause, or the position (a seek) changes.
        if self.config.discord && !self.config.discord_app_id.is_empty() && let Some(t) = self.now_playing().cloned() {
            let expected = self.discord_sent.as_ref().map(|(_, was, at, when)| at + if *was { when.elapsed().as_secs_f32() } else { 0.0 });
            let changed = self.discord_sent.as_ref().is_none_or(|(id, was, ..)| *id != t.video_id || *was != playing) || expected.is_some_and(|e| (e - pos).abs() > 3.0);
            if changed {
                self.discord_sent = Some((t.video_id.clone(), playing, pos, Instant::now()));
                let cover = t.thumb.as_ref().map(|u| api::thumb_sized(u, 512));
                rymplayer::discord::show(&self.config.discord_app_id, rymplayer::discord::Show::Song { video_id: t.video_id.clone(), title: t.title.clone(), artist: t.artist.clone(), cover, at: pos, length: dur, paused: !playing });
            }
        }
        // Crossfade: the next song starts that many seconds before this one ends. Smart automix:
        // also as soon as it has faded out in its last 15 seconds.
        let automix = self.config.automix;
        let secs = if automix { rymplayer::automix::mix_secs(self.config.crossfade) } else { self.config.crossfade as f32 };
        let now_id = self.now_playing().map(|t| t.video_id.clone());
        let has_next = self.q.current.is_some_and(|c| c + 1 < self.q.songs.len());
        let end_of_song_sleep = matches!(self.sleep, Some((None, _)));
        let lead = if automix { rymplayer::automix::LOAD_LEAD } else { 0.0 };
        let due = pos >= dur - secs - lead || (automix && pos >= dur - rymplayer::automix::WINDOW_SECS && self.audio.as_ref().is_some_and(|a| a.faded_out()));
        if secs > 0.0 && playing && dur > secs + 10.0 && due && has_next && !end_of_song_sleep && self.crossfade_for != now_id {
            self.crossfade_for = now_id;
            self.crossfade_next = true;
            self.next();
        }
    }

    /// Five songs from the end of a page that goes on: its next part loads (once at a time).
    fn load_more_near_the_end(&mut self) {
        if self.busy || self.loading_more || self.tracks.is_empty() {
            return;
        }
        let Some(selected) = self.track_state.selected() else { return };
        if selected + 5 < self.tracks.len() {
            return;
        }
        let (Some(more), Some(api)) = (self.more.clone(), self.api.clone()) else { return };
        self.loading_more = true;
        self.status = tr("Loading more...").into();
        let page = self.page.clone();
        self.spawn(async move { Event::More(page, s(api.more(more).await)) });
    }

    /// Writes what plays, where in it, and the volume to the session file, when any of it changed.
    fn save_session(&mut self) {
        let now = self.now_playing().map(|t| t.video_id.clone()).unwrap_or_default();
        let position = match (&self.resume_at, &self.audio) {
            (Some((id, at)), _) if *id == now => *at,
            (_, Some(a)) if a.is_loaded() => a.position(),
            _ => 0.0,
        };
        let key = (now, position as u32, (self.volume * 100.0) as u32, self.q.songs.len());
        if self.session_key.as_ref() == Some(&key) {
            return;
        }
        rymplayer::session::save(&rymplayer::session::Session::new(&self.q, position, self.volume));
        self.session_key = Some(key);
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
        let cookie = rymplayer::secret::read_cookie_io().expect("signed-in cookie");
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
        let cookie = rymplayer::secret::read_cookie_io().expect("signed-in cookie");
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
        let cookie = rymplayer::secret::read_cookie_io().expect("signed-in cookie");
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
