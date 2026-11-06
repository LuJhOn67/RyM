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
use rymplayer::config::Config;
use rymplayer::i18n::{tr, tr_any};
use rymplayer::likes::Likes;
use rymplayer::lyrics::Lyrics;
use rymplayer::queue::Queue;
use rymplayer::{cookie_path, feed, fmt_time, login, parse_duration, stream};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

const MINI_TITLE: &str = "RyM mini player";
/// Covers asked for at these sizes (pixels): rows, cards, and the big one.
const ROW_PX: u32 = 120;
const CARD_PX: u32 = 200;
const ART_PX: u32 = 600;
/// Covers kept in memory, at most (the oldest go first).
const COVERS_KEPT: usize = 400;
/// Up next rows on the window, at most.
const QUEUE_SHOWN: usize = 60;

enum Ev {
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
    Similar(String, anyhow::Result<Vec<Track>>),
    SameArtist(String, anyhow::Result<Vec<Track>>),
    ArtistFound(String, anyhow::Result<Option<String>>),
    Rated(String, bool, anyhow::Result<()>),
    Downloaded(anyhow::Result<PathBuf>),
    Update(anyhow::Result<Option<rymplayer::updates::Release>>, bool),
    Installed(anyhow::Result<PathBuf>),
    MediaKey(souvlaki::MediaControlEvent),
    Peer(rymplayer::link::Announce, std::net::IpAddr),
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
    toggles: Rc<VecModel<Toggle>>,
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
    crossfade_for: u64,
    resume_at: Option<(String, f32)>,
    radio_pending: bool,
    repeat_one: bool,
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
    link_seen: Option<(String, String)>,
    /// Likes made on the phone come in every few minutes.
    likes_timer: slint::Timer,
}

type Shared = Rc<RefCell<State>>;

thread_local! {
    static STATE: RefCell<Option<Shared>> = const { RefCell::new(None) };
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
        log(&format!("CRASH at {at}: {msg}"));
    }));
    if std::env::args().any(|a| a == "--login") {
        login::run();
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--render") {
        // A picture of the window with sample songs (no window opens): the layout's check.
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "rym-slint.png".into());
        let dark = args.get(i + 2).is_none_or(|t| t != "light");
        let page = args.get(i + 3).and_then(|p| p.parse().ok()).unwrap_or(0);
        render_sample(&out, dark, page);
        return;
    }
    rymplayer::updates::clean_old();
    let config = Config::load();
    rymplayer::i18n::set_arabic(config.arabic);
    rymplayer::fx::set(config.fx());
    stream::use_source(config.source);
    stream::download_best(config.download_best);
    stream::download_parallel(config.download_connections, config.download_piece_mb);

    let ui = AppWindow::new().expect("window");
    let st = build(&ui, config);
    STATE.with(|s| *s.borrow_mut() = Some(st.clone()));
    start(&st);
    let ticker = slint::Timer::default();
    {
        let st = st.clone();
        ticker.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || tick(&st));
    }
    let _ = ui.run();
    save_session(&st);
    rymplayer::stats::flush();
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
        toggles: Rc::new(VecModel::default()),
    }
}

fn build(ui: &AppWindow, config: Config) -> Shared {
    let m = models();
    let app = ui.global::<App>();
    ui.global::<Theme>().set_dark(config.gui_theme != 1);
    app.set_queue(ModelRc::from(m.queue.clone()));
    app.set_similar(ModelRc::from(m.similar.clone()));
    app.set_same_artist(ModelRc::from(m.same.clone()));
    app.set_home(ModelRc::from(m.home.clone()));
    app.set_list(ModelRc::from(m.list.clone()));
    app.set_results(ModelRc::from(m.results.clone()));
    app.set_playlists(ModelRc::from(m.playlists.clone()));
    app.set_suggestions(ModelRc::from(m.suggestions.clone()));
    app.set_lyrics(ModelRc::from(m.lyrics.clone()));
    app.set_toggles(ModelRc::from(m.toggles.clone()));
    app.set_version(rymplayer::VERSION.into());
    app.set_lyrics_size(config.lyrics_font.clamp(14, 48) as i32);
    app.set_crossfade(config.crossfade as i32);
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
        crossfade_for: 0,
        resume_at: None,
        radio_pending: false,
        repeat_one: false,
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
        link_seen: None,
        likes_timer: slint::Timer::default(),
    }));
    if let Some(saved) = session {
        let mut s = st.borrow_mut();
        s.q = saved.queue();
        if let Some(t) = s.q.now() {
            s.resume_at = Some((t.video_id.clone(), saved.position));
        }
    }
    wire(ui, &st);
    fill_toggles(&st);
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
            s.repeat_one = !s.repeat_one;
            s.repeat_one
        };
        with_app(st, |a| a.set_repeat_one(on));
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
    app.on_check_updates(w(|st| check_updates(st, true)));
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
        app.on_set_number(move |id, v| set_number(&st, &id, v));
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
    match std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty()) {
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
    // The window needs to be on screen before Windows' media controls can take its handle.
    let st2 = st.clone();
    slint::Timer::single_shot(Duration::from_millis(1500), move || start_media_controls(&st2));
    {
        let st4 = st.clone();
        st.borrow().likes_timer.start(slint::TimerMode::Repeated, Duration::from_secs(300), move || {
            let api = st4.borrow().api.clone().filter(Api::logged_in);
            if let Some(api) = api {
                post(async move { Ev::Liked(api.playlist_tracks(LIKED_ID.into()).await) });
            }
        });
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
    let (empty_home, loading, signed) = {
        let s = st.borrow();
        (s.home.is_empty(), s.home_loading, s.api.as_ref().is_some_and(Api::logged_in))
    };
    match page {
        3 if empty_home && !loading => refresh_home(st),
        2 if signed => show_playlists(st),
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
    for (model, tracks) in [(&s.m.list, &s.list), (&s.m.results, &s.results)] {
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
    }
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
        post(async move {
            let sized = rymplayer::api::thumb_sized(&url, ART_PX);
            let art = fetch(&sized).await.ok().and_then(|b| {
                let big = decode_square(&b, ART_PX)?;
                let img = image::RgbaImage::from_raw(big.0, big.1, big.2.clone())?;
                let small = image::imageops::resize(&img, 96, 96, image::imageops::FilterType::Triangle);
                let soft = image::imageops::blur(&small, 7.0);
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
    let start = s.q.current.map_or(0, |c| c + 1);
    let rows: Vec<(usize, Track)> = s.q.songs.iter().enumerate().skip(start).take(QUEUE_SHOWN).map(|(i, t)| (i, t.clone())).collect();
    let items: Vec<Song> = rows.iter().map(|(i, t)| song_item(&mut s, format!("q:{i}"), t)).collect();
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
    let tracks = if which == 'l' { s.list.clone() } else { s.results.clone() };
    let items: Vec<Song> = tracks.iter().enumerate().map(|(i, t)| song_item(&mut s, format!("{which}:{i}"), t)).collect();
    if which == 'l' {
        s.m.list.set_vec(items);
    } else {
        s.m.results.set_vec(items);
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
    let mut s = st.borrow_mut();
    let mut cards = vec![
        Card { key: "p:liked".into(), title: tr("Liked songs").into(), sub: format!("{} {}", s.liked_tracks.len(), tr("songs")).into(), cover: Image::default() },
        Card { key: "p:history".into(), title: tr("History").into(), sub: SharedString::new(), cover: Image::default() },
    ];
    // The liked songs' tile shows the newest liked song's cover.
    if let Some(t) = s.liked_tracks.first().cloned() {
        cards[0].cover = cover(&mut s, t.thumb.as_ref(), CARD_PX);
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
    let results: Vec<&Track> = s.results.iter().collect();
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
    for (model, tracks) in [(&s.m.list, &s.list), (&s.m.results, &s.results)] {
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

fn fill_toggles(st: &Shared) {
    let s = st.borrow();
    let c = &s.config;
    let rows = [
        ("dark", "Dark theme (RyM Dusk)", "Off: the light theme (RyM Mist)", c.gui_theme != 1),
        ("arabic", "العربية", "Arabic words in the app (after a restart)", c.arabic),
        ("automix", "Smart Automix", "Songs blend like a DJ mix: on the fade-out, on the beat, without the basses clashing", c.automix),
        ("skip-silence", "Skip silence", "Quiet starts and ends are skipped", c.skip_silence),
        ("normalize", "Even loudness", "Every song about as loud", c.normalize),
        ("translate", "Translate Japanese lyrics", "English under each Japanese line", c.lyrics_translation),
        ("seamless", "Seamless Music", "Music playing on your phone (same account, same Wi-Fi) moves here when this app is idle", c.cross_ears),
        ("auto-update", "Check for updates", "Every 12 hours", c.auto_update_check),
        ("beta", "Beta updates", "Pre-releases too", c.beta_updates),
    ];
    let items: Vec<Toggle> = rows.iter().map(|(id, title, about, on)| Toggle { id: (*id).into(), title: tr_any(title).into(), about: tr_any(about).into(), on: *on }).collect();
    s.m.toggles.set_vec(items);
}

fn set_toggle(st: &Shared, id: &str, on: bool) {
    {
        let mut s = st.borrow_mut();
        let c = &mut s.config;
        match id {
            "dark" => c.gui_theme = if on { 0 } else { 1 },
            "arabic" => c.arabic = on,
            "automix" => c.automix = on,
            "skip-silence" => c.skip_silence = on,
            "normalize" => c.normalize = on,
            "translate" => c.lyrics_translation = on,
            "seamless" => c.cross_ears = on,
            "auto-update" => c.auto_update_check = on,
            "beta" => c.beta_updates = on,
            _ => {}
        }
        rymplayer::fx::set(s.config.fx());
        s.config.save();
        if id == "dark"
            && let Some(ui) = s.ui.upgrade()
        {
            ui.global::<Theme>().set_dark(on);
        }
    }
    if id == "arabic" {
        toast(st, if on { "أعد تشغيل RyM حتى تتغيّر اللغة" } else { "Restart RyM to change the language" });
    }
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

fn set_number(st: &Shared, id: &str, v: i32) {
    if id == "zoom" {
        set_zoom(st, v as f32 / 100.0);
        return;
    }
    let mut s = st.borrow_mut();
    match id {
        "crossfade" => s.config.crossfade = v.clamp(0, 12) as u8,
        "lyrics-size" => s.config.lyrics_font = v.clamp(14, 48) as u8,
        _ => return,
    }
    s.config.save();
    let (fade, size) = (s.config.crossfade as i32, s.config.lyrics_font as i32);
    if let Some(ui) = s.ui.upgrade() {
        let a = ui.global::<App>();
        a.set_crossfade(fade);
        a.set_lyrics_size(size);
    }
}

// ---------------------------------------------------------------- account, home, search

fn sign_out(st: &Shared) {
    let _ = std::fs::remove_file(cookie_path());
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
    let q = q.trim().to_string();
    if q.is_empty() {
        return;
    }
    let Some(api) = st.borrow().api.clone() else { return };
    let id = {
        let mut s = st.borrow_mut();
        s.search_id += 1;
        s.suggestions.clear();
        s.suggest_for = None;
        s.m.suggestions.set_vec(Vec::new());
        s.search_id
    };
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
        _ => {
            if let Some(t) = track_of(st, key) {
                play_with_radio(st, t);
            }
        }
    }
}

fn open_card(st: &Shared, key: &str) {
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
        feed::opened(&t);
        play_with_radio(st, t);
    }
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
    let Some(t) = track_of(st, key) else { return };
    match action {
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

fn play_with_radio(st: &Shared, t: Track) {
    let at = st.borrow_mut().q.start(vec![t], 0);
    play_at(st, at);
    continue_with_radio(st);
}

fn play_at(st: &Shared, i: usize) {
    let (track, id, api) = {
        let mut s = st.borrow_mut();
        let Some(track) = s.q.songs.get(i).cloned() else { return };
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
        (track, s.request_id, s.api.clone())
    };
    with_app(st, |a| {
        a.set_loading(true);
        a.set_position(0.0);
        a.set_duration(parse_duration(&track.duration).unwrap_or(0.0));
        a.set_pos_text("0:00".into());
        a.set_lyrics_note(tr("Looking for lyrics…").into());
        a.set_lyric_line(-1);
    });
    show_now(st);
    render_queue(st);
    mark_playing(st);
    save_session(st);
    log(&format!("play {} ({})", track.title, track.video_id));
    let vid = track.video_id.clone();
    post(async move { Ev::Fetched(id, stream::open(vid).await) });
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
    let (current, len) = {
        let s = st.borrow();
        (s.q.current, s.q.songs.len())
    };
    match current {
        Some(i) if i + 1 < len => play_at(st, i + 1),
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
            rymplayer::discord::show(&s.config.discord_app_id, rymplayer::discord::Show::Song { title: t.title.clone(), artist: t.artist.clone(), cover, at: pos, length: dur, paused: !playing });
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
    let rows: Vec<LyricLine> = lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| LyricLine { text: text.into(), tr: l.translation.get(i).cloned().unwrap_or_default().into(), state: if l.synced.is_empty() { 1 } else { 0 } })
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
    let dark = ui.global::<Theme>().get_dark();
    if mini.global::<Theme>().get_dark() != dark {
        mini.global::<Theme>().set_dark(dark);
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
        with_app(st, |a| a.set_update_text(tr("Checking…").into()));
    }
    post(async move { Ev::Update(rymplayer::updates::check(beta).await, asked) });
}

/// Seamless Music: a phone of the same account plays while this app is idle: the music moves here.
fn heard_peer(st: &Shared, note: rymplayer::link::Announce, ip: std::net::IpAddr) {
    let (key, idle) = {
        let s = st.borrow();
        if !s.config.cross_ears {
            return;
        }
        let key = s.account.as_ref().map(|a| rymplayer::link::account_key(&a.name, &a.handle));
        (key, !s.loading && !s.audio.as_ref().is_some_and(|a| a.is_playing()))
    };
    let Some(key) = key else { return };
    if note.kind != "phone" || note.account != key || !note.playing || note.video_id.is_empty() || !idle {
        return;
    }
    let seen = (note.device.clone(), note.video_id.clone());
    if st.borrow().link_seen.as_ref() == Some(&seen) {
        return;
    }
    st.borrow_mut().link_seen = Some(seen);
    let (device, port) = (note.device.clone(), note.port);
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
            let now = st.borrow().q.now().cloned();
            if let Some(t) = now {
                similar_and_artist(st, &t);
            }
        }
        Ev::Auth(new_login, Err(e)) => {
            if new_login {
                toast(st, &format!("{}: {e}", tr("Sign-in failed")));
            } else if st.borrow().api.is_none() {
                if std::fs::metadata(cookie_path()).is_ok() {
                    toast(st, tr("Your saved sign-in ended: continuing as a guest"));
                    let _ = std::fs::remove_file(cookie_path());
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
            st.borrow_mut().account = Some(a);
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
                    st.borrow_mut().results = p.tracks.into_iter().filter(|t| t.link.is_none()).collect();
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
            if current && let Some((big, soft)) = art {
                let (big, soft) = (to_image(big), to_image(soft));
                with_app(st, |a| {
                    a.set_cover(big);
                    a.set_backdrop(soft);
                });
            }
        }
        Ev::Fetched(id, result) => {
            let mut s = st.borrow_mut();
            if id != s.request_id {
                return;
            }
            s.loading = false;
            let Some(t) = s.q.now().cloned() else { return };
            let known = parse_duration(&t.duration).or_else(|| stream::cached_duration(&t.video_id));
            let resume = s.resume_at.take().filter(|(id, at)| *id == t.video_id && *at > 1.0).map(|(_, at)| at);
            let crossfade = std::mem::take(&mut s.crossfade_next);
            let automix = s.config.automix;
            let secs = if automix { rymplayer::automix::mix_secs(s.config.crossfade) } else { s.config.crossfade as f32 };
            let started = match (result, &mut s.audio) {
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
                Ok(()) => prefetch_next(st),
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
                let url = r.download_for("RyM-Slint.exe");
                with_app(st, |a| a.set_update_text(format!("RyM {} — {}", r.version, tr("downloading…")).into()));
                toast(st, &format!("RyM {} — {}", r.version, tr("downloading…")));
                if url.ends_with(".exe") {
                    post(async move { Ev::Installed(rymplayer::updates::install(&url, |_| {}).await) });
                } else {
                    let _ = rymplayer::updates::open_link(&url);
                }
            }
            Ok(None) if asked => with_app(st, |a| a.set_update_text(tr("You have the latest version").into())),
            Err(e) if asked => with_app(st, |a| a.set_update_text(format!("{}: {e}", tr("Couldn't check for updates")).into())),
            _ => {}
        },
        Ev::Installed(Ok(path)) => {
            with_app(st, |a| a.set_update_text(tr("Updated: RyM opens again").into()));
            save_session(st);
            if std::process::Command::new(&path).spawn().is_ok() {
                let _ = slint::quit_event_loop();
            }
        }
        Ev::Installed(Err(e)) => with_app(st, |a| a.set_update_text(format!("{}: {e}", tr("Update failed")).into())),
        Ev::MediaKey(k) => media_key(st, k),
        Ev::Peer(note, ip) => heard_peer(st, note, ip),
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
fn render_sample(out: &str, dark: bool, page: i32) {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl slint::platform::Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let (w, h) = (1600u32, 1024u32);
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let _ = slint::platform::set_platform(Box::new(Headless(window.clone())));
    let ui = AppWindow::new().expect("window");
    let app = ui.global::<App>();
    ui.global::<Theme>().set_dark(dark);
    app.on_tr(|s| tr_any(&s).into());
    let swatch = |r: u8, g: u8, b: u8| {
        let px: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                let k = (i % 64) as u8;
                [r.saturating_add(k), g, b.saturating_add(k / 2), 255]
            })
            .collect();
        to_image((64, 64, px))
    };
    app.set_has_song(true);
    app.set_title("Seasons".into());
    app.set_artist("Rival x Cadmium, feat. Harley Bird".into());
    app.set_cover(swatch(200, 60, 90));
    app.set_backdrop(swatch(150, 80, 110));
    app.set_liked(true);
    app.set_playing(true);
    app.set_duration(205.0);
    app.set_position(62.0);
    app.set_pos_text("1:02".into());
    app.set_len_text("3:25".into());
    app.set_volume(0.5);
    app.set_wave(ModelRc::from(Rc::new(VecModel::from(wave_for("seasons")))));
    app.set_artist_name("Rival".into());
    let song = |t: &str, a: &str, c: Image| Song { key: "q:1".into(), title: t.into(), artist: a.into(), cover: c, liked: false, playing: false };
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
    app.set_toggles(ModelRc::from(Rc::new(VecModel::from(vec![
        Toggle { id: "dark".into(), title: "Dark theme (RyM Dusk)".into(), about: "Off: the light theme (RyM Mist)".into(), on: dark },
        Toggle { id: "automix".into(), title: "Smart Automix".into(), about: "Songs blend like a DJ mix".into(), on: true },
    ]))));
    app.set_suggestions(ModelRc::from(Rc::new(VecModel::from(vec![
        Suggest { text: "tame impala".into(), history: true },
        Suggest { text: "باسم الكربلائي".into(), history: false },
    ]))));
    app.set_query("ta".into());
    let shelf_cards = Rc::new(VecModel::from(vec![card("Night Changes", "One Direction", swatch(30, 30, 30)), card("Royalty", "Egzod", swatch(70, 90, 160))]));
    app.set_home(ModelRc::from(Rc::new(VecModel::from(vec![Shelf { title: "For you".into(), cards: ModelRc::from(shelf_cards.clone()) }, Shelf { title: "حسيني لك".into(), cards: ModelRc::from(shelf_cards) }]))));
    app.set_show_lyrics(page == 9);
    app.set_lyrics(ModelRc::from(Rc::new(VecModel::from(vec![
        LyricLine { text: "夜に駆ける".into(), tr: "Racing into the night".into(), state: 2 },
        LyricLine { text: "沈むように溶けてゆくように".into(), tr: "As if sinking, as if melting".into(), state: 1 },
        LyricLine { text: "二人だけの空が広がる夜に".into(), tr: "".into(), state: 0 },
    ]))));
    ui.show().expect("show");
    window.set_size(slint::PhysicalSize::new(w, h));
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    // Twice: the first frame lays out, the second has the scale in place.
    for _ in 0..2 {
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
