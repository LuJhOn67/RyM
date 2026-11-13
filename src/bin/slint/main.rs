//! RyM Slint: the window app drawn with Slint (ui/app.slint), on the shared core: the same
//! account, queue, Up next (RaM points), lyrics, likes, sound settings and session as the
//! other apps. The window holds what it shows; this side keeps the state, starts the work on a
//! tokio runtime, and puts the answers back on the window's thread (`post` -> `handle`).

#![cfg_attr(windows, windows_subsystem = "windows")]

slint::include_modules!();

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rymplayer::api::{Account, Api, LIKED_ID, Link, More, Paged, Playlist, Suggestion, Track};
use rymplayer::audio::Audio;
use rymplayer::player_core as core;
use rymplayer::config::Config;
use rymplayer::i18n::{tr, tr_any};
use rymplayer::likes::Likes;
use rymplayer::lyrics::Lyrics;
use rymplayer::queue::Queue;
use rymplayer::{feed, fmt_time, login, parse_duration, stream};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

#[cfg_attr(not(windows), allow(dead_code))]
const MINI_TITLE: &str = "RyM mini player";
/// Covers asked for at these sizes (pixels): rows, cards, and the big one.
const ROW_PX: u32 = 120;
const CARD_PX: u32 = 200;
/// The playing song's big cover (drawn at 506 px; twice that stays sharp on 150-200% screens).
const ART_PX: u32 = 1200;
/// Covers kept in memory, at most (the oldest go first; each up to about 160 KB).
const COVERS_KEPT: usize = 250;
/// Up next rows on the window, at most.
const QUEUE_SHOWN: usize = 60;

enum Ev {
    /// The folder picked in the system's folder window (None: closed without one).
    FolderPicked(Option<std::path::PathBuf>),
    Auth(bool, anyhow::Result<Api>),
    Account(anyhow::Result<Account>),
    Avatar(anyhow::Result<Vec<u8>>),
    Home(anyhow::Result<feed::Refreshed>),
    HomeMore(anyhow::Result<Paged>),
    Playlists(anyhow::Result<Vec<Playlist>>),
    Liked(anyhow::Result<Vec<Track>>),
    List(u64, anyhow::Result<Vec<Track>>),
    ListPaged(u64, anyhow::Result<Paged>),
    ListMore(u64, anyhow::Result<Paged>),
    Search(u64, anyhow::Result<Paged>),
    Suggestions(String, anyhow::Result<Vec<Suggestion>>),
    Cover(String, Option<(u32, u32, Vec<u8>)>),
    /// The playing song's big cover and its blurred copy.
    Art(String, Option<Art>),
    Fetched(u64, anyhow::Result<stream::Playable>),
    Lyrics(String, Option<Lyrics>),
    Translated(String, String, Vec<String>),
    Radio(anyhow::Result<Vec<Track>>),
    /// Three skips in a row: a new direction's seed and its radio (see `player_core::SkipStreak`).
    NewDirection(anyhow::Result<(Track, Vec<Track>)>),
    /// An album, playlist or mix's songs, to play (0), play next (1) or add to the queue (2).
    LinkSongs(u8, String, anyhow::Result<Vec<Track>>),
    /// "Play in RyM": a link handed over by another copy (see `open_incoming`).
    Incoming(String),
    /// The songs of a link from outside, to play now (picked by hand).
    Handed(anyhow::Result<Vec<Track>>),
    Similar(String, anyhow::Result<Vec<Track>>),
    SameArtist(String, anyhow::Result<Vec<Track>>),
    ArtistFound(String, anyhow::Result<Option<String>>),
    Rated(String, bool, anyhow::Result<()>),
    Downloaded(anyhow::Result<PathBuf>),
    Update(anyhow::Result<Option<rymplayer::updates::Release>>, bool),
    Installed(anyhow::Result<PathBuf>),
    MediaKey(souvlaki::MediaControlEvent),
    /// The plugin store's list.
    Store(anyhow::Result<Vec<rymplayer::plugins::Plugin>>),
    /// A video's picture and sound links (video plugin), and a song's direct link for VLC.
    VideoLinks(Track, anyhow::Result<(String, Option<String>)>),
    InVlc(u64, anyhow::Result<(String, Vec<(String, String)>)>),
    /// Up next saved as a playlist (what to say), a song added to one.
    SavedQueue(anyhow::Result<String>),
    AddedTo(String, anyhow::Result<()>),
    /// The account's history (Recently played, kept in step with the phone).
    History(anyhow::Result<Vec<Track>>),
    ResultsMore(u64, anyhow::Result<Paged>),
    /// The tray icon was clicked (show the window), or one of its menu's items picked.
    TrayShow,
    TrayMenu(String),
    /// Every source's lyrics for a song (the source pills).
    Sources(String, Vec<Lyrics>),
    /// Romaji for a song's lyrics (the lyrics' source, the lines).
    Romaji(String, String, Vec<String>),
    YtDlp(anyhow::Result<String>),
    /// "Update parsers" finished: what happened.
    Parsers(String),
    /// The PO Token helper's install finished: what happened.
    Bgutil(String),
    /// What ytmusicapi is (version, or why it isn't working).
    ParsersStatus(String),
    LastFmToken(anyhow::Result<(String, String)>),
    LastFmSession(anyhow::Result<(String, String)>),
    Batch(stream::Batch),
    /// A phone took the music (Seamless Music the other way): pause here.
    LinkPause,
    Peer(rymplayer::link::Announce, std::net::IpAddr),
    /// The phone's remote: an action and its value.
    Remote(String, f32),
    /// SponsorBlock's non-music parts of the song asked for (by its request id).
    Segments(String, rymplayer::sponsorblock::Segments),
    /// What yt-dlp, ejs and Node are (the settings' line).
    YtDlpStatus(String),
    TookOver(String, anyhow::Result<rymplayer::link::State>),
    Done,
}

type Pixels = (u32, u32, Vec<u8>);
type Art = (Pixels, Pixels);

/// The window's lists, kept so a row can change without building the list again.
struct Models {
    queue: Rc<VecModel<Song>>,
    similar: Rc<VecModel<Card>>,
    same: Rc<VecModel<Card>>,
    home: Rc<VecModel<Shelf>>,
    home_cards: Vec<Rc<VecModel<Card>>>,
    list: Rc<VecModel<Song>>,
    results: Rc<VecModel<Song>>,
    playlists: Rc<VecModel<Card>>,
    suggestions: Rc<VecModel<Suggest>>,
    lyrics: Rc<VecModel<LyricLine>>,
    settings: Rc<VecModel<Setting>>,
}

struct State {
    ui: slint::Weak<AppWindow>,
    m: Models,
    api: Option<Api>,
    config: Config,
    account: Option<Account>,
    audio: Option<Audio>,
    q: Queue,
    /// Songs played this session (Up next leaves them out).
    played: HashSet<String>,
    request_id: u64,
    loading: bool,
    ended_for: u64,
    crossfade_next: bool,
    /// Gapless: the next song was loaded to start on the frame this one ends.
    gapless_next: bool,
    /// SponsorBlock, Last.fm and Seamless Music's rules (shared with GTK and the terminal).
    non_music: core::NonMusic,
    scrobbler: core::Scrobbler,
    peers: core::PeerWatch,
    streak: core::SkipStreak,
    /// yt-dlp, ejs and Node as found (the settings).
    ytdlp_status: String,
    crossfade_for: u64,
    resume_at: Option<(String, f32)>,
    radio_pending: bool,
    /// The next song was picked by hand (RaM points: its minus points go).
    user_pick: bool,
    /// Liquid Aurora: the loudest bass lately (the pulse is against it).
    bass_peak: f32,
    /// "Reset all points" pressed once (a second press within 5 s resets).
    reset_asked: Option<Instant>,
    /// What the yes/no question is about ("ram-weekly").
    confirm_for: String,
    /// When the scheduled points reset was last looked at.
    reset_checked: Option<Instant>,
    /// The animations' clock: its timer (running only while something animates), its frame
    /// interval, when it started; frames counted and their average time (the badge).
    clock_timer: slint::Timer,
    clock_every: Option<Duration>,
    clock_start: Instant,
    frames: u32,
    frame_ms: f32,
    last_frame: Option<Instant>,
    perf_at: Instant,
    /// Battery saver or a low battery, looked at every 30 s.
    saving: bool,
    saving_at: Option<Instant>,
    /// Rain or ash: every particle as x, y, vx, vy, life (−1 a drop, > 0 a droplet's seconds
    /// left, 0 free), in one array made once; the window's size it was made for; the path text
    /// (kept, cleared and refilled each frame); the bass now.
    rain: Vec<f32>,
    rain_size: (f32, f32),
    rain_path: String,
    rain_time: f32,
    bass_now: f32,
    /// The pulse (Liquid Aurora, the heartbeat) and the needles: what the sound says now, and what
    /// the window shows, eased toward it on the animations' clock (a Slint animation on them
    /// redrew the whole window at the screen's own rate, past the frame rate chosen).
    energy_to: f32,
    energy: f32,
    vu_to: (f32, f32),
    vu: (f32, f32),
    repeat_one: bool,
    /// Repeat all: at the end of the list it starts over.
    repeat_all: bool,
    lyrics: Option<Lyrics>,
    liked: Likes,
    liked_tracks: Vec<Track>,
    playlists: Vec<Playlist>,
    /// Home: its items (songs, and playlists / albums / mixes to open), grouped into shelves.
    home: Vec<Track>,
    home_shelves: Vec<Vec<usize>>,
    home_more: Option<More>,
    home_loading: bool,
    loading_more: bool,
    /// The list page (a playlist, liked songs, an artist, history) and where it goes on.
    list: Vec<Track>,
    list_more: Option<More>,
    list_id: u64,
    results: Vec<Track>,
    search_id: u64,
    suggestions: Vec<Suggestion>,
    suggest_for: Option<String>,
    suggest_timer: slint::Timer,
    /// Similar songs and the artist's songs, and the song / artist they're for.
    similar: Vec<Track>,
    similar_for: String,
    same: Vec<Track>,
    same_for: String,
    covers: HashMap<String, Image>,
    cover_order: VecDeque<String>,
    requested: HashSet<String>,
    art_for: String,
    toast_id: u64,
    last_prev: Instant,
    listened_at: Instant,
    stats_written: Instant,
    session_key: Option<(String, u32, u32, usize)>,
    discord_sent: Option<(String, bool, f32, Instant)>,
    /// What the window shows now (playing, second, length, sung line): told only on a change.
    shown: (bool, u32, u32, i32),
    mini: Option<MiniWindow>,
    media: Option<souvlaki::MediaControls>,
    media_sent: Option<(String, bool)>,
    /// The floating lyrics, and what they show; the share picture's window.
    float: Option<FloatWindow>,
    float_shown: (String, String),
    share: Option<ShareWindow>,
    /// Sleep timer: when the music stops (or at the end of the song).
    sleep: Option<Sleep>,
    /// Every source's lyrics for the playing song, once looked up (the source pills).
    alt_lyrics: Option<(String, Vec<Lyrics>)>,
    sources_for: String,
    /// Romaji by song.
    romaji: HashMap<String, Vec<String>>,
    /// Last.fm: the token while connecting.
    lastfm_token: Option<String>,
    /// What this computer plays, for the phones.
    link: std::sync::Arc<std::sync::Mutex<rymplayer::link::Playing>>,
    /// The visualizer: its loudest lately, and whether the window shows the moving wave.
    wave_peak: f32,
    wave_live: bool,
    /// What the last yt-dlp update and update check said (shown in the settings).
    ytdlp_text: String,
    /// "Update parsers": what it's doing or did, and what's installed (ytmusicapi).
    parsers_text: String,
    parsers_status: String,
    /// The PO Token helper's install: what it's doing or did.
    bgutil_text: String,
    update_text: String,
    /// The plugin store's list once loaded.
    store: Vec<rymplayer::plugins::Plugin>,
    /// Where the search results go on.
    results_more: Option<More>,
    /// The search's rows shown (indices into `results`) under the chosen filter: all, songs, videos.
    results_shown: Vec<usize>,
    results_filter: i32,
    /// A link from outside that came before the sign-in was ready: opened once it is.
    pending_link: Option<String>,
    /// Seamless Music, asked first: the phone waiting for an answer (address, port, key, name).
    peer_pending: Option<(std::net::IpAddr, u16, String, String)>,
    /// What the text box question is for ("save-queue", "link").
    prompt_for: String,
    /// VLC playing a song or a video.
    vlc: Option<std::process::Child>,
    /// The title bar follows Windows' dark / light setting (what it was last set to).
    title_dark: Option<bool>,
    /// The icon next to the clock (while RyM runs in the background, its window hidden).
    tray: Option<tray_icon::TrayIcon>,
    /// Likes made on the phone come in every few minutes.
    likes_timer: slint::Timer,
}

type Shared = Rc<RefCell<State>>;

#[derive(Clone, Copy, PartialEq)]
enum Sleep {
    At(Instant),
    EndOfSong,
}

/// The layouts to pick from, with their number in the window (Theme.index) and the settings.
const THEMES: [(&str, usize); 2] = [("RyM Dusk", 0), ("RyM Wide", 2)];

/// A saved layout's place in [`THEMES`] (the light RyM Mist, gone, opens as Dusk).
fn theme_slot(saved: usize) -> usize {
    THEMES.iter().position(|t| t.1 == saved).unwrap_or(0)
}
const SLEEP_CHOICES: [u32; 7] = [0, 15, 30, 45, 60, 90, u32::MAX];

thread_local! {
    static STATE: RefCell<Option<Shared>> = const { RefCell::new(None) };
    /// The window is in front (focused) and shown (not minimized or covered), from winit.
    static WINDOW_UP: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((true, false)) };
    /// The window is hidden in the tray (RyM plays on in the background).
    static IN_TRAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Nobody sees the window: hidden in the tray, minimized, or covered. Then every effect stops and
/// the window isn't told the position, the lyrics or the levels (the sound and the small windows
/// go on); it's all brought up to date when the window comes back.
fn unseen(ui: Option<&AppWindow>) -> bool {
    WINDOW_UP.with(|w| w.get().1) || IN_TRAY.with(|t| t.get()) || ui.is_some_and(|ui| ui.window().is_minimized())
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio"))
}

/// Starts `task` in the background; its answer is handled on the window's thread.
fn post(task: impl Future<Output = Ev> + Send + 'static) {
    runtime().spawn(async move {
        send(task.await);
    });
}

/// Hands an answer from another thread to the window's thread.
fn send(ev: Ev) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(st) = STATE.with(|s| s.borrow().clone()) {
            handle(&st, ev);
        }
    });
}

fn log(line: &str) {
    use std::io::Write;
    let path = rymplayer::data_dir().join("rym-slint.log");
    let fresh = std::fs::metadata(&path).map(|m| m.len() > 200_000).unwrap_or(false);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(!fresh).write(true).truncate(fresh).open(&path) {
        let _ = writeln!(f, "{} {line}", rymplayer::updates::now());
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let thread = std::thread::current();
        log(&format!("CRASH at {at} (thread {}): {msg}", thread.name().unwrap_or("unnamed")));
        rymplayer::crash::record("RyM Slint", &at, &msg);
    }));
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--render") {
        // A picture of the window with sample songs (no window opens): the layout's check.
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "rym-slint.png".into());
        let page = args.get(i + 3).and_then(|p| p.parse().ok()).unwrap_or(0);
        let theme = args.get(i + 2).map_or(0, |t| if t == "wide" { 2 } else { 0 });
        render_sample(&out, page, theme);
        return;
    }
    // "Play in RyM": a link on the command line (a rym:// link from the browser too) goes to the
    // RyM already open, which plays it; this copy then quits.
    let incoming = args.iter().skip(1).find(|a| rymplayer::links::is_incoming(a)).cloned();
    if let Some(link) = &incoming
        && rymplayer::link::hand_over(link)
    {
        return;
    }
    rymplayer::updates::clean_old();
    // Linux: the Japanese and dot-digit fonts that come with RyM (beside it or in its AppImage).
    rymplayer::fonts::add_bundled();
    let config = Config::load();
    rymplayer::i18n::set_arabic(config.arabic);
    rymplayer::fx::set(config.fx());
    stream::use_source(config.source);
    rymplayer::ytmusicapi::use_engine(rymplayer::ytmusicapi::Engine::from_index(config.music_engine));
    rymplayer::ram_points::set_discovery(config.discovery);
    rymplayer::ytmusicapi::use_language(config.arabic);
    stream::prefer_lossless(config.prefer_lossless);
    stream::download_best(config.download_best);
    stream::download_parallel(config.download_connections, config.download_piece_mb);

    // Skia draws text sharper than the default renderer; the default stays if Skia can't start.
    if let Err(e) = slint::BackendSelector::new().renderer_name("skia".into()).select() {
        log(&format!("skia renderer not available: {e}"));
    }
    let ui = AppWindow::new().expect("window");
    {
        // Animations stop while the window isn't in front or is minimized / covered.
        use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
        ui.window().on_winit_window_event(|_, ev| {
            match ev {
                WindowEvent::Focused(f) => WINDOW_UP.with(|w| w.set((*f, w.get().1))),
                WindowEvent::Occluded(o) => WINDOW_UP.with(|w| w.set((w.get().0, *o))),
                _ => {}
            }
            EventResult::Propagate
        });
    }
    let st = build(&ui, config);
    STATE.with(|s| *s.borrow_mut() = Some(st.clone()));
    start(&st);
    // A check of the quit path: quits by itself after so many seconds.
    if let Some(secs) = args.iter().position(|a| a == "--quit-after").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u64>().ok()) {
        let st = st.clone();
        slint::Timer::single_shot(Duration::from_secs(secs), move || quit(&st));
    }
    let files: Vec<Track> = args.iter().skip(1).map(std::path::PathBuf::from).filter(|p| p.is_file()).map(|p| rymplayer::local::track(&p)).collect();
    if !files.is_empty() {
        play_list(&st, files, 0);
    }
    rymplayer::link::take_handed_links(|link| send(Ev::Incoming(link)));
    std::thread::spawn(rymplayer::link::register_rym_links);
    if let Some(link) = incoming {
        open_incoming(&st, link);
    }
    let ticker = slint::Timer::default();
    {
        let st = st.clone();
        ticker.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || tick(&st));
    }
    // The X: close, or hide to the tray and keep playing (see `close_requested`).
    {
        let st = st.clone();
        ui.window().on_close_requested(move || close_requested(&st));
    }
    let _ = ui.show();
    // Windows 11's glass, once the window exists (`--glass mica|acrylic` tries it for one run).
    let glass = match args.iter().position(|a| a == "--glass").and_then(|i| args.get(i + 1)).map(String::as_str) {
        Some("mica") => 1,
        Some("acrylic") => 2,
        _ => st.borrow().config.window_glass,
    };
    if glass > 0 {
        window_glass(&ui, glass);
    }
    start_tray(&st);
    // Runs on while the window is hidden in the tray; Quit ends it.
    let _ = slint::run_event_loop_until_quit();
    save_session(&st);
    rymplayer::stats::flush();
    shut_down(st);
}

/// Closes the sound, the media keys, the tray icon and the small windows now, while their threads
/// still run. Left to the end of the process (the state lives in a thread-local), Windows has
/// already stopped those threads, and the sound player waiting for its own panicked ("threads
/// should not terminate unexpectedly") on every quit.
fn shut_down(st: Shared) {
    STATE.with(|s| s.borrow_mut().take());
    if let Ok(mut s) = st.try_borrow_mut() {
        if let Some(a) = s.audio.as_mut() {
            a.stop();
        }
        s.audio = None;
        s.media = None;
        s.tray = None;
        s.mini = None;
        s.float = None;
    }
    drop(st);
}

fn models() -> Models {
    Models {
        queue: Rc::new(VecModel::default()),
        similar: Rc::new(VecModel::default()),
        same: Rc::new(VecModel::default()),
        home: Rc::new(VecModel::default()),
        home_cards: Vec::new(),
        list: Rc::new(VecModel::default()),
        results: Rc::new(VecModel::default()),
        playlists: Rc::new(VecModel::default()),
        suggestions: Rc::new(VecModel::default()),
        lyrics: Rc::new(VecModel::default()),
        settings: Rc::new(VecModel::default()),
    }
}

fn build(ui: &AppWindow, config: Config) -> Shared {
    let m = models();
    let app = ui.global::<App>();
    ui.global::<Theme>().set_index(THEMES[theme_slot(config.gui_theme)].1 as i32);
    ui.global::<Theme>().set_cover_accent(false);
    paint(&ui.global::<Theme>(), current_colors(&config).as_ref());
    ui.global::<Theme>().set_aurora(config.aurora);
    ui.global::<Theme>().set_vinyl(config.vinyl);
    ui.global::<Ui>().set_rtl(config.arabic);
    app.set_karaoke(config.karaoke);
    app.set_queue(ModelRc::from(m.queue.clone()));
    app.set_similar(ModelRc::from(m.similar.clone()));
    app.set_same_artist(ModelRc::from(m.same.clone()));
    app.set_home(ModelRc::from(m.home.clone()));
    app.set_list(ModelRc::from(m.list.clone()));
    app.set_results(ModelRc::from(m.results.clone()));
    app.set_playlists(ModelRc::from(m.playlists.clone()));
    app.set_suggestions(ModelRc::from(m.suggestions.clone()));
    app.set_lyrics(ModelRc::from(m.lyrics.clone()));
    app.set_settings(ModelRc::from(m.settings.clone()));
    app.set_version(rymplayer::VERSION.into());
    app.set_lyrics_size(config.lyrics_font.clamp(14, 48) as i32);
    ui.global::<Ui>().set_zoom(config.ui_zoom.clamp(0.5, 1.5));
    app.set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for("")))));
    app.on_tr(|s| tr_any(&s).into());

    let audio = Audio::new().map_err(|e| log(&format!("no audio device: {e}"))).ok();
    let session = rymplayer::session::load();
    let volume = session.as_ref().map_or(0.8, |s| s.volume).clamp(0.0, 1.0);
    if let Some(a) = &audio {
        a.set_volume(volume);
        a.set_speed(config.speed);
    }
    app.set_volume(volume);
    let st = Rc::new(RefCell::new(State {
        ui: ui.as_weak(),
        m,
        api: None,
        config,
        account: None,
        audio,
        q: Queue::default(),
        played: HashSet::new(),
        request_id: 0,
        loading: false,
        ended_for: 0,
        crossfade_next: false,
        gapless_next: false,
        non_music: Default::default(),
        scrobbler: Default::default(),
        peers: Default::default(),
        streak: Default::default(),
        ytdlp_status: String::new(),
        crossfade_for: 0,
        resume_at: None,
        radio_pending: false,
        user_pick: false,
        bass_peak: 0.05,
        reset_asked: None,
        confirm_for: String::new(),
        reset_checked: None,
        clock_timer: slint::Timer::default(),
        clock_every: None,
        clock_start: Instant::now(),
        frames: 0,
        frame_ms: 0.0,
        last_frame: None,
        perf_at: Instant::now(),
        saving: false,
        saving_at: None,
        rain: vec![0.0; RAIN_POOL * 5],
        rain_size: (0.0, 0.0),
        rain_path: String::with_capacity(RAIN_POOL * 32),
        rain_time: 0.0,
        bass_now: 0.0,
        energy_to: 0.0,
        energy: 0.0,
        vu_to: (0.0, 0.0),
        vu: (0.0, 0.0),
        repeat_one: false,
        repeat_all: false,
        lyrics: None,
        liked: Likes::default(),
        liked_tracks: Vec::new(),
        playlists: Vec::new(),
        home: Vec::new(),
        home_shelves: Vec::new(),
        home_more: None,
        home_loading: true,
        loading_more: false,
        list: Vec::new(),
        list_more: None,
        list_id: 0,
        results: Vec::new(),
        search_id: 0,
        suggestions: Vec::new(),
        suggest_for: None,
        suggest_timer: slint::Timer::default(),
        similar: Vec::new(),
        similar_for: String::new(),
        same: Vec::new(),
        same_for: String::new(),
        covers: HashMap::new(),
        cover_order: VecDeque::new(),
        requested: HashSet::new(),
        art_for: String::new(),
        toast_id: 0,
        last_prev: Instant::now() - Duration::from_secs(10),
        listened_at: Instant::now(),
        stats_written: Instant::now(),
        session_key: None,
        discord_sent: None,
        shown: (false, u32::MAX, u32::MAX, -2),
        mini: None,
        media: None,
        media_sent: None,
        likes_timer: slint::Timer::default(),
        tray: None,
        store: Vec::new(),
        results_more: None,
        results_shown: Vec::new(),
        results_filter: 0,
        pending_link: None,
        peer_pending: None,
        prompt_for: String::new(),
        vlc: None,
        title_dark: None,
        float: None,
        float_shown: (String::new(), String::new()),
        share: None,
        sleep: None,
        romaji: HashMap::new(),
        alt_lyrics: None,
        sources_for: String::new(),
        lastfm_token: None,
        link: Default::default(),
        wave_peak: 0.05,
        wave_live: false,
        ytdlp_text: String::new(),
        parsers_text: String::new(),
        parsers_status: String::new(),
        bgutil_text: String::new(),
        update_text: String::new(),
    }));
    if let Some(saved) = session {
        let mut s = st.borrow_mut();
        s.q = saved.queue();
        if let Some(t) = s.q.now() {
            s.resume_at = Some((t.video_id.clone(), saved.position));
        }
    }
    wire(ui, &st);
    fill_settings(&st);
    show_speed(&st);
    st
}

/// The window's buttons, to what they do.
fn wire(ui: &AppWindow, st: &Shared) {
    let app = ui.global::<App>();
    let w = |f: fn(&Shared)| {
        let st = st.clone();
        move || f(&st)
    };
    app.on_toggle_play(w(toggle_playback));
    app.on_next(w(skip));
    app.on_previous(w(previous));
    app.on_like_now(w(|st| {
        let t = st.borrow().q.now().cloned();
        if let Some(t) = t {
            toggle_like(st, &t);
        }
    }));
    app.on_repeat(w(|st| {
        let on = {
            let mut s = st.borrow_mut();
            // Off, then repeat all (for good), then this song (for good), then off.
            let (one, all) = match (s.repeat_one, s.repeat_all) {
                (false, false) => (false, true),
                (false, true) => (true, false),
                _ => (false, false),
            };
            s.repeat_one = one;
            s.repeat_all = all;
            (one, all)
        };
        with_app(st, |a| {
            a.set_repeat_one(on.0);
            a.set_repeat_all(on.1);
        });
        toast(st, if on.0 { tr("Repeat this song") } else if on.1 { tr("Repeat all") } else { tr("Repeat off") });
    }));
    app.on_refresh_home(w(refresh_home));
    app.on_more(w(load_more));
    app.on_play_all(w(|st| {
        let songs: Vec<Track> = st.borrow().list.iter().filter(|t| t.link.is_none()).cloned().collect();
        play_list(st, songs, 0);
    }));
    app.on_sign_in(w(|st| {
        toast(st, tr("A Google window opens: sign in there"));
        post(async {
            let r = async {
                let cookie = login::sign_in().await?;
                Api::with_cookie(cookie).await
            }
            .await;
            Ev::Auth(true, r)
        });
    }));
    app.on_sign_out(w(sign_out));
    app.on_float_lyrics(w(|st| {
        let on = !st.borrow().config.lyrics_overlay;
        set_toggle(st, "float", on);
    }));
    app.on_share(w(open_share));
    app.on_copy_link(w(copy_link));
    {
        let st = st.clone();
        app.on_confirm_done(move |yes| confirm_done(&st, yes));
    }
    {
        let st = st.clone();
        app.on_prompt_done(move |ok| prompt_done(&st, ok));
    }
    {
        let st = st.clone();
        app.on_peer_choice(move |yes| {
            with_app(&st, |a| a.set_peer_ask(false));
            let pending = st.borrow_mut().peer_pending.take();
            if yes && let Some((ip, port, key, device)) = pending {
                post(async move { Ev::TookOver(device, rymplayer::link::take_over(ip, port, &key).await) });
            }
        });
    }
    {
        let st = st.clone();
        app.on_close_choice(move |tray, remember| {
            with_app(&st, |a| a.set_close_ask(false));
            if remember {
                let mut s = st.borrow_mut();
                s.config.close_action = if tray { "tray" } else { "quit" }.into();
                s.config.save();
            }
            if tray {
                to_tray(&st);
            } else {
                quit(&st);
            }
        });
    }
    {
        let st = st.clone();
        app.on_set_speed(move |id| set_speed(&st, &id));
    }
    // The lyrics stage fills the screen (and gives it back when it closes).
    {
        let st = st.clone();
        app.on_fullscreen(move |on| {
            if let Some(ui) = st.borrow().ui.upgrade() {
                ui.window().set_fullscreen(on);
            }
        });
    }
    app.on_next_theme(w(|st| {
        let next = (theme_slot(st.borrow().config.gui_theme) + 1) % THEMES.len();
        set_theme(st, next);
    }));
    app.on_open_artist(w(|st| {
        let t = st.borrow().q.now().cloned();
        if let Some(t) = t {
            go_artist(st, &t);
        }
    }));
    app.on_mini(w(|st| {
        let open = st.borrow().mini.is_some();
        show_mini(st, !open);
    }));
    {
        let st = st.clone();
        app.on_seek(move |secs| {
            if let Some(a) = &st.borrow().audio {
                a.seek(secs.max(0.0));
            }
        });
    }
    {
        // ← →: the step from the settings, back or forward, within the song.
        let st = st.clone();
        app.on_skip_by(move |dir| {
            let s = st.borrow();
            if let Some(a) = s.audio.as_ref().filter(|a| a.is_loaded()) {
                let step = s.config.seek_step as f32 * dir.signum() as f32;
                let end = a.duration().unwrap_or(f32::MAX);
                a.seek((a.position() + step).clamp(0.0, (end - 1.0).max(0.0)));
            }
        });
    }
    {
        let st = st.clone();
        app.on_set_volume(move |v| {
            let v = v.clamp(0.0, 1.0);
            if let Some(a) = &st.borrow().audio {
                a.set_volume(v);
            }
            with_app(&st, |a| a.set_volume(v));
        });
    }
    {
        let st = st.clone();
        app.on_go(move |page| go(&st, page));
    }
    {
        let st = st.clone();
        app.on_play(move |key| play_key(&st, &key));
    }
    {
        let st = st.clone();
        app.on_open_card(move |key| open_card(&st, &key));
    }
    {
        let st = st.clone();
        app.on_menu(move |action, key| song_menu(&st, &action, &key));
    }
    {
        let st = st.clone();
        app.on_search(move |q| search(&st, q.to_string()));
    }
    {
        let st = st.clone();
        app.on_typed(move |q| typed(&st, q.to_string()));
    }
    {
        let st = st.clone();
        let st2 = st.clone();
        app.on_set_results_filter(move |f| {
            st2.borrow_mut().results_filter = f;
            show_songs(&st2, 'r');
            mark_playing(&st2);
        });
        app.on_pick_suggestion(move |i| {
            let text = st.borrow().suggestions.get(i as usize).map(|s| s.text.clone());
            if let Some(text) = text {
                with_app(&st, |a| a.set_query(text.clone().into()));
                search(&st, text);
            }
        });
    }
    {
        let st = st.clone();
        app.on_forget_suggestion(move |i| {
            let (token, api) = {
                let mut s = st.borrow_mut();
                let i = i as usize;
                if i >= s.suggestions.len() {
                    return;
                }
                let g = s.suggestions.remove(i);
                s.m.suggestions.remove(i);
                (g.forget, s.api.clone())
            };
            if let (Some(token), Some(api)) = (token, api) {
                post(async move {
                    let _ = api.forget_search(token).await;
                    Ev::Done
                });
            }
        });
    }
    {
        let st = st.clone();
        app.on_lyric_clicked(move |i| {
            let s = st.borrow();
            if let (Some(l), Some(a)) = (&s.lyrics, &s.audio)
                && let Some((at, _)) = l.synced.get(i as usize)
            {
                a.seek(*at);
            }
        });
    }
    {
        let st = st.clone();
        app.on_set_toggle(move |id, on| set_toggle(&st, &id, on));
    }
    {
        let st = st.clone();
        app.on_pick_source(move |i| {
            let picked = {
                let s = st.borrow();
                let vid = s.q.now().map(|t| t.video_id.clone()).unwrap_or_default();
                s.alt_lyrics.as_ref().filter(|(id, _)| *id == vid).and_then(|(_, list)| list.get(i as usize).cloned()).map(|l| (vid, l))
            };
            if let Some((vid, l)) = picked {
                // Kept for the song, so it comes back next time.
                rymplayer::lyrics::choose(&vid, &l);
                show_lyrics(&st, Some(l));
            }
        });
    }
    {
        let st = st.clone();
        app.on_choose(move |id, step| choose(&st, &id, step));
    }
    {
        let st = st.clone();
        app.on_set_text(move |id, text| set_text(&st, &id, &text));
    }
    {
        let st = st.clone();
        app.on_press(move |id| press(&st, &id));
    }
    {
        let st = st.clone();
        app.on_move_queue(move |key, by| {
            let Some(from) = key.strip_prefix("q:").and_then(|i| i.parse::<usize>().ok()) else { return };
            {
                let mut s = st.borrow_mut();
                let first = s.q.current.map_or(0, |c| c + 1);
                let to = (from as i64 + by as i64).clamp(first as i64, s.q.songs.len().saturating_sub(1) as i64) as usize;
                if to != from {
                    s.q.move_song(from, to);
                }
            }
            render_queue(&st);
        });
    }
    {
        let st = st.clone();
        app.on_zoom(move |z| set_zoom(&st, z));
    }
}

fn with_app(st: &Shared, f: impl FnOnce(App<'_>)) {
    let ui = st.borrow().ui.upgrade();
    if let Some(ui) = ui {
        f(ui.global::<App>());
    }
}

fn toast(st: &Shared, text: &str) {
    let id = {
        let mut s = st.borrow_mut();
        s.toast_id += 1;
        s.toast_id
    };
    with_app(st, |a| a.set_toast(text.into()));
    let st = st.clone();
    slint::Timer::single_shot(Duration::from_millis(3200), move || {
        if st.borrow().toast_id == id {
            with_app(&st, |a| a.set_toast(SharedString::new()));
        }
    });
}

/// Start: sign in with the saved session (or as a guest), the last home feed at once, media
/// keys, and Seamless Music listening on the Wi-Fi.
fn start(st: &Shared) {
    match rymplayer::secret::read_cookie_io().ok().filter(|c| !c.trim().is_empty()) {
        Some(cookie) => {
            let home = rymplayer::page_cache::load_home();
            if !home.is_empty() {
                show_home(st, home);
            }
            post(async move { Ev::Auth(false, Api::with_cookie(cookie).await) });
        }
        None => post(async { Ev::Auth(false, Api::guest().await) }),
    }
    with_app(st, |a| a.set_home_loading(true));
    show_now(st);
    render_queue(st);
    rymplayer::link::listen(|note, ip| send(Ev::Peer(note, ip)));
    {
        let s = st.borrow();
        if let Ok(mut p) = s.link.lock() {
            p.enabled = s.config.link_announce;
        }
        let device = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
        rymplayer::link::announce(device, s.link.clone(), || send(Ev::LinkPause), |what, value| send(Ev::Remote(what, value)));
    }
    background_upkeep(st);
    if st.borrow().config.lyrics_overlay {
        let st2 = st.clone();
        slint::Timer::single_shot(Duration::from_millis(800), move || show_float(&st2, true));
    }
    // The window needs to be on screen before Windows' media controls can take its handle.
    let st2 = st.clone();
    slint::Timer::single_shot(Duration::from_millis(1500), move || start_media_controls(&st2));
    {
        let st4 = st.clone();
        // Likes and the account's history (songs played on the phone) every 2 minutes; the
        // title bar follows Windows' dark / light setting.
        st.borrow().likes_timer.start(slint::TimerMode::Repeated, Duration::from_secs(120), move || {
            sync_with_account(&st4);
        });
    }
    post(async { Ev::Store(rymplayer::plugins::store().await) });
    {
        let st5 = st.clone();
        slint::Timer::single_shot(Duration::from_millis(400), move || title_bar(&st5));
        let st6 = st.clone();
        let t = slint::Timer::default();
        t.start(slint::TimerMode::Repeated, Duration::from_secs(3), move || title_bar(&st6));
        std::mem::forget(t);
    }
    let st3 = st.clone();
    slint::Timer::single_shot(Duration::from_secs(8), move || {
        let (auto, last) = {
            let s = st3.borrow();
            (s.config.auto_update_check, s.config.last_update_check)
        };
        if rymplayer::updates::due(auto, last) {
            check_updates(&st3, false);
        }
    });
}

fn go(st: &Shared, page: i32) {
    with_app(st, |a| a.set_page(page));
    if page == 6 {
        fill_stats(st);
    }
    let (empty_home, loading) = {
        let s = st.borrow();
        (s.home.is_empty(), s.home_loading)
    };
    match page {
        3 if empty_home && !loading => refresh_home(st),
        2 => show_playlists(st),
        _ => {}
    }
}

// ---------------------------------------------------------------- covers

/// The cover at `px` if it's here; otherwise an empty picture, and it's fetched.
fn cover(s: &mut State, url: Option<&String>, px: u32) -> Image {
    let Some(url) = url else { return Image::default() };
    let sized = rymplayer::api::thumb_sized(url, px);
    if let Some(img) = s.covers.get(&sized) {
        return img.clone();
    }
    if s.requested.insert(sized.clone()) {
        post(async move {
            let px_data = fetch(&sized).await.ok().and_then(|b| decode_square(&b, px));
            Ev::Cover(sized, px_data)
        });
    }
    Image::default()
}

fn covers_http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap_or_default())
}

async fn fetch(url: &str) -> anyhow::Result<Vec<u8>> {
    Ok(covers_http().get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
}

/// A picture cut to a centered square and scaled to `px`, as RGBA.
fn decode_square(bytes: &[u8], px: u32) -> Option<Pixels> {
    let img = image::load_from_memory(bytes).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let sized = if side > px { square.resize_exact(px, px, image::imageops::FilterType::Triangle) } else { square };
    let rgba = sized.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

fn to_image((w, h, px): Pixels) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h))
}

fn empty(img: &Image) -> bool {
    img.size().width == 0
}

/// Covers that came in go into the rows still waiting for them.
fn fill_covers(st: &Shared) {
    let s = st.borrow();
    let covers = &s.covers;
    let get = |t: &Track, px: u32| t.thumb.as_ref().and_then(|u| covers.get(&rymplayer::api::thumb_sized(u, px)).cloned());
    let start = s.q.current.map_or(0, |c| c + 1);
    for (i, t) in s.q.songs.iter().skip(start).take(QUEUE_SHOWN).enumerate() {
        fill_song(&s.m.queue, i, get(t, ROW_PX));
    }
    let list: Vec<&Track> = s.list.iter().collect();
    for (model, tracks) in [(&s.m.list, list), (&s.m.results, shown_results(&s))] {
        for (i, t) in tracks.iter().enumerate() {
            fill_song(model, i, get(t, ROW_PX));
        }
    }
    for (model, tracks) in [(&s.m.similar, &s.similar), (&s.m.same, &s.same)] {
        for (i, t) in tracks.iter().enumerate() {
            fill_card(model, i, get(t, CARD_PX));
        }
    }
    for (shelf, rows) in s.home_shelves.iter().enumerate() {
        if let Some(model) = s.m.home_cards.get(shelf) {
            for (i, &at) in rows.iter().enumerate() {
                if let Some(t) = s.home.get(at) {
                    fill_card(model, i, get(t, CARD_PX));
                }
            }
        }
    }
    if let Some(t) = s.liked_tracks.first() {
        fill_card(&s.m.playlists, 0, get(t, CARD_PX));
    }
    for (i, p) in s.playlists.iter().enumerate() {
        let img = p.thumb.as_ref().and_then(|u| covers.get(&rymplayer::api::thumb_sized(u, CARD_PX)).cloned());
        fill_card(&s.m.playlists, i + 2, img);
    }
}

fn fill_song(model: &VecModel<Song>, i: usize, img: Option<Image>) {
    if let (Some(img), Some(mut row)) = (img, model.row_data(i))
        && empty(&row.cover)
    {
        row.cover = img;
        model.set_row_data(i, row);
    }
}

fn fill_card(model: &VecModel<Card>, i: usize, img: Option<Image>) {
    if let (Some(img), Some(mut row)) = (img, model.row_data(i))
        && empty(&row.cover)
    {
        row.cover = img;
        model.set_row_data(i, row);
    }
}

fn song_item(s: &mut State, key: String, t: &Track) -> Song {
    let playing = s.q.now().is_some_and(|n| n.video_id == t.video_id);
    Song {
        key: key.into(),
        title: t.title.clone().into(),
        artist: t.artist.clone().into(),
        cover: cover(s, t.thumb.as_ref(), ROW_PX),
        liked: s.liked.contains(&t.video_id),
        playing,
        video: rymplayer::api::found_as_video(&t.video_id),
        why: SharedString::new(),
    }
}

/// The search's rows under the chosen filter.
fn shown_results(s: &State) -> Vec<&Track> {
    s.results_shown.iter().filter_map(|&i| s.results.get(i)).collect()
}

fn card_item(s: &mut State, key: String, t: &Track) -> Card {
    let sub = if t.link.is_some() && t.artist.is_empty() { t.album.clone() } else { t.artist.clone() };
    Card { key: key.into(), title: t.title.clone().into(), sub: sub.into(), cover: cover(s, t.thumb.as_ref(), CARD_PX) }
}

// ---------------------------------------------------------------- what's on screen

/// The playing song's names, heart and cover.
fn show_now(st: &Shared) {
    let (t, liked) = {
        let s = st.borrow();
        let t = s.q.now().cloned();
        let liked = t.as_ref().is_some_and(|t| s.liked.contains(&t.video_id));
        (t, liked)
    };
    with_app(st, |a| {
        a.set_has_song(t.is_some());
        a.set_title(t.as_ref().map_or(String::new(), |t| t.title.clone()).into());
        a.set_artist(t.as_ref().map_or(String::new(), |t| t.artist.clone()).into());
        a.set_liked(liked);
        if let Some(t) = &t {
            a.set_len_text(if t.duration.is_empty() { "0:00".into() } else { t.duration.clone().into() });
            a.set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for(&t.video_id)))));
            a.set_artist_name(t.artist.split([',', '&']).next().unwrap_or_default().trim().into());
        }
    });
    let Some(t) = t else { return };
    if st.borrow().art_for == t.video_id {
        return;
    }
    st.borrow_mut().art_for = t.video_id.clone();
    with_app(st, |a| {
        a.set_cover(Image::default());
        a.set_backdrop(Image::default());
    });
    if let Some(url) = t.thumb.clone() {
        let vid = t.video_id.clone();
        let blur = st.borrow().config.glass_blur.clamp(2.0, 20.0);
        post(async move {
            let sized = rymplayer::api::thumb_sized(&url, ART_PX);
            let art = fetch(&sized).await.ok().and_then(|b| {
                let big = decode_square(&b, ART_PX)?;
                // The backdrop is made from the big cover's pixels where they are (no copy of them).
                let img = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(big.0, big.1, &big.2[..])?;
                let small = image::imageops::resize(&img, 96, 96, image::imageops::FilterType::Triangle);
                let soft = image::imageops::blur(&small, blur);
                Some((big, (soft.width(), soft.height(), soft.into_raw())))
            });
            Ev::Art(vid, art)
        });
    }
}

/// The wave's bars for a song: always the same for one song, different between songs.
fn wave_for(video_id: &str) -> Vec<f32> {
    let mut seed: u64 = video_id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) | 1;
    (0..51)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let r = (seed % 1000) as f32 / 1000.0;
            // Fuller in the middle, like a song.
            let middle = 1.0 - ((i as f32 - 25.0) / 25.0).powi(2) * 0.25;
            (0.42 + 0.58 * r) * middle
        })
        .collect()
}

/// Up next: the songs after the playing one.
fn render_queue(st: &Shared) {
    let mut s = st.borrow_mut();
    if let Ok(mut p) = s.link.lock() {
        let current = s.q.current.unwrap_or(0);
        let kept: Vec<(usize, &Track)> = s.q.songs.iter().enumerate().filter(|(_, t)| !rymplayer::local::is_local(&t.video_id)).collect();
        p.current = kept.iter().position(|(i, _)| *i >= current).unwrap_or(0);
        p.songs = kept.into_iter().map(|(_, t)| t.clone()).collect();
    }
    let start = s.q.current.map_or(0, |c| c + 1);
    let rows: Vec<(usize, Track)> = s.q.songs.iter().enumerate().skip(start).take(QUEUE_SHOWN).map(|(i, t)| (i, t.clone())).collect();
    let mut items: Vec<Song> = rows.iter().map(|(i, t)| song_item(&mut s, format!("q:{i}"), t)).collect();
    // Why each song RyM picked is there (not the ones added by hand).
    if let Some(seed) = s.q.now().cloned() {
        let picked_from = start + s.q.added;
        let tracks: Vec<Track> = rows.iter().map(|(_, t)| t.clone()).collect();
        for ((i, _), (item, why)) in rows.iter().zip(items.iter_mut().zip(rymplayer::ram_points::reasons(&tracks, &seed, |id| s.liked.contains(id)))) {
            if *i >= picked_from {
                item.why = why.into();
            }
        }
    }
    s.m.queue.set_vec(items);
}

fn show_cards(st: &Shared, which: char) {
    let mut s = st.borrow_mut();
    let tracks = if which == 's' { s.similar.clone() } else { s.same.clone() };
    let items: Vec<Card> = tracks.iter().enumerate().map(|(i, t)| card_item(&mut s, format!("{which}:{i}"), t)).collect();
    if which == 's' {
        s.m.similar.set_vec(items);
    } else {
        s.m.same.set_vec(items);
    }
}

fn show_songs(st: &Shared, which: char) {
    let mut s = st.borrow_mut();
    if which == 'l' {
        let tracks = s.list.clone();
        let items: Vec<Song> = tracks.iter().enumerate().map(|(i, t)| song_item(&mut s, format!("l:{i}"), t)).collect();
        s.m.list.set_vec(items);
        return;
    }
    // Search: all, the songs, or the videos (the keys keep each row's place in `results`).
    let tracks = s.results.clone();
    let videos = tracks.iter().filter(|t| rymplayer::api::found_as_video(&t.video_id)).count();
    let mixed = videos > 0 && videos < tracks.len();
    if !mixed {
        s.results_filter = 0;
    }
    let filter = s.results_filter;
    let shown: Vec<usize> = (0..tracks.len())
        .filter(|&i| match filter {
            1 => !rymplayer::api::found_as_video(&tracks[i].video_id),
            2 => rymplayer::api::found_as_video(&tracks[i].video_id),
            _ => true,
        })
        .collect();
    let items: Vec<Song> = shown.iter().map(|&i| song_item(&mut s, format!("r:{i}"), &tracks[i])).collect();
    s.results_shown = shown;
    s.m.results.set_vec(items);
    if let Some(ui) = s.ui.upgrade() {
        ui.global::<App>().set_results_mixed(mixed);
        ui.global::<App>().set_results_filter(filter);
    }
}

/// Home: the items grouped by shelf, each a row of cards.
fn show_home(st: &Shared, home: Vec<Track>) {
    let mut s = st.borrow_mut();
    s.home = home;
    let mut names: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, t) in s.home.iter().enumerate() {
        match names.iter().position(|n| *n == t.album) {
            Some(g) => groups[g].push(i),
            None => {
                names.push(t.album.clone());
                groups.push(vec![i]);
            }
        }
    }
    let mut shelves = Vec::new();
    let mut card_models = Vec::new();
    for (name, rows) in names.iter().zip(&groups) {
        let cards: Vec<Card> = rows
            .iter()
            .map(|&i| {
                let t = s.home[i].clone();
                card_item(&mut s, format!("h:{i}"), &t)
            })
            .collect();
        let model = Rc::new(VecModel::from(cards));
        shelves.push(Shelf { title: shelf_name(name).into(), cards: ModelRc::from(model.clone()) });
        card_models.push(model);
    }
    s.home_shelves = groups;
    s.m.home_cards = card_models;
    s.m.home.set_vec(shelves);
}

fn shelf_name(name: &str) -> String {
    match name {
        feed::RECENTLY_PLAYED | feed::FOR_YOU | feed::MORE_FOR_YOU => tr_any(name),
        other if rymplayer::ram_points::SHELVES.contains(&other) => tr_any(other),
        other => other.to_string(),
    }
}

fn show_playlists(st: &Shared) {
    let names: Vec<SharedString> = st.borrow().playlists.iter().map(|p| SharedString::from(p.title.as_str())).collect();
    with_app(st, |a| a.set_playlist_names(ModelRc::from(Rc::new(VecModel::from(names)))));
    let mut s = st.borrow_mut();
    // Songs on this computer (the Music folder and the folders from the settings): no account needed.
    let mut cards = vec![Card { key: "p:local".into(), title: tr("On this device").into(), sub: SharedString::new(), cover: Image::default() }];
    if s.api.as_ref().is_some_and(Api::logged_in) {
        // The liked songs' tile shows the newest liked song's cover.
        let liked_cover = match s.liked_tracks.first().cloned() {
            Some(t) => cover(&mut s, t.thumb.as_ref(), CARD_PX),
            None => Image::default(),
        };
        cards.push(Card { key: "p:liked".into(), title: tr("Liked songs").into(), sub: format!("{} {}", s.liked_tracks.len(), tr("songs")).into(), cover: liked_cover });
        cards.push(Card { key: "p:history".into(), title: tr("History").into(), sub: SharedString::new(), cover: Image::default() });
    }
    let lists = s.playlists.clone();
    for (i, p) in lists.iter().enumerate() {
        let img = cover(&mut s, p.thumb.as_ref(), CARD_PX);
        cards.push(Card { key: format!("p:{i}").into(), title: p.title.clone().into(), sub: SharedString::new(), cover: img });
    }
    s.m.playlists.set_vec(cards);
}

/// Hearts in every list follow the likes.
fn refresh_hearts(st: &Shared) {
    let s = st.borrow();
    let liked_now = s.q.now().is_some_and(|t| s.liked.contains(&t.video_id));
    let start = s.q.current.map_or(0, |c| c + 1);
    let queue: Vec<&Track> = s.q.songs.iter().skip(start).take(QUEUE_SHOWN).collect();
    let list: Vec<&Track> = s.list.iter().collect();
    let results = shown_results(&s);
    for (model, tracks) in [(&s.m.queue, queue), (&s.m.list, list), (&s.m.results, results)] {
        for (i, t) in tracks.iter().enumerate() {
            if let Some(mut row) = model.row_data(i) {
                let liked = s.liked.contains(&t.video_id);
                if row.liked != liked {
                    row.liked = liked;
                    model.set_row_data(i, row);
                }
            }
        }
    }
    if let Some(ui) = s.ui.upgrade() {
        ui.global::<App>().set_liked(liked_now);
    }
}

/// Rows mark the playing song.
fn mark_playing(st: &Shared) {
    let s = st.borrow();
    let now = s.q.now().map(|t| t.video_id.clone()).unwrap_or_default();
    let list: Vec<&Track> = s.list.iter().collect();
    for (model, tracks) in [(&s.m.list, list), (&s.m.results, shown_results(&s))] {
        for (i, t) in tracks.iter().enumerate() {
            if let Some(mut row) = model.row_data(i) {
                let playing = t.video_id == now;
                if row.playing != playing {
                    row.playing = playing;
                    model.set_row_data(i, row);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- settings

/// The plugins in use show their parts (video menu items, SoundCloud search).
fn plugin_flags(st: &Shared) {
    let (video, sc) = {
        let s = st.borrow();
        (s.config.plugins.contains_key(rymplayer::plugins::VIDEO), s.config.plugins.contains_key(rymplayer::plugins::SOUNDCLOUD))
    };
    with_app(st, |a| {
        a.set_has_video(video);
        a.set_has_soundcloud(sc);
        if !sc {
            a.set_search_in(0);
        }
    });
}

fn fill_settings(st: &Shared) {
    let s = st.borrow();
    let c = &s.config;
    let t = |x: &str| tr_any(x);
    let head = |title: &str| Setting { id: SharedString::new(), kind: 4, title: t(title).into(), about: SharedString::new(), on: false, value: SharedString::new() };
    let switch = |id: &str, title: &str, about: &str, on: bool| Setting { id: id.into(), kind: 0, title: t(title).into(), about: t(about).into(), on, value: SharedString::new() };
    let choice = |id: &str, title: &str, value: String| Setting { id: id.into(), kind: 1, title: t(title).into(), about: SharedString::new(), on: false, value: value.into() };
    let field = |id: &str, title: &str, about: &str, value: &str| Setting { id: id.into(), kind: 2, title: t(title).into(), about: t(about).into(), on: false, value: value.into() };
    let button = |id: &str, title: &str, about: String, label: &str| Setting { id: id.into(), kind: 3, title: t(title).into(), about: about.into(), on: false, value: t(label).into() };
    let sleep = match s.sleep {
        None => t("Off"),
        Some(Sleep::EndOfSong) => t("End of song"),
        Some(Sleep::At(when)) => format!("{} min", when.saturating_duration_since(Instant::now()).as_secs().div_ceil(60)),
    };
    let only = if c.only_kind.is_empty() { t("Every kind heard") } else { t(&c.only_kind) };
    let lastfm = if !c.lastfm_session.is_empty() {
        button("lastfm", "Last.fm", format!("{} {}", t("Connected as"), c.lastfm_user), "Disconnect")
    } else if s.lastfm_token.is_some() {
        button("lastfm", "Last.fm", t("Allow RyM on the Last.fm page that opened, then press here"), "Finish connecting")
    } else {
        button("lastfm", "Last.fm", t("Songs you play go into your Last.fm profile (needs an API key and secret from last.fm/api)"), "Connect")
    };
    let rows = vec![
        head("Look"),
        choice("theme", "Theme", THEMES[theme_slot(c.gui_theme)].0.to_string()),
        choice("palette", "Colors", if c.palette.is_empty() { t("The layout's own") } else { c.palette.clone() }),
        switch("aurora", "Liquid Aurora", "The cover's colors move behind everything with the bass", c.aurora),
        switch("vinyl", "Vinyl record", "The cover turns like a record: faster with Nightcore, slower with Slowed", c.vinyl),
        choice("glass-blur", "Glass blur", format!("{}", c.glass_blur.round())),
        choice("window-glass", "Windows glass", t(["Off", "Mica", "Acrylic"][c.window_glass.min(2) as usize])),
        choice("seek-step", "Arrow keys skip", format!("{} s", c.seek_step)),
        choice("fps", "Animation frame rate", if c.fps_cap == 0 { t("As fast as the screen") } else { format!("{} FPS", c.fps_cap) }),
        switch("perf", "Performance monitor", "Frames a second, frame time and memory in a corner", c.perf_badge),
        switch("lossless", "Prefer lossless (FLAC)", "Looks for the song as FLAC on the Internet Archive first (free and legal); YouTube when it isn't there", c.prefer_lossless),
        button("theme-export", "Export colors", t("Saves these colors as a .json file in Downloads"), "Export"),
        button("theme-import", "Import colors", t("A .json theme file (its path), or the JSON itself"), "Import"),
        choice("zoom", "App size", format!("{}%", (c.ui_zoom * 100.0).round())),
        switch("accent", "Accent from the cover", "The playing song's colors on the buttons and the wave", c.accent_from_cover),
        switch("visualizer", "Moving wave", "The wave under the song moves with the music", c.visualizer),
        switch("arabic", "العربية", "Arabic words, and the layout turned around (after a restart)", c.arabic),
        head("Sound"),
        switch("automix", "Smart Automix", "Songs blend like a DJ mix: on the fade-out, on the beat, without the basses clashing", c.automix),
        choice("crossfade", "Crossfade", format!("{} s", c.crossfade)),
        switch("skip-silence", "Skip silence", "Quiet starts and ends are skipped", c.skip_silence),
        switch("normalize", "Even loudness", "Every song about as loud (by YouTube's own measure when it has one)", c.normalize),
        switch("gapless", "Gapless", "With no crossfade, the next song starts the instant this one ends", c.gapless),
        switch("sponsorblock", "Skip non-music parts", "SponsorBlock: a music video's intro, talk and outro are skipped", c.sponsorblock),
        choice("eq", "Equalizer", t(c.eq_name())),
        choice("mode", "Speed", t(c.mode_name())),
        choice("sleep", "Sleep timer", sleep),
        head("On this device"),
        field("local-folders", "Other music folders", "Besides your Music folder; separate them with ;", &c.local_folders.join("; ")),
        button("local-add", "Add a folder", t("Pick a folder with songs; it joins the list above"), "Add"),
        head("Lyrics"),
        choice("lyrics-size", "Lyrics size", format!("{} px", c.lyrics_font)),
        switch("translate", "Translate Japanese lyrics", "English under each Japanese line", c.lyrics_translation),
        switch("romaji", "Romaji", "Japanese lines in Latin letters under them", c.romaji),
        switch("float", "Floating lyrics", "The sung line in a small window above the others", c.lyrics_overlay),
        head("Suggestions (RaM points)"),
        choice("only-kind", "Only this kind", only),
        choice("discovery", "Familiar or new", t(rymplayer::ram_points::DISCOVERY_NAMES[c.discovery.min(4) as usize])),
        button("ram-reset-all", "Reset all points", ram_summary(), "Reset all"),
        button("ram-reset-neg", "Reset negative points only", t("Skipped songs come back; your favorites stay"), "Reset"),
        field("ram-reset-date", "Reset on a date", "Like 2026-12-31; empty: no date. On that day all points go back to 0", &c.ram_reset_date),
        switch("ram-weekly", "Weekly reset", "Every 7 days all points go back to 0 (off unless you turn it on)", c.ram_weekly_reset),
        head("Alarm"),
        switch("alarm", "Alarm", "Music starts at this time (RyM has to be open)", c.alarm_on),
        field("alarm-time", "Time", "Like 07:30", &c.alarm_time),
        head("Devices"),
        switch("seamless", "Seamless Music", "Music playing on your phone (same account, same Wi-Fi) moves here when this app is idle", c.cross_ears),
        switch("link-auto", "Take over from the phone by itself", "Off: RyM asks first", c.link_auto),
        switch("announce", "Tell the phone what plays here", "Open RyM on your phone and it takes the music from here", c.link_announce),
        switch("remote", "Remote from the phone", "Your phone can play, pause, skip, seek and set the volume here, and play along in step", c.remote_control),
        head("Playback and downloads"),
        choice("source", "Audio source", t(c.source.label())),
        choice("quality", "Download quality", t(if c.download_best { "Best (yt-dlp)" } else { "Fast" })),
        choice("connections", "Download connections", c.download_connections.to_string()),
        choice("cache", "Keep played songs up to", if c.cache_limit_gb == 0 { t("No limit") } else { format!("{} GB", c.cache_limit_gb) }),
        button("innertube", "YouTube's ways", {
            let r = rymplayer::innertube::report();
            if r.is_empty() { t("How each way of asking YouTube did (after a few songs)") } else { r }
        }, "Refresh"),
        head("Discord"),
        switch("discord", "Show on Discord", "What you play on your Discord profile; it works as it is, or put your own application id below", c.discord),
        field("discord-id", "Application id", "", &c.discord_app_id),
        head("Last.fm"),
        field("lastfm-key", "API key", "", &c.lastfm_key),
        field("lastfm-secret", "Shared secret", "", &c.lastfm_secret),
        lastfm,
        head("Network & Parsers"),
        choice("engine", "Song finder", t(rymplayer::ytmusicapi::Engine::from_index(c.music_engine).label())),
        button("parsers", "Update parsers", {
            let mut text = if s.parsers_text.is_empty() { s.parsers_status.clone() } else { s.parsers_text.clone() };
            if let Some(why) = rymplayer::ytmusicapi::last_failure() {
                text = format!("{text} · {}: {why}", t("Last fallback"));
            }
            if text.is_empty() { t("yt-dlp and ytmusicapi to their newest, saved answers cleared") } else { text }
        }, "Update"),
        button("bgutil", "PO Token helper (bgutil)", if s.bgutil_text.is_empty() { rymplayer::bgutil::status() } else { s.bgutil_text.clone() }, if rymplayer::bgutil::installed() { "Update" } else { "Install" }),
        head("yt-dlp"),
        switch("ytdlp-auto", "Update by itself", "Once a week, when RyM opens", c.ytdlp_auto_update),
        switch("ytdlp-nightly", "Nightly builds", "The newest yt-dlp every day (fixes come sooner)", c.ytdlp_nightly),
        button("ytdlp", "Update yt-dlp", if s.ytdlp_text.is_empty() { s.ytdlp_status.clone() } else { format!("{} · {}", s.ytdlp_text, s.ytdlp_status) }, "Update"),
        head("Window"),
        choice("close", "When closing the window", t(match c.close_action.as_str() {
            "tray" => "Keep in background",
            "quit" => "Close RyM",
            _ => "Ask",
        })),
        head("Plugins"),
        head("Updates"),
        switch("auto-update", "Check for updates", "Every 12 hours", c.auto_update_check),
        switch("beta", "Beta updates", "Pre-releases too", c.beta_updates),
        button("check", "Check now", s.update_text.clone(), "Check"),
    ];
    let mut rows = rows;
    if rymplayer::vlc::find().is_some() {
        let at = rows.iter().position(|r| r.id == "connections").map_or(rows.len(), |i| i + 1);
        rows.insert(at, switch("vlc", "Play songs in VLC", "Each song opens in VLC", c.play_in_vlc));
    }
    if let Some(at) = rows.iter().position(|r| r.kind == 4 && r.title == tr_any("Plugins")) {
        let mut extra: Vec<Setting> = s
            .store
            .iter()
            .map(|p| Setting { id: format!("plugin:{}", p.id).into(), kind: 0, title: p.name().into(), about: p.description().into(), on: c.plugins.contains_key(&p.id), value: SharedString::new() })
            .collect();
        if c.plugins.contains_key(rymplayer::plugins::VIDEO) {
            extra.push(button("link-dl", "Download from a link", tr_any("Video or sound from YouTube or any site"), "Download"));
        }
        for (i, row) in extra.into_iter().enumerate() {
            rows.insert(at + 1 + i, row);
        }
    }
    s.m.settings.set_vec(rows);
}

fn set_toggle(st: &Shared, id: &str, on: bool) {
    {
        let mut s = st.borrow_mut();
        let c = &mut s.config;
        match id {
            "accent" => c.accent_from_cover = on,
            "aurora" => c.aurora = on,
            "perf" => c.perf_badge = on,
            "lossless" => {
                c.prefer_lossless = on;
                stream::prefer_lossless(on);
            }
            // Turned on only after the warning (see `ask_weekly_reset`).
            "ram-weekly" if !on => c.ram_weekly_reset = false,
            "ram-weekly" => {}
            "vinyl" => c.vinyl = on,
            "visualizer" => c.visualizer = on,
            "arabic" => c.arabic = on,
            "automix" => c.automix = on,
            "skip-silence" => c.skip_silence = on,
            "normalize" => c.normalize = on,
            "karaoke" => c.karaoke = on,
            "translate" => c.lyrics_translation = on,
            "romaji" => c.romaji = on,
            "float" => c.lyrics_overlay = on,
            "alarm" => c.alarm_on = on,
            "seamless" => c.cross_ears = on,
            "announce" => c.link_announce = on,
            "link-auto" => c.link_auto = on,
            "remote" => c.remote_control = on,
            "gapless" => c.gapless = on,
            "sponsorblock" => c.sponsorblock = on,
            "ytdlp-auto" => c.ytdlp_auto_update = on,
            "vlc" => c.play_in_vlc = on,
            p if p.starts_with("plugin:") => {
                let pid = &p[7..];
                if on {
                    let version = s.store.iter().find(|x| x.id == pid).map(|x| x.version.clone()).unwrap_or_default();
                    s.config.plugins.insert(pid.to_string(), version);
                } else {
                    s.config.plugins.remove(pid);
                }
            }
            "discord" => c.discord = on,
            "ytdlp-nightly" => c.ytdlp_nightly = on,
            "auto-update" => c.auto_update_check = on,
            "beta" => c.beta_updates = on,
            _ => {}
        }
        rymplayer::fx::set(s.config.fx());
        s.config.save();
        if let Ok(mut p) = s.link.lock() {
            p.enabled = s.config.link_announce;
        }
        if let Some(ui) = s.ui.upgrade() {
            ui.global::<App>().set_karaoke(s.config.karaoke);
            if id == "accent" && !on {
                ui.global::<Theme>().set_cover_accent(false);
            }
        }
    }
    match id {
        "arabic" => toast(st, if on { "أعد تشغيل RyM حتى تتغيّر اللغة" } else { "Restart RyM to change the language" }),
        "float" => show_float(st, on),
        "accent" if on => {
            st.borrow_mut().art_for.clear();
            show_now(st);
        }
        "discord" if !on => rymplayer::discord::show("", rymplayer::discord::Show::Off),
        "ram-weekly" if on => ask_weekly_reset(st),
        "aurora" => {
            if let Some(ui) = st.borrow().ui.upgrade() {
                ui.global::<Theme>().set_aurora(on);
                ui.global::<Theme>().set_energy(0.0);
            }
        }
        "vinyl" => {
            if let Some(ui) = st.borrow().ui.upgrade() {
                ui.global::<Theme>().set_vinyl(on);
            }
        }
        "romaji" | "translate" => {
            let l = st.borrow().lyrics.clone();
            show_lyrics(st, l);
        }
        "visualizer" if !on => {
            st.borrow_mut().wave_live = false;
            let id = st.borrow().q.now().map(|t| t.video_id.clone()).unwrap_or_default();
            with_app(st, |a| a.set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for(&id))))));
        }
        _ => {}
    }
    fill_settings(st);
}

/// One of the settings' "< value >" rows moved by `step`.
fn choose(st: &Shared, id: &str, step: i32) {
    let cycle = |i: usize, n: usize| (i as i32 + step).rem_euclid(n as i32) as usize;
    match id {
        "theme" => {
            let next = cycle(theme_slot(st.borrow().config.gui_theme), THEMES.len());
            set_theme(st, next);
            return;
        }
        "palette" => {
            // The layout's own colors, the built-in themes, then the imported one (if any).
            let mut names = vec![String::new()];
            names.extend(rymplayer::themes::presets().into_iter().map(|t| t.name));
            if st.borrow().config.custom_theme.is_some() {
                names.push("Custom".into());
            }
            let at = names.iter().position(|n| *n == st.borrow().config.palette).unwrap_or(0);
            let name = names[cycle(at, names.len())].clone();
            set_colors(st, &name);
            return;
        }
        "seek-step" => {
            const STEPS: [u32; 4] = [5, 10, 15, 30];
            let mut s = st.borrow_mut();
            let at = STEPS.iter().position(|f| *f == s.config.seek_step).unwrap_or(0);
            s.config.seek_step = STEPS[cycle(at, STEPS.len())];
            s.config.save();
        }
        "window-glass" => {
            let kind = {
                let mut s = st.borrow_mut();
                s.config.window_glass = cycle(s.config.window_glass.min(2) as usize, 3) as u8;
                s.config.save();
                s.config.window_glass
            };
            if let Some(ui) = st.borrow().ui.upgrade() {
                window_glass(&ui, kind);
            }
        }
        "fps" => {
            const STEPS: [u32; 7] = [24, 30, 60, 120, 144, 240, 0];
            let mut s = st.borrow_mut();
            let at = STEPS.iter().position(|f| *f == s.config.fps_cap).unwrap_or(2);
            s.config.fps_cap = STEPS[cycle(at, STEPS.len())];
            s.config.save();
        }
        "glass-blur" => {
            {
                let mut s = st.borrow_mut();
                s.config.glass_blur = (s.config.glass_blur + step as f32).clamp(2.0, 20.0);
                s.config.save();
                // The cover is blurred again.
                s.art_for.clear();
            }
            show_now(st);
        }
        "zoom" => {
            let z = st.borrow().config.ui_zoom + step as f32 * 0.05;
            set_zoom(st, z);
        }
        "sleep" => {
            let s = st.borrow().sleep;
            let at = match s {
                None => 0,
                Some(Sleep::EndOfSong) => SLEEP_CHOICES.len() - 1,
                Some(Sleep::At(when)) => {
                    let mins = when.saturating_duration_since(Instant::now()).as_secs() / 60;
                    SLEEP_CHOICES.iter().position(|m| *m as u64 >= mins).unwrap_or(1)
                }
            };
            let next = cycle(at, SLEEP_CHOICES.len());
            let mut s = st.borrow_mut();
            s.sleep = match SLEEP_CHOICES[next] {
                0 => None,
                u32::MAX => Some(Sleep::EndOfSong),
                m => Some(Sleep::At(Instant::now() + Duration::from_secs(m as u64 * 60))),
            };
            if s.sleep.is_none() && let Some(a) = &s.audio {
                a.set_fade(1.0);
            }
        }
        "close" => {
            let names = ["ask", "tray", "quit"];
            let mut s = st.borrow_mut();
            let at = names.iter().position(|n| *n == s.config.close_action).unwrap_or(0);
            s.config.close_action = names[cycle(at, names.len())].to_string();
            s.config.save();
        }
        "engine" => {
            let mut s = st.borrow_mut();
            s.config.music_engine = cycle(s.config.music_engine as usize, 3) as u8;
            rymplayer::ytmusicapi::use_engine(rymplayer::ytmusicapi::Engine::from_index(s.config.music_engine));
            s.config.save();
        }
        "source" => {
            let mut s = st.borrow_mut();
            s.config.source = s.config.source.next();
            stream::use_source(s.config.source);
            s.config.save();
        }
        "quality" => {
            let mut s = st.borrow_mut();
            s.config.download_best = !s.config.download_best;
            stream::download_best(s.config.download_best);
            s.config.save();
        }
        "cache" => {
            const SIZES: [u32; 7] = [0, 1, 2, 5, 10, 20, 50];
            let (limit, keep) = {
                let mut s = st.borrow_mut();
                let at = SIZES.iter().position(|g| *g == s.config.cache_limit_gb).unwrap_or(3);
                s.config.cache_limit_gb = SIZES[cycle(at, SIZES.len())];
                s.config.save();
                (s.config.cache_limit_gb, s.q.now().map(|t| t.video_id.clone()).unwrap_or_default())
            };
            trim_cache(limit, keep);
        }
        "connections" => {
            let mut s = st.borrow_mut();
            s.config.download_connections = (s.config.download_connections as i32 + step).clamp(1, 16) as u8;
            stream::download_parallel(s.config.download_connections, s.config.download_piece_mb);
            s.config.save();
        }
        "discovery" => {
            let mut s = st.borrow_mut();
            s.config.discovery = (s.config.discovery as i32 + step).clamp(0, 4) as u8;
            rymplayer::ram_points::set_discovery(s.config.discovery);
            s.config.save();
        }
        "only-kind" => {
            let names: Vec<&str> = std::iter::once("").chain(rymplayer::ram_points::Kind::ALL.iter().map(|k| k.name())).collect();
            let mut s = st.borrow_mut();
            let at = names.iter().position(|n| *n == s.config.only_kind).unwrap_or(0);
            s.config.only_kind = names[cycle(at, names.len())].to_string();
            s.config.save();
        }
        _ => {
            let mut s = st.borrow_mut();
            let c = &mut s.config;
            match id {
                "crossfade" => c.crossfade = (c.crossfade as i32 + step).clamp(0, 12) as u8,
                "lyrics-size" => c.lyrics_font = (c.lyrics_font as i32 + step).clamp(14, 48) as u8,
                "eq" => c.eq_preset = cycle(c.eq_preset, rymplayer::fx::PRESETS.len()),
                "mode" => {
                    let names: Vec<&str> = rymplayer::fx::MODES.iter().map(|m| m.0).collect();
                    let at = names.iter().position(|n| *n == c.mode_name()).unwrap_or(0);
                    c.set_mode(names[cycle(at, names.len())]);
                }
                _ => {}
            }
            rymplayer::fx::set(s.config.fx());
            if let Some(a) = &s.audio {
                a.set_speed(s.config.speed);
            }
            s.config.save();
            let speed_row = id == "mode";
            let size = s.config.lyrics_font as i32;
            if speed_row {
                drop(s);
                show_speed(st);
                fill_settings(st);
                return;
            }
            if let Some(ui) = s.ui.upgrade() {
                ui.global::<App>().set_lyrics_size(size);
            }
        }
    }
    fill_settings(st);
}

fn set_text(st: &Shared, id: &str, text: &str) {
    let text = text.trim().to_string();
    let mut s = st.borrow_mut();
    let c = &mut s.config;
    match id {
        "alarm-time" => {
            // "7:5" or "07:05": kept as HH:MM.
            let mut parts = text.split(':').map(|p| p.trim().parse::<u32>().ok());
            if let (Some(Some(h)), Some(Some(m))) = (parts.next(), parts.next())
                && h < 24
                && m < 60
            {
                c.alarm_time = format!("{h:02}:{m:02}");
                c.alarm_rang.clear();
            }
        }
        "discord-id" => c.discord_app_id = text,
        "lastfm-key" => c.lastfm_key = text,
        "lastfm-secret" => c.lastfm_secret = text,
        "ram-reset-date" => {
            if text.is_empty() {
                c.ram_reset_date.clear();
            } else if let Some(d) = rymplayer::ram_points::reset_date(&text) {
                c.ram_reset_date = d;
            } else {
                drop(s);
                toast(st, tr("Write the date like 2026-12-31"));
                fill_settings(st);
                return;
            }
        }
        "local-folders" => c.local_folders = text.split(';').map(|f| f.trim().trim_matches('"').to_string()).filter(|f| !f.is_empty()).collect(),
        _ => return,
    }
    s.config.save();
}

/// Windows 11's glass behind the window (0 off, 1 Mica, 2 Acrylic): DWM draws it, and the window's
/// backgrounds go see-through so it shows. Elsewhere it does nothing.
fn window_glass(ui: &AppWindow, kind: u8) {
    #[cfg(windows)]
    {
        use slint::winit_030::WinitWindowAccessor;
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        #[link(name = "dwmapi")]
        unsafe extern "system" {
            fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const i32, size: u32) -> i32;
            fn DwmExtendFrameIntoClientArea(hwnd: isize, margins: *const [i32; 4]) -> i32;
        }
        ui.window().with_winit_window(|w| {
            if let Ok(h) = w.window_handle()
                && let RawWindowHandle::Win32(h) = h.as_raw()
            {
                let hwnd = h.hwnd.get();
                // DWMWA_SYSTEMBACKDROP_TYPE: 1 none, 2 Mica, 3 Acrylic.
                let backdrop: i32 = match kind {
                    1 => 2,
                    2 => 3,
                    _ => 1,
                };
                let margins = if kind > 0 { [-1; 4] } else { [0; 4] };
                // SAFETY: the window's own handle, and values that outlive the calls.
                unsafe {
                    DwmExtendFrameIntoClientArea(hwnd, &margins);
                    DwmSetWindowAttribute(hwnd, 38, &backdrop, 4);
                }
            }
        });
        ui.global::<App>().set_see_through(kind > 0);
    }
    #[cfg(not(windows))]
    let _ = (ui, kind);
}

/// One clock for every animation (rain and ash, Liquid Aurora's drift, the record): it moves at
/// the chosen frame rate only while something animates, the song plays, the window is in front
/// and shown, and there's no battery saver or low battery. Otherwise it stops: 0 frames.
fn animations(st: &Shared) {
    let (playing, wants, smooth, cap, perf) = {
        let mut s = st.borrow_mut();
        if s.saving_at.is_none_or(|at| at.elapsed() > Duration::from_secs(30)) {
            s.saving_at = Some(Instant::now());
            s.saving = rymplayer::power::saving();
        }
        let playing = s.audio.as_ref().is_some_and(Audio::is_playing);
        let window = s.ui.upgrade();
        let wants = window.as_ref().is_some_and(|ui| {
            let t = ui.global::<Theme>();
            t.get_falling() || t.get_aurora() || t.get_vinyl() || t.get_heartbeat() || t.get_vu()
        });
        // Rain and ash, the needles and the record need every frame; Liquid Aurora and the
        // heartbeat drift slowly (a cycle takes seconds), so alone they get 30 frames a second.
        let smooth = window.as_ref().is_some_and(|ui| {
            let t = ui.global::<Theme>();
            t.get_falling() || t.get_vinyl() || t.get_vu()
        });
        (playing, wants && !unseen(window.as_ref()), smooth, s.config.fps_cap, s.config.perf_badge)
    };
    let covered = unseen(st.borrow().ui.upgrade().as_ref());
    let saving = st.borrow().saving;
    // Not in front but still showing (another window has the focus): it keeps moving; minimized,
    // covered or in the tray: it stops.
    let run = playing && wants && !saving;
    let cap = if smooth { cap } else if cap == 0 { 30 } else { cap.min(30) };
    let every = run.then(|| Duration::from_micros(if cap == 0 { 4_000 } else { 1_000_000 / cap as u64 }));
    if st.borrow().clock_every != every {
        let mut s = st.borrow_mut();
        s.clock_every = every;
        s.last_frame = None;
        match every {
            Some(d) => {
                let st2 = st.clone();
                s.clock_timer.start(slint::TimerMode::Repeated, d, move || clock_tick(&st2));
            }
            None => {
                s.clock_timer.stop();
                // The rain stops with the clock: none left hanging.
                s.rain_size = (0.0, 0.0);
                if let Some(ui) = s.ui.upgrade() {
                    ui.global::<App>().set_drops(SharedString::new());
                }
            }
        }
    }
    // The badge, once a second.
    with_app(st, |a| a.set_perf_on(perf));
    if perf && st.borrow().perf_at.elapsed() >= Duration::from_secs(1) {
        let text = {
            let mut s = st.borrow_mut();
            let secs = s.perf_at.elapsed().as_secs_f32();
            let fps = s.frames as f32 / secs;
            s.frames = 0;
            s.perf_at = Instant::now();
            let why = if !playing {
                tr("paused")
            } else if covered {
                tr("window hidden")
            } else if !wants {
                tr("nothing moves in these colors")
            } else if saving {
                tr("battery saver")
            } else {
                ""
            };
            let frames = if run { format!("{fps:.0} FPS · {:.1} ms", s.frame_ms) } else { format!("0 FPS · {why}") };
            format!("{frames} · {:.0} MB", rymplayer::power::ram_mb())
        };
        with_app(st, |a| a.set_perf_text(text.into()));
    }
}

/// At most this many rain or ash particles (drops first, the rest free for splashes).
const RAIN_POOL: usize = 120;

/// One frame of rain or ash: drops fall (faster with the bass); in RyM Wide one that reaches the
/// player bar's top bursts into 2–3 droplets that bounce (higher with the bass), land and slide
/// off its ends. Writes every particle into one path for the window to draw at once.
fn rain_step(s: &mut State, ash: bool, size: (f32, f32), bar_top: Option<f32>, dt: f32) {
    let (w, h) = size;
    let mut seed = (s.rain_time.to_bits() as u64) ^ 0x9e37_79b9_7f4a_7c15;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f32 / (1u64 << 53) as f32
    };
    let drops = if ash { 50 } else { 75 };
    let fall = if ash { h * 0.06 } else { h * 0.9 };
    if s.rain_size != size {
        s.rain_size = size;
        for i in 0..RAIN_POOL {
            let p = &mut s.rain[i * 5..i * 5 + 5];
            if i < drops {
                p.copy_from_slice(&[rand() * w, rand() * h, if ash { 0.0 } else { -0.04 * fall }, fall * (0.8 + rand() * 0.4), -1.0]);
            } else {
                p[4] = 0.0;
            }
        }
    }
    s.rain_time += dt;
    let t = s.rain_time;
    let b = s.bass_now.clamp(0.0, 1.0);
    for i in 0..RAIN_POOL {
        let o = i * 5;
        let life = s.rain[o + 4];
        if life == 0.0 {
            continue;
        }
        let y0 = s.rain[o + 1];
        if life < 0.0 {
            let vx = if ash { (t * 0.8 + i as f32).sin() * 12.0 } else { s.rain[o + 2] };
            s.rain[o] += vx * dt;
            s.rain[o + 1] += s.rain[o + 3] * (1.0 + b * if ash { 0.5 } else { 0.8 }) * dt;
            let (x, y) = (s.rain[o], s.rain[o + 1]);
            let hit = bar_top.is_some_and(|top| y0 < top && y >= top);
            if hit {
                let top = bar_top.unwrap_or(h);
                let want = 2 + (rand() * 2.0) as usize;
                let mut made = 0;
                for j in 0..RAIN_POOL {
                    if made == want {
                        break;
                    }
                    let q = j * 5;
                    if s.rain[q + 4] != 0.0 {
                        continue;
                    }
                    let side = if rand() < 0.5 { 1.0 } else { -1.0 };
                    s.rain[q..q + 5].copy_from_slice(&[x, top - 1.0, side * (20.0 + rand() * 50.0), -(25.0 + rand() * 35.0) * (1.0 + b * 1.5), 1.6]);
                    made += 1;
                }
            }
            if hit || y > h {
                s.rain[o] = rand() * w;
                s.rain[o + 1] = -rand() * h * 0.15;
            }
        } else {
            s.rain[o + 3] += 470.0 * dt;
            s.rain[o] += s.rain[o + 2] * dt;
            s.rain[o + 1] += s.rain[o + 3] * dt;
            if let Some(top) = bar_top
                && s.rain[o + 3] > 0.0
                && y0 <= top
                && s.rain[o + 1] >= top
            {
                s.rain[o + 1] = top - 1.0;
                s.rain[o + 3] = 0.0;
                s.rain[o + 2] *= 1.01;
            }
            s.rain[o + 4] = life - dt;
            if s.rain[o + 4] <= 0.0 || s.rain[o + 1] > h || s.rain[o] < 0.0 || s.rain[o] > w {
                s.rain[o + 4] = 0.0;
            }
        }
    }
    // The path: a streak per raindrop, a dot (a tiny round-capped line) per ash flake and droplet.
    use std::fmt::Write as _;
    let mut path = std::mem::take(&mut s.rain_path);
    path.clear();
    for i in 0..RAIN_POOL {
        let o = i * 5;
        let life = s.rain[o + 4];
        if life == 0.0 {
            continue;
        }
        let (x, y) = (s.rain[o], s.rain[o + 1]);
        if life < 0.0 && !ash {
            let _ = write!(path, "M{x:.1} {y:.1}L{:.1} {:.1}", x - s.rain[o + 2] * 0.015, y - 14.0);
        } else {
            let _ = write!(path, "M{x:.1} {y:.1}L{x:.1} {:.1}", y + 0.1);
        }
    }
    s.rain_path = path;
}

fn clock_tick(st: &Shared) {
    // Rain or ash, when the colors have it.
    let falling = {
        let s = st.borrow();
        current_colors(&s.config).filter(|t| t.fall() > 0.0).map(|t| t.style == rymplayer::themes::Style::Ash)
    };
    // The window first, its borrow let go: held through the block, the state's borrow below
    // panicked ("RefCell already borrowed") on every frame of rain or ash.
    let window = st.borrow().ui.upgrade();
    if let (Some(ash), Some(ui)) = (falling, window) {
        let win = ui.window();
        let scale = win.scale_factor().max(0.1);
        let size = (win.size().width as f32 / scale, win.size().height as f32 / scale);
        let k = ui.global::<Ui>().get_k();
        let wide = ui.global::<Theme>().get_wide() && ui.global::<App>().get_page() != 0;
        let bar_top = wide.then_some(size.1 - 80.0 * k);
        let text = {
            let mut s = st.borrow_mut();
            let dt = s.last_frame.map_or(0.0, |l| l.elapsed().as_secs_f32().min(0.05));
            rain_step(&mut s, ash, size, bar_top, dt);
            SharedString::from(s.rain_path.as_str())
        };
        ui.global::<App>().set_drops_ash(ash);
        ui.global::<App>().set_drops(text);
    }
    // The pulse and the needles ease toward the sound (quick for the needles, slow for the heartbeat).
    let window = st.borrow().ui.upgrade();
    if let Some(ui) = window {
        let mut s = st.borrow_mut();
        let dt = s.last_frame.map_or(0.0, |l| l.elapsed().as_secs_f32().min(0.1));
        let theme = ui.global::<Theme>();
        let ease = |shown: f32, to: f32, secs: f32| shown + (to - shown) * (1.0 - (-dt / secs).exp());
        let slow = if theme.get_heartbeat() { 0.22 } else { 0.05 };
        let e = ease(s.energy, s.energy_to, slow);
        if (e - s.energy).abs() > 0.001 {
            s.energy = e;
            theme.set_energy(e);
        }
        if theme.get_vu() {
            let vu = (ease(s.vu.0, s.vu_to.0, 0.03), ease(s.vu.1, s.vu_to.1, 0.03));
            if vu != s.vu {
                s.vu = vu;
                theme.set_vu_l(vu.0);
                theme.set_vu_r(vu.1);
            }
        }
    }
    let ms = {
        let mut s = st.borrow_mut();
        let now = Instant::now();
        if let Some(last) = s.last_frame {
            let took = now.duration_since(last).as_secs_f32() * 1000.0;
            s.frame_ms = if s.frame_ms == 0.0 { took } else { s.frame_ms * 0.9 + took * 0.1 };
        }
        s.last_frame = Some(now);
        s.frames += 1;
        // Wraps every ~2.3 hours (a float keeps whole milliseconds that far).
        (s.clock_start.elapsed().as_millis() % 8_388_608) as f32
    };
    with_app(st, |a| a.set_clock(ms));
}

/// The weekly reset is turned on only after its warning.
fn ask_weekly_reset(st: &Shared) {
    st.borrow_mut().confirm_for = "ram-weekly".into();
    with_app(st, |a| {
        a.set_confirm_title(tr("Turn on the weekly reset?").into());
        a.set_confirm_text(tr("Every 7 days, all the points RyM learned from what you play go back to 0, and the suggestions start learning again from scratch. Your likes, playlists and history stay. You can still reset by hand or pick a date instead.").into());
        a.set_confirm_yes(tr("Turn on").into());
        a.set_confirm_open(true);
    });
}

fn confirm_done(st: &Shared, yes: bool) {
    with_app(st, |a| a.set_confirm_open(false));
    let what = std::mem::take(&mut st.borrow_mut().confirm_for);
    if yes && what == "ram-weekly" {
        let mut s = st.borrow_mut();
        s.config.ram_weekly_reset = true;
        // The first reset comes a week from now.
        s.config.ram_last_reset = rymplayer::updates::now();
        s.config.save();
    }
    fill_settings(st);
}

/// The reset the user scheduled (a day, or every week), when it's due; looked at once a minute.
fn scheduled_reset(st: &Shared) {
    let due = {
        let mut s = st.borrow_mut();
        if s.reset_checked.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
            return;
        }
        s.reset_checked = Some(Instant::now());
        let c = &s.config;
        rymplayer::ram_points::reset_due(&c.ram_reset_date, c.ram_weekly_reset, c.ram_last_reset, rymplayer::updates::now(), &rymplayer::ram_points::today())
    };
    let Some(due) = due else { return };
    rymplayer::ram_points::reset_all();
    {
        let mut s = st.borrow_mut();
        s.config.ram_last_reset = rymplayer::updates::now();
        if due == rymplayer::ram_points::Due::Date {
            s.config.ram_reset_date.clear();
        }
        s.config.save();
    }
    toast(st, if due == rymplayer::ram_points::Due::Date { tr("Points reset: the day you picked came") } else { tr("Points reset: a week passed") });
    fill_settings(st);
}

/// The RaM points row's words: songs with points, and the top artists.
fn ram_summary() -> String {
    let sum = rymplayer::ram_points::summary();
    if sum.tracked == 0 {
        return tr("Nothing learned yet").into();
    }
    let mut text = format!("{} {}", sum.tracked, tr("songs with points"));
    if !sum.top_artists.is_empty() {
        text += &format!(" · {}: {}", tr("Top artists"), sum.top_artists.join(", "));
    }
    text
}

fn press(st: &Shared, id: &str) {
    match id {
        "ram-reset-all" => {
            let sure = st.borrow().reset_asked.is_some_and(|at| at.elapsed() < Duration::from_secs(5));
            if sure {
                st.borrow_mut().reset_asked = None;
                rymplayer::ram_points::reset_all();
                toast(st, tr("All points are back to 0"));
                fill_settings(st);
            } else {
                st.borrow_mut().reset_asked = Some(Instant::now());
                toast(st, tr("Press again to reset all points"));
            }
        }
        // The system's folder window, off the window's thread (it waits for the user).
        "local-add" => {
            let title = tr("Pick a music folder");
            std::thread::spawn(move || send(Ev::FolderPicked(rfd::FileDialog::new().set_title(title).pick_folder())));
        }
        "theme-export" => export_colors(st),
        "theme-import" => ask_text(st, "theme", tr("Import colors"), tr("A .json theme file (its path), or the JSON itself")),
        "ram-reset-neg" => {
            rymplayer::ram_points::reset_negative();
            toast(st, tr("Negative points are back to 0"));
            fill_settings(st);
        }
        "link-dl" => ask_text(st, "link", tr("Download from a link"), tr("Paste a link from YouTube or any site")),
        "check" => check_updates(st, true),
        "innertube" => fill_settings(st),
        "bgutil" => {
            st.borrow_mut().bgutil_text = tr("Installing… (git, npm; a minute or two)").into();
            fill_settings(st);
            post(async move { Ev::Bgutil(rymplayer::bgutil::install().await.unwrap_or_else(|e| e.to_string())) });
        }
        "parsers" => {
            let nightly = st.borrow().config.ytdlp_nightly;
            st.borrow_mut().parsers_text = tr("Updating…").into();
            fill_settings(st);
            post(async move { Ev::Parsers(rymplayer::ytmusicapi::update_parsers(nightly).await) });
        }
        "ytdlp" => {
            let nightly = st.borrow().config.ytdlp_nightly;
            st.borrow_mut().ytdlp_text = tr("Updating…").into();
            fill_settings(st);
            post(async move { Ev::YtDlp(rymplayer::ytdlp::update(nightly).await) });
        }
        "lastfm" => {
            let (key, secret, session, token) = {
                let s = st.borrow();
                (s.config.lastfm_key.clone(), s.config.lastfm_secret.clone(), s.config.lastfm_session.clone(), s.lastfm_token.clone())
            };
            if !session.is_empty() {
                let mut s = st.borrow_mut();
                s.config.lastfm_session.clear();
                s.config.lastfm_user.clear();
                s.config.save();
                drop(s);
                fill_settings(st);
            } else if key.is_empty() || secret.is_empty() {
                toast(st, tr("Put your Last.fm API key and secret first (last.fm/api)"));
            } else if let Some(token) = token {
                post(async move { Ev::LastFmSession(rymplayer::lastfm::finish(&key, &secret, &token).await) });
            } else {
                post(async move { Ev::LastFmToken(rymplayer::lastfm::start(&key, &secret).await) });
            }
        }
        _ => {}
    }
}

/// The speed button's pick: a speed (sound at its own pitch), or Nightcore / Slowed.
fn set_speed(st: &Shared, id: &str) {
    {
        let mut s = st.borrow_mut();
        match id {
            "nightcore" => s.config.set_mode("Nightcore"),
            "slowed" => s.config.set_mode("Slowed"),
            other => {
                s.config.set_mode("Normal");
                s.config.speed = other.parse::<f32>().unwrap_or(1.0).clamp(0.5, 2.0);
            }
        }
        rymplayer::fx::set(s.config.fx());
        if let Some(a) = &s.audio {
            a.set_speed(s.config.speed);
        }
        s.config.save();
    }
    show_speed(st);
    fill_settings(st);
}

/// What the speed button shows.
fn show_speed(st: &Shared) {
    let text = {
        let c = &st.borrow().config;
        match c.mode_name() {
            "Nightcore" => "Nightcore".to_string(),
            "Slowed" => "Slowed".to_string(),
            _ => format!("{}×", (c.speed * 100.0).round() / 100.0),
        }
    };
    with_app(st, |a| a.set_speed_text(text.into()));
    // The record turns at 33⅓ a minute (200° a second) times the speed.
    let speed = st.borrow().config.speed;
    if let Some(ui) = st.borrow().ui.upgrade() {
        ui.global::<Theme>().set_spin_rate(200.0 * speed);
    }
}

/// The color theme picked in the settings (None: the layout's own colors).
fn current_colors(c: &Config) -> Option<rymplayer::themes::Theme> {
    match c.palette.as_str() {
        "" => None,
        "Custom" => c.custom_theme.clone(),
        name => rymplayer::themes::preset(name),
    }
}

/// A theme as the window's colors.
fn pal_of(t: &rymplayer::themes::Theme) -> Pal {
    let c = |hex: &str| {
        let (r, g, b, a) = rymplayer::themes::rgba(hex).unwrap_or((128, 128, 128, 255));
        slint::Color::from_argb_u8(a, r, g, b)
    };
    Pal {
        dark: t.dark,
        bg: c(&t.bg),
        raised: c(&t.raised),
        text: c(&t.text),
        dim: c(&t.dim),
        icon: c(&t.icon),
        accent: c(&t.accent),
        accent2: c(&t.accent2),
        bar: c(&t.bar),
        border: c(&t.border),
        round: t.round,
        glow: t.glow,
        cover_border: t.cover_border,
        glass: t.glass,
        dot_clock: t.dot_clock,
        vu: t.vu,
        fall: t.fall(),
        vignette: t.vignette,
        heartbeat: t.heartbeat,
        cut: t.cut,
        pattern: t.tile().map(to_image).unwrap_or_default(),
    }
}

fn paint(theme: &Theme, colors: Option<&rymplayer::themes::Theme>) {
    theme.set_custom(colors.is_some());
    if let Some(t) = colors {
        theme.set_p(pal_of(t));
    }
}

/// Settings > Colors: a built-in theme by name, "Custom", or "" (the layout's own).
fn set_colors(st: &Shared, name: &str) {
    let colors = {
        let mut s = st.borrow_mut();
        s.config.palette = name.to_string();
        s.config.save();
        current_colors(&s.config)
    };
    if let Some(ui) = st.borrow().ui.upgrade() {
        paint(&ui.global::<Theme>(), colors.as_ref());
    }
    toast(st, if name.is_empty() { tr("The layout's own") } else { name });
    fill_settings(st);
}

/// The colors shown now, as a .json file in Downloads.
fn export_colors(st: &Shared) {
    let theme = current_colors(&st.borrow().config).unwrap_or_else(|| rymplayer::themes::Theme { name: "RyM".into(), ..Default::default() });
    let dir = directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.to_path_buf())).unwrap_or_else(rymplayer::data_dir);
    let file = dir.join(format!("RyM theme - {}.json", theme.name.replace(['/', '\\', ':'], "-")));
    match std::fs::write(&file, theme.to_json()) {
        Ok(()) => toast(st, &format!("{} {}", tr("Saved to"), file.display())),
        Err(e) => toast(st, &e.to_string()),
    }
}

/// A theme from a .json file (its path) or the JSON itself: it becomes "Custom" and is shown.
fn import_colors(st: &Shared, text: &str) {
    let text = text.trim();
    let json = if text.starts_with('{') { Ok(text.to_string()) } else { std::fs::read_to_string(text.trim_matches('"')).map_err(|e| e.to_string()) };
    match json.and_then(|j| rymplayer::themes::Theme::from_json(&j)) {
        Ok(t) => {
            st.borrow_mut().config.custom_theme = Some(t);
            set_colors(st, "Custom");
        }
        Err(e) => toast(st, &format!("{}: {e}", tr("Import colors"))),
    }
}

/// Picks layout `slot` of [`THEMES`].
fn set_theme(st: &Shared, slot: usize) {
    let (name, index) = THEMES[slot.min(THEMES.len() - 1)];
    {
        let mut s = st.borrow_mut();
        s.config.gui_theme = index;
        s.config.save();
        if let Some(ui) = s.ui.upgrade() {
            ui.global::<Theme>().set_index(index as i32);
        }
    }
    toast(st, name);
    fill_settings(st);
}

/// The app's size (the layout times `z`, 50% to 150%), kept for the next start.
fn set_zoom(st: &Shared, z: f32) {
    let z = ((z * 20.0).round() / 20.0).clamp(0.5, 1.5);
    let mut s = st.borrow_mut();
    s.config.ui_zoom = z;
    s.config.save();
    if let Some(ui) = s.ui.upgrade() {
        ui.global::<Ui>().set_zoom(z);
    }
}


// ---------------------------------------------------------------- account, home, search

fn sign_out(st: &Shared) {
    rymplayer::innertube::forget_account();
    rymplayer::secret::forget_cookie();
    {
        let mut s = st.borrow_mut();
        s.account = None;
        s.liked = Likes::default();
        s.liked_tracks.clear();
        s.playlists.clear();
    }
    with_app(st, |a| {
        a.set_signed_in(false);
        a.set_has_avatar(false);
        a.set_account_name(SharedString::new());
        a.set_account_handle(SharedString::new());
    });
    toast(st, tr("Signed out"));
    post(async { Ev::Auth(false, Api::guest().await) });
}

fn refresh_home(st: &Shared) {
    let Some(api) = st.borrow().api.clone() else { return };
    st.borrow_mut().home_loading = true;
    with_app(st, |a| a.set_home_loading(true));
    post(async move { Ev::Home(feed::refresh(api).await) });
}

/// The open page goes on (home, or a list) when scrolled near its end.
fn load_more(st: &Shared) {
    let mut s = st.borrow_mut();
    let page = s.ui.upgrade().map_or(0, |ui| ui.global::<App>().get_page());
    let Some(api) = s.api.clone() else { return };
    if s.loading_more {
        return;
    }
    match page {
        1 => {
            if let Some(more) = s.results_more.take() {
                s.loading_more = true;
                let id = s.search_id;
                post(async move { Ev::ResultsMore(id, api.more(more).await) });
            }
        }
        3 => {
            if let Some(more) = s.home_more.take() {
                s.loading_more = true;
                post(async move { Ev::HomeMore(api.more(more).await) });
            }
        }
        5 => {
            if let Some(more) = s.list_more.take() {
                s.loading_more = true;
                let id = s.list_id;
                post(async move { Ev::ListMore(id, api.more(more).await) });
            }
        }
        _ => {}
    }
}

fn typed(st: &Shared, q: String) {
    let q = q.trim().to_string();
    let Some(api) = st.borrow().api.clone() else { return };
    st.borrow_mut().suggest_for = Some(q);
    let st2 = st.clone();
    st.borrow().suggest_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(250), move || {
        let asked = st2.borrow().suggest_for.clone().unwrap_or_default();
        let api = api.clone();
        post(async move {
            let r = api.search_suggestions(asked.clone()).await;
            Ev::Suggestions(asked, r)
        });
    });
}

fn search(st: &Shared, q: String) {
    use rymplayer::{plugins, soundcloud, spotify};
    let q = q.trim().to_string();
    if q.is_empty() {
        return;
    }
    let (api, on_soundcloud, has_sc, has_spotify) = {
        let s = st.borrow();
        let sc = s.config.plugins.contains_key(plugins::SOUNDCLOUD);
        let at = s.ui.upgrade().map_or(0, |ui| ui.global::<App>().get_search_in());
        (s.api.clone(), sc && at == 1, sc, s.config.plugins.contains_key(plugins::SPOTIFY))
    };
    let id = {
        let mut s = st.borrow_mut();
        s.search_id += 1;
        s.results_more = None;
        s.suggestions.clear();
        s.suggest_for = None;
        s.m.suggestions.set_vec(Vec::new());
        s.search_id
    };
    // A Spotify or SoundCloud link (their plugins) opens as a list; SoundCloud searches there.
    if spotify::is_link(&q) {
        let Some(api) = api else { return };
        if !has_spotify {
            toast(st, tr("Install the Spotify plugin to open Spotify links"));
            return;
        }
        toast(st, tr("Finding the Spotify songs on YouTube Music..."));
        open_list(st, "Spotify".into(), spotify::import(api, q));
        return;
    }
    if soundcloud::is_link(&q) || on_soundcloud {
        if !has_sc {
            toast(st, tr("Install the SoundCloud plugin to open SoundCloud links"));
            return;
        }
        if soundcloud::is_link(&q) {
            open_list(st, "SoundCloud".into(), soundcloud::open_link(q));
            return;
        }
        with_app(st, |a| {
            a.set_searching(true);
            a.set_page(1);
        });
        post(async move { Ev::Search(id, soundcloud::search(q).await.map(|tracks| Paged { tracks, more: None })) });
        return;
    }
    let Some(api) = api else { return };
    with_app(st, |a| {
        a.set_searching(true);
        a.set_page(1);
    });
    post(async move { Ev::Search(id, api.search_paged(q).await) });
}

// ---------------------------------------------------------------- lists

fn new_list(st: &Shared, title: String, paged: bool) -> u64 {
    let id = {
        let mut s = st.borrow_mut();
        s.list_id += 1;
        s.list.clear();
        s.list_more = None;
        s.m.list.set_vec(Vec::new());
        s.list_id
    };
    with_app(st, |a| {
        a.set_list_title(title.into());
        a.set_list_loading(true);
        a.set_list_more(paged);
        a.set_page(5);
    });
    id
}

fn open_list(st: &Shared, title: String, load: impl Future<Output = anyhow::Result<Vec<Track>>> + Send + 'static) {
    let id = new_list(st, title, false);
    post(async move { Ev::List(id, load.await) });
}

fn open_paged(st: &Shared, title: String, load: impl Future<Output = anyhow::Result<Paged>> + Send + 'static) {
    let id = new_list(st, title, true);
    post(async move { Ev::ListPaged(id, load.await) });
}

/// A song or a home item by its key in the window ("q:3", "h:12", "l:0"...).
fn track_of(st: &Shared, key: &str) -> Option<Track> {
    let s = st.borrow();
    // The playing song (its cover's right-click menu).
    if key == "now" {
        return s.q.now().cloned();
    }
    let (kind, rest) = key.split_once(':')?;
    let i: usize = rest.parse().ok()?;
    match kind {
        "q" => s.q.songs.get(i).cloned(),
        "h" => s.home.get(i).cloned(),
        "l" => s.list.get(i).cloned(),
        "r" => s.results.get(i).cloned(),
        "s" => s.similar.get(i).cloned(),
        "a" => s.same.get(i).cloned(),
        _ => None,
    }
}

fn play_key(st: &Shared, key: &str) {
    st.borrow_mut().peers.played_here();
    st.borrow_mut().user_pick = true;
    let Some((kind, rest)) = key.split_once(':') else { return };
    let Ok(i) = rest.parse::<usize>() else { return };
    match kind {
        "q" => play_at(st, i),
        "l" => {
            let songs: Vec<Track> = st.borrow().list.clone();
            let at = songs.iter().take(i).filter(|t| t.link.is_none()).count();
            let songs: Vec<Track> = songs.into_iter().filter(|t| t.link.is_none()).collect();
            play_list(st, songs, at);
        }
        // A search result, a similar song or one of the artist's: it plays, then songs like it.
        // An album, playlist or mix: its songs play.
        _ => {
            if let Some(t) = track_of(st, key) {
                if t.link.is_some() {
                    link_songs(st, &t, 0);
                } else {
                    play_with_radio(st, t);
                }
            }
        }
    }
}

fn open_card(st: &Shared, key: &str) {
    st.borrow_mut().peers.played_here();
    if key == "p:local" {
        open_local(st);
        return;
    }
    if let Some(rest) = key.strip_prefix("p:") {
        let Some(api) = st.borrow().api.clone() else { return };
        match rest {
            "liked" => {
                let liked = st.borrow().liked_tracks.clone();
                open_list(st, tr("Liked songs").into(), async move { if liked.is_empty() { api.playlist_tracks(LIKED_ID.into()).await } else { Ok(liked) } });
            }
            "history" => open_list(st, tr("History").into(), api.history()),
            _ => {
                let p = rest.parse::<usize>().ok().and_then(|i| st.borrow().playlists.get(i).cloned());
                if let Some(p) = p {
                    open_paged(st, p.title.clone(), api.playlist_paged(p.id));
                }
            }
        }
        return;
    }
    let Some(t) = track_of(st, key) else { return };
    if t.link.is_some() {
        open_link(st, &t);
    } else {
        st.borrow_mut().user_pick = true;
        feed::opened(&t);
        play_with_radio(st, t);
    }
}

/// The songs on this computer, read off the UI thread.
fn open_local(st: &Shared) {
    let folders = rymplayer::local::folders(&st.borrow().config.local_folders);
    open_list(st, tr("On this device").into(), async move {
        let songs = tokio::task::spawn_blocking(move || rymplayer::local::scan(&folders)).await?;
        if songs.is_empty() {
            anyhow::bail!("{}", tr("No songs found in your Music folder"));
        }
        Ok(songs)
    });
}

/// A home playlist, album or mix opens as a list.
fn open_link(st: &Shared, t: &Track) {
    let Some(api) = st.borrow().api.clone() else { return };
    feed::opened(t);
    match t.link.clone() {
        Some(Link::Playlist(id)) => open_paged(st, t.title.clone(), api.playlist_paged(id)),
        Some(Link::Album(id)) => open_list(st, t.title.clone(), api.album_tracks(id)),
        Some(Link::Mix(id)) => open_list(st, t.title.clone(), api.mix_tracks(id)),
        None => {}
    }
}

/// "Play in RyM": a link from outside (the command line, a `rym://` link, another copy of RyM).
/// A song plays now, as one picked by hand (RaM points: a song below zero comes back to zero),
/// with its radio after it; a playlist, an album, a mix or an artist opens.
fn open_incoming(st: &Shared, link: String) {
    use rymplayer::links::Target;
    use rymplayer::plugins;
    let Some(target) = rymplayer::links::parse(&link) else {
        toast(st, tr("Not a YouTube Music link"));
        return;
    };
    let Some(api) = st.borrow().api.clone() else {
        st.borrow_mut().pending_link = Some(link);
        return;
    };
    let has = |p: &str| st.borrow().config.plugins.contains_key(p);
    match target {
        Target::Song(id) => {
            toast(st, &format!("{}…", tr("Loading")));
            post(async move { Ev::Handed(Ok(core::song_with_radio(api, id).await)) });
        }
        Target::Playlist(id) => open_paged(st, tr("Playlist").into(), api.playlist_paged(id)),
        Target::Album(id) => open_list(st, tr("Album").into(), api.album_tracks(id)),
        Target::Mix(id) => open_list(st, tr("Mix").into(), api.mix_tracks(id)),
        Target::Artist(id) => open_list(st, tr("Artist").into(), api.artist_page(id)),
        t @ Target::Spotify { one_song: true, .. } if has(plugins::SPOTIFY) => {
            toast(st, tr("Finding the Spotify songs on YouTube Music..."));
            post(async move { Ev::Handed(core::found_song(api, t).await) });
        }
        t @ Target::SoundCloud { one_song: true, .. } if has(plugins::SOUNDCLOUD) => post(async move { Ev::Handed(core::found_song(api, t).await) }),
        // A Spotify or SoundCloud list (or its plugin isn't there: the search says so).
        Target::Spotify { link, .. } | Target::SoundCloud { link, .. } => search(st, link),
    }
}

fn go_artist(st: &Shared, t: &Track) {
    let Some(api) = st.borrow().api.clone() else { return };
    let name = t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string();
    match t.artist_id.clone() {
        Some(id) => open_list(st, name, api.artist_page(id)),
        None => post(async move {
            let r = api.artist_id_for(name.clone()).await;
            Ev::ArtistFound(name, r)
        }),
    }
}

fn song_menu(st: &Shared, action: &str, key: &str) {
    if action == "similar-all" {
        let songs = st.borrow().similar.clone();
        open_list(st, tr("Similar songs").into(), async move { Ok(songs) });
        return;
    }
    if action == "save-queue" {
        ask_text(st, "save-queue", tr("Save Up next as a playlist"), tr("The playlist's name"));
        return;
    }
    if action == "download-all" {
        let songs: Vec<Track> = st.borrow().list.iter().filter(|t| t.link.is_none()).cloned().collect();
        if songs.is_empty() {
            return;
        }
        toast(st, &format!("{} {} {}…", tr("Downloading"), songs.len(), tr("songs")));
        post(async move {
            let done = stream::download_many(songs, |b| send(Ev::Batch(b))).await;
            Ev::Batch(done)
        });
        return;
    }
    let Some(t) = track_of(st, key) else { return };
    if let Some(name) = action.strip_prefix("kind:")
        && let Some(kind) = rymplayer::ram_points::Kind::from_name(name)
    {
        rymplayer::ram_points::correct(&t, kind);
        let artist = t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string();
        toast(st, &format!("{artist}: {}", tr_any(name)));
        return;
    }
    match action {
        "radio" => play_with_radio(st, t),
        "album" => {
            let api = st.borrow().api.clone();
            match (api, t.album_id.clone()) {
                (Some(api), Some(id)) => open_list(st, t.album.clone(), api.album_tracks(id)),
                _ => toast(st, tr("No album for this song")),
            }
        }
        "video" => {
            toast(st, tr("Opening the video..."));
            post(async move {
                let links = rymplayer::video::links(&t.video_id).await;
                Ev::VideoLinks(t, links)
            });
        }
        "video-dl" => {
            let url = rymplayer::video::watch_url(&t.video_id);
            toast(st, &format!("{} {}", tr("Downloading"), t.title));
            post(async move { Ev::Downloaded(rymplayer::video::download(&url, true).await) });
        }
        a if a.starts_with("save:") => {
            let (api, list) = {
                let s = st.borrow();
                (s.api.clone(), a[5..].parse::<usize>().ok().and_then(|i| s.playlists.get(i).cloned()))
            };
            if let (Some(api), Some(p)) = (api, list) {
                let (vid, title) = (t.video_id.clone(), p.title.clone());
                rymplayer::ram_points::loved(t.clone(), rymplayer::ram_points::Love::AddToPlaylist);
                post(async move { Ev::AddedTo(title, api.add_to_playlist(p.id, vid).await) });
            }
        }
        "more" => {
            rymplayer::ram_points::recommend_more(&t);
            toast(st, &format!("{}: {}", tr("More like this"), t.artist));
        }
        "less" => {
            rymplayer::ram_points::dont_recommend(&t);
            // Out of Up next too, if it's waiting there.
            if let Some(i) = key.strip_prefix("q:").and_then(|i| i.parse::<usize>().ok()) {
                st.borrow_mut().q.remove(i);
                render_queue(st);
            }
            toast(st, &format!("{}: {}", tr("Won't be suggested"), t.title));
        }
        "next" | "add" if t.link.is_some() => link_songs(st, &t, if action == "next" { 1 } else { 2 }),
        "next" => {
            st.borrow_mut().q.play_next(t.clone());
            render_queue(st);
            toast(st, &format!("{}: {}", tr("Plays next"), t.title));
        }
        "add" => {
            st.borrow_mut().q.add(t.clone());
            render_queue(st);
            toast(st, &format!("{}: {}", tr("Added to queue"), t.title));
        }
        "like" => toggle_like(st, &t),
        "artist" => go_artist(st, &t),
        "download" => {
            rymplayer::ram_points::loved(t.clone(), rymplayer::ram_points::Love::Download);
            toast(st, &format!("{} \"{}\"…", tr("Downloading"), t.title));
            post(async move { Ev::Downloaded(stream::save_to_music(&t.video_id, &t.artist, &t.title).await) });
        }
        "share" => {
            let link = rymplayer::queue::share_link(&t);
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(link);
            }
            toast(st, tr("Link copied"));
        }
        "remove" => {
            if let Some(i) = key.strip_prefix("q:").and_then(|i| i.parse::<usize>().ok()) {
                st.borrow_mut().q.remove(i);
                render_queue(st);
            }
        }
        _ => {}
    }
}

/// A question with a text box; the answer goes to `prompt_done`.
fn ask_text(st: &Shared, what: &str, title: &str, hint: &str) {
    st.borrow_mut().prompt_for = what.to_string();
    with_app(st, |a| {
        a.set_prompt_title(title.into());
        a.set_prompt_hint(hint.into());
        a.set_prompt_text(SharedString::new());
        a.set_prompt_open(true);
    });
}

fn prompt_done(st: &Shared, ok: bool) {
    let text = st.borrow().ui.upgrade().map(|ui| ui.global::<App>().get_prompt_text().trim().to_string()).unwrap_or_default();
    with_app(st, |a| a.set_prompt_open(false));
    // The crash report: OK opens the filled-in issue; either way it isn't offered again.
    if st.borrow().prompt_for == "crash" {
        st.borrow_mut().prompt_for.clear();
        rymplayer::crash::dismiss();
        if ok && !text.is_empty() {
            let _ = rymplayer::updates::open_link(&rymplayer::crash::issue_url(&text));
        }
        return;
    }
    if !ok || text.is_empty() {
        return;
    }
    let what = std::mem::take(&mut st.borrow_mut().prompt_for);
    match what.as_str() {
        // Up next as a new playlist: the playing song and every one after it.
        "save-queue" => {
            let (api, songs) = {
                let s = st.borrow();
                let from = s.q.current.unwrap_or(0);
                (s.api.clone().filter(Api::logged_in), s.q.songs.iter().skip(from).filter(|t| rymplayer::is_youtube(&t.video_id)).map(|t| t.video_id.clone()).collect::<Vec<_>>())
            };
            let Some(api) = api else { return };
            toast(st, tr("Saving…"));
            post(async move {
                let r = async {
                    let p = api.clone().create_playlist(text.clone()).await?;
                    let n = songs.len();
                    for vid in songs {
                        let _ = api.clone().add_to_playlist(p.id.clone(), vid).await;
                    }
                    Ok::<_, anyhow::Error>(format!("{text}: {n} {}", tr("songs")))
                }
                .await;
                Ev::SavedQueue(r)
            });
        }
        "theme" => import_colors(st, &text),
        // Video plugin: download from a link (YouTube or any site yt-dlp knows).
        "link" => {
            toast(st, &format!("{} {text}", tr("Downloading")));
            post(async move { Ev::Downloaded(rymplayer::video::download(&text, true).await) });
        }
        _ => {}
    }
}

/// Likes, and Recently played from the account's history (the phone's plays come in there).
fn sync_with_account(st: &Shared) {
    let api = st.borrow().api.clone().filter(Api::logged_in);
    if let Some(api) = api {
        let a2 = api.clone();
        post(async move { Ev::Liked(a2.playlist_tracks(LIKED_ID.into()).await) });
        post(async move { Ev::History(api.history().await) });
    }
}

/// Windows: the title bar dark or light as Windows is set (apps' mode), on every RyM window.
fn title_bar(st: &Shared) {
    #[cfg(windows)]
    {
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn RegGetValueW(key: isize, sub: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut u8, len: *mut u32) -> i32;
        }
        #[link(name = "dwmapi")]
        unsafe extern "system" {
            fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const i32, size: u32) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn EnumWindows(each: unsafe extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
            fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        }
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (sub, name) = (wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), wide("AppsUseLightTheme"));
        let mut value: u32 = 1;
        let mut len: u32 = 4;
        // HKEY_CURRENT_USER, REG_DWORD only.
        // SAFETY: the buffers outlive the call and `len` is their size.
        let found = unsafe { RegGetValueW(0x8000_0001u32 as i32 as isize, sub.as_ptr(), name.as_ptr(), 0x10, std::ptr::null_mut(), &mut value as *mut u32 as *mut u8, &mut len) } == 0;
        let dark = found && value == 0;
        if st.borrow().title_dark == Some(dark) {
            return;
        }
        st.borrow_mut().title_dark = Some(dark);
        unsafe extern "system" fn each(hwnd: isize, dark: isize) -> i32 {
            let mut pid = 0u32;
            // SAFETY: plain Win32 calls on a handle Windows just gave.
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == std::process::id() {
                    let on: i32 = dark as i32;
                    // DWMWA_USE_IMMERSIVE_DARK_MODE
                    DwmSetWindowAttribute(hwnd, 20, &on, 4);
                }
            }
            1
        }
        // SAFETY: the callback only touches the handles it's given.
        unsafe { EnumWindows(each, dark as isize) };
    }
    #[cfg(not(windows))]
    let _ = st;
}

fn toggle_like(st: &Shared, t: &Track) {
    if !rymplayer::is_youtube(&t.video_id) {
        toast(st, tr("Likes are for YouTube Music songs"));
        return;
    }
    let Some(api) = st.borrow().api.clone().filter(Api::logged_in) else {
        toast(st, tr("Sign in to like songs"));
        return;
    };
    let vid = t.video_id.clone();
    let like = {
        let mut s = st.borrow_mut();
        let like = !s.liked.contains(&vid);
        s.liked.set(&vid, like);
        if like {
            if !s.liked_tracks.iter().any(|x| x.video_id == vid) {
                s.liked_tracks.insert(0, t.clone());
            }
        } else {
            s.liked_tracks.retain(|x| x.video_id != vid);
        }
        like
    };
    refresh_hearts(st);
    post(async move { Ev::Rated(vid.clone(), like, api.rate(vid, like).await) });
}

// ---------------------------------------------------------------- playback

fn play_list(st: &Shared, songs: Vec<Track>, i: usize) {
    if songs.is_empty() {
        return;
    }
    let at = st.borrow_mut().q.start(songs, i);
    play_at(st, at);
}

/// An album, playlist or mix's songs, fetched, then played (0), put next (1) or added (2).
fn link_songs(st: &Shared, t: &Track, what: u8) {
    let Some(api) = st.borrow().api.clone() else { return };
    feed::opened(t);
    let title = t.title.clone();
    let load: std::pin::Pin<Box<dyn Future<Output = anyhow::Result<Vec<Track>>> + Send>> = match t.link.clone() {
        Some(Link::Album(id)) => Box::pin(api.album_tracks(id)),
        Some(Link::Playlist(id)) => Box::pin(api.playlist_all(id, 200)),
        Some(Link::Mix(id)) => Box::pin(api.mix_tracks(id)),
        None => return,
    };
    toast(st, &format!("{} \"{}\"…", tr("Loading"), title));
    post(async move { Ev::LinkSongs(what, title, load.await) });
}

fn play_with_radio(st: &Shared, t: Track) {
    let at = st.borrow_mut().q.start(vec![t], 0);
    play_at(st, at);
    continue_with_radio(st);
}

fn play_at(st: &Shared, i: usize) {
    let unplayable = st.borrow().q.songs.get(i).is_some_and(|t| t.video_id.is_empty());
    if unplayable {
        let t = {
            let mut s = st.borrow_mut();
            let t = s.q.songs.remove(i);
            // The queue's places after it move up by one.
            match s.q.current {
                Some(c) if c == i => s.q.current = i.checked_sub(1),
                Some(c) if c > i => s.q.current = Some(c - 1),
                _ => {}
            }
            t
        };
        render_queue(st);
        if t.link.is_some() {
            link_songs(st, &t, 1);
        } else if i < st.borrow().q.songs.len() {
            play_at(st, i);
        }
        return;
    }
    let (track, id, api) = {
        let mut s = st.borrow_mut();
        let Some(track) = s.q.songs.get(i).cloned() else { return };
        // RaM points: the song that was playing gets its listening time, then this one its play.
        let left = s.q.now().cloned().map(|p| {
            let a = s.audio.as_ref().filter(|a| a.is_loaded());
            (p, a.map_or(0.0, Audio::position), a.and_then(Audio::duration).unwrap_or(0.0))
        });
        let manual = std::mem::take(&mut s.user_pick);
        // Three quick skips in a row: Up next turns to another artist.
        let liked = s.liked_tracks.clone();
        if let (Some(seed), Some(api)) = (s.streak.left(left.as_ref(), &liked), s.api.clone()) {
            post(async move { Ev::NewDirection(core::new_direction(api, seed).await) });
        }
        rymplayer::ram_points::moved_on(left, track.clone(), manual);
        s.request_id += 1;
        s.q.went_to(i);
        s.played.insert(track.video_id.clone());
        rymplayer::recent::played(&track);
        if s.resume_at.as_ref().is_some_and(|(id, _)| *id != track.video_id) {
            s.resume_at = None;
        }
        s.loading = true;
        s.lyrics = None;
        s.shown = (false, u32::MAX, u32::MAX, -2);
        if !s.crossfade_next
            && let Some(a) = &mut s.audio
        {
            a.stop();
        }
        s.m.lyrics.set_vec(Vec::new());
        s.alt_lyrics = None;
        s.sources_for.clear();
        (track, s.request_id, s.api.clone())
    };
    with_app(st, |a| {
        a.set_loading(true);
        a.set_position(0.0);
        a.set_duration(parse_duration(&track.duration).unwrap_or(0.0));
        a.set_pos_text("0:00".into());
        a.set_lyrics_note(tr("Looking for lyrics…").into());
        a.set_lyric_line(-1);
        a.set_sources(ModelRc::default());
        a.set_source_at(-1);
    });
    show_now(st);
    render_queue(st);
    mark_playing(st);
    save_session(st);
    log(&format!("play {} ({})", track.title, track.video_id));
    let vid = track.video_id.clone();
    let in_vlc = st.borrow().config.play_in_vlc && rymplayer::vlc::find().is_some();
    if in_vlc {
        post(async move { Ev::InVlc(id, stream::link(&vid).await) });
    } else {
        rymplayer::vlc::stop(&mut st.borrow_mut().vlc);
        // FLAC first when "Prefer lossless" is on and the Internet Archive has the song.
        let song = track.clone();
        post(async move { Ev::Fetched(id, stream::open_track(song).await) });
    }
    if let Some(api) = api.clone() {
        let vid = track.video_id.clone();
        post(async move {
            let _ = api.report_play(vid).await;
            Ev::Done
        });
    }
    {
        let (api, t) = (api.clone(), track.clone());
        post(async move { Ev::Lyrics(t.video_id.clone(), rymplayer::lyrics::for_track(api, t).await) });
    }
    similar_and_artist(st, &track);
}

/// Similar songs (the song's radio) and the artist's songs, once per song / artist.
fn similar_and_artist(st: &Shared, t: &Track) {
    let Some(api) = st.borrow().api.clone() else { return };
    if rymplayer::is_youtube(&t.video_id) && st.borrow().similar_for != t.video_id {
        st.borrow_mut().similar_for = t.video_id.clone();
        let (api, vid) = (api.clone(), t.video_id.clone());
        post(async move {
            let r = api.radio(vid.clone()).await;
            Ev::Similar(vid, r)
        });
    }
    let artist = t.artist.split([',', '&']).next().unwrap_or_default().trim().to_string();
    if artist.is_empty() || st.borrow().same_for == artist {
        return;
    }
    st.borrow_mut().same_for = artist.clone();
    let id = t.artist_id.clone();
    post(async move {
        let r = async {
            let id = match id {
                Some(id) => Some(id),
                None => api.clone().artist_id_for(artist.clone()).await?,
            };
            match id {
                Some(id) => api.artist_songs(id).await,
                None => Ok(Vec::new()),
            }
        }
        .await;
        Ev::SameArtist(artist, r)
    });
}

fn prefetch_next(st: &Shared) {
    let (next, api) = {
        let s = st.borrow();
        (s.q.current.and_then(|i| s.q.songs.get(i + 1)).cloned(), s.api.clone())
    };
    if let Some(t) = next.clone() {
        post(async move {
            rymplayer::lyrics::prefetch(api, t).await;
            Ev::Done
        });
    }
    if let Some(id) = next.map(|t| t.video_id).filter(|id| stream::cached(id).is_none()) {
        post(async move {
            let _ = stream::fetch(id).await;
            Ev::Done
        });
    }
}

fn toggle_playback(st: &Shared) {
    let (loaded, current) = {
        let s = st.borrow();
        (s.audio.as_ref().is_some_and(Audio::is_loaded), s.q.current)
    };
    if loaded {
        if let Some(a) = &st.borrow().audio {
            a.toggle();
        }
    } else if let Some(i) = current {
        play_at(st, i);
    }
}

fn skip(st: &Shared) {
    let (current, len, all) = {
        let s = st.borrow();
        (s.q.current, s.q.songs.len(), s.repeat_all)
    };
    match current {
        Some(i) if i + 1 < len => play_at(st, i + 1),
        // Repeat all: from the top again.
        Some(_) if all && len > 0 => play_at(st, 0),
        Some(_) => continue_with_radio(st),
        None => {}
    }
}

/// Previous: back to the start after the first 3 seconds; before that (or twice in a row), the
/// song before.
fn previous(st: &Shared) {
    let (current, pos, again) = {
        let mut s = st.borrow_mut();
        let again = s.last_prev.elapsed() < Duration::from_millis(1500);
        s.last_prev = Instant::now();
        (s.q.current, s.audio.as_ref().filter(|a| a.is_loaded()).map_or(0.0, Audio::position), again)
    };
    match current {
        Some(i) if (pos < 3.0 || again) && i > 0 => play_at(st, i - 1),
        Some(_) => {
            if let Some(a) = &st.borrow().audio {
                a.seek(0.0);
            }
            // Back to its start by hand: a sign it's loved (RaM points).
            let now = st.borrow().q.now().cloned();
            if let Some(t) = now.filter(|_| pos >= 3.0) {
                rymplayer::ram_points::loved(t, rymplayer::ram_points::Love::BackToStart);
            }
        }
        None => {}
    }
}

fn continue_with_radio(st: &Shared) {
    let (api, t) = {
        let s = st.borrow();
        (s.api.clone(), s.q.now().cloned())
    };
    let (Some(api), Some(t)) = (api, t) else { return };
    if rymplayer::local::is_local(&t.video_id) {
        return;
    }
    {
        let mut s = st.borrow_mut();
        if s.radio_pending {
            return;
        }
        s.radio_pending = true;
    }
    post(async move { Ev::Radio(rymplayer::radio::with_artist(api, t).await) });
}

fn save_session(st: &Shared) {
    let Ok(mut s) = st.try_borrow_mut() else { return };
    let pos = match s.audio.as_ref().filter(|a| a.is_loaded()) {
        Some(a) => a.position(),
        None => s.resume_at.as_ref().map_or(0.0, |r| r.1),
    };
    let volume = s.ui.upgrade().map_or(0.8, |ui| ui.global::<App>().get_volume());
    let key = (s.q.now().map(|t| t.video_id.clone()).unwrap_or_default(), pos as u32, (volume * 100.0) as u32, s.q.songs.len());
    if s.session_key.as_ref() == Some(&key) {
        return;
    }
    s.session_key = Some(key);
    rymplayer::session::save(&rymplayer::session::Session::new(&s.q, pos, volume));
}

fn tick(st: &Shared) {
    media_sync(st);
    mini_sync(st);
    extras(st);
    scheduled_reset(st);
    animations(st);
    let mut s = st.borrow_mut();
    let Some((pos, dur, loaded, playing, finished)) = s.audio.as_ref().map(|a| (a.position(), a.duration().unwrap_or(0.0), a.is_loaded(), a.is_playing(), a.finished())) else { return };
    if let Some(a) = &mut s.audio {
        a.tick();
    }
    let since = s.listened_at.elapsed();
    s.listened_at = Instant::now();
    if playing && let Some(t) = s.q.now() {
        rymplayer::stats::listened(t, since.as_millis().min(1000) as u64);
    }
    if s.stats_written.elapsed() > Duration::from_secs(60) {
        s.stats_written = Instant::now();
        rymplayer::stats::flush();
        drop(s);
        save_session(st);
        s = st.borrow_mut();
    }
    if s.loading {
        return;
    }
    // Discord: told when the song, play/pause or the position (a seek) changes.
    if s.config.discord && !s.config.discord_app_id.is_empty() && let Some(t) = s.q.now().cloned() {
        let expected = s.discord_sent.as_ref().map(|(_, was, at, when)| at + if *was { when.elapsed().as_secs_f32() } else { 0.0 });
        let changed = s.discord_sent.as_ref().is_none_or(|(id, was, ..)| *id != t.video_id || *was != playing) || expected.is_some_and(|e| (e - pos).abs() > 3.0);
        if changed && loaded {
            s.discord_sent = Some((t.video_id.clone(), playing, pos, Instant::now()));
            let cover = t.thumb.as_ref().map(|u| rymplayer::api::thumb_sized(u, 512));
            rymplayer::discord::show(&s.config.discord_app_id, rymplayer::discord::Show::Song { video_id: t.video_id.clone(), title: t.title.clone(), artist: t.artist.clone(), cover, at: pos, length: dur, paused: !playing });
        }
    }
    // Crossfade, or Smart Automix: the next song starts before this one ends.
    let automix = s.config.automix;
    let fade_secs = if automix { rymplayer::automix::mix_secs(s.config.crossfade) } else { s.config.crossfade as f32 };
    let next_index = s.q.current.map(|c| c + 1).filter(|n| *n < s.q.songs.len());
    let lead = if automix { rymplayer::automix::LOAD_LEAD } else { 0.0 };
    let due = pos >= dur - fade_secs - lead || (automix && pos >= dur - rymplayer::automix::WINDOW_SECS && s.audio.as_ref().is_some_and(|a| a.faded_out()));
    if fade_secs > 0.0 && playing && !s.repeat_one && dur > fade_secs + 10.0 && due && s.crossfade_for != s.request_id && let Some(n) = next_index {
        s.crossfade_for = s.request_id;
        s.crossfade_next = true;
        drop(s);
        play_at(st, n);
        return;
    }
    // Gapless (no crossfade): the next song, already downloaded, waits to start on the very frame
    // this one ends.
    if fade_secs == 0.0
        && s.config.gapless
        && playing
        && !s.repeat_one
        && dur > 10.0
        && pos >= dur - 1.5
        && s.crossfade_for != s.request_id
        && let Some(n) = next_index
        && s.q.songs.get(n).is_some_and(|t| stream::cached(&t.video_id).is_some())
    {
        s.crossfade_for = s.request_id;
        s.gapless_next = true;
        drop(s);
        play_at(st, n);
        return;
    }
    // SponsorBlock: a non-music part (an intro, a talk) is jumped over.
    let now_id = s.q.now().map(|t| t.video_id.clone()).unwrap_or_default();
    let sponsor = s.config.sponsorblock;
    if playing && let Some(to) = s.non_music.jump(&now_id, pos, sponsor) {
        if let Some(a) = &s.audio {
            a.seek(to);
        }
        drop(s);
        toast(st, tr("Skipped a part that isn't music (SponsorBlock)"));
        return;
    }
    // Nobody sees the window: only the end of the song is looked at below; what it showed is
    // forgotten, so all of it is told again when it comes back.
    if unseen(s.ui.upgrade().as_ref()) {
        s.shown = (!playing, u32::MAX, u32::MAX, -2);
    } else {
    // The window is told only what changed.
    let sec = pos as u32;
    let line = s.lyrics.as_ref().and_then(|l| rymplayer::lyrics::current_line(&l.synced, pos)).map_or(-1, |l| l as i32);
    let old = s.shown;
    s.shown = (playing, sec, dur as u32, line);
    let synced = s.lyrics.as_ref().is_some_and(|l| !l.synced.is_empty());
    if line != old.3 && synced {
        for i in 0..s.m.lyrics.row_count() {
            if let Some(mut row) = s.m.lyrics.row_data(i) {
                let state = match (i as i32).cmp(&line) {
                    std::cmp::Ordering::Less => 2,
                    std::cmp::Ordering::Equal => 1,
                    std::cmp::Ordering::Greater => 0,
                };
                if row.state != state {
                    row.state = state;
                    s.m.lyrics.set_row_data(i, row);
                }
            }
        }
    }
    // Word by word: the sung line's sung part and what's left (sources with word times).
    if synced && line >= 0 {
        let i = line as usize;
        let split = s.lyrics.as_ref().and_then(|l| {
            let words = l.words.get(i).filter(|w| !w.is_empty())?;
            let end = l.synced.get(i + 1).map_or(words.last().map_or(pos, |w| w.0) + 3.0, |n| n.0);
            Some(rymplayer::lyrics::sung_split(words, end, pos))
        });
        if let Some((sung, left)) = split
            && let Some(mut row) = s.m.lyrics.row_data(i)
            && (row.sung.as_str() != sung || row.left.as_str() != left)
        {
            row.sung = sung.into();
            row.left = left.into();
            s.m.lyrics.set_row_data(i, row);
        }
    }
    if let Some(ui) = s.ui.upgrade() {
        let a = ui.global::<App>();
        a.set_position(pos);
        if old.0 != playing {
            a.set_playing(playing);
        }
        if old.1 != sec {
            a.set_pos_text(fmt_time(pos).into());
        }
        if loaded && dur > 0.0 && old.2 != dur as u32 {
            a.set_duration(dur);
            a.set_len_text(fmt_time(dur).into());
        }
        if line != old.3 {
            a.set_lyric_line(line);
        }
    }
    }
    if loaded && finished && s.q.current.is_some() && s.ended_for != s.request_id {
        s.ended_for = s.request_id;
        let again = s.repeat_one;
        if let Some(a) = &mut s.audio {
            a.stop();
        }
        let current = s.q.current;
        drop(s);
        match (again, current) {
            (true, Some(i)) => play_at(st, i),
            _ => skip(st),
        }
    }
}

/// Each tick: the sleep timer, the alarm, the moving wave, the karaoke, the floating lyrics,
/// Last.fm, and what the phones are told.
fn extras(st: &Shared) {
    let Ok(mut s) = st.try_borrow_mut() else { return };
    let Some((pos, dur, playing, loaded)) = s.audio.as_ref().map(|a| (a.position(), a.duration().unwrap_or(0.0), a.is_playing(), a.is_loaded())) else { return };
    let ui = s.ui.upgrade();
    // Sleep timer: the last 10 seconds fade out, then the music stops.
    if let Some(sleep) = s.sleep {
        let left = match sleep {
            Sleep::At(when) => when.saturating_duration_since(Instant::now()).as_secs_f32(),
            Sleep::EndOfSong if dur > 0.0 => dur - pos,
            Sleep::EndOfSong => f32::MAX,
        };
        if playing && left < 10.0
            && let Some(a) = &s.audio
        {
            a.set_fade((left / 10.0).clamp(0.0, 1.0));
            if left <= 0.25 {
                a.pause();
                a.set_fade(1.0);
                s.sleep = None;
            }
        }
        let text = match s.sleep {
            None => String::new(),
            Some(Sleep::EndOfSong) => tr("Stops at the end of this song").into(),
            Some(Sleep::At(_)) => format!("{} {}", tr("Stops in"), fmt_time(left)),
        };
        if let Some(ui) = &ui {
            ui.global::<App>().set_sleep_text(text.into());
        }
    } else if let Some(ui) = &ui
        && !ui.global::<App>().get_sleep_text().is_empty()
    {
        ui.global::<App>().set_sleep_text(SharedString::new());
    }
    // The alarm: once a day at its time.
    if core::alarm_rings(&mut s.config) {
        drop(s);
        ring_alarm(st);
        return;
    }
    // The moving wave: the last second's loudness, one bar per 20 ms.
    let id = s.q.now().map(|t| t.video_id.clone()).unwrap_or_default();
    let away = unseen(ui.as_ref());
    if away {
        // Nothing of the window moves.
    } else if s.config.visualizer && playing {
        let loud = s.audio.as_ref().map(|a| a.loudness(1.02)).unwrap_or_default();
        if loud.len() >= 102 {
            let bars: Vec<f32> = loud.chunks(2).map(|c| (c[0] + c.get(1).copied().unwrap_or(c[0])) / 2.0).collect();
            let top = bars.iter().copied().fold(0.0f32, f32::max);
            s.wave_peak = (s.wave_peak * 0.97).max(top).max(0.02);
            let peak = s.wave_peak;
            let wave: Vec<f32> = bars.iter().take(51).map(|b| (b / peak).sqrt().clamp(0.08, 1.0)).collect();
            if let Some(ui) = &ui {
                // The same bars changed in place (a new list each tick made them all again).
                let model = ui.global::<App>().get_wave();
                if s.wave_live && model.row_count() == wave.len() {
                    for (i, h) in wave.into_iter().enumerate() {
                        model.set_row_data(i, h);
                    }
                } else {
                    ui.global::<App>().set_wave(ModelRc::from(Rc::new(VecModel::from(wave))));
                }
            }
            s.wave_live = true;
        }
    } else if s.wave_live {
        s.wave_live = false;
        if let Some(ui) = &ui {
            ui.global::<App>().set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for(&id)))));
        }
    }
    // Liquid Aurora pulses with the bass; the needle meters follow left and right.
    if let Some(ui) = ui.as_ref().filter(|_| !away) {
        let theme = ui.global::<Theme>();
        let clock = s.clock_every.is_some();
        // The heartbeat (Remorse) beats with the bass like Liquid Aurora.
        let (aurora, vu) = (theme.get_aurora() || theme.get_heartbeat() || theme.get_falling(), theme.get_vu());
        if aurora || vu {
            let (bass, left, right) = if playing { s.audio.as_ref().map(Audio::bands).unwrap_or_default() } else { (0.0, 0.0, 0.0) };
            if aurora {
                s.bass_peak = (s.bass_peak * 0.995).max(bass).max(0.01);
                s.energy_to = (bass / s.bass_peak).powf(1.5).clamp(0.0, 1.0);
            }
            s.bass_now = (bass / s.bass_peak.max(0.01)).clamp(0.0, 1.0);
            if vu {
                let level = |x: f32| ((20.0 * x.max(1e-5).log10() + 30.0) / 30.0).clamp(0.0, 1.0);
                s.vu_to = (level(left), level(right));
            }
            // No clock (paused, battery saver): straight there, once.
            if !clock {
                if aurora && s.energy != s.energy_to {
                    s.energy = s.energy_to;
                    theme.set_energy(s.energy);
                }
                if vu && s.vu != s.vu_to {
                    s.vu = s.vu_to;
                    theme.set_vu_l(s.vu.0);
                    theme.set_vu_r(s.vu.1);
                }
            }
        }
    }
    // The karaoke and the floating lyrics: the sung line, how far into it, and the next one.
    if let Some(l) = s.lyrics.as_ref().filter(|l| !l.synced.is_empty()) {
        let line = rymplayer::lyrics::current_line(&l.synced, pos);
        if let (Some(i), Some(ui), false) = (line, &ui, away) {
            let start = l.synced[i].0;
            let end = l.synced.get(i + 1).map_or(start + 5.0, |n| n.0);
            ui.global::<App>().set_line_progress(((pos - start) / (end - start).max(0.3)).clamp(0.0, 1.0));
        }
        let now_line = line.and_then(|i| l.synced.get(i)).map_or("♪".to_string(), |x| if x.1.is_empty() { "♪".into() } else { x.1.clone() });
        let next_line = l.synced.get(line.map_or(0, |i| i + 1)).map(|x| x.1.clone()).unwrap_or_default();
        let shown = (now_line, next_line);
        if s.float_shown != shown {
            if let Some(f) = &s.float {
                f.set_now(shown.0.clone().into());
                f.set_next(shown.1.clone().into());
            }
            s.float_shown = shown;
        }
    }
    // Last.fm: once half the song (or 4 minutes) played.
    let now = s.q.now().cloned();
    if playing && let Some(send) = { let cfg = s.config.clone(); s.scrobbler.heard(now.as_ref(), 0.1, dur, &cfg) } {
        post(async move {
            send.await;
            Ev::Done
        });
    }
    // What the phones are told.
    let volume = s.audio.as_ref().map_or(0.8, Audio::volume);
    if let Ok(mut p) = s.link.lock() {
        p.playing = playing && loaded;
        p.position = pos;
        p.since = Some(Instant::now());
        p.duration = dur;
        p.volume = volume;
        p.remote = s.config.remote_control;
    }
}

/// When the app opens: the player script and its solver made ready, yt-dlp's versions looked at
/// (and yt-dlp updated if a week went by), and the downloaded songs kept under their limit.
fn background_upkeep(st: &Shared) {
    post(async {
        rymplayer::innertube::warm_up().await;
        Ev::Done
    });
    let (auto, last, nightly, limit) = {
        let s = st.borrow();
        (s.config.ytdlp_auto_update, s.config.ytdlp_updated_at, s.config.ytdlp_nightly, s.config.cache_limit_gb)
    };
    let now = rymplayer::updates::now();
    if auto && rymplayer::ytdlp::update_due(last, now) {
        {
            let mut s = st.borrow_mut();
            s.config.ytdlp_updated_at = now;
            s.config.save();
        }
        post(async move {
            let r = rymplayer::ytdlp::update(nightly).await;
            log(&format!("yt-dlp weekly update: {}", r.as_ref().map_or_else(|e| e.to_string(), |t| t.clone())));
            Ev::YtDlpStatus(rymplayer::ytdlp::status().await)
        });
    } else {
        post(async { Ev::YtDlpStatus(rymplayer::ytdlp::status().await) });
    }
    // ytmusicapi: started (and installed with pip the first time) now, so the first search is quick.
    if rymplayer::ytmusicapi::engine() != rymplayer::ytmusicapi::Engine::Builtin {
        post(async { Ev::ParsersStatus(rymplayer::ytmusicapi::status().await) });
    }
    trim_cache(limit, String::new());
    // The last run crashed: the report is offered (the box shows what would be sent).
    if let Some(report) = rymplayer::crash::pending() {
        ask_text(st, "crash", tr("RyM closed unexpectedly last time"), tr("OK opens a GitHub issue with this report filled in, and you send it yourself. Nothing is sent otherwise."));
        with_app(st, |a| a.set_prompt_text(report.into()));
    }
}

/// Downloaded songs over the limit go (off the window's thread).
fn trim_cache(limit_gb: u32, keep: String) {
    if limit_gb == 0 {
        return;
    }
    post(async move {
        let freed = tokio::task::spawn_blocking(move || stream::trim_cache(limit_gb, &keep)).await.unwrap_or(0);
        if freed > 0 {
            log(&format!("cache: {} MB freed", freed / (1024 * 1024)));
        }
        Ev::Done
    });
}

/// The phone's remote.
fn remote(st: &Shared, what: &str, value: f32) {
    if !st.borrow().config.remote_control {
        return;
    }
    let playing = st.borrow().audio.as_ref().is_some_and(Audio::is_playing);
    match what {
        "toggle" => toggle_playback(st),
        "play" if !playing => toggle_playback(st),
        "pause" if playing => toggle_playback(st),
        "next" => skip(st),
        "prev" => previous(st),
        "seek" => {
            if let Some(a) = &st.borrow().audio {
                a.seek(value.max(0.0));
            }
        }
        "volume" => {
            let v = value.clamp(0.0, 1.0);
            if let Some(a) = &st.borrow().audio {
                a.set_volume(v);
            }
            with_app(st, |a| a.set_volume(v));
        }
        _ => {}
    }
}

/// The alarm rings: what was playing goes on, else the liked songs (shuffled), else home's songs.
fn ring_alarm(st: &Shared) {
    toast(st, tr("Alarm"));
    let (loaded, playing, current) = {
        let s = st.borrow();
        (s.audio.as_ref().is_some_and(Audio::is_loaded), s.audio.as_ref().is_some_and(Audio::is_playing), s.q.current)
    };
    if playing {
        return;
    }
    if loaded || current.is_some() {
        toggle_playback(st);
        return;
    }
    let mut songs: Vec<Track> = st.borrow().liked_tracks.clone();
    if songs.is_empty() {
        songs = st.borrow().home.iter().filter(|t| t.link.is_none()).cloned().collect();
    }
    play_list(st, core::shuffled(songs), 0);
}

/// The floating lyrics window, on or off.
fn show_float(st: &Shared, on: bool) {
    if !on {
        let old = st.borrow_mut().float.take();
        if let Some(w) = old {
            let _ = w.hide();
        }
        return;
    }
    if st.borrow().float.is_some() {
        return;
    }
    let Ok(w) = FloatWindow::new() else { return };
    {
        let st = st.clone();
        w.on_hide_float(move || {
            let st = st.clone();
            slint::Timer::single_shot(Duration::ZERO, move || set_toggle(&st, "float", false));
        });
    }
    {
        let weak = w.as_weak();
        w.on_drag(move |dx, dy| {
            if let Some(w) = weak.upgrade() {
                let p = w.window().position();
                let scale = w.window().scale_factor();
                w.window().set_position(slint::PhysicalPosition::new(p.x + (dx * scale) as i32, p.y + (dy * scale) as i32));
            }
        });
    }
    let accent = st.borrow().ui.upgrade().map(|ui| ui.global::<Theme>().get_accent());
    if let Some(c) = accent {
        w.set_accent(c);
    }
    w.set_now("♪".into());
    let _ = w.show();
    let mut s = st.borrow_mut();
    s.float_shown = (String::new(), String::new());
    s.float = Some(w);
}

/// The song as a picture to save or copy.
/// The playing song's link (YouTube Music, or SoundCloud) on the clipboard.
fn copy_link(st: &Shared) {
    let Some(t) = st.borrow().q.now().cloned() else { return };
    if rymplayer::local::is_local(&t.video_id) {
        toast(st, tr("This song is already on your device"));
        return;
    }
    let link = rymplayer::queue::share_link(&t);
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(link)) {
        Ok(()) => toast(st, tr("Link copied")),
        Err(e) => toast(st, &e.to_string()),
    }
}

fn open_share(st: &Shared) {
    let Some(ui) = st.borrow().ui.upgrade() else { return };
    let app = ui.global::<App>();
    if !app.get_has_song() {
        return;
    }
    let Ok(w) = ShareWindow::new() else { return };
    w.global::<App>().on_tr(|s| tr_any(&s).into());
    w.set_cover(app.get_cover());
    w.set_backdrop(app.get_backdrop());
    w.set_song(app.get_title());
    w.set_artist(app.get_artist());
    w.set_accent(ui.global::<Theme>().get_accent());
    let line = {
        let s = st.borrow();
        let pos = s.audio.as_ref().map_or(0.0, |a| a.position());
        s.lyrics.as_ref().and_then(|l| {
            let at = rymplayer::lyrics::current_line(&l.synced, pos)?;
            l.synced.get(at).map(|x| x.1.clone())
        })
    };
    w.set_line(line.unwrap_or_default().into());
    let title = app.get_title().to_string();
    for copy in [false, true] {
        let weak = w.as_weak();
        let st2 = st.clone();
        let title = title.clone();
        let take = move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_capturing(true);
            let weak = w.as_weak();
            let st3 = st2.clone();
            let title = title.clone();
            slint::Timer::single_shot(Duration::from_millis(150), move || {
                let Some(w) = weak.upgrade() else { return };
                let shot = w.window().take_snapshot();
                w.set_capturing(false);
                match shot {
                    Ok(px) => {
                        let (wd, ht) = (px.width(), px.height());
                        let bytes: Vec<u8> = px.as_bytes().to_vec();
                        if copy {
                            let ok = arboard::Clipboard::new().and_then(|mut c| c.set_image(arboard::ImageData { width: wd as usize, height: ht as usize, bytes: bytes.into() })).is_ok();
                            toast(&st3, if ok { tr("Picture copied") } else { tr("Couldn't copy the picture") });
                        } else {
                            let dir = directories::UserDirs::new().and_then(|d| d.picture_dir().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from(".")).join("RyM");
                            let _ = std::fs::create_dir_all(&dir);
                            let name: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
                            let path = dir.join(format!("{}.png", name.trim()));
                            match image::RgbaImage::from_raw(wd, ht, bytes).map(|img| img.save(&path)) {
                                Some(Ok(())) => toast(&st3, &format!("{} {}", tr("Saved to"), path.display())),
                                _ => toast(&st3, tr("Couldn't save the picture")),
                            }
                        }
                    }
                    Err(e) => toast(&st3, &format!("{}: {e}", tr("Couldn't save the picture"))),
                }
            });
        };
        if copy {
            w.on_copy(take);
        } else {
            w.on_save(take);
        }
    }
    let _ = w.show();
    st.borrow_mut().share = Some(w);
}

/// The most colorful strong color of a cover (its accent).
fn cover_color(px: &Pixels) -> Option<slint::Color> {
    let (_, _, data) = px;
    let mut bins = [(0f32, 0f32, 0f32, 0f32); 12];
    for p in data.as_chunks::<4>().0.iter().step_by(11) {
        let (r, g, b) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if sat < 0.25 || max < 0.25 {
            continue;
        }
        let hue = if max == r { ((g - b) / (max - min)).rem_euclid(6.0) } else if max == g { (b - r) / (max - min) + 2.0 } else { (r - g) / (max - min) + 4.0 };
        let bin = ((hue / 6.0 * 12.0) as usize).min(11);
        let w = sat * max;
        bins[bin].0 += w;
        bins[bin].1 += r * w;
        bins[bin].2 += g * w;
        bins[bin].3 += b * w;
    }
    let best = bins.iter().max_by(|a, b| a.0.total_cmp(&b.0))?;
    if best.0 <= 0.0 {
        return None;
    }
    let (mut r, mut g, mut b) = (best.1 / best.0, best.2 / best.0, best.3 / best.0);
    // Bright enough to read on the dark themes.
    let max = r.max(g).max(b);
    if max < 0.75 {
        let k = 0.75 / max.max(0.01);
        r = (r * k).min(1.0);
        g = (g * k).min(1.0);
        b = (b * k).min(1.0);
    }
    Some(slint::Color::from_rgb_u8((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8))
}

/// The stats page: hours, the kinds heard (RaM points), top artists and songs.
fn fill_stats(st: &Shared) {
    let recap = rymplayer::stats::recap();
    let kinds = rymplayer::ram_points::kind_counts(&rymplayer::recent::tracks());
    let total: usize = kinds.iter().sum();
    let kind_bars: Vec<Bar> = rymplayer::ram_points::Kind::ALL
        .iter()
        .zip(kinds)
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| Bar { label: tr_any(k.name()).into(), value: n as f32 / total.max(1) as f32, text: format!("{}%", n * 100 / total.max(1)).into() })
        .collect();
    let top_hours = recap.top_artists.first().map_or(1.0, |a| a.1).max(0.01);
    let artists: Vec<Bar> = recap.top_artists.iter().map(|(a, h)| Bar { label: a.clone().into(), value: h / top_hours, text: format!("{:.1} h", h).into() }).collect();
    let top_plays = recap.top_songs.first().map_or(1, |s| s.1).max(1) as f32;
    let songs: Vec<Bar> = recap.top_songs.iter().map(|(t, n)| Bar { label: format!("{} — {}", t.title, t.artist).into(), value: *n as f32 / top_plays, text: format!("{n}\u{00d7}").into() }).collect();
    let text = format!("{:.1} {} · {:.1} {}", recap.hours_month, tr("hours this month"), recap.hours_total, tr("hours in all"));
    // RaM points' own favorites.
    let (fav_songs, fav_artists) = rymplayer::ram_points::favorites(&rymplayer::recent::tracks(), 8);
    let bars = |rows: Vec<(String, f32)>| -> Vec<Bar> {
        let top = rows.first().map_or(1.0, |r| r.1).max(0.01);
        rows.into_iter().map(|(label, p)| Bar { label: label.into(), value: p / top, text: format!("{p:.0}").into() }).collect()
    };
    let fav_songs = bars(fav_songs.into_iter().map(|(t, p)| (format!("{} — {}", t.title, t.artist), p)).collect());
    let fav_artists = bars(fav_artists);
    with_app(st, |a| {
        a.set_fav_songs(ModelRc::from(Rc::new(VecModel::from(fav_songs))));
        a.set_fav_artists(ModelRc::from(Rc::new(VecModel::from(fav_artists))));
        a.set_kinds(ModelRc::from(Rc::new(VecModel::from(kind_bars))));
        a.set_top_artists(ModelRc::from(Rc::new(VecModel::from(artists))));
        a.set_top_songs(ModelRc::from(Rc::new(VecModel::from(songs))));
        a.set_stats_text(text.into());
    });
}

/// The source pills over the lyrics; every source is looked up once per song.
fn show_sources(st: &Shared, shown: &Lyrics, vid: &str) {
    let (alt, asked, api, track) = {
        let s = st.borrow();
        (s.alt_lyrics.clone().filter(|(id, _)| id == vid), s.sources_for == vid, s.api.clone(), s.q.now().cloned())
    };
    match alt {
        Some((_, list)) => {
            let names: Vec<SharedString> = list.iter().map(|l| SharedString::from(l.source.as_str())).collect();
            let at = list.iter().position(|l| rymplayer::lyrics::same(l, shown)).map_or(-1, |i| i as i32);
            with_app(st, |a| {
                a.set_sources(ModelRc::from(Rc::new(VecModel::from(names))));
                a.set_source_at(at);
            });
        }
        None if !asked => {
            let Some(t) = track else { return };
            st.borrow_mut().sources_for = vid.to_string();
            let shown = shown.clone();
            post(async move {
                let vid = t.video_id.clone();
                Ev::Sources(vid, rymplayer::lyrics::sources(api, t, Some(shown)).await)
            });
        }
        None => {}
    }
}

fn show_lyrics(st: &Shared, lyrics: Option<Lyrics>) {
    let translate = st.borrow().config.lyrics_translation;
    let lyrics = lyrics.map(|mut l| {
        if !translate {
            l.translation.clear();
        }
        l
    });
    let Some(l) = lyrics else {
        {
            let mut s = st.borrow_mut();
            s.lyrics = None;
            s.m.lyrics.set_vec(Vec::new());
        }
        with_app(st, |a| a.set_lyrics_note(tr("No lyrics for this song").into()));
        return;
    };
    let lines: Vec<String> = if l.synced.is_empty() { l.plain.clone() } else { l.synced.iter().map(|(_, x)| if x.is_empty() { "♪".to_string() } else { x.clone() }).collect() };
    // Romaji under Japanese lines: looked up once per song (kana by table meanwhile).
    let (romaji_on, vid) = {
        let s = st.borrow();
        (s.config.romaji, s.q.now().map(|t| t.video_id.clone()).unwrap_or_default())
    };
    let japanese = rymplayer::lyrics::is_japanese(&l);
    let known = st.borrow().romaji.get(&format!("{vid}|{}", l.source)).cloned();
    let ro: Vec<String> = match (romaji_on && japanese, known) {
        (false, _) => Vec::new(),
        (true, Some(r)) => r,
        (true, None) => {
            let (lines2, source, vid2) = (lines.clone(), l.source.clone(), vid.clone());
            post(async move { Ev::Romaji(vid2, source, rymplayer::romaji::romanize(&lines2).await) });
            lines.iter().map(|x| if rymplayer::romaji::has_kana(x) { rymplayer::romaji::kana(x) } else { String::new() }).collect()
        }
    };
    let rows: Vec<LyricLine> = lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| LyricLine { rtl: rymplayer::i18n::is_rtl(&text), text: text.into(), tr: l.translation.get(i).cloned().unwrap_or_default().into(), ro: ro.get(i).cloned().unwrap_or_default().into(), state: if l.synced.is_empty() { 1 } else { 0 }, ..Default::default() })
        .collect();
    let note = if l.synced.is_empty() { format!("{} · {} {}", tr("These lyrics aren't synced to the song"), tr("Lyrics from"), l.source) } else { format!("{} {}", tr("Lyrics from"), l.source) };
    let wants = translate && l.translation.is_empty() && rymplayer::lyrics::is_japanese(&l);
    {
        let mut s = st.borrow_mut();
        s.m.lyrics.set_vec(rows);
        s.lyrics = Some(l.clone());
        s.shown.3 = -2;
    }
    with_app(st, |a| a.set_lyrics_note(note.into()));
    show_sources(st, &l, &vid);
    let now = st.borrow().q.now().cloned();
    if wants && let Some(t) = now {
        let source = l.source.clone();
        post(async move {
            let tr = rymplayer::lyrics::translate(&t, &l).await;
            Ev::Translated(t.video_id, source, tr)
        });
    }
}

// ---------------------------------------------------------------- media keys, mini player

fn start_media_controls(st: &Shared) {
    if st.borrow().media.is_some() {
        return;
    }
    #[cfg(windows)]
    let hwnd = main_window_handle();
    #[cfg(not(windows))]
    let hwnd = None;
    if cfg!(windows) && hwnd.is_none() {
        return;
    }
    let config = souvlaki::PlatformConfig { display_name: "RyM", dbus_name: "rym_player_slint", hwnd };
    let Ok(mut controls) = souvlaki::MediaControls::new(config) else {
        log("media controls: not available");
        return;
    };
    if controls.attach(|e| send(Ev::MediaKey(e))).is_err() {
        log("media controls: couldn't attach");
        return;
    }
    let mut s = st.borrow_mut();
    s.media = Some(controls);
    s.media_sent = None;
}

/// This app's main window (Windows): its visible top window that isn't the mini player.
#[cfg(windows)]
fn main_window_handle() -> Option<*mut std::ffi::c_void> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(each: unsafe extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn GetWindowTextW(hwnd: isize, text: *mut u16, max: i32) -> i32;
    }
    unsafe extern "system" fn each(hwnd: isize, found: isize) -> i32 {
        let mut pid = 0u32;
        // SAFETY: plain Win32 calls on a window handle Windows just gave; `found` is our isize.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != std::process::id() || IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256).max(0) as usize;
            let title = String::from_utf16_lossy(&buf[..n]);
            if title.is_empty() || title == MINI_TITLE {
                return 1;
            }
            *(found as *mut isize) = hwnd;
        }
        0
    }
    let mut found: isize = 0;
    // SAFETY: the callback writes only to `found`, which outlives the call.
    unsafe { EnumWindows(each, &mut found as *mut isize as isize) };
    (found != 0).then_some(found as *mut std::ffi::c_void)
}

fn media_key(st: &Shared, key: souvlaki::MediaControlEvent) {
    use souvlaki::MediaControlEvent as K;
    let playing = st.borrow().audio.as_ref().is_some_and(|a| a.is_playing());
    match key {
        K::Toggle => toggle_playback(st),
        K::Play if !playing => toggle_playback(st),
        K::Pause | K::Stop if playing => toggle_playback(st),
        K::Next => skip(st),
        K::Previous => previous(st),
        K::SetPosition(souvlaki::MediaPosition(at)) => {
            if let Some(a) = &st.borrow().audio {
                a.seek(at.as_secs_f32());
            }
        }
        _ => {}
    }
}

fn media_sync(st: &Shared) {
    let Ok(mut s) = st.try_borrow_mut() else { return };
    if s.media.is_none() {
        return;
    }
    let Some(t) = s.q.now().cloned() else { return };
    let playing = s.audio.as_ref().is_some_and(|a| a.is_playing());
    let key = (t.video_id.clone(), playing);
    if s.media_sent.as_ref() == Some(&key) {
        return;
    }
    let pos = s.audio.as_ref().map_or(0.0, |a| a.position());
    let dur = s.audio.as_ref().and_then(|a| a.duration()).or_else(|| parse_duration(&t.duration));
    let new_song = s.media_sent.as_ref().is_none_or(|(id, _)| *id != t.video_id);
    s.media_sent = Some(key);
    let cover = t.thumb.as_ref().map(|u| rymplayer::api::thumb_sized(u, 544));
    let Some(m) = s.media.as_mut() else { return };
    if new_song {
        let _ = m.set_metadata(souvlaki::MediaMetadata {
            title: Some(&t.title),
            artist: Some(&t.artist),
            album: (!t.album.is_empty()).then_some(t.album.as_str()),
            cover_url: cover.as_deref(),
            duration: dur.map(Duration::from_secs_f32),
        });
    }
    let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f32(pos.max(0.0))));
    let _ = m.set_playback(if playing { souvlaki::MediaPlayback::Playing { progress } } else { souvlaki::MediaPlayback::Paused { progress } });
}

/// The mini player: a small window above the others.
fn show_mini(st: &Shared, on: bool) {
    if !on {
        let old = st.borrow_mut().mini.take();
        if let Some(w) = old {
            let _ = w.hide();
        }
        return;
    }
    let Ok(w) = MiniWindow::new() else { return };
    {
        let st = st.clone();
        w.on_hide_mini(move || {
            let st = st.clone();
            // Not while the mini window's own callback runs.
            slint::Timer::single_shot(Duration::ZERO, move || show_mini(&st, false));
        });
    }
    {
        let weak = w.as_weak();
        w.on_drag(move |dx, dy| {
            if let Some(w) = weak.upgrade() {
                let p = w.window().position();
                let scale = w.window().scale_factor();
                w.window().set_position(slint::PhysicalPosition::new(p.x + (dx * scale) as i32, p.y + (dy * scale) as i32));
            }
        });
    }
    // Its buttons do what the main window's do.
    let app = w.global::<App>();
    app.on_tr(|s| tr_any(&s).into());
    {
        let st = st.clone();
        app.on_toggle_play(move || toggle_playback(&st));
    }
    {
        let st = st.clone();
        app.on_next(move || skip(&st));
    }
    {
        let st = st.clone();
        app.on_previous(move || previous(&st));
    }
    let _ = w.show();
    st.borrow_mut().mini = Some(w);
    mini_sync(st);
}

/// Each window has its own copy of the App global: the mini window's follows the main one.
fn mini_sync(st: &Shared) {
    let Ok(s) = st.try_borrow() else { return };
    let (Some(mini), Some(ui)) = (&s.mini, s.ui.upgrade()) else { return };
    let from = ui.global::<App>();
    let to = mini.global::<App>();
    if to.get_title() != from.get_title() || to.get_has_song() != from.get_has_song() || empty(&to.get_cover()) != empty(&from.get_cover()) {
        to.set_title(from.get_title());
        to.set_artist(from.get_artist());
        to.set_has_song(from.get_has_song());
        to.set_cover(from.get_cover());
    }
    if to.get_playing() != from.get_playing() {
        to.set_playing(from.get_playing());
    }
    let theme = ui.global::<Theme>().get_index();
    if mini.global::<Theme>().get_index() != theme {
        mini.global::<Theme>().set_index(theme);
    }
    let (custom, pal) = (ui.global::<Theme>().get_custom(), ui.global::<Theme>().get_p());
    if mini.global::<Theme>().get_custom() != custom || mini.global::<Theme>().get_p() != pal {
        mini.global::<Theme>().set_custom(custom);
        mini.global::<Theme>().set_p(pal);
    }
}

fn set_update_text(st: &Shared, text: impl Into<String>) {
    st.borrow_mut().update_text = text.into();
    fill_settings(st);
}

/// Last.fm: the song is "now playing", and its listening starts counting.
fn lastfm_start(st: &Shared) {
    let mut s = st.borrow_mut();
    let Some(t) = s.q.now().cloned() else { return };
    let dur = s.audio.as_ref().and_then(|a| a.duration()).unwrap_or(0.0);
    let cfg = s.config.clone();
    if let Some(send) = s.scrobbler.start(&t, dur, &cfg) {
        post(async move {
            send.await;
            Ev::Done
        });
    }
}

/// The window's X: close RyM, hide it in the tray (the music goes on), or ask which.
fn close_requested(st: &Shared) -> slint::CloseRequestResponse {
    let action = st.borrow().config.close_action.clone();
    match action.as_str() {
        "quit" => {
            quit(st);
            slint::CloseRequestResponse::HideWindow
        }
        "tray" => {
            to_tray(st);
            slint::CloseRequestResponse::HideWindow
        }
        _ => {
            with_app(st, |a| a.set_close_ask(true));
            slint::CloseRequestResponse::KeepWindowShown
        }
    }
}

fn to_tray(st: &Shared) {
    start_tray(st);
    IN_TRAY.with(|t| t.set(true));
    let window = st.borrow().ui.upgrade();
    if let Some(ui) = window {
        let _ = ui.hide();
    }
    // The effects stop now, not at the next tick.
    animations(st);
}

fn show_window(st: &Shared) {
    IN_TRAY.with(|t| t.set(false));
    let window = st.borrow().ui.upgrade();
    if let Some(ui) = window {
        let _ = ui.show();
        ui.window().set_minimized(false);
    }
    animations(st);
}

fn quit(st: &Shared) {
    save_session(st);
    rymplayer::stats::flush();
    let (mini, float) = {
        let mut s = st.borrow_mut();
        (s.mini.take(), s.float.take())
    };
    if let Some(w) = mini {
        let _ = w.hide();
    }
    if let Some(w) = float {
        let _ = w.hide();
    }
    st.borrow_mut().tray = None;
    let _ = slint::quit_event_loop();
}

/// The icon next to the clock: a click brings the window back; its menu shows it, plays or
/// pauses, skips, or quits.
fn start_tray(st: &Shared) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    if st.borrow().tray.is_some() {
        return;
    }
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("show", tr("Show RyM"), true, None));
    let _ = menu.append(&MenuItem::with_id("play", tr("Play / pause"), true, None));
    let _ = menu.append(&MenuItem::with_id("next", tr("Next"), true, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", tr("Quit RyM"), true, None));
    let icon = image::load_from_memory(include_bytes!("../../../assets/icons/rym-slint.png"))
        .ok()
        .map(|i| i.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgba8())
        .and_then(|i| tray_icon::Icon::from_rgba(i.into_raw(), 32, 32).ok());
    let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("RyM").with_menu_on_left_click(false);
    if let Some(icon) = icon {
        builder = builder.with_icon(icon);
    }
    match builder.build() {
        Ok(tray) => {
            TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    send(Ev::TrayShow);
                }
            }));
            MenuEvent::set_event_handler(Some(|e: MenuEvent| send(Ev::TrayMenu(e.id.0.clone()))));
            st.borrow_mut().tray = Some(tray);
        }
        Err(e) => log(&format!("tray icon: {e}")),
    }
}

fn check_updates(st: &Shared, asked: bool) {
    let beta = {
        let mut s = st.borrow_mut();
        s.config.last_update_check = rymplayer::updates::now();
        s.config.save();
        s.config.beta_updates
    };
    if asked {
        st.borrow_mut().update_text = tr("Checking…").into();
        fill_settings(st);
    }
    post(async move { Ev::Update(rymplayer::updates::check(beta).await, asked) });
}

/// Seamless Music: a phone of the same account plays while this app is idle: the music moves here.
fn heard_peer(st: &Shared, note: rymplayer::link::Announce, ip: std::net::IpAddr) {
    let key = {
        let mut s = st.borrow_mut();
        let key = s.account.as_ref().map(|a| rymplayer::link::account_key(&a.name, &a.handle));
        let idle = !s.loading && !s.audio.as_ref().is_some_and(|a| a.is_playing());
        let on = s.config.cross_ears;
        if !s.peers.wants(&note, key.as_deref(), idle, on) {
            return;
        }
        key
    };
    let Some(key) = key else { return };
    let (device, port) = (note.device.clone(), note.port);
    // "Take over from the phone" off: asked first.
    if !st.borrow().config.link_auto {
        st.borrow_mut().peer_pending = Some((ip, port, key, device.clone()));
        let text = format!("{} {}:\n{} — {}", tr("Playing on"), device, note.title, note.artist);
        with_app(st, |a| {
            a.set_peer_text(text.into());
            a.set_peer_ask(true);
        });
        return;
    }
    post(async move { Ev::TookOver(device, rymplayer::link::take_over(ip, port, &key).await) });
}

// ---------------------------------------------------------------- answers

fn handle(st: &Shared, ev: Ev) {
    match ev {
        Ev::Auth(new_login, Ok(api)) => {
            let user = api.logged_in();
            st.borrow_mut().api = Some(api.clone());
            with_app(st, |a| a.set_signed_in(user));
            if user {
                let a2 = api.clone();
                post(async move { Ev::Account(a2.account().await) });
                let a3 = api.clone();
                post(async move { Ev::Playlists(a3.library_playlists().await) });
            }
            if new_login {
                toast(st, tr("Signed in"));
            }
            refresh_home(st);
            let waiting = st.borrow_mut().pending_link.take();
            if let Some(link) = waiting {
                open_incoming(st, link);
            }
            let now = st.borrow().q.now().cloned();
            if let Some(t) = now {
                similar_and_artist(st, &t);
            }
        }
        Ev::Auth(new_login, Err(e)) => {
            if new_login {
                toast(st, &format!("{}: {e}", tr("Sign-in failed")));
            } else if st.borrow().api.is_none() {
                if rymplayer::secret::has_cookie() {
                    toast(st, tr("Your saved sign-in ended: continuing as a guest"));
                    rymplayer::secret::forget_cookie();
                    post(async { Ev::Auth(false, Api::guest().await) });
                } else {
                    with_app(st, |a| a.set_home_loading(false));
                    toast(st, &format!("Can't reach YouTube Music: {e}"));
                }
            }
        }
        Ev::Account(Ok(a)) => {
            with_app(st, |app| {
                app.set_account_name(a.name.clone().into());
                app.set_account_handle(a.handle.clone().into());
            });
            if let Some(url) = a.photo.clone() {
                post(async move { Ev::Avatar(fetch(&url).await) });
            }
            {
                let mut s = st.borrow_mut();
                if let Ok(mut p) = s.link.lock() {
                    p.account = Some(rymplayer::link::account_key(&a.name, &a.handle));
                }
                s.account = Some(a);
            }
        }
        Ev::Account(Err(e)) => {
            log(&format!("account: {e}"));
            with_app(st, |app| app.set_account_name(tr("Signed in").into()));
        }
        Ev::Avatar(Ok(bytes)) => {
            if let Some(px) = decode_square(&bytes, 128) {
                let img = to_image(px);
                with_app(st, |a| {
                    a.set_avatar(img);
                    a.set_has_avatar(true);
                });
            }
        }
        Ev::Avatar(Err(e)) => log(&format!("account picture: {e}")),
        Ev::Home(result) => {
            st.borrow_mut().home_loading = false;
            with_app(st, |a| a.set_home_loading(false));
            match result {
                Ok(fresh) => {
                    if let Some(liked) = fresh.liked {
                        {
                            let mut s = st.borrow_mut();
                            s.liked.sync(liked.iter().map(|t| t.video_id.clone()));
                            s.liked_tracks = liked;
                        }
                        refresh_hearts(st);
                    }
                    rymplayer::page_cache::save_home(&fresh.home);
                    st.borrow_mut().home_more = fresh.more;
                    show_home(st, fresh.home);
                }
                Err(e) => toast(st, &format!("{}: {e}", tr("Couldn't load the home page"))),
            }
        }
        Ev::HomeMore(result) => {
            st.borrow_mut().loading_more = false;
            if let Ok(part) = result {
                let mut home = st.borrow().home.clone();
                home.extend(part.tracks);
                st.borrow_mut().home_more = part.more;
                show_home(st, home);
            }
        }
        Ev::Playlists(Ok(lists)) => {
            st.borrow_mut().playlists = lists;
            show_playlists(st);
        }
        Ev::Playlists(Err(e)) => log(&format!("playlists: {e}")),
        Ev::Liked(Ok(liked)) => {
            {
                let mut s = st.borrow_mut();
                s.liked.sync(liked.iter().map(|t| t.video_id.clone()));
                s.liked_tracks = liked;
            }
            refresh_hearts(st);
        }
        Ev::Liked(Err(_)) => {}
        Ev::FolderPicked(Some(dir)) => {
            {
                let mut s = st.borrow_mut();
                let dir = dir.to_string_lossy().to_string();
                if !s.config.local_folders.contains(&dir) {
                    s.config.local_folders.push(dir);
                    s.config.save();
                }
            }
            toast(st, tr("Folder added"));
            fill_settings(st);
        }
        Ev::FolderPicked(None) => {}
        Ev::List(id, result) => {
            if st.borrow().list_id != id {
                return;
            }
            with_app(st, |a| a.set_list_loading(false));
            match result {
                Ok(tracks) => {
                    st.borrow_mut().list = tracks;
                    show_songs(st, 'l');
                    mark_playing(st);
                }
                Err(e) => toast(st, &e.to_string()),
            }
        }
        Ev::ListPaged(id, result) => {
            if st.borrow().list_id != id {
                return;
            }
            with_app(st, |a| a.set_list_loading(false));
            match result {
                Ok(p) => {
                    let more = p.more.is_some();
                    {
                        let mut s = st.borrow_mut();
                        s.list = p.tracks;
                        s.list_more = p.more;
                    }
                    with_app(st, |a| a.set_list_more(more));
                    show_songs(st, 'l');
                    mark_playing(st);
                }
                Err(e) => toast(st, &e.to_string()),
            }
        }
        Ev::ListMore(id, result) => {
            st.borrow_mut().loading_more = false;
            if st.borrow().list_id != id {
                return;
            }
            if let Ok(p) = result {
                let more = p.more.is_some();
                {
                    let mut s = st.borrow_mut();
                    s.list.extend(p.tracks);
                    s.list_more = p.more;
                }
                with_app(st, |a| a.set_list_more(more));
                show_songs(st, 'l');
                mark_playing(st);
            }
        }
        Ev::Search(id, result) => {
            if st.borrow().search_id != id {
                return;
            }
            with_app(st, |a| a.set_searching(false));
            match result {
                Ok(p) => {
                    {
                        let mut s = st.borrow_mut();
                        s.results = p.tracks.into_iter().filter(|t| t.link.is_none()).collect();
                        s.results_more = p.more;
                        s.results_filter = 0;
                    }
                    show_songs(st, 'r');
                    mark_playing(st);
                }
                Err(e) => toast(st, &format!("{}: {e}", tr("Search failed"))),
            }
        }
        Ev::Suggestions(asked, Ok(list)) => {
            if st.borrow().suggest_for.as_deref() != Some(asked.as_str()) {
                return;
            }
            let list: Vec<Suggestion> = list.into_iter().take(8).collect();
            let rows: Vec<Suggest> = list.iter().map(|g| Suggest { text: g.text.clone().into(), history: g.from_history() }).collect();
            let mut s = st.borrow_mut();
            s.suggestions = list;
            s.m.suggestions.set_vec(rows);
        }
        Ev::Suggestions(..) => {}
        Ev::Cover(url, px) => {
            if let Some(px) = px {
                let img = to_image(px);
                {
                    let mut s = st.borrow_mut();
                    s.covers.insert(url.clone(), img);
                    s.cover_order.push_back(url);
                    while s.cover_order.len() > COVERS_KEPT {
                        if let Some(old) = s.cover_order.pop_front() {
                            s.covers.remove(&old);
                            s.requested.remove(&old);
                        }
                    }
                }
                fill_covers(st);
            }
        }
        Ev::Art(vid, art) => {
            let current = st.borrow().q.now().is_some_and(|t| t.video_id == vid);
            if current && let Some((mut big, mut soft)) = art {
                // Remorse: the cover loses its color.
                if current_colors(&st.borrow().config).is_some_and(|t| t.muted) {
                    rymplayer::themes::mute(&mut big.2);
                    rymplayer::themes::mute(&mut soft.2);
                }
                let from_cover = cover_color(&big);
                let accent = st.borrow().config.accent_from_cover.then_some(from_cover).flatten();
                let lights = rymplayer::themes::cover_colors(&big.2, 4);
                let (big, soft) = (to_image(big), to_image(soft));
                with_app(st, |a| {
                    a.set_cover(big);
                    a.set_backdrop(soft);
                });
                if let Some(ui) = st.borrow().ui.upgrade() {
                    let theme = ui.global::<Theme>();
                    theme.set_cover_accent(accent.is_some());
                    if let Some(c) = from_cover {
                        theme.set_cover_color(c);
                    }
                    let color = |c: (u8, u8, u8)| slint::Color::from_rgb_u8(c.0, c.1, c.2);
                    if let [a, b, c, d] = lights[..] {
                        theme.set_a1(color(a));
                        theme.set_a2(color(b));
                        theme.set_a3(color(c));
                        theme.set_a4(color(d));
                    }
                }
            }
        }
        Ev::Fetched(id, result) => {
            let mut s = st.borrow_mut();
            if id != s.request_id {
                return;
            }
            s.loading = false;
            // The gold badge: lossless (FLAC from the Archive, or a FLAC file on this computer).
            let flac = result.as_ref().is_ok_and(stream::is_lossless);
            if let Some(ui) = s.ui.upgrade() {
                ui.global::<App>().set_lossless(flac);
            }
            let Some(t) = s.q.now().cloned() else { return };
            let known = parse_duration(&t.duration).or_else(|| stream::cached_duration(&t.video_id));
            let resume = s.resume_at.take().filter(|(id, at)| *id == t.video_id && *at > 1.0).map(|(_, at)| at);
            let crossfade = std::mem::take(&mut s.crossfade_next);
            let gapless = std::mem::take(&mut s.gapless_next);
            let automix = s.config.automix;
            let secs = if automix { rymplayer::automix::mix_secs(s.config.crossfade) } else { s.config.crossfade as f32 };
            let started = match (result, &mut s.audio) {
                (Ok(song), Some(a)) if gapless => a.gapless_to(&song, known).map_err(|e| e.to_string()),
                (Ok(song), Some(a)) if crossfade && automix => a.automix_to(&song, known, secs).map_err(|e| e.to_string()),
                (Ok(song), Some(a)) if crossfade => a.crossfade_to(&song, known, secs).map_err(|e| e.to_string()),
                (Ok(song), Some(a)) => a.play(&song, known).map_err(|e| e.to_string()).map(|()| {
                    if let Some(at) = resume {
                        a.seek(at);
                    }
                }),
                (Ok(_), None) => Err("no audio device".into()),
                (Err(e), _) => Err(e.to_string()),
            };
            drop(s);
            with_app(st, |a| a.set_loading(false));
            match started {
                Ok(()) => {
                    prefetch_next(st);
                    lastfm_start(st);
                    let (ask, limit) = {
                        let mut s = st.borrow_mut();
                        let sponsor = s.config.sponsorblock;
                        (s.non_music.song(&t.video_id, sponsor), s.config.cache_limit_gb)
                    };
                    if let Some(ask) = ask {
                        post(async move {
                            let (vid, parts) = ask.await;
                            Ev::Segments(vid, parts)
                        });
                    }
                    trim_cache(limit, t.video_id.clone());
                }
                Err(e) => {
                    log(&format!("can't play {}: {e}", t.video_id));
                    toast(st, &format!("{} \"{}\": {e}", tr("Can't play"), t.title));
                }
            }
        }
        Ev::Lyrics(vid, found) => {
            let current = st.borrow().q.now().is_some_and(|t| t.video_id == vid);
            if current {
                show_lyrics(st, found);
            }
        }
        Ev::Translated(vid, source, lines) => {
            let current = {
                let s = st.borrow();
                s.lyrics.clone().filter(|l| l.source == source && s.q.now().is_some_and(|t| t.video_id == vid))
            };
            if let Some(mut l) = current
                && !lines.is_empty()
            {
                l.translation = lines;
                rymplayer::lyrics::remember(&vid, &l);
                show_lyrics(st, Some(l));
            }
        }
        Ev::Incoming(link) => {
            show_window(st);
            if let Some(ui) = st.borrow().ui.upgrade() {
                use slint::winit_030::WinitWindowAccessor;
                ui.window().with_winit_window(|w| w.focus_window());
            }
            open_incoming(st, link);
        }
        Ev::Handed(result) => {
            {
                let mut s = st.borrow_mut();
                s.user_pick = true;
                s.peers.played_here();
            }
            handle(st, Ev::LinkSongs(0, "RyM".into(), result));
        }
        Ev::LinkSongs(what, title, result) => match result {
            Ok(songs) => {
                let songs: Vec<Track> = songs.into_iter().filter(|t| !t.video_id.is_empty() && t.link.is_none()).collect();
                if songs.is_empty() {
                    toast(st, &format!("{}: {title}", tr("No songs in")));
                    return;
                }
                match what {
                    0 => play_list(st, songs, 0),
                    // Nothing in the queue yet: they just play.
                    1 if st.borrow().q.current.is_none() => play_list(st, songs, 0),
                    1 => {
                        {
                            let mut s = st.borrow_mut();
                            for t in songs.into_iter().rev() {
                                s.q.play_next(t);
                            }
                        }
                        render_queue(st);
                        toast(st, &format!("{}: {title}", tr("Plays next")));
                    }
                    _ => {
                        let mut start = false;
                        {
                            let mut s = st.borrow_mut();
                            for t in songs {
                                start |= s.q.add(t);
                            }
                        }
                        render_queue(st);
                        if start && let Some(c) = st.borrow().q.current {
                            play_at(st, c);
                        }
                        toast(st, &format!("{}: {title}", tr("Added to queue")));
                    }
                }
            }
            Err(e) => toast(st, &format!("{}: {e}", title)),
        },
        Ev::Radio(result) => {
            st.borrow_mut().radio_pending = false;
            let Ok(tracks) = result else { return };
            let (fresh, at_end) = {
                let s = st.borrow();
                let Some(seed) = s.q.now().cloned() else { return };
                let queued: HashSet<String> = s.q.songs.iter().map(|t| t.video_id.clone()).collect();
                let heard = rymplayer::radio::artists_of(s.q.songs.iter().take(s.q.current.map_or(0, |c| c + 1)));
                let skip: HashSet<String> = s.played.union(&queued).cloned().collect();
                // RaM points: only the playing song's kind, its artist and songs like it first.
                let fresh = rymplayer::ram_points::up_next(tracks, &seed, &s.liked_tracks, &skip, &heard, rymplayer::updates::now() | 1);
                let at_end = s.q.current.is_some_and(|i| i + 1 >= s.q.songs.len()) && !s.loading && s.audio.as_ref().is_some_and(|a| !a.is_loaded());
                (fresh, at_end)
            };
            let next = st.borrow().q.songs.len();
            st.borrow_mut().q.songs.extend(fresh);
            let grew = next < st.borrow().q.songs.len();
            if at_end && grew {
                play_at(st, next);
            } else {
                prefetch_next(st);
                render_queue(st);
            }
        }
        Ev::NewDirection(Ok((seed, radio))) => {
            let fresh = {
                let s = st.borrow();
                let queued: HashSet<String> = s.q.songs.iter().map(|t| t.video_id.clone()).collect();
                let heard = rymplayer::radio::artists_of(s.q.songs.iter().take(s.q.current.map_or(0, |c| c + 1)));
                let skip: HashSet<String> = s.played.union(&queued).cloned().collect();
                rymplayer::ram_points::up_next(radio, &seed, &s.liked_tracks, &skip, &heard, rymplayer::updates::now() | 1)
            };
            if fresh.is_empty() {
                return;
            }
            {
                let mut s = st.borrow_mut();
                s.q.drop_coming();
                s.q.songs.extend(fresh);
            }
            prefetch_next(st);
            render_queue(st);
            toast(st, &format!("{} {}", tr("New direction:"), seed.artist));
        }
        Ev::NewDirection(Err(_)) => {}
        Ev::Similar(vid, Ok(tracks)) => {
            if st.borrow().similar_for != vid {
                return;
            }
            let picked: Vec<Track> = {
                let s = st.borrow();
                let queued: HashSet<&str> = s.q.songs.iter().map(|t| t.video_id.as_str()).collect();
                tracks.into_iter().filter(|t| t.video_id != vid && !queued.contains(t.video_id.as_str())).take(20).collect()
            };
            st.borrow_mut().similar = picked;
            show_cards(st, 's');
        }
        Ev::Similar(..) => {}
        Ev::SameArtist(artist, Ok(tracks)) => {
            if st.borrow().same_for != artist {
                return;
            }
            let now = st.borrow().q.now().map(|t| t.video_id.clone()).unwrap_or_default();
            st.borrow_mut().same = tracks.into_iter().filter(|t| t.video_id != now).take(20).collect();
            show_cards(st, 'a');
        }
        Ev::SameArtist(..) => {}
        Ev::ArtistFound(name, Ok(Some(id))) => {
            let api = st.borrow().api.clone();
            if let Some(api) = api {
                open_list(st, name, api.artist_page(id));
            }
        }
        Ev::ArtistFound(..) => toast(st, tr("Artist not found")),
        Ev::Rated(vid, like, Err(e)) => {
            st.borrow_mut().liked.set(&vid, !like);
            refresh_hearts(st);
            toast(st, &format!("{}: {e}", tr("Like failed")));
        }
        Ev::Rated(..) => {}
        Ev::Downloaded(Ok(p)) => toast(st, &format!("{} {}", tr("Saved to"), p.display())),
        Ev::Downloaded(Err(e)) => toast(st, &format!("{}: {e}", tr("Download failed"))),
        Ev::Update(result, asked) => match result {
            Ok(Some(r)) => {
                // The release's file for this system: the .exe on Windows, the AppImage on Linux.
                let url = r.download_for(if cfg!(windows) { "RyM-Slint.exe" } else { "RyM-Slint-x86_64.AppImage" });
                set_update_text(st, format!("RyM {} — {}", r.version, tr("downloading…")));
                toast(st, &format!("RyM {} — {}", r.version, tr("downloading…")));
                if url.ends_with(".exe") || url.ends_with(".AppImage") {
                    post(async move { Ev::Installed(rymplayer::updates::install(&url, |_| {}).await) });
                } else {
                    let _ = rymplayer::updates::open_link(&url);
                }
            }
            Ok(None) if asked => set_update_text(st, tr("You have the latest version")),
            Err(e) if asked => set_update_text(st, format!("{}: {e}", tr("Couldn't check for updates"))),
            _ => {}
        },
        Ev::Installed(Ok(path)) => {
            set_update_text(st, tr("Updated: RyM opens again"));
            save_session(st);
            rymplayer::stats::flush();
            if std::process::Command::new(&path).spawn().is_ok() {
                // This copy goes for good, not to the tray (the small windows and the tray icon
                // too), so the old version doesn't stay open beside the new one.
                if let Some(ui) = st.borrow().ui.upgrade() {
                    let _ = ui.hide();
                }
                quit(st);
                // Whatever still holds it (the sound or the media keys' threads) can't keep it open.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(3));
                    std::process::exit(0);
                });
            }
        }
        Ev::Installed(Err(e)) => set_update_text(st, format!("{}: {e}", tr("Update failed"))),
        Ev::MediaKey(k) => media_key(st, k),
        Ev::Store(Ok(list)) => {
            st.borrow_mut().store = list.into_iter().filter(|p| p.for_this_pc()).collect();
            plugin_flags(st);
            fill_settings(st);
        }
        Ev::Store(Err(e)) => log(&format!("plugin store: {e}")),
        Ev::VideoLinks(t, result) => {
            let msg = match (result, rymplayer::vlc::find()) {
                (Ok((video, audio)), Some(exe)) => {
                    // The song pauses while the video plays.
                    let playing = st.borrow().audio.as_ref().is_some_and(Audio::is_playing);
                    if playing {
                        toggle_playback(st);
                    }
                    match rymplayer::video::watch_in_vlc(&exe, &video, audio.as_deref(), &format!("{} - {}", t.artist, t.title)) {
                        Ok(child) => {
                            st.borrow_mut().vlc = Some(child);
                            None
                        }
                        Err(e) => Some(format!("{}: {e}", tr("Can't open the video"))),
                    }
                }
                (_, None) => {
                    let _ = rymplayer::updates::open_link(&rymplayer::video::watch_url(&t.video_id));
                    Some(tr("VLC isn't installed: opening YouTube in the browser").to_string())
                }
                (Err(e), _) => Some(format!("{}: {e}", tr("Can't open the video"))),
            };
            if let Some(m) = msg {
                toast(st, &m);
            }
        }
        Ev::InVlc(id, result) => {
            let mut s = st.borrow_mut();
            if id != s.request_id {
                return;
            }
            s.loading = false;
            let Some(t) = s.q.now().cloned() else { return };
            let msg = match (result, rymplayer::vlc::find()) {
                (Ok((url, headers)), Some(exe)) => {
                    rymplayer::vlc::stop(&mut s.vlc);
                    match rymplayer::vlc::play(&exe, &url, &headers, &format!("{} - {}", t.artist, t.title)) {
                        Ok(child) => {
                            s.vlc = Some(child);
                            tr("Playing in VLC").to_string()
                        }
                        Err(e) => format!("VLC: {e}"),
                    }
                }
                (Err(e), _) => format!("{}: {e}", tr("Can't play")),
                (_, None) => tr("VLC isn't installed").into(),
            };
            drop(s);
            with_app(st, |a| a.set_loading(false));
            toast(st, &msg);
        }
        Ev::SavedQueue(Ok(text)) => {
            toast(st, &format!("{} {text}", tr("Saved:")));
            let api = st.borrow().api.clone();
            if let Some(api) = api {
                post(async move { Ev::Playlists(api.library_playlists().await) });
            }
        }
        Ev::SavedQueue(Err(e)) => toast(st, &format!("{}: {e}", tr("Couldn't save"))),
        Ev::AddedTo(title, Ok(())) => toast(st, &format!("{} {title}", tr("Added to"))),
        Ev::AddedTo(_, Err(e)) => toast(st, &format!("{}: {e}", tr("Couldn't add"))),
        Ev::History(Ok(history)) => {
            // Recently played on top of Home follows the account's history (plays from the phone too).
            let mut home = st.borrow().home.clone();
            if home.iter().any(|t| t.album == feed::RECENTLY_PLAYED) {
                let mut seen = HashSet::new();
                let row: Vec<Track> = history
                    .into_iter()
                    .chain(rymplayer::recent::tracks())
                    .filter(|t| t.link.is_none() && seen.insert(t.video_id.clone()))
                    .take(20)
                    .map(|t| Track { album: feed::RECENTLY_PLAYED.into(), ..t })
                    .collect();
                let changed = {
                    let old: Vec<&str> = home.iter().filter(|t| t.album == feed::RECENTLY_PLAYED).map(|t| t.video_id.as_str()).collect();
                    old != row.iter().map(|t| t.video_id.as_str()).collect::<Vec<_>>()
                };
                if changed {
                    home.retain(|t| t.album != feed::RECENTLY_PLAYED);
                    let rest = std::mem::take(&mut home);
                    home = row.into_iter().chain(rest).collect();
                    show_home(st, home);
                }
            }
        }
        Ev::History(Err(_)) => {}
        Ev::ResultsMore(id, result) => {
            st.borrow_mut().loading_more = false;
            if st.borrow().search_id != id {
                return;
            }
            if let Ok(p) = result {
                {
                    let mut s = st.borrow_mut();
                    s.results.extend(p.tracks.into_iter().filter(|t| t.link.is_none()));
                    s.results_more = p.more;
                }
                show_songs(st, 'r');
                mark_playing(st);
            }
        }
        Ev::TrayShow => show_window(st),
        Ev::TrayMenu(id) => match id.as_str() {
            "show" => show_window(st),
            "play" => toggle_playback(st),
            "next" => skip(st),
            "quit" => quit(st),
            _ => {}
        },
        Ev::Sources(vid, list) => {
            let (current, shown) = {
                let s = st.borrow();
                (s.q.now().is_some_and(|t| t.video_id == vid), s.lyrics.clone())
            };
            if current {
                st.borrow_mut().alt_lyrics = Some((vid.clone(), list));
                if let Some(l) = shown {
                    show_sources(st, &l, &vid);
                }
            }
        }
        Ev::Romaji(vid, source, lines) => {
            st.borrow_mut().romaji.insert(format!("{vid}|{source}"), lines);
            let current = {
                let s = st.borrow();
                s.lyrics.clone().filter(|l| l.source == source && s.q.now().is_some_and(|t| t.video_id == vid))
            };
            if current.is_some() {
                show_lyrics(st, current);
            }
        }
        Ev::YtDlp(result) => {
            let text = match result {
                Ok(t) => t,
                Err(e) => e.to_string(),
            };
            toast(st, &format!("yt-dlp: {text}"));
            st.borrow_mut().ytdlp_text = text;
            fill_settings(st);
            post(async { Ev::YtDlpStatus(rymplayer::ytdlp::status().await) });
        }
        Ev::LastFmToken(Ok((token, page))) => {
            st.borrow_mut().lastfm_token = Some(token);
            let _ = rymplayer::updates::open_link(&page);
            fill_settings(st);
        }
        Ev::LastFmSession(Ok((session, user))) => {
            {
                let mut s = st.borrow_mut();
                s.lastfm_token = None;
                s.config.lastfm_session = session;
                s.config.lastfm_user = user.clone();
                s.config.save();
            }
            toast(st, &format!("Last.fm: {user}"));
            fill_settings(st);
        }
        Ev::LastFmToken(Err(e)) | Ev::LastFmSession(Err(e)) => toast(st, &e.to_string()),
        Ev::Batch(b) => {
            if b.finished() {
                toast(st, &format!("{}: {} · {}: {} · {}: {}", tr("Saved"), b.saved, tr("Already there"), b.skipped, tr("Failed"), b.failed));
            } else if b.done > 0 {
                toast(st, &format!("{} {}/{}", tr("Downloading"), b.done, b.total));
            }
        }
        Ev::LinkPause => {
            // No taking it back from the phone for a while (until something is played here).
            st.borrow_mut().peers.phone_took();
            let playing = st.borrow().audio.as_ref().is_some_and(Audio::is_playing);
            if playing {
                toggle_playback(st);
                toast(st, tr("Your phone took the music"));
            }
        }
        Ev::Peer(note, ip) => heard_peer(st, note, ip),
        Ev::Remote(what, value) => remote(st, &what, value),
        Ev::Segments(vid, segments) => st.borrow_mut().non_music.got(&vid, segments),
        Ev::YtDlpStatus(text) => {
            st.borrow_mut().ytdlp_status = text;
            fill_settings(st);
        }
        Ev::Parsers(text) => {
            toast(st, &format!("{}: {text}", tr("Parsers")));
            {
                let mut s = st.borrow_mut();
                s.parsers_text = format!("{} · {text}", tr("Up to date"));
            }
            fill_settings(st);
            post(async { Ev::YtDlpStatus(rymplayer::ytdlp::status().await) });
        }
        Ev::Bgutil(text) => {
            toast(st, &text);
            st.borrow_mut().bgutil_text = text;
            fill_settings(st);
        }
        Ev::ParsersStatus(text) => {
            st.borrow_mut().parsers_status = text;
            fill_settings(st);
        }
        Ev::TookOver(device, Ok(state)) => {
            let current = state.current;
            let songs: Vec<Track> = state.songs.into_iter().map(Track::from).collect();
            {
                let mut s = st.borrow_mut();
                let vid = songs.get(current).map(|t| t.video_id.clone()).unwrap_or_default();
                s.q = Queue { songs, current: Some(current), added: 0 };
                s.resume_at = Some((vid, state.position));
            }
            play_at(st, current);
            toast(st, &format!("{} {device}", tr("Playing here, moved from")));
        }
        Ev::TookOver(_, Err(e)) => log(&format!("seamless music: {e}")),
        Ev::Done => {}
    }
}

// ---------------------------------------------------------------- the layout's check

/// Draws the window with sample songs into a PNG with the software renderer (no window).
fn render_sample(out: &str, page: i32, theme_index: i32) {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
    // A clock of its own, so animations can be stepped through (a second between frames).
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl slint::platform::Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
        fn duration_since_start(&self) -> Duration {
            Duration::from_millis(CLOCK.load(std::sync::atomic::Ordering::Relaxed))
        }
    }
    let (w, h) = (1600u32, 1024u32);
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let _ = slint::platform::set_platform(Box::new(Headless(window.clone())));
    let ui = AppWindow::new().expect("window");
    let app = ui.global::<App>();
    ui.global::<Theme>().set_index(theme_index);
    // Extra words after the page: a color theme's name, "vinyl", "aurora".
    let extras: Vec<String> = std::env::args().skip_while(|a| a != "--render").skip(4).collect();
    let preset = extras.iter().find_map(|e| rymplayer::themes::preset(e));
    paint(&ui.global::<Theme>(), preset.as_ref());
    ui.global::<Theme>().set_vinyl(extras.iter().any(|e| e == "vinyl"));
    ui.global::<Theme>().set_aurora(extras.iter().any(|e| e == "aurora"));
    ui.global::<Theme>().set_energy(0.6);
    ui.global::<Theme>().set_vu_l(0.62);
    ui.global::<Theme>().set_vu_r(0.55);
    app.on_tr(|s| tr_any(&s).into());
    // Sample cover art: a diagonal gradient from the color to a shifted one, a soft sun, two hills.
    let art = |r: u8, g: u8, b: u8| -> Pixels {
        let n = 256u32;
        let (r2, g2, b2) = (b.saturating_add(40), r / 2 + 30, g.saturating_add(70));
        let mut px = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
                let t = (fx + fy) / 2.0;
                let mix = |a: u8, c: u8| a as f32 + (c as f32 - a as f32) * t;
                let (mut cr, mut cg, mut cb) = (mix(r, r2), mix(g, g2), mix(b, b2));
                let d = ((fx - 0.68).powi(2) + (fy - 0.34).powi(2)).sqrt();
                let sun = (1.0 - d / 0.2).clamp(0.0, 1.0);
                cr += (255.0 - cr) * sun * 0.75;
                cg += (235.0 - cg) * sun * 0.75;
                cb += (205.0 - cb) * sun * 0.6;
                let (near, far) = (0.84 + 0.05 * (fx * 4.0 + 1.3).cos(), 0.72 + 0.06 * (fx * 6.0).sin());
                // Soft edges (a pixel and a half), not stairs.
                let edge = |line: f32| ((fy - line) * n as f32 / 1.5).clamp(0.0, 1.0);
                let shade = 1.0 - 0.4 * edge(far) - 0.25 * edge(near);
                px.extend_from_slice(&[(cr * shade).clamp(0.0, 255.0) as u8, (cg * shade).clamp(0.0, 255.0) as u8, (cb * shade).clamp(0.0, 255.0) as u8, 255]);
            }
        }
        (n, n, px)
    };
    let swatch = |r: u8, g: u8, b: u8| to_image(art(r, g, b));
    app.set_has_song(true);
    app.set_title("Seasons".into());
    app.set_artist("Rival x Cadmium, feat. Harley Bird".into());
    let mut cover = art(70, 60, 170);
    if preset.as_ref().is_some_and(|t| t.muted) {
        rymplayer::themes::mute(&mut cover.2);
    }
    let lights = rymplayer::themes::cover_colors(&cover.2, 4);
    let color = |c: (u8, u8, u8)| slint::Color::from_rgb_u8(c.0, c.1, c.2);
    if let [l1, l2, l3, l4] = lights[..] {
        let theme = ui.global::<Theme>();
        theme.set_a1(color(l1));
        theme.set_a2(color(l2));
        theme.set_a3(color(l3));
        theme.set_a4(color(l4));
        theme.set_cover_color(color(l1));
    }
    let soft = image::RgbaImage::from_raw(cover.0, cover.1, cover.2.clone()).map(|img| image::imageops::blur(&image::imageops::resize(&img, 96, 96, image::imageops::FilterType::Triangle), 7.0));
    app.set_backdrop(soft.map(|s| to_image((s.width(), s.height(), s.into_raw()))).unwrap_or_default());
    app.set_cover(to_image(cover));
    app.set_liked(true);
    app.set_playing(true);
    app.set_duration(205.0);
    app.set_position(62.0);
    app.set_pos_text("1:02".into());
    app.set_len_text("3:25".into());
    app.set_volume(0.5);
    app.set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for("seasons")))));
    app.set_artist_name("Rival".into());
    let song = |t: &str, a: &str, c: Image| Song { key: "q:1".into(), title: t.into(), artist: a.into(), cover: c, liked: false, playing: false, video: false, why: SharedString::new() };
    app.set_queue(ModelRc::from(Rc::new(VecModel::from(vec![
        song("Another Love", "Tom Odell", swatch(40, 90, 120)),
        song("Die For You", "VALORANT, Grabbitz", swatch(200, 20, 30)),
        song("I'm Just A Kid", "Moe Zein, Al3s3s", swatch(120, 120, 120)),
        song("بشرة خير", "حسين الجسمي", swatch(60, 110, 70)),
    ]))));
    let card = |t: &str, a: &str, c: Image| Card { key: "s:0".into(), title: t.into(), sub: a.into(), cover: c };
    let cards = vec![
        card("Night Changes", "One Direction", swatch(30, 30, 30)),
        card("'tis The Damn Season", "Taylor Swift", swatch(110, 100, 90)),
        card("Royalty", "Egzod, Maestro Chives", swatch(70, 90, 160)),
        card("Extra", "More", swatch(90, 50, 50)),
    ];
    app.set_similar(ModelRc::from(Rc::new(VecModel::from(cards.clone()))));
    app.set_same_artist(ModelRc::from(Rc::new(VecModel::from(cards))));
    app.set_page(page);
    app.set_settings(ModelRc::from(Rc::new(VecModel::from(vec![
        Setting { id: "".into(), kind: 4, title: "Look".into(), about: "".into(), on: false, value: "".into() },
        Setting { id: "theme".into(), kind: 1, title: "Theme".into(), about: "".into(), on: false, value: "RyM Dusk".into() },
        Setting { id: "automix".into(), kind: 0, title: "Smart Automix".into(), about: "Songs blend like a DJ mix".into(), on: true, value: "".into() },
        Setting { id: "alarm-time".into(), kind: 2, title: "Time".into(), about: "Like 07:30".into(), on: false, value: "07:30".into() },
        Setting { id: "ytdlp".into(), kind: 3, title: "Update yt-dlp".into(), about: "".into(), on: false, value: "Update".into() },
    ]))));
    app.set_kinds(ModelRc::from(Rc::new(VecModel::from(vec![
        Bar { label: "Songs".into(), value: 0.7, text: "70%".into() },
        Bar { label: "Husseini".into(), value: 0.3, text: "30%".into() },
    ]))));
    app.set_suggestions(ModelRc::from(Rc::new(VecModel::from(vec![
        Suggest { text: "tame impala".into(), history: true },
        Suggest { text: "باسم الكربلائي".into(), history: false },
    ]))));
    app.set_query("ta".into());
    let shelf_cards = Rc::new(VecModel::from(vec![card("Night Changes", "One Direction", swatch(30, 30, 30)), card("Royalty", "Egzod", swatch(70, 90, 160))]));
    app.set_home(ModelRc::from(Rc::new(VecModel::from(vec![Shelf { title: "For you".into(), cards: ModelRc::from(shelf_cards.clone()) }, Shelf { title: "حسيني لك".into(), cards: ModelRc::from(shelf_cards) }]))));
    app.set_show_lyrics(page == 9);
    app.set_close_ask(page == 7);
    // Page 8: the lyrics stage.
    app.set_stage(page == 8);
    app.set_line_progress(0.5);
    app.set_sources(ModelRc::from(Rc::new(VecModel::from(vec![SharedString::from("LRCLIB"), SharedString::from("NetEase"), SharedString::from("YouTube Music")]))));
    app.set_source_at(1);
    app.set_lyrics(ModelRc::from(Rc::new(VecModel::from(vec![
        LyricLine { rtl: false, text: "夜に駆ける".into(), tr: "Racing into the night".into(), ro: "Yoru ni kakeru".into(), state: 2, ..Default::default() },
        LyricLine { rtl: false, text: "沈むように溶けてゆくように".into(), tr: "As if sinking, as if melting".into(), ro: "Shizumu yō ni tokete yuku yō ni".into(), state: 1, sung: "沈むように".into(), left: "溶けてゆくように".into() },
        LyricLine { rtl: false, text: "二人だけの空が広がる夜に".into(), tr: "".into(), ro: "".into(), state: 0, ..Default::default() },
        LyricLine { rtl: true, text: "يا حسين يا مظلوم".into(), tr: "".into(), ro: "".into(), state: 0, ..Default::default() },
    ]))));
    ui.show().expect("show");
    window.set_size(slint::PhysicalSize::new(w, h));
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    // Twice: the first frame lays out, the second has the scale in place.
    for _ in 0..3 {
        CLOCK.fetch_add(1000, std::sync::atomic::Ordering::Relaxed);
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, w as usize);
        });
    }
    let mut img = image::RgbaImage::new(w, h);
    for (i, p) in buf.iter().enumerate() {
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba([p.red, p.green, p.blue, 255]));
    }
    img.save(out).expect("png");
    println!("saved {out}");
}
