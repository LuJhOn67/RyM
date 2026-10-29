#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Button, ComboBoxText, CssProvider, HeaderBar, Label,
    Orientation, Scale,
};
use rymplayer::api::{Api, Link, Track};
use rymplayer::audio::{self, Audio, PreparedAudio};
use rymplayer::lyrics::Lyrics;
use rymplayer::{cookie_path, login, parse_duration, stream, ytdlp_cookie_path};

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
.card { background: #171c1a; border: 1px solid #29332e; border-radius: 18px; padding: 16px; }
entry { min-height: 40px; color: #edf4f0; background: #202724; border: 1px solid #35423b; border-radius: 12px; padding: 0 12px; }
button { color: #e8f2ec; background: #252d29; border: 1px solid #35423b; border-radius: 12px; padding: 9px 14px; }
button:hover { background: #303b35; border-color: #607267; }
button.accent { color: #101312; background: @ACCENT@; border-color: @ACCENT@; font-weight: 800; }
button.accent:hover { background: @ACCENT_HOVER@; }
button.track-row { text-align: left; background: #191f1c; border-color: transparent; padding: 10px 12px; }
button.track-row:hover { background: #242d28; }
button.track-row.selected { background: @STATUS@; border-color: @ACCENT@; }
.track-title { color: #f1f6f2; font-size: 14px; font-weight: 700; }
.track-subtitle { color: #9eaaa2; font-size: 12px; }
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

enum WorkerEvent {
    Auth(AuthMode, anyhow::Result<Api>),
    Tracks(String, anyhow::Result<Vec<Track>>),
    Prepared(u64, Track, Option<f32>, anyhow::Result<PreparedAudio>),
    Lyrics(String, Option<Lyrics>),
    PlayReported,
}

struct Ui {
    runtime: tokio::runtime::Runtime,
    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    api: Option<Api>,
    tracks: Vec<Track>,
    current: Option<usize>,
    request_id: u64,
    audio: Option<Audio>,
    lyrics: Option<Lyrics>,
    active_lyric: Option<usize>,
    list: gtk::Box,
    rows: Vec<Button>,
    lyrics_list: gtk::Box,
    lyrics_scroll: gtk::ScrolledWindow,
    status: Label,
    sign_in: Button,
    title: Label,
    artist: Label,
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
    AppTheme {
        name: "Mint",
        accent: "#a8f0c2",
        accent_hover: "#c4f7d5",
        status: "#24382c",
        accent_dark: "#385541",
    },
    AppTheme {
        name: "Sky",
        accent: "#9bcaff",
        accent_hover: "#c2ddff",
        status: "#202f45",
        accent_dark: "#3b5574",
    },
    AppTheme {
        name: "Sakura",
        accent: "#ffc1d8",
        accent_hover: "#ffdae8",
        status: "#49283b",
        accent_dark: "#70445f",
    },
    AppTheme {
        name: "Midnight",
        accent: "#c7b0ff",
        accent_hover: "#ddceff",
        status: "#30264a",
        accent_dark: "#514277",
    },
    AppTheme {
        name: "YouTube Music",
        accent: "#ff7777",
        accent_hover: "#ff9a9a",
        status: "#482626",
        accent_dark: "#743d3d",
    },
    AppTheme {
        name: "Spotify",
        accent: "#1ed760",
        accent_hover: "#61e78d",
        status: "#203b2b",
        accent_dark: "#2d6341",
    },
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
    if let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    {
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
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    provider
}

fn build_window(app: &Application) {
    let provider = install_style();

    let (tx, rx) = mpsc::channel();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create async runtime");

    let list = gtk::Box::new(Orientation::Vertical, 7);
    let lyrics_list = gtk::Box::new(Orientation::Vertical, 5);
    let lyrics_scroll = gtk::ScrolledWindow::new();
    lyrics_scroll.set_vexpand(true);
    lyrics_scroll.set_child(Some(&lyrics_list));

    let status = Label::new(Some("Starting RymPlayer…"));
    status.set_xalign(0.0);
    status.add_css_class("statusbar");
    status.set_hexpand(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let sign_in = Button::with_label("Sign in with Google");
    let title = Label::new(Some("Nothing playing"));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("current-title");
    let artist = Label::new(Some("Choose a song to start"));
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.add_css_class("current-artist");
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

    let mut audio = None;
    let initial_status = match Audio::new() {
        Ok(player) => {
            player.set_volume(0.8);
            audio = Some(player);
            "Connecting to YouTube Music…"
        }
        Err(err) => {
            status.set_text(&format!("No audio device: {err}"));
            "YouTube Music"
        }
    };
    if status.text().as_str() == "Starting RymPlayer…" {
        status.set_text(initial_status);
    }

    let ui = Rc::new(RefCell::new(Ui {
        runtime,
        tx,
        rx,
        api: None,
        tracks: Vec::new(),
        current: None,
        request_id: 0,
        audio,
        lyrics: None,
        active_lyric: None,
        list: list.clone(),
        rows: Vec::new(),
        lyrics_list: lyrics_list.clone(),
        lyrics_scroll: lyrics_scroll.clone(),
        status: status.clone(),
        sign_in: sign_in.clone(),
        title: title.clone(),
        artist: artist.clone(),
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
    topbar.append(&search);
    topbar.append(&search_button);
    topbar.append(&theme_picker);
    topbar.append(&sign_in);

    let page_title = Label::new(Some("For you"));
    page_title.set_xalign(0.0);
    page_title.add_css_class("section-title");
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

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RyM · Music Player")
        .default_width(1180)
        .default_height(760)
        .build();
    let titlebar = HeaderBar::new();
    titlebar.add_css_class("rym-titlebar");
    titlebar.set_show_title_buttons(false);
    let titlebar_title = Label::new(Some("RyM · Music Player"));
    titlebar.set_title_widget(Some(&titlebar_title));
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
        let page_title = page_title.clone();
        let submit = move || search_tracks(&ui, search.text().to_string(), &page_title);
        search_button.connect_clicked(move |_| submit());
    }
    {
        let ui = ui.clone();
        let search = search.clone();
        let page_title = page_title.clone();
        search.connect_activate(move |entry| {
            search_tracks(&ui, entry.text().to_string(), &page_title)
        });
    }
    {
        let ui = ui.clone();
        let page_title = page_title.clone();
        home_button.connect_clicked(move |_| load_home(&ui, &page_title));
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
            let ui = ui.borrow_mut();
            if !ui.syncing_progress {
                if let Some(audio) = &ui.audio {
                    audio.seek(scale.value() as f32);
                }
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
            volume_button.set_label(if scale.value() <= 0.001 {
                "♪̸"
            } else {
                "♪"
            });
        });
    }

    let poll = ui.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        poll_events(&poll, &page_title);
        gtk::glib::ControlFlow::Continue
    });

    if let Some(cookie) = std::fs::read_to_string(cookie_path())
        .ok()
        .filter(|c| !c.trim().is_empty())
    {
        set_status(&ui, "Checking saved Google sign-in…");
        post(&ui, async move {
            WorkerEvent::Auth(AuthMode::Saved, Api::with_cookie(cookie).await)
        });
    } else {
        post(&ui, async {
            WorkerEvent::Auth(AuthMode::Guest, Api::guest().await)
        });
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

fn search_tracks(ui: &Rc<RefCell<Ui>>, query: String, title: &Label) {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return;
    }
    let Some(api) = ui.borrow().api.clone() else {
        set_status(
            ui,
            "YouTube Music is still connecting. Try again in a moment.",
        );
        return;
    };
    title.set_text(&format!("Results for {query}"));
    set_status(ui, "Searching…");
    post(ui, async move {
        WorkerEvent::Tracks(query.clone(), api.search(query).await)
    });
}

fn load_home(ui: &Rc<RefCell<Ui>>, title: &Label) {
    let Some(api) = ui.borrow().api.clone() else {
        set_status(ui, "YouTube Music is still connecting.");
        return;
    };
    title.set_text("For you");
    set_status(ui, "Loading recommendations…");
    post(ui, async move {
        WorkerEvent::Tracks("For you".into(), api.home().await)
    });
}

fn sign_in_or_out(ui: &Rc<RefCell<Ui>>) {
    let logged_in = ui.borrow().api.as_ref().is_some_and(Api::logged_in);
    if logged_in {
        let _ = std::fs::remove_file(cookie_path());
        let _ = std::fs::remove_file(ytdlp_cookie_path());
        if let Some(audio) = &mut ui.borrow_mut().audio {
            audio.stop();
        }
        ui.borrow().sign_in.set_label("Sign in with Google");
        post(ui, async {
            WorkerEvent::Auth(AuthMode::Guest, Api::guest().await)
        });
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

fn poll_events(ui: &Rc<RefCell<Ui>>, page_title: &Label) {
    loop {
        let event = ui.borrow().rx.try_recv();
        let Ok(event) = event else { break };
        match event {
            WorkerEvent::Auth(mode, Ok(api)) => {
                let is_user = api.logged_in();
                if is_user {
                    if let Ok(cookie) = std::fs::read_to_string(cookie_path()) {
                        let _ = stream::write_cookie_file(&cookie, &ytdlp_cookie_path());
                    }
                }
                ui.borrow().sign_in.set_label(if is_user {
                    "Sign out"
                } else {
                    "Sign in with Google"
                });
                ui.borrow_mut().api = Some(api);
                match mode {
                    AuthMode::NewLogin => set_status(ui, "Signed in to YouTube Music."),
                    AuthMode::Saved => set_status(ui, "Signed in with your saved Google session."),
                    AuthMode::Guest => set_status(ui, "Connected in guest mode."),
                }
                load_home(ui, page_title);
            }
            WorkerEvent::Auth(mode, Err(error)) => match mode {
                AuthMode::Saved => {
                    set_status(
                        ui,
                        &format!("Saved sign-in failed: {error}. Trying guest mode…"),
                    );
                    post(ui, async {
                        WorkerEvent::Auth(AuthMode::Guest, Api::guest().await)
                    });
                }
                AuthMode::NewLogin => set_status(ui, &format!("Google sign-in failed: {error}")),
                AuthMode::Guest => {
                    set_status(ui, &format!("Couldn't connect to YouTube Music: {error}"))
                }
            },
            WorkerEvent::Tracks(label, Ok(tracks)) => {
                let count = tracks.len();
                warm_streams(ui, &tracks);
                rebuild_tracks(ui, tracks, page_title);
                set_status(ui, &format!("{label} · {count} songs"));
            }
            WorkerEvent::Tracks(label, Err(error)) => set_status(ui, &format!("{label}: {error}")),
            WorkerEvent::Prepared(request_id, track, known_duration, Ok(prepared)) => {
                let mut state = ui.borrow_mut();
                if request_id != state.request_id {
                    continue;
                }
                let duration = if let Some(audio) = &mut state.audio {
                    audio.play_prepared(prepared, known_duration);
                    Some(audio.duration().unwrap_or(100.0).max(1.0) as f64)
                } else {
                    None
                };
                if let Some(duration) = duration {
                    state.play.set_label("❚❚");
                    state.status.set_text("Playing while the rest loads…");
                    state.progress.set_range(0.0, duration);
                } else {
                    state
                        .status
                        .set_text("No audio output device is available.");
                }
                drop(state);
                if let Some(api) = ui_api(ui) {
                    let video_id = track.video_id.clone();
                    post(ui, async move {
                        let _ = api.report_play(video_id).await;
                        WorkerEvent::PlayReported
                    });
                }
            }
            WorkerEvent::Prepared(request_id, _track, _, Err(error)) => {
                if request_id == ui.borrow().request_id {
                    set_status(ui, &format!("Couldn't start this song: {error}"));
                    ui.borrow().play.set_label("▶");
                }
            }
            WorkerEvent::Lyrics(video_id, lyrics) => {
                let current_id = {
                    let state = ui.borrow();
                    state
                        .current
                        .and_then(|index| state.tracks.get(index).map(|t| t.video_id.clone()))
                };
                if current_id.as_deref() == Some(video_id.as_str()) {
                    show_lyrics(ui, lyrics);
                }
            }
            WorkerEvent::PlayReported => {}
        }
    }

    tick(ui);
}

fn ui_api(ui: &Rc<RefCell<Ui>>) -> Option<Api> {
    ui.borrow().api.clone()
}

fn warm_streams(ui: &Rc<RefCell<Ui>>, tracks: &[Track]) {
    let (handle, cookies) = {
        let state = ui.borrow();
        let cookies = state
            .api
            .as_ref()
            .filter(|api| api.logged_in())
            .map(|_| ytdlp_cookie_path())
            .filter(|path| path.exists());
        (state.runtime.handle().clone(), cookies)
    };
    for track in tracks
        .iter()
        .filter(|track| track.link.is_none() && !track.video_id.is_empty())
        .take(8)
    {
        handle.spawn(stream::warm(track.video_id.clone(), cookies.clone()));
    }
}

fn rebuild_tracks(ui: &Rc<RefCell<Ui>>, tracks: Vec<Track>, page_title: &Label) {
    let mut state = ui.borrow_mut();
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
    state.rows.clear();
    state.tracks = tracks;
    state.current = None;
    state.request_id = state.request_id.wrapping_add(1);
    state.lyrics = None;
    state.active_lyric = None;
    while let Some(child) = state.lyrics_list.first_child() {
        state.lyrics_list.remove(&child);
    }
    let empty = state.tracks.is_empty();
    if empty {
        let label = Label::new(Some("No songs here yet. Search for an artist or song."));
        label.add_css_class("muted");
        state.list.append(&label);
    }
    let rows: Vec<_> = state
        .tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let button = Button::new();
            button.add_css_class("track-row");
            let row = gtk::Box::new(Orientation::Horizontal, 12);
            let number = Label::new(Some(&format!("{:02}", index + 1)));
            number.add_css_class("muted");
            number.set_width_chars(3);
            let details = gtk::Box::new(Orientation::Vertical, 3);
            let title = Label::new(Some(&track.title));
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.add_css_class("track-title");
            let subtitle = Label::new(Some(&format!(
                "{}{}",
                track.artist,
                if track.album.is_empty() {
                    String::new()
                } else {
                    format!("  ·  {}", track.album)
                }
            )));
            subtitle.set_xalign(0.0);
            subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
            subtitle.add_css_class("track-subtitle");
            details.set_hexpand(true);
            details.append(&title);
            details.append(&subtitle);
            let duration = Label::new(Some(&track.duration));
            duration.add_css_class("muted");
            row.append(&number);
            row.append(&details);
            row.append(&duration);
            button.set_child(Some(&row));
            let weak = Rc::downgrade(ui);
            let page_title = page_title.clone();
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    activate_track(&ui, index, &page_title);
                }
            });
            button
        })
        .collect();
    for row in &rows {
        state.list.append(row);
    }
    state.rows = rows;
}

fn activate_track(ui: &Rc<RefCell<Ui>>, index: usize, page_title: &Label) {
    let (track, api) = {
        let state = ui.borrow();
        let Some(track) = state.tracks.get(index).cloned() else {
            return;
        };
        (track, state.api.clone())
    };
    let Some(link) = track.link.clone() else {
        play_index(ui, index);
        return;
    };
    let Some(api) = api else {
        set_status(ui, "Sign in or connect to YouTube Music first.");
        return;
    };
    page_title.set_text(&track.title);
    set_status(ui, &format!("Opening {}…", track.title));
    post(ui, async move {
        let result = match link {
            Link::Playlist(id) => api.playlist_tracks(id).await,
            Link::Album(id) => api.album_tracks(id).await,
            Link::Mix(id) => api.mix_tracks(id).await,
        };
        WorkerEvent::Tracks(track.title, result)
    });
}

fn play_index(ui: &Rc<RefCell<Ui>>, index: usize) {
    let (track, request_id, cookies, api) = {
        let mut state = ui.borrow_mut();
        let Some(track) = state.tracks.get(index).cloned() else {
            return;
        };
        state.request_id = state.request_id.wrapping_add(1);
        let request_id = state.request_id;
        state.current = Some(index);
        state.lyrics = None;
        state.active_lyric = None;
        state.title.set_text(&track.title);
        state.artist.set_text(&track.artist);
        state.play.set_label("…");
        state.status.set_text("Connecting to the song…");
        state.progress.set_value(0.0);
        state.time.set_text("0:00 / 0:00");
        for (i, row) in state.rows.iter().enumerate() {
            if i == index {
                row.add_css_class("selected");
            } else {
                row.remove_css_class("selected");
            }
        }
        let cookies = state
            .api
            .as_ref()
            .is_some_and(Api::logged_in)
            .then(ytdlp_cookie_path)
            .filter(|path| path.exists());
        (track, request_id, cookies, state.api.clone())
    };

    let lyrics_api = api.clone();
    let lyric_track = track.clone();
    post(ui, async move {
        let lyrics = match lyrics_api {
            Some(api) => api
                .lyrics(lyric_track.video_id.clone())
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let lyrics = if lyrics.is_some() {
            lyrics
        } else {
            let duration = parse_duration(&lyric_track.duration);
            rymplayer::lyrics::lrclib(
                lyric_track.title,
                lyric_track.artist,
                lyric_track.album,
                duration,
            )
            .await
            .ok()
            .flatten()
        };
        WorkerEvent::Lyrics(lyric_track.video_id, lyrics)
    });

    let video_id = track.video_id.clone();
    let known_duration = parse_duration(&track.duration);
    post(ui, async move {
        let result = audio::prepare_playback(video_id, cookies).await;
        WorkerEvent::Prepared(request_id, track, known_duration, result)
    });
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
    let lines: Vec<String> = if !lyrics.synced.is_empty() {
        lyrics.synced.iter().map(|(_, line)| line.clone()).collect()
    } else {
        lyrics.plain.clone()
    };
    for line in lines {
        let label = Label::new(Some(&line));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("lyric-line");
        state.lyrics_list.append(&label);
    }
    let source = Label::new(Some(&format!("Lyrics · {}", lyrics.source)));
    source.add_css_class("muted");
    source.set_xalign(0.0);
    state.lyrics_list.append(&source);
    state.lyrics = Some(lyrics);
}

fn toggle_playback(ui: &Rc<RefCell<Ui>>) {
    let (playing, current) = {
        let state = ui.borrow();
        (
            state.audio.as_ref().is_some_and(Audio::is_playing),
            state.current,
        )
    };
    if let Some(audio) = &ui.borrow().audio {
        audio.toggle();
        ui.borrow().play.set_label(if playing { "▶" } else { "❚❚" });
    } else if let Some(current) = current {
        play_index(ui, current);
    } else if !ui.borrow().tracks.is_empty() {
        play_index(ui, 0);
    }
}

fn skip_track(ui: &Rc<RefCell<Ui>>, forward: bool) {
    let next = {
        let state = ui.borrow();
        if state.tracks.is_empty() {
            None
        } else {
            match state.current {
                None => Some(0),
                Some(index) if forward && index + 1 < state.tracks.len() => Some(index + 1),
                Some(index) if !forward => Some(index.saturating_sub(1)),
                _ => None,
            }
        }
    };
    if let Some(index) = next {
        play_index(ui, index);
    } else {
        set_status(ui, "No more songs in this list.");
    }
}

fn tick(ui: &Rc<RefCell<Ui>>) {
    let mut state = ui.borrow_mut();
    let Some((position, duration, finished)) = state.audio.as_ref().map(|audio| {
        (
            audio.position(),
            audio.duration().unwrap_or(0.0),
            audio.finished(),
        )
    }) else {
        return;
    };
    state.syncing_progress = true;
    if duration > 0.0 {
        state.progress.set_range(0.0, duration as f64);
        state.progress.set_value(position.min(duration) as f64);
    }
    state
        .time
        .set_text(&format!("{} / {}", fmt_time(position), fmt_time(duration)));
    state.syncing_progress = false;

    let new_line = state
        .lyrics
        .as_ref()
        .and_then(|lyrics| rymplayer::lyrics::current_line(&lyrics.synced, position));
    if new_line != state.active_lyric {
        if let Some(old_index) = state.active_lyric {
            if let Some(old) = child_at(&state.lyrics_list, old_index) {
                old.remove_css_class("lyric-current");
            }
        }
        if let Some(index) = new_line {
            if let Some(line) = child_at(&state.lyrics_list, index) {
                line.add_css_class("lyric-current");
                let adjustment = state.lyrics_scroll.vadjustment();
                let target = (index as f64 * 42.0 - adjustment.page_size() / 2.0)
                    .max(adjustment.lower())
                    .min((adjustment.upper() - adjustment.page_size()).max(adjustment.lower()));
                adjustment.set_value(target);
            }
        }
        state.active_lyric = new_line;
    }

    if finished {
        let next = state
            .current
            .and_then(|i| i.checked_add(1))
            .filter(|i| *i < state.tracks.len());
        drop(state);
        if let Some(next) = next {
            play_index(ui, next);
        }
    }
}

fn child_at(container: &gtk::Box, index: usize) -> Option<gtk::Widget> {
    let mut child = container.first_child();
    for _ in 0..index {
        child = child?.next_sibling();
    }
    child
}

fn fmt_time(secs: f32) -> String {
    let total = secs.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}
