#![windows_subsystem = "windows"]
//! The GTK 4 window app: the same player as the iced one (home feed with "New for you", liked
//! songs kept in step with the phone, search, playlists, synced lyrics), drawn with GTK so it
//! fits Linux desktops. Build: `cargo build --release --features gtk-ui --bin RymPlayer-GTK`.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Button, ComboBoxText, CssProvider, HeaderBar, Label, Orientation, Scale};
use rymplayer::api::{Api, LIKED_ID, Link, Track};
use rymplayer::audio::Audio;
use rymplayer::config::Config;
use rymplayer::likes::{Likes, SYNC_EVERY};
use rymplayer::lyrics::Lyrics;
use rymplayer::{cookie_path, feed, fmt_time, login, page_cache, parse_duration, stream};

const APP_ID: &str = "com.rymplayer.GtkPlayer";

const STYLE: &str = r#"
window { background: #101312; color: #edf4f0; }
.rym-shell { padding: 18px; background: #101312; }
.topbar { background: #171c1a; border: 1px solid #29332e; border-radius: 16px; padding: 12px; }
.playerbar, .statusbar { background: @STATUS@; border: 1px solid @ACCENT_DARK@; border-radius: 16px; padding: 12px; }
.statusbar { color: #edf4f0; padding: 9px 12px; }
.rym-titlebar { background: #101312; color: #edf4f0; padding: 4px 10px; }
.window-control { color: #ffffff; background: #000000; border: 1px solid #303030; border-radius: 4px; min-width: 34px; min-height: 30px; padding: 0; font-size: 18px; font-weight: 900; }
.window-control:hover { color: #ffffff; background: #202020; border-color: #606060; }
.theme-picker { color: #edf4f0; background: #202724; border: 1px solid #35423b; border-radius: 10px; padding: 5px 9px; }
.brand { color: #e9fff1; font-size: 26px; font-weight: 900; }
.muted { color: #9daaa3; }
.section-title { color: #edf4f0; font-size: 24px; font-weight: 800; padding: 5px 2px; }
.shelf-title { color: @ACCENT@; font-size: 15px; font-weight: 800; padding: 12px 2px 2px 2px; }
.card { background: #171c1a; border: 1px solid #29332e; border-radius: 18px; padding: 16px; }
entry { min-height: 40px; color: #edf4f0; background: #202724; border: 1px solid #35423b; border-radius: 12px; padding: 0 12px; }
button { color: #e8f2ec; background: #252d29; border: 1px solid #35423b; border-radius: 12px; padding: 9px 14px; }
button:hover { background: #303b35; border-color: #607267; }
button.accent { color: #101312; background: @ACCENT@; border-color: @ACCENT@; font-weight: 800; }
button.accent:hover { background: @ACCENT_HOVER@; }
button.heart { color: #9daaa3; min-width: 36px; padding: 6px 10px; }
button.heart.on { color: @ACCENT@; border-color: @ACCENT@; }
button.track-row { background: #191f1c; border-color: transparent; padding: 10px 12px; }
button.track-row:hover { background: #242d28; }
button.track-row.selected { background: @STATUS@; border-color: @ACCENT@; }
.track-title { color: #f1f6f2; font-size: 14px; font-weight: 700; }
.track-subtitle { color: #9eaaa2; font-size: 12px; }
.row-heart { color: @ACCENT@; font-size: 14px; }
.lyrics-heading { color: @ACCENT@; font-size: 13px; font-weight: 800; letter-spacing: 1px; }
.lyric-line { color: #9daaa3; font-size: 17px; padding: 7px 9px; border-radius: 9px; transition: color 180ms ease, background-color 180ms ease; }
.lyric-current { color: #ffffff; font-weight: 800; background: @STATUS@; }
.current-title { color: #f1f6f2; font-weight: 800; }
.current-artist { color: #9eaaa2; font-size: 12px; }
scale trough { min-height: 6px; background: #303a34; border-radius: 8px; }
scale highlight { background: @ACCENT@; border-radius: 8px; }
scale slider { min-width: 12px; min-height: 12px; background: #d5ffe1; border: 1px solid #8bd7a5; }
scrollbar slider { background: #4b5c52; border-radius: 8px; }
"#;

#[derive(Clone, Copy)]
enum AuthMode {
    Saved,
    Guest,
    NewLogin,
}

/// What the list shows.
#[derive(Clone, PartialEq)]
enum Page {
    Home,
    Liked,
    Other,
}

enum WorkerEvent {
    Auth(AuthMode, anyhow::Result<Api>),
    Tracks(Page, String, anyhow::Result<Vec<Track>>),
    Home(anyhow::Result<feed::Refreshed>),
    Liked(anyhow::Result<Vec<Track>>),
    /// The song to play: from the cache, or still arriving (it plays while it downloads).
    Fetched(u64, Track, anyhow::Result<stream::Playable>),
    Lyrics(String, Option<Lyrics>),
    Rated(String, bool, anyhow::Result<()>),
    Radio(anyhow::Result<Vec<Track>>),
    Done,
}

struct Ui {
    runtime: tokio::runtime::Runtime,
    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    api: Option<Api>,
    page: Page,
    /// The list on screen.
    tracks: Vec<Track>,
    /// What plays, and where in it.
    queue: Vec<Track>,
    current: Option<usize>,
    request_id: u64,
    audio: Option<Audio>,
    loading: bool,
    radio_pending: bool,
    lyrics: Option<Lyrics>,
    active_lyric: Option<usize>,
    liked: Likes,
    likes_synced: Instant,
    list: gtk::Box,
    rows: Vec<(Button, Label)>,
    lyrics_list: gtk::Box,
    lyrics_scroll: gtk::ScrolledWindow,
    page_title: Label,
    status: Label,
    sign_in: Button,
    title: Label,
    artist: Label,
    heart: Button,
    play: Button,
    progress: Scale,
    time: Label,
    provider: CssProvider,
    last_volume: f64,
    syncing_progress: bool,
}

#[derive(Clone, Copy)]
struct AppTheme {
    name: &'static str,
    accent: &'static str,
    accent_hover: &'static str,
    status: &'static str,
    accent_dark: &'static str,
}

const THEMES: &[AppTheme] = &[
    AppTheme { name: "Mint", accent: "#a8f0c2", accent_hover: "#c4f7d5", status: "#24382c", accent_dark: "#385541" },
    AppTheme { name: "Sky", accent: "#9bcaff", accent_hover: "#c2ddff", status: "#202f45", accent_dark: "#3b5574" },
    AppTheme { name: "Sakura", accent: "#ffc1d8", accent_hover: "#ffdae8", status: "#49283b", accent_dark: "#70445f" },
    AppTheme { name: "Midnight", accent: "#c7b0ff", accent_hover: "#ddceff", status: "#30264a", accent_dark: "#514277" },
    AppTheme { name: "Red", accent: "#ff7777", accent_hover: "#ff9a9a", status: "#482626", accent_dark: "#743d3d" },
    AppTheme { name: "Green", accent: "#1ed760", accent_hover: "#61e78d", status: "#203b2b", accent_dark: "#2d6341" },
];

fn main() {
    configure_gtk_runtime();
    if std::env::args().any(|arg| arg == "--login") {
        login::run();
    }
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    app.run();
}

/// Keep a portable GTK runtime beside the executable instead of requiring a system install.
fn configure_gtk_runtime() {
    #[cfg(windows)]
    if let Some(root) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) {
        let bin = root.join("bin");
        if bin.is_dir() {
            let old_path = std::env::var_os("PATH").unwrap_or_default();
            let path = format!("{};{}", bin.display(), old_path.to_string_lossy());
            // SAFETY: this runs before GTK or Tokio creates any threads.
            unsafe {
                std::env::set_var("PATH", path);
                std::env::set_var("GTK_DATA_PREFIX", &root);
                std::env::set_var("GTK_EXE_PREFIX", &root);
                std::env::set_var("GSETTINGS_SCHEMA_DIR", root.join("share/glib-2.0/schemas"));
                std::env::set_var("FONTCONFIG_PATH", root.join("etc/fonts"));
            }
        }
    }
}

fn theme_style(index: usize) -> String {
    let theme = THEMES[index.min(THEMES.len() - 1)];
    STYLE
        .replace("@ACCENT@", theme.accent)
        .replace("@ACCENT_HOVER@", theme.accent_hover)
        .replace("@STATUS@", theme.status)
        .replace("@ACCENT_DARK@", theme.accent_dark)
}

fn install_style() -> CssProvider {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&theme_style(0));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    provider
}

fn label(text: &str, class: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.add_css_class(class);
    l
}

fn build_window(app: &Application) {
    let provider = install_style();
    let config = Config::load();
    stream::use_source(config.source);
    stream::download_best(config.download_best);
    stream::download_parallel(config.download_connections, config.download_piece_mb);

    let (tx, rx) = mpsc::channel();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("create async runtime");

    let list = gtk::Box::new(Orientation::Vertical, 7);
    let lyrics_list = gtk::Box::new(Orientation::Vertical, 5);
    let lyrics_scroll = gtk::ScrolledWindow::new();
    lyrics_scroll.set_vexpand(true);
    lyrics_scroll.set_child(Some(&lyrics_list));

    let status = label("Starting RyM…", "statusbar");
    status.set_hexpand(true);

    let sign_in = Button::with_label("Sign in with Google");
    let title = label("Nothing playing", "current-title");
    let artist = label("Choose a song to start", "current-artist");
    let heart = Button::with_label("♡");
    heart.add_css_class("heart");
    heart.set_tooltip_text(Some("Like (synced with your account)"));
    let play = Button::with_label("▶");
    play.add_css_class("accent");
    let previous = Button::with_label("⏮");
    let next = Button::with_label("⏭");
    let progress = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 0.1);
    progress.set_draw_value(false);
    progress.set_hexpand(true);
    let time = Label::new(Some("0:00 / 0:00"));
    time.add_css_class("muted");
    let volume = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    volume.set_draw_value(false);
    volume.set_value(0.8);
    volume.set_size_request(110, -1);
    let volume_button = Button::with_label("♪");
    volume_button.add_css_class("window-control");
    volume_button.set_tooltip_text(Some("Mute / unmute"));

    let audio = match Audio::new() {
        Ok(player) => {
            player.set_volume(0.8);
            status.set_text("Connecting to YouTube Music…");
            Some(player)
        }
        Err(err) => {
            status.set_text(&format!("No audio device: {err}"));
            None
        }
    };

    let page_title = Label::new(Some("For you"));
    page_title.set_xalign(0.0);
    page_title.add_css_class("section-title");

    let ui = Rc::new(RefCell::new(Ui {
        runtime,
        tx,
        rx,
        api: None,
        page: Page::Home,
        tracks: Vec::new(),
        queue: Vec::new(),
        current: None,
        request_id: 0,
        audio,
        loading: false,
        radio_pending: false,
        lyrics: None,
        active_lyric: None,
        liked: Likes::default(),
        likes_synced: Instant::now(),
        list: list.clone(),
        rows: Vec::new(),
        lyrics_list: lyrics_list.clone(),
        lyrics_scroll: lyrics_scroll.clone(),
        page_title: page_title.clone(),
        status: status.clone(),
        sign_in: sign_in.clone(),
        title: title.clone(),
        artist: artist.clone(),
        heart: heart.clone(),
        play: play.clone(),
        progress: progress.clone(),
        time: time.clone(),
        provider,
        last_volume: 0.8,
        syncing_progress: false,
    }));

    let brand = Label::new(Some("RyM"));
    brand.add_css_class("brand");
    let tagline = Label::new(Some("MUSIC, YOUR WAY"));
    tagline.add_css_class("muted");
    let brand_stack = gtk::Box::new(Orientation::Vertical, 0);
    brand_stack.append(&brand);
    brand_stack.append(&tagline);

    let search = gtk::Entry::new();
    search.set_placeholder_text(Some("Search songs, artists, albums…"));
    search.set_hexpand(true);
    let search_button = Button::with_label("Search");
    search_button.add_css_class("accent");
    let home_button = Button::with_label("For you");
    let refresh_button = Button::with_label("⟳");
    refresh_button.set_tooltip_text(Some("Refresh: new songs on top (F5)"));
    let liked_button = Button::with_label("♥ Liked");
    let theme_picker = ComboBoxText::new();
    theme_picker.add_css_class("theme-picker");
    for theme in THEMES {
        theme_picker.append_text(theme.name);
    }
    theme_picker.set_active(Some(0));
    let topbar = gtk::Box::new(Orientation::Horizontal, 12);
    topbar.add_css_class("topbar");
    topbar.append(&brand_stack);
    topbar.append(&home_button);
    topbar.append(&refresh_button);
    topbar.append(&liked_button);
    topbar.append(&search);
    topbar.append(&search_button);
    topbar.append(&theme_picker);
    topbar.append(&sign_in);

    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_min_content_height(260);
    list_scroll.set_vexpand(true);
    list_scroll.set_child(Some(&list));
    let tracks_card = gtk::Box::new(Orientation::Vertical, 10);
    tracks_card.add_css_class("card");
    tracks_card.set_hexpand(true);
    tracks_card.set_vexpand(true);
    tracks_card.append(&page_title);
    tracks_card.append(&list_scroll);

    let lyric_heading = Label::new(Some("LYRICS"));
    lyric_heading.add_css_class("lyrics-heading");
    lyric_heading.set_xalign(0.0);
    lyrics_list.append(&Label::new(Some("Play a song to find its lyrics")));
    let lyric_card = gtk::Box::new(Orientation::Vertical, 12);
    lyric_card.add_css_class("card");
    lyric_card.set_size_request(310, -1);
    lyric_card.append(&lyric_heading);
    lyric_card.append(&lyrics_scroll);

    let body = gtk::Paned::new(Orientation::Horizontal);
    body.set_wide_handle(true);
    body.set_position(730);
    body.set_start_child(Some(&tracks_card));
    body.set_end_child(Some(&lyric_card));
    body.set_vexpand(true);

    let now_playing = gtk::Box::new(Orientation::Vertical, 3);
    now_playing.set_size_request(190, -1);
    now_playing.append(&title);
    now_playing.append(&artist);
    let playerbar = gtk::Box::new(Orientation::Horizontal, 12);
    playerbar.add_css_class("playerbar");
    playerbar.append(&now_playing);
    playerbar.append(&heart);
    playerbar.append(&previous);
    playerbar.append(&play);
    playerbar.append(&next);
    playerbar.append(&progress);
    playerbar.append(&time);
    playerbar.append(&Label::new(Some("VOL")));
    playerbar.append(&volume);

    let root = gtk::Box::new(Orientation::Vertical, 12);
    root.add_css_class("rym-shell");
    root.append(&topbar);
    root.append(&body);
    root.append(&playerbar);
    root.append(&status);

    let window = ApplicationWindow::builder().application(app).title("RyM").default_width(1180).default_height(760).build();
    let titlebar = HeaderBar::new();
    titlebar.add_css_class("rym-titlebar");
    titlebar.set_show_title_buttons(false);
    titlebar.set_title_widget(Some(&Label::new(Some("RyM"))));
    let window_controls = gtk::Box::new(Orientation::Horizontal, 6);
    let hide_button = Button::with_label("—");
    hide_button.add_css_class("window-control");
    hide_button.set_tooltip_text(Some("Hide to taskbar"));
    let close_button = Button::with_label("×");
    close_button.add_css_class("window-control");
    close_button.set_tooltip_text(Some("Exit RyM"));
    window_controls.append(&hide_button);
    window_controls.append(&volume_button);
    window_controls.append(&close_button);
    titlebar.pack_end(&window_controls);
    window.set_titlebar(Some(&titlebar));
    window.set_child(Some(&root));

    {
        let ui = ui.clone();
        let search = search.clone();
        search_button.connect_clicked(move |_| search_tracks(&ui, search.text().to_string()));
    }
    {
        let ui = ui.clone();
        search.connect_activate(move |entry| search_tracks(&ui, entry.text().to_string()));
    }
    {
        let ui = ui.clone();
        home_button.connect_clicked(move |_| load_home(&ui));
    }
    {
        let ui = ui.clone();
        refresh_button.connect_clicked(move |_| load_home(&ui));
    }
    {
        let ui = ui.clone();
        liked_button.connect_clicked(move |_| open_liked(&ui));
    }
    {
        // F5 refreshes, like a browser.
        let ui = ui.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::F5 {
                load_home(&ui);
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        window.add_controller(keys);
    }
    {
        let ui = ui.clone();
        sign_in.connect_clicked(move |_| sign_in_or_out(&ui));
    }
    {
        let ui = ui.clone();
        theme_picker.connect_changed(move |picker| {
            let index = picker.active().unwrap_or(0) as usize;
            if let Ok(state) = ui.try_borrow_mut() {
                state.provider.load_from_data(&theme_style(index));
            }
        });
    }
    {
        let window = window.clone();
        hide_button.connect_clicked(move |_| window.minimize());
    }
    {
        let window = window.clone();
        close_button.connect_clicked(move |_| window.close());
    }
    {
        let ui = ui.clone();
        let volume = volume.clone();
        let clicked_button = volume_button.clone();
        volume_button.connect_clicked(move |_| {
            let current = volume.value();
            let target = {
                let mut state = ui.borrow_mut();
                if current > 0.001 {
                    state.last_volume = current;
                    0.0
                } else {
                    state.last_volume.max(0.2)
                }
            };
            volume.set_value(target);
            clicked_button.set_label(if target <= 0.001 { "♪̸" } else { "♪" });
        });
    }
    {
        let ui = ui.clone();
        heart.connect_clicked(move |_| toggle_like(&ui));
    }
    {
        let ui = ui.clone();
        play.connect_clicked(move |_| toggle_playback(&ui));
    }
    {
        let ui = ui.clone();
        previous.connect_clicked(move |_| skip_track(&ui, false));
    }
    {
        let ui = ui.clone();
        next.connect_clicked(move |_| skip_track(&ui, true));
    }
    {
        let ui = ui.clone();
        progress.connect_value_changed(move |scale| {
            let ui = ui.borrow();
            if !ui.syncing_progress
                && let Some(audio) = &ui.audio
            {
                audio.seek(scale.value() as f32);
            }
        });
    }
    {
        let ui = ui.clone();
        let volume_button = volume_button.clone();
        volume.connect_value_changed(move |scale| {
            if let Some(audio) = &ui.borrow().audio {
                audio.set_volume(scale.value() as f32);
            }
            volume_button.set_label(if scale.value() <= 0.001 { "♪̸" } else { "♪" });
        });
    }

    let poll = ui.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        poll_events(&poll);
        gtk::glib::ControlFlow::Continue
    });

    if let Some(cookie) = std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty()) {
        // The last home feed shows at once; the fresh one replaces it.
        let saved = page_cache::load_home();
        if !saved.is_empty() {
            rebuild_tracks(&ui, Page::Home, "For you", saved);
        }
        set_status(&ui, "Checking saved Google sign-in…");
        post(&ui, async move { WorkerEvent::Auth(AuthMode::Saved, Api::with_cookie(cookie).await) });
    } else {
        post(&ui, async { WorkerEvent::Auth(AuthMode::Guest, Api::guest().await) });
    }

    window.present();
}

fn post(ui: &Rc<RefCell<Ui>>, task: impl Future<Output = WorkerEvent> + Send + 'static) {
    let (handle, tx) = {
        let ui = ui.borrow();
        (ui.runtime.handle().clone(), ui.tx.clone())
    };
    handle.spawn(async move {
        let _ = tx.send(task.await);
    });
}

fn set_status(ui: &Rc<RefCell<Ui>>, message: &str) {
    ui.borrow().status.set_text(message);
}

fn ui_api(ui: &Rc<RefCell<Ui>>) -> Option<Api> {
    ui.borrow().api.clone()
}

fn search_tracks(ui: &Rc<RefCell<Ui>>, query: String) {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return;
    }
    let Some(api) = ui_api(ui) else {
        set_status(ui, "YouTube Music is still connecting. Try again in a moment.");
        return;
    };
    set_status(ui, "Searching…");
    post(ui, async move { WorkerEvent::Tracks(Page::Other, format!("Results for \"{query}\""), api.search(query).await) });
}

/// A fresh home feed: new songs on top, and the liked songs read on the way.
fn load_home(ui: &Rc<RefCell<Ui>>) {
    let Some(api) = ui_api(ui) else {
        set_status(ui, "YouTube Music is still connecting.");
        return;
    };
    set_status(ui, "Refreshing your feed…");
    post(ui, async move { WorkerEvent::Home(feed::refresh(api).await) });
}

fn open_liked(ui: &Rc<RefCell<Ui>>) {
    let Some(api) = ui_api(ui).filter(Api::logged_in) else {
        set_status(ui, "Sign in to see your liked songs.");
        return;
    };
    set_status(ui, "Loading liked songs…");
    post(ui, async move { WorkerEvent::Tracks(Page::Liked, "Liked songs".into(), api.playlist_tracks(LIKED_ID.into()).await) });
}

fn sync_likes(ui: &Rc<RefCell<Ui>>) {
    let Some(api) = ui_api(ui).filter(Api::logged_in) else { return };
    ui.borrow_mut().likes_synced = Instant::now();
    post(ui, async move { WorkerEvent::Liked(api.playlist_tracks(LIKED_ID.into()).await) });
}

fn sign_in_or_out(ui: &Rc<RefCell<Ui>>) {
    let logged_in = ui_api(ui).is_some_and(|a| a.logged_in());
    if logged_in {
        let _ = std::fs::remove_file(cookie_path());
        page_cache::clear();
        {
            let mut state = ui.borrow_mut();
            if let Some(audio) = &mut state.audio {
                audio.stop();
            }
            state.liked.clear();
            state.sign_in.set_label("Sign in with Google");
        }
        post(ui, async { WorkerEvent::Auth(AuthMode::Guest, Api::guest().await) });
        set_status(ui, "Signed out. Loading guest recommendations…");
        return;
    }
    set_status(ui, "Complete sign-in in the Google window…");
    post(ui, async {
        let result = async {
            let cookie = login::sign_in().await?;
            Api::with_cookie(cookie).await
        }
        .await;
        WorkerEvent::Auth(AuthMode::NewLogin, result)
    });
}

fn poll_events(ui: &Rc<RefCell<Ui>>) {
    loop {
        let event = ui.borrow().rx.try_recv();
        let Ok(event) = event else { break };
        match event {
            WorkerEvent::Auth(mode, Ok(api)) => {
                let is_user = api.logged_in();
                ui.borrow().sign_in.set_label(if is_user { "Sign out" } else { "Sign in with Google" });
                ui.borrow_mut().api = Some(api);
                match mode {
                    AuthMode::NewLogin => set_status(ui, "Signed in to YouTube Music."),
                    AuthMode::Saved => set_status(ui, "Signed in with your saved Google session."),
                    AuthMode::Guest => set_status(ui, "Connected in guest mode."),
                }
                load_home(ui);
            }
            WorkerEvent::Auth(mode, Err(error)) => match mode {
                AuthMode::Saved => {
                    set_status(ui, &format!("Saved sign-in failed: {error}. Trying guest mode…"));
                    post(ui, async { WorkerEvent::Auth(AuthMode::Guest, Api::guest().await) });
                }
                AuthMode::NewLogin => set_status(ui, &format!("Google sign-in failed: {error}")),
                AuthMode::Guest => set_status(ui, &format!("Couldn't connect to YouTube Music: {error}")),
            },
            WorkerEvent::Home(Ok(fresh)) => {
                if let Some(liked) = fresh.liked {
                    sync_liked_list(ui, liked);
                }
                page_cache::save_home(&fresh.home);
                let count = fresh.home.len();
                rebuild_tracks(ui, Page::Home, "For you", fresh.home);
                set_status(ui, &format!("For you · {count} items"));
            }
            WorkerEvent::Home(Err(error)) => set_status(ui, &format!("For you: {error}")),
            WorkerEvent::Tracks(page, title, Ok(tracks)) => {
                if page == Page::Liked {
                    sync_liked_list(ui, tracks.clone());
                }
                let count = tracks.len();
                rebuild_tracks(ui, page, &title, tracks);
                set_status(ui, &format!("{title} · {count} songs"));
            }
            WorkerEvent::Tracks(_, title, Err(error)) => set_status(ui, &format!("{title}: {error}")),
            WorkerEvent::Liked(Ok(tracks)) => {
                let on_liked_page = ui.borrow().page == Page::Liked;
                sync_liked_list(ui, tracks.clone());
                if on_liked_page {
                    rebuild_tracks(ui, Page::Liked, "Liked songs", tracks);
                }
            }
            WorkerEvent::Liked(Err(_)) => {}
            WorkerEvent::Fetched(request_id, track, Ok(song)) => {
                let mut state = ui.borrow_mut();
                if request_id != state.request_id {
                    continue;
                }
                state.loading = false;
                let known = parse_duration(&track.duration).or_else(|| stream::cached_duration(&track.video_id));
                let started = match &mut state.audio {
                    Some(audio) => audio.play(&song, known).map_err(|e| e.to_string()),
                    None => Err("No audio output device is available.".into()),
                };
                match started {
                    Ok(()) => {
                        state.play.set_label("❚❚");
                        state.status.set_text("Playing");
                    }
                    Err(e) => {
                        state.status.set_text(&format!("Can't play: {e}"));
                        state.play.set_label("▶");
                    }
                }
                drop(state);
                prefetch_next(ui);
                if let Some(api) = ui_api(ui) {
                    let video_id = track.video_id.clone();
                    post(ui, async move {
                        let _ = api.report_play(video_id).await;
                        WorkerEvent::Done
                    });
                }
            }
            WorkerEvent::Fetched(request_id, _track, Err(error)) => {
                if request_id == ui.borrow().request_id {
                    ui.borrow_mut().loading = false;
                    set_status(ui, &format!("Couldn't start this song: {error}"));
                    ui.borrow().play.set_label("▶");
                }
            }
            WorkerEvent::Lyrics(video_id, lyrics) => {
                let current_id = playing(ui).map(|t| t.video_id);
                if current_id.as_deref() == Some(video_id.as_str()) {
                    show_lyrics(ui, lyrics);
                }
            }
            WorkerEvent::Rated(video_id, like, Err(error)) => {
                // Take back the heart that was shown right away.
                ui.borrow_mut().liked.set(&video_id, !like);
                refresh_hearts(ui);
                set_status(ui, &format!("Like failed: {error}"));
            }
            WorkerEvent::Rated(..) => {}
            WorkerEvent::Radio(result) => {
                ui.borrow_mut().radio_pending = false;
                match result {
                    Ok(tracks) => {
                        let next = {
                            let mut state = ui.borrow_mut();
                            let fresh: Vec<Track> =
                                tracks.into_iter().filter(|t| !state.queue.iter().any(|q| q.video_id == t.video_id)).collect();
                            let next = (!fresh.is_empty()).then_some(state.queue.len());
                            state.queue.extend(fresh);
                            next
                        };
                        match next {
                            Some(i) => play_queue(ui, i),
                            None => set_status(ui, "Radio has nothing new to play."),
                        }
                    }
                    Err(error) => set_status(ui, &format!("Radio: {error}")),
                }
            }
            WorkerEvent::Done => {}
        }
    }
    let due = {
        let state = ui.borrow();
        state.api.as_ref().is_some_and(Api::logged_in) && state.likes_synced.elapsed() > SYNC_EVERY
    };
    if due {
        // Likes made on the phone show here too.
        sync_likes(ui);
    }
    tick(ui);
}

fn sync_liked_list(ui: &Rc<RefCell<Ui>>, tracks: Vec<Track>) {
    {
        let mut state = ui.borrow_mut();
        state.liked.sync(tracks.into_iter().map(|t| t.video_id));
        state.likes_synced = Instant::now();
    }
    refresh_hearts(ui);
}

fn playing(ui: &Rc<RefCell<Ui>>) -> Option<Track> {
    let state = ui.borrow();
    state.current.and_then(|i| state.queue.get(i).cloned())
}

/// Hearts on the rows and the player bar, after the liked songs changed.
fn refresh_hearts(ui: &Rc<RefCell<Ui>>) {
    let state = ui.borrow();
    for (track, (_, heart)) in state.tracks.iter().zip(&state.rows) {
        heart.set_visible(state.liked.contains(&track.video_id));
    }
    let on = state.current.and_then(|i| state.queue.get(i)).is_some_and(|t| state.liked.contains(&t.video_id));
    state.heart.set_label(if on { "♥" } else { "♡" });
    if on {
        state.heart.add_css_class("on");
    } else {
        state.heart.remove_css_class("on");
    }
}

fn toggle_like(ui: &Rc<RefCell<Ui>>) {
    let Some(track) = playing(ui) else { return };
    let Some(api) = ui_api(ui).filter(Api::logged_in) else {
        set_status(ui, "Sign in to like songs.");
        return;
    };
    let vid = track.video_id.clone();
    let like = {
        let mut state = ui.borrow_mut();
        let like = !state.liked.contains(&vid);
        state.liked.set(&vid, like);
        like
    };
    // The Liked page follows at once.
    let liked_page = {
        let state = ui.borrow();
        (state.page == Page::Liked).then(|| state.tracks.clone())
    };
    match liked_page {
        Some(mut list) => {
            if like {
                if !list.iter().any(|t| t.video_id == vid) {
                    list.insert(0, track.clone());
                }
            } else {
                list.retain(|t| t.video_id != vid);
            }
            rebuild_tracks(ui, Page::Liked, "Liked songs", list);
        }
        None => refresh_hearts(ui),
    }
    post(ui, async move { WorkerEvent::Rated(vid.clone(), like, api.rate(vid, like).await) });
}

fn rebuild_tracks(ui: &Rc<RefCell<Ui>>, page: Page, title: &str, tracks: Vec<Track>) {
    // The first songs' links are looked up now, so a click starts at once.
    let ids: Vec<String> = tracks.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect();
    post(ui, async move {
        stream::look_ahead(ids).await;
        WorkerEvent::Done
    });

    let mut state = ui.borrow_mut();
    state.page_title.set_text(title);
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
    state.rows.clear();
    state.tracks = tracks;
    state.page = page;
    if state.tracks.is_empty() {
        let empty = Label::new(Some("No songs here yet. Search for an artist or song."));
        empty.add_css_class("muted");
        state.list.append(&empty);
    }
    let playing_id = state.current.and_then(|i| state.queue.get(i)).map(|t| t.video_id.clone());
    let mut shelf = None;
    let mut rows = Vec::new();
    for (index, track) in state.tracks.iter().enumerate() {
        // The home feed comes in shelves ("New for you", "Quick picks"...): a heading for each.
        if state.page == Page::Home && shelf.as_deref() != Some(track.album.as_str()) {
            shelf = Some(track.album.clone());
            state.list.append(&label(&track.album, "shelf-title"));
        }
        let button = Button::new();
        button.add_css_class("track-row");
        let row = gtk::Box::new(Orientation::Horizontal, 12);
        let number = Label::new(Some(&format!("{:02}", index + 1)));
        number.add_css_class("muted");
        number.set_width_chars(3);
        let details = gtk::Box::new(Orientation::Vertical, 3);
        let kind = match &track.link {
            Some(Link::Playlist(_)) => "Playlist · ",
            Some(Link::Album(_)) => "Album · ",
            Some(Link::Mix(_)) => "Mix · ",
            None => "",
        };
        let more = if track.album.is_empty() || state.page == Page::Home { String::new() } else { format!("  ·  {}", track.album) };
        details.set_hexpand(true);
        details.append(&label(&track.title, "track-title"));
        details.append(&label(&format!("{kind}{}{more}", track.artist), "track-subtitle"));
        let heart = Label::new(Some("♥"));
        heart.add_css_class("row-heart");
        heart.set_visible(state.liked.contains(&track.video_id));
        let duration = Label::new(Some(&track.duration));
        duration.add_css_class("muted");
        row.append(&number);
        row.append(&details);
        row.append(&heart);
        row.append(&duration);
        button.set_child(Some(&row));
        if playing_id.as_deref() == Some(track.video_id.as_str()) {
            button.add_css_class("selected");
        }
        let weak = Rc::downgrade(ui);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                activate_track(&ui, index);
            }
        });
        state.list.append(&button);
        rows.push((button, heart));
    }
    state.rows = rows;
}

fn activate_track(ui: &Rc<RefCell<Ui>>, index: usize) {
    let (track, api, home) = {
        let state = ui.borrow();
        let Some(track) = state.tracks.get(index).cloned() else { return };
        (track, state.api.clone(), state.page == Page::Home)
    };
    if home {
        feed::opened(&track);
    }
    let Some(link) = track.link.clone() else {
        // The queue becomes this list's songs.
        let start = {
            let mut state = ui.borrow_mut();
            state.queue = state.tracks.iter().filter(|t| t.link.is_none()).cloned().collect();
            state.queue.iter().position(|t| t.video_id == track.video_id)
        };
        if let Some(i) = start {
            play_queue(ui, i);
        }
        return;
    };
    let Some(api) = api else {
        set_status(ui, "Sign in or connect to YouTube Music first.");
        return;
    };
    set_status(ui, &format!("Opening {}…", track.title));
    post(ui, async move {
        let result = match link {
            Link::Playlist(id) => api.playlist_tracks(id).await,
            Link::Album(id) => api.album_tracks(id).await,
            Link::Mix(id) => api.mix_tracks(id).await,
        };
        WorkerEvent::Tracks(Page::Other, track.title, result)
    });
}

fn play_queue(ui: &Rc<RefCell<Ui>>, index: usize) {
    let (track, request_id, api) = {
        let mut state = ui.borrow_mut();
        let Some(track) = state.queue.get(index).cloned() else { return };
        state.request_id = state.request_id.wrapping_add(1);
        let request_id = state.request_id;
        state.current = Some(index);
        state.loading = true;
        state.lyrics = None;
        state.active_lyric = None;
        if let Some(audio) = &mut state.audio {
            audio.stop();
        }
        state.title.set_text(&track.title);
        state.artist.set_text(&track.artist);
        state.play.set_label("…");
        state.status.set_text("Getting the song…");
        state.progress.set_value(0.0);
        state.time.set_text("0:00 / 0:00");
        for ((button, _), shown) in state.rows.iter().zip(&state.tracks) {
            if shown.video_id == track.video_id {
                button.add_css_class("selected");
            } else {
                button.remove_css_class("selected");
            }
        }
        (track, request_id, state.api.clone())
    };
    refresh_hearts(ui);

    let video_id = track.video_id.clone();
    let song = track.clone();
    post(ui, async move { WorkerEvent::Fetched(request_id, song, stream::open(video_id).await) });

    post(ui, async move {
        let duration = parse_duration(&track.duration);
        let found = match rymplayer::lyrics::lrclib(track.title, track.artist, track.album, duration).await {
            Ok(Some(l)) => Some(l),
            _ => match api {
                Some(api) => api.lyrics(track.video_id.clone()).await.ok().flatten(),
                None => None,
            },
        };
        WorkerEvent::Lyrics(track.video_id, found)
    });
}

/// The next song downloads while this one plays.
fn prefetch_next(ui: &Rc<RefCell<Ui>>) {
    let next = {
        let state = ui.borrow();
        state.current.and_then(|i| state.queue.get(i + 1)).map(|t| t.video_id.clone())
    };
    if let Some(id) = next.filter(|id| stream::cached(id).is_none()) {
        post(ui, async move {
            let _ = stream::fetch(id).await;
            WorkerEvent::Done
        });
    }
}

fn show_lyrics(ui: &Rc<RefCell<Ui>>, lyrics: Option<Lyrics>) {
    let mut state = ui.borrow_mut();
    while let Some(child) = state.lyrics_list.first_child() {
        state.lyrics_list.remove(&child);
    }
    state.active_lyric = None;
    let Some(lyrics) = lyrics else {
        let empty = Label::new(Some("No lyrics found for this song."));
        empty.add_css_class("muted");
        state.lyrics_list.append(&empty);
        state.lyrics = None;
        return;
    };
    let lines: Vec<String> =
        if !lyrics.synced.is_empty() { lyrics.synced.iter().map(|(_, line)| line.clone()).collect() } else { lyrics.plain.clone() };
    for line in lines {
        let l = Label::new(Some(&line));
        l.set_xalign(0.0);
        l.set_wrap(true);
        l.add_css_class("lyric-line");
        state.lyrics_list.append(&l);
    }
    let source = Label::new(Some(&format!("Lyrics · {}", lyrics.source)));
    source.add_css_class("muted");
    source.set_xalign(0.0);
    state.lyrics_list.append(&source);
    state.lyrics = Some(lyrics);
}

fn toggle_playback(ui: &Rc<RefCell<Ui>>) {
    let (loaded, playing_now, current, has_list) = {
        let state = ui.borrow();
        let loaded = state.audio.as_ref().is_some_and(Audio::is_loaded);
        (loaded, state.audio.as_ref().is_some_and(Audio::is_playing), state.current, !state.tracks.is_empty())
    };
    if loaded {
        let state = ui.borrow();
        if let Some(audio) = &state.audio {
            audio.toggle();
        }
        state.play.set_label(if playing_now { "▶" } else { "❚❚" });
    } else if let Some(current) = current {
        play_queue(ui, current);
    } else if has_list {
        activate_track(ui, 0);
    }
}

fn skip_track(ui: &Rc<RefCell<Ui>>, forward: bool) {
    let (current, len) = {
        let state = ui.borrow();
        (state.current, state.queue.len())
    };
    match current {
        Some(i) if forward && i + 1 < len => play_queue(ui, i + 1),
        Some(_) if forward => continue_with_radio(ui),
        Some(i) => play_queue(ui, i.saturating_sub(1)),
        None => set_status(ui, "Pick a song first."),
    }
}

/// The queue ran out: YouTube Music's radio for the last song keeps it going.
fn continue_with_radio(ui: &Rc<RefCell<Ui>>) {
    let (Some(api), Some(track)) = (ui_api(ui), playing(ui)) else { return };
    {
        let mut state = ui.borrow_mut();
        if state.radio_pending {
            return;
        }
        state.radio_pending = true;
    }
    set_status(ui, "Finding more songs like this…");
    post(ui, async move { WorkerEvent::Radio(api.radio(track.video_id).await) });
}

fn tick(ui: &Rc<RefCell<Ui>>) {
    let mut state = ui.borrow_mut();
    if state.loading {
        return;
    }
    let Some((position, duration, finished)) =
        state.audio.as_ref().map(|audio| (audio.position(), audio.duration().unwrap_or(0.0), audio.is_loaded() && audio.finished()))
    else {
        return;
    };
    state.syncing_progress = true;
    if duration > 0.0 {
        state.progress.set_range(0.0, duration as f64);
        state.progress.set_value(position.min(duration) as f64);
    }
    state.time.set_text(&format!("{} / {}", fmt_time(position), fmt_time(duration)));
    state.syncing_progress = false;

    let new_line = state.lyrics.as_ref().and_then(|lyrics| rymplayer::lyrics::current_line(&lyrics.synced, position));
    if new_line != state.active_lyric {
        if let Some(old) = state.active_lyric.and_then(|i| child_at(&state.lyrics_list, i)) {
            old.remove_css_class("lyric-current");
        }
        if let Some(index) = new_line
            && let Some(line) = child_at(&state.lyrics_list, index)
        {
            line.add_css_class("lyric-current");
            let adjustment = state.lyrics_scroll.vadjustment();
            let target = (index as f64 * 42.0 - adjustment.page_size() / 2.0)
                .max(adjustment.lower())
                .min((adjustment.upper() - adjustment.page_size()).max(adjustment.lower()));
            adjustment.set_value(target);
        }
        state.active_lyric = new_line;
    }

    if finished && state.current.is_some() {
        if let Some(audio) = &mut state.audio {
            audio.stop();
        }
        drop(state);
        skip_track(ui, true);
    }
}

fn child_at(container: &gtk::Box, index: usize) -> Option<gtk::Widget> {
    let mut child = container.first_child();
    for _ in 0..index {
        child = child?.next_sibling();
    }
    child
}
