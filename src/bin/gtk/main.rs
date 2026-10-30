#![windows_subsystem = "windows"]
//! The GTK 4 window app, laid out like the iced one: an icon rail on the left, the page in the
//! middle (home shelves of cards, song lists with covers, library, history, settings), lyrics
//! that slide in on the right, and a slim player bar with the cover at the bottom. Same themes
//! as the iced app (shared `gui_theme` setting).
//! Build: `cargo build --release --features gtk-ui --bin RymPlayer-GTK`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{Align, Application, ApplicationWindow, Button, CssProvider, HeaderBar, Label, Orientation, Scale, gdk, glib};
use rymplayer::i18n::tr;
use rymplayer::api::{self, Api, LIKED_ID, Link, Playlist, Track};
use rymplayer::audio::Audio;
use rymplayer::config::{Config, Source};
use rymplayer::likes::{Likes, SYNC_EVERY};
use rymplayer::lyrics::Lyrics;
use rymplayer::{cookie_path, feed, fmt_time, login, page_cache, parse_duration, stream, vlc};

const APP_ID: &str = "com.rymplayer.Gtk";
/// Cover sizes requested from Google (pixels; drawn at about half for sharp HiDPI).
const ROW_PX: u32 = 104;
const CARD_PX: u32 = 160;
const ART_PX: u32 = 300;

/// Hybrid laptops (a built-in GPU and a fast one): asks the NVIDIA and AMD drivers to run this
/// program on the fast graphics card. The drivers read these from the .exe's exports
/// (build.rs exports them).
#[cfg(windows)]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
#[used]
pub static NvOptimusEnablement: u32 = 1;
#[cfg(windows)]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
#[used]
pub static AmdPowerXpressRequestHighPerformance: u32 = 1;

// ---------------------------------------------------------------- themes

/// The iced app's palettes, as CSS colors (same order, so `gui_theme` means the same theme).
struct Theme {
    name: &'static str,
    bg: &'static str,
    surface: &'static str,
    bar: &'static str,
    hover: &'static str,
    selected: &'static str,
    text: &'static str,
    dim: &'static str,
    accent: &'static str,
    pill_bg: &'static str,
    pill_text: &'static str,
    input: &'static str,
    divider: &'static str,
    menu: &'static str,
}

const fn light(name: &'static str, accent: &'static str, bg: &'static str, selected: &'static str) -> Theme {
    Theme {
        name,
        bg,
        surface: "#ffffff",
        bar: "#ffffff",
        hover: "rgba(0,0,0,0.045)",
        selected,
        text: "#282c34",
        dim: "#808691",
        accent,
        pill_bg: accent,
        pill_text: "#ffffff",
        input: "rgba(0,20,60,0.06)",
        divider: "rgba(0,0,0,0.07)",
        menu: "#ffffff",
    }
}

const THEMES: [Theme; 6] = [
    light("Sky", "#4285c4", "#ecf2f8", "rgba(66,133,196,0.13)"),
    light("Mint", "#38a675", "#ecf6f0", "rgba(56,166,117,0.13)"),
    light("Sakura", "#de5e8c", "#faf0f4", "rgba(222,94,140,0.12)"),
    Theme {
        name: "Midnight",
        bg: "#0e1219",
        surface: "#141a23",
        bar: "#11161e",
        hover: "rgba(255,255,255,0.06)",
        selected: "rgba(90,160,255,0.16)",
        text: "#e6ecf4",
        dim: "#8c98aa",
        accent: "#60a6ff",
        pill_bg: "#60a6ff",
        pill_text: "#0a0e14",
        input: "rgba(255,255,255,0.07)",
        divider: "rgba(255,255,255,0.07)",
        menu: "#1e2632",
    },
    Theme {
        name: "Red night",
        bg: "#030303",
        surface: "#0f0e0d",
        bar: "#212121",
        hover: "rgba(255,255,255,0.10)",
        selected: "rgba(255,255,255,0.16)",
        text: "#ffffff",
        dim: "#aaaaaa",
        accent: "#ff0000",
        pill_bg: "#ffffff",
        pill_text: "#030303",
        input: "rgba(255,255,255,0.10)",
        divider: "rgba(255,255,255,0.10)",
        menu: "#3a3431",
    },
    Theme {
        name: "Green night",
        bg: "#000000",
        surface: "#121212",
        bar: "#000000",
        hover: "#1f1f1f",
        selected: "#2a2a2a",
        text: "#ffffff",
        dim: "#b3b3b3",
        accent: "#1ed760",
        pill_bg: "#1ed760",
        pill_text: "#000000",
        input: "#242424",
        divider: "#282828",
        menu: "#282828",
    },
];

const STYLE: &str = r#"
window, .rym { background: @bg@; color: @text@; }
headerbar { background: @bg@; color: @text@; border: none; box-shadow: none; min-height: 46px; }
headerbar button { color: @text@; }
.rail { background: @bg@; padding: 8px 6px; }
.rail button { background: transparent; border: none; box-shadow: none; border-radius: 12px; padding: 8px 4px; min-width: 64px; color: @dim@; }
.rail button:hover { background: @hover@; color: @text@; }
.rail button.active { background: @selected@; color: @accent@; }
.rail-label { font-size: 11px; }
.page { background: @surface@; border-radius: 14px 0 0 0; }
.search { background: @input@; color: @text@; border: none; border-radius: 20px; min-height: 36px; padding: 0 12px; box-shadow: none; }
.heading { font-size: 22px; font-weight: 800; color: @text@; }
.shelf { font-size: 17px; font-weight: 700; color: @text@; margin-top: 6px; }
.dim { color: @dim@; }
.small { font-size: 12px; }
.title { font-weight: 600; color: @text@; }
button.flat-row { background: transparent; border: none; box-shadow: none; border-radius: 8px; padding: 6px 10px; color: @text@; }
button.flat-row:hover { background: @hover@; }
button.flat-row.playing { background: @selected@; }
button.flat-row.playing .title { color: @accent@; }
button.card { background: transparent; border: none; box-shadow: none; border-radius: 10px; padding: 8px; color: @text@; }
button.card:hover { background: @hover@; }
.cover { border-radius: 6px; background: @input@; }
.cover-big { border-radius: 10px; background: @input@; }
.tile { border-radius: 6px; background: @accent@; color: #ffffff; }
button.pill { background: @pill_bg@; color: @pill_text@; border: none; box-shadow: none; border-radius: 18px; padding: 6px 18px; font-weight: 700; }
button.pill:hover { opacity: 0.9; }
button.ghost { background: @input@; color: @text@; border: none; box-shadow: none; border-radius: 18px; padding: 6px 16px; }
button.icon { background: transparent; border: none; box-shadow: none; border-radius: 20px; padding: 6px; color: @text@; min-width: 32px; min-height: 32px; }
button.icon:hover { background: @hover@; }
button.icon.dim { color: @dim@; }
button.heart { color: @dim@; font-size: 16px; }
button.heart.on { color: @accent@; }
button.play { background: @text@; color: @surface@; border-radius: 22px; min-width: 40px; min-height: 40px; }
button.play:hover { background: @accent@; }
.bar { background: @bar@; border-top: 1px solid @divider@; padding: 8px 14px; }
.lyrics { border-radius: 14px; margin: 10px; padding: 26px 28px; }
.lyric { font-size: 25px; font-weight: 800; padding: 7px 0; transition: color 300ms ease; }
.toast { background: @menu@; color: @text@; border-radius: 10px; padding: 10px 16px; margin: 14px; box-shadow: 0 4px 16px rgba(0,0,0,0.3); }
popover > contents { background: @menu@; color: @text@; border-radius: 10px; padding: 6px; }
popover button { background: transparent; border: none; box-shadow: none; color: @text@; padding: 8px 12px; border-radius: 6px; }
popover button:hover { background: @hover@; }
scale trough { background: @divider@; min-height: 4px; border-radius: 3px; }
scale highlight { background: @accent@; border-radius: 3px; }
scale slider { background: @text@; min-width: 12px; min-height: 12px; margin: -4px; border: none; box-shadow: none; }
dropdown button { background: @input@; color: @text@; border: none; box-shadow: none; border-radius: 8px; }
scrollbar slider { background: @dim@; border-radius: 8px; min-width: 6px; }
"#;

fn css(theme: usize) -> String {
    let t = &THEMES[theme % THEMES.len()];
    STYLE
        .replace("@bg@", t.bg)
        .replace("@surface@", t.surface)
        .replace("@bar@", t.bar)
        .replace("@hover@", t.hover)
        .replace("@selected@", t.selected)
        .replace("@text@", t.text)
        .replace("@dim@", t.dim)
        .replace("@accent@", t.accent)
        .replace("@pill_bg@", t.pill_bg)
        .replace("@pill_text@", t.pill_text)
        .replace("@input@", t.input)
        .replace("@divider@", t.divider)
        .replace("@menu@", t.menu)
}

/// The lyrics panel in the playing song's cover color: lines to come dark, the sung line white,
/// sung lines soft white (see rymplayer::colors).
fn lyrics_css(bg: (u8, u8, u8)) -> String {
    let [sung, _, next] = rymplayer::colors::lyric_colors(bg);
    let c = |(r, g, b): (u8, u8, u8)| format!("rgb({r},{g},{b})");
    format!(
        ".lyrics {{ background: {}; }} .lyric {{ color: {}; }} .lyric.sung {{ color: {}; }} .lyric.now {{ color: #ffffff; }} .lyrics-note {{ color: {}; font-weight: 600; }}",
        c(bg),
        c(next),
        c(sung),
        c(sung)
    )
}

fn apply_theme(provider: &CssProvider, theme: usize) {
    provider.load_from_string(&css(theme));
}

// ---------------------------------------------------------------- state

#[derive(Clone, Copy)]
enum AuthMode {
    Saved,
    Guest,
    NewLogin,
}

#[derive(Clone, PartialEq)]
enum Page {
    Home,
    Liked,
    Library,
    History,
    Settings,
    /// A playlist, album, mix, radio or search results.
    Songs(String),
}

enum Ev {
    Auth(AuthMode, anyhow::Result<Api>),
    Home(anyhow::Result<feed::Refreshed>),
    /// A page's songs; the number is the page it was asked for (older answers are dropped).
    Tracks(u64, anyhow::Result<Vec<Track>>),
    Playlists(anyhow::Result<Vec<Playlist>>),
    Liked(anyhow::Result<Vec<Track>>),
    /// A cover's pixels (width, height, RGBA), decoded off the window's thread.
    Cover(String, Option<(u32, u32, Vec<u8>)>),
    /// The song to play: from the cache, or still arriving (it plays while it downloads).
    Fetched(u64, anyhow::Result<stream::Playable>),
    /// The song's direct link for VLC.
    InVlc(u64, anyhow::Result<(String, Vec<(String, String)>)>),
    Lyrics(String, Option<Lyrics>),
    /// The lyrics panel's color, from the song's cover.
    LyricsBg(String, (u8, u8, u8)),
    Rated(String, bool, anyhow::Result<()>),
    Radio(anyhow::Result<Vec<Track>>),
    Downloaded(anyhow::Result<PathBuf>),
    /// "Go to artist" for a song without its artist's id: found by name.
    ArtistFound(String, anyhow::Result<Option<String>>),
    /// The liked songs batch download, after each song.
    Batch(stream::Batch),
    Done,
}

/// Widgets the code changes after building them.
struct Widgets {
    window: ApplicationWindow,
    provider: CssProvider,
    /// The lyrics panel's colors, changed for each song.
    lyrics_provider: CssProvider,
    rail: Vec<(Page, Button)>,
    content: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toast: gtk::Revealer,
    toast_text: Label,
    lyrics_panel: gtk::Revealer,
    lyrics_box: gtk::Box,
    lyrics_scroll: gtk::ScrolledWindow,
    bar_cover: gtk::Picture,
    bar_title: Label,
    bar_artist: Label,
    bar_heart: Button,
    play: Button,
    progress: Scale,
    pos: Label,
    len: Label,
}

struct Ui {
    runtime: tokio::runtime::Runtime,
    tx: Sender<Ev>,
    rx: Receiver<Ev>,
    api: Option<Api>,
    config: Config,
    w: Widgets,

    page: Page,
    /// Bumped on every page change: answers for an older page are dropped.
    page_id: u64,
    home: Vec<Track>,
    home_loading: bool,
    /// The songs on a list page (playlist, liked, history, results).
    tracks: Vec<Track>,
    tracks_loading: bool,
    playlists: Vec<Playlist>,
    liked: Likes,
    liked_tracks: Vec<Track>,
    likes_synced: Instant,

    queue: Vec<Track>,
    current: Option<usize>,
    request_id: u64,
    loading: bool,
    radio_pending: bool,
    audio: Option<Audio>,
    vlc: Option<std::process::Child>,
    lyrics: Option<Lyrics>,
    lyric_line: Option<usize>,

    covers: HashMap<String, gdk::Texture>,
    waiting: HashMap<String, Vec<glib::WeakRef<gtk::Picture>>>,
    requested: HashSet<String>,
    /// Heart buttons on screen, by song, so likes synced from the phone show at once.
    hearts: Vec<(String, glib::WeakRef<Button>)>,
    /// Song rows on screen, by song, to mark the playing one.
    rows: Vec<(String, glib::WeakRef<Button>)>,
    toast_id: u64,
    /// The list page holds search results: a click plays the song, then a radio like it.
    from_search: bool,
    /// Liked songs batch download: how it's going, and the widgets that show it.
    batch: Option<stream::Batch>,
    batch_label: Option<glib::WeakRef<Label>>,
    batch_bar: Option<glib::WeakRef<gtk::ProgressBar>>,
    /// When Previous was last pressed: twice in a row goes back even mid-song.
    last_prev: Instant,
    /// What the player bar shows now, so the ticker only changes what changed.
    shown_playing: Option<bool>,
    shown_len: f32,
    shown_pos: Option<u32>,
    shown_slider: f32,
    /// When the playing song was asked for (for the log).
    play_asked: Instant,
}

type Shared = Rc<RefCell<Ui>>;
/// Covers a page wants: the sized URL and the picture to put it in.
type Wants = Vec<(String, u32, gtk::Picture)>;

/// A cover cut to a centered square and scaled to `px`, as RGBA pixels. Runs off the main thread,
/// so decoding never holds up the window.
fn decode_cover(bytes: &[u8], px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let sized = if side > px { square.resize_exact(px, px, image::imageops::FilterType::Triangle) } else { square };
    let rgba = sized.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Things that took too long on the window's thread, and which renderer GTK picked:
/// `rym-gtk.log` in the data folder, for finding stutters without screenshots.
fn log(line: &str) {
    use std::io::Write;
    let path = rymplayer::data_dir().join("rym-gtk.log");
    // Kept small: started over past 200 KB.
    let fresh = std::fs::metadata(&path).map(|m| m.len() > 200_000).unwrap_or(false);
    let file = std::fs::OpenOptions::new().create(true).append(!fresh).write(true).truncate(fresh).open(&path);
    if let Ok(mut f) = file {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs} {line}");
    }
}

fn main() {
    configure_gtk_runtime();
    if std::env::args().any(|arg| arg == "--login") {
        login::run();
    }
    // Each start is its own window (like the other apps), not a second window of the first.
    let app = Application::builder().application_id(APP_ID).flags(gtk::gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(build);
    app.run();
}

/// Windows: a portable GTK runtime beside the program (its DLLs next to the .exe, its data in
/// `share` and `lib`) instead of a system install.
fn configure_gtk_runtime() {
    // Draw on the graphics card (OpenGL) unless the user picked a renderer themselves.
    // GTK on Windows draws with OpenGL only through DirectComposition, which it turns on when
    // asked (GDK_DEBUG=dcomp); without it, it falls back to drawing on the processor.
    #[cfg(windows)]
    {
        let debug = std::env::var("GDK_DEBUG").unwrap_or_default();
        // SAFETY: this runs before GTK or Tokio creates any threads.
        unsafe {
            if !debug.split(',').any(|f| f == "dcomp") {
                std::env::set_var("GDK_DEBUG", if debug.is_empty() { "dcomp".to_string() } else { format!("{debug},dcomp") });
            }
            if std::env::var_os("GSK_RENDERER").is_none() {
                std::env::set_var("GSK_RENDERER", "gl");
            }
        }
    }
    #[cfg(windows)]
    if let Some(root) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        && root.join("share").join("glib-2.0").is_dir()
    {
        // SAFETY: this runs before GTK or Tokio creates any threads.
        unsafe {
            std::env::set_var("GTK_DATA_PREFIX", &root);
            std::env::set_var("GTK_EXE_PREFIX", &root);
            std::env::set_var("GSETTINGS_SCHEMA_DIR", root.join("share/glib-2.0/schemas"));
            std::env::set_var("XDG_DATA_DIRS", root.join("share"));
        }
    }
}

// ---------------------------------------------------------------- small widget helpers

fn label(text: &str, classes: &[&str]) -> Label {
    let l = Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn icon_button(icon: &str, tip: &str, classes: &[&str]) -> Button {
    let b = Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("icon");
    for c in classes {
        b.add_css_class(c);
    }
    b
}

/// A heart drawn as text: icon themes don't all have one, and its color follows the theme.
fn heart_button(tip: &str) -> Button {
    let b = Button::with_label("♥");
    b.set_tooltip_text(Some(tip));
    b.add_css_class("icon");
    b.add_css_class("heart");
    b
}

fn heart_glyph(px: i32) -> Label {
    let l = Label::new(Some("♥"));
    l.add_css_class("glyph");
    l.set_markup(&format!("<span size=\"{}pt\">♥</span>", px * 3 / 4));
    l
}

fn picture(size: i32, class: &str) -> gtk::Picture {
    let p = gtk::Picture::new();
    p.set_size_request(size, size);
    p.set_content_fit(gtk::ContentFit::Cover);
    p.set_can_shrink(true);
    p.set_overflow(gtk::Overflow::Hidden);
    p.add_css_class(class);
    p.set_halign(Align::Start);
    p.set_valign(Align::Center);
    p.set_hexpand(false);
    p
}

/// A cover at `size` (shown pixels), from the cache or asked for.
fn cover(state: &Ui, url: Option<&String>, px: u32, size: i32, class: &str, wants: &mut Wants) -> gtk::Picture {
    let pic = picture(size, class);
    if let Some(url) = url {
        let url = api::thumb_sized(url, px);
        match state.covers.get(&url) {
            Some(t) => pic.set_paintable(Some(t)),
            None => wants.push((url, px, pic.clone())),
        }
    }
    pic
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn spawn(state: &Ui, task: impl Future<Output = Ev> + Send + 'static) {
    let tx = state.tx.clone();
    state.runtime.spawn(async move {
        let _ = tx.send(task.await);
    });
}

fn post(ui: &Shared, task: impl Future<Output = Ev> + Send + 'static) {
    spawn(&ui.borrow(), task);
}

fn covers_http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("http client"))
}

fn covers_limit() -> &'static tokio::sync::Semaphore {
    static S: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(6))
}

fn want_covers(ui: &Shared, wants: Wants) {
    let mut s = ui.borrow_mut();
    for (url, px, pic) in wants {
        s.waiting.entry(url.clone()).or_default().push(pic.downgrade());
        if s.requested.insert(url.clone()) {
            spawn(&s, async move {
                // A few at a time over one connection pool, so covers don't hold up a song.
                let pixels = async {
                    let _turn = covers_limit().acquire().await.ok()?;
                    let r = covers_http().get(&url).send().await.ok()?.error_for_status().ok()?;
                    let bytes = r.bytes().await.ok()?;
                    tokio::task::spawn_blocking(move || decode_cover(&bytes, px)).await.ok().flatten()
                }
                .await;
                Ev::Cover(url, pixels)
            });
        }
    }
}

fn toast(ui: &Shared, text: &str) {
    let id = {
        let mut s = ui.borrow_mut();
        s.toast_id += 1;
        s.w.toast_text.set_text(text);
        s.w.toast.set_reveal_child(!text.is_empty());
        s.toast_id
    };
    let weak = Rc::downgrade(ui);
    glib::timeout_add_local_once(Duration::from_secs(4), move || {
        if let Some(ui) = weak.upgrade()
            && let Ok(s) = ui.try_borrow()
            && s.toast_id == id
        {
            s.w.toast.set_reveal_child(false);
        }
    });
}

fn api_of(ui: &Shared) -> Option<Api> {
    ui.borrow().api.clone()
}

fn signed_in(ui: &Shared) -> bool {
    ui.borrow().api.as_ref().is_some_and(Api::logged_in)
}

fn now_playing(ui: &Shared) -> Option<Track> {
    let s = ui.borrow();
    s.current.and_then(|i| s.queue.get(i).cloned())
}

// ---------------------------------------------------------------- window

fn build(app: &Application) {
    let config = Config::load();
    rymplayer::i18n::set_arabic(config.arabic);
    // Arabic: the whole window reads right to left.
    gtk::Widget::set_default_direction(if config.arabic { gtk::TextDirection::Rtl } else { gtk::TextDirection::Ltr });
    stream::use_source(config.source);
    stream::download_best(config.download_best);
    stream::download_parallel(config.download_connections, config.download_piece_mb);

    let provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    apply_theme(&provider, config.gui_theme);
    let lyrics_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &lyrics_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
    lyrics_provider.load_from_string(&lyrics_css(rymplayer::colors::PLAIN));

    let window = ApplicationWindow::builder().application(app).title("RyM").default_width(1240).default_height(800).build();
    window.add_css_class("rym");

    // Header: the system's title buttons, search in the middle, refresh at the end.
    let header = HeaderBar::new();
    let search = gtk::Entry::new();
    search.add_css_class("search");
    search.set_placeholder_text(Some(tr("Search songs, albums, artists")));
    search.set_primary_icon_name(Some("system-search-symbolic"));
    search.set_width_chars(46);
    header.set_title_widget(Some(&search));
    let refresh = icon_button("view-refresh-symbolic", tr("Refresh: new songs on top (F5)"), &[]);
    header.pack_end(&refresh);
    window.set_titlebar(Some(&header));

    // Rail.
    let rail_box = gtk::Box::new(Orientation::Vertical, 4);
    rail_box.add_css_class("rail");
    let mut rail = Vec::new();
    for (page, icon, text) in [
        (Page::Home, "go-home-symbolic", tr("Home")),
        (Page::Liked, "", tr("Liked")),
        (Page::Library, "view-grid-symbolic", tr("Library")),
        (Page::History, "document-open-recent-symbolic", tr("History")),
        (Page::Settings, "emblem-system-symbolic", tr("Settings")),
    ] {
        let b = Button::new();
        let col = gtk::Box::new(Orientation::Vertical, 3);
        let img: gtk::Widget = if page == Page::Liked {
            heart_glyph(20).upcast()
        } else {
            let i = gtk::Image::from_icon_name(icon);
            i.set_pixel_size(22);
            i.upcast()
        };
        let l = Label::new(Some(text));
        l.add_css_class("rail-label");
        col.append(&img);
        col.append(&l);
        b.set_child(Some(&col));
        rail_box.append(&b);
        rail.push((page, b));
    }

    // Page area with a toast on top.
    let content = gtk::Box::new(Orientation::Vertical, 0);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&content));
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.add_css_class("page");
    let toast_text = label("", &[]);
    let toast_box = gtk::Box::new(Orientation::Horizontal, 0);
    toast_box.add_css_class("toast");
    toast_box.append(&toast_text);
    let toast_rev = gtk::Revealer::new();
    toast_rev.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    toast_rev.set_child(Some(&toast_box));
    toast_rev.set_halign(Align::Center);
    toast_rev.set_valign(Align::End);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scroll));
    overlay.add_overlay(&toast_rev);

    // Lyrics on the right.
    let lyrics_box = gtk::Box::new(Orientation::Vertical, 4);
    let lyrics_scroll = gtk::ScrolledWindow::new();
    lyrics_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    lyrics_scroll.set_child(Some(&lyrics_box));
    lyrics_scroll.set_vexpand(true);
    lyrics_scroll.set_size_request(440, -1);
    lyrics_scroll.add_css_class("lyrics");
    let lyrics_panel = gtk::Revealer::new();
    lyrics_panel.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
    lyrics_panel.set_reveal_child(true);
    lyrics_panel.set_child(Some(&lyrics_scroll));

    let middle = gtk::Box::new(Orientation::Horizontal, 0);
    middle.append(&rail_box);
    middle.append(&overlay);
    middle.append(&lyrics_panel);
    middle.set_vexpand(true);

    // Player bar: song on the left, controls in the middle, lyrics and volume on the right.
    let bar_cover = picture(52, "cover");
    let bar_title = label(tr("Nothing playing"), &["title"]);
    let bar_artist = label("", &["dim", "small"]);
    let names = gtk::Box::new(Orientation::Vertical, 2);
    names.set_valign(Align::Center);
    names.append(&bar_title);
    names.append(&bar_artist);
    names.set_hexpand(true);
    let bar_heart = heart_button(tr("Like"));
    bar_heart.set_valign(Align::Center);
    let left = gtk::Box::new(Orientation::Horizontal, 12);
    left.set_size_request(320, -1);
    left.append(&bar_cover);
    left.append(&names);
    left.append(&bar_heart);

    let prev = icon_button("media-skip-backward-symbolic", tr("Previous"), &[]);
    let play = Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("icon");
    play.add_css_class("play");
    play.set_tooltip_text(Some(tr("Play / pause")));
    let next = icon_button("media-skip-forward-symbolic", tr("Next"), &[]);
    let controls = gtk::Box::new(Orientation::Horizontal, 10);
    controls.set_halign(Align::Center);
    controls.append(&prev);
    controls.append(&play);
    controls.append(&next);
    let pos = label("0:00", &["dim", "small"]);
    let len = label("0:00", &["dim", "small"]);
    let progress = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.5);
    progress.set_draw_value(false);
    progress.set_hexpand(true);
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.append(&pos);
    line.append(&progress);
    line.append(&len);
    let center = gtk::Box::new(Orientation::Vertical, 2);
    center.set_hexpand(true);
    center.append(&controls);
    center.append(&line);

    let lyrics_toggle = icon_button("format-justify-left-symbolic", tr("Lyrics"), &[]);
    let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    let volume = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    volume.set_draw_value(false);
    volume.set_value(0.8);
    volume.set_size_request(110, -1);
    let right = gtk::Box::new(Orientation::Horizontal, 8);
    right.set_size_request(320, -1);
    right.set_halign(Align::End);
    right.set_valign(Align::Center);
    right.append(&lyrics_toggle);
    right.append(&vol_icon);
    right.append(&volume);

    let bar = gtk::Box::new(Orientation::Horizontal, 16);
    bar.add_css_class("bar");
    bar.append(&left);
    bar.append(&center);
    bar.append(&right);

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.append(&middle);
    root.append(&bar);
    window.set_child(Some(&root));

    let audio = match Audio::new() {
        Ok(a) => Some(a),
        Err(e) => {
            log(&format!("no audio device: {e}"));
            None
        }
    };
    if let Some(a) = &audio {
        a.set_volume(0.8);
    }
    let (tx, rx) = mpsc::channel();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("async runtime");
    let ui: Shared = Rc::new(RefCell::new(Ui {
        runtime,
        tx,
        rx,
        api: None,
        config,
        w: Widgets {
            window: window.clone(),
            provider,
            lyrics_provider,
            rail,
            content,
            scroll,
            toast: toast_rev,
            toast_text,
            lyrics_panel,
            lyrics_box,
            lyrics_scroll,
            bar_cover,
            bar_title,
            bar_artist,
            bar_heart: bar_heart.clone(),
            play: play.clone(),
            progress: progress.clone(),
            pos,
            len,
        },
        page: Page::Home,
        page_id: 0,
        home: Vec::new(),
        home_loading: false,
        tracks: Vec::new(),
        tracks_loading: false,
        playlists: Vec::new(),
        liked: Likes::default(),
        liked_tracks: Vec::new(),
        likes_synced: Instant::now(),
        queue: Vec::new(),
        current: None,
        request_id: 0,
        loading: false,
        radio_pending: false,
        audio,
        vlc: None,
        lyrics: None,
        lyric_line: None,
        covers: HashMap::new(),
        waiting: HashMap::new(),
        requested: HashSet::new(),
        hearts: Vec::new(),
        rows: Vec::new(),
        toast_id: 0,
        from_search: false,
        batch: None,
        batch_label: None,
        batch_bar: None,
        last_prev: Instant::now() - Duration::from_secs(60),
        shown_playing: None,
        shown_len: 0.0,
        shown_pos: None,
        shown_slider: -1.0,
        play_asked: Instant::now(),
    }));
    if ui.borrow().audio.is_none() {
        toast(&ui, tr("No audio device found"));
    }

    // ---- signals
    for (page, b) in ui.borrow().w.rail.iter() {
        let ui = ui.clone();
        let page = page.clone();
        b.connect_clicked(move |_| go(&ui, page.clone()));
    }
    {
        let ui = ui.clone();
        search.connect_activate(move |e| {
            let q = e.text().trim().to_string();
            if !q.is_empty() {
                search_songs(&ui, q);
            }
        });
    }
    {
        let ui = ui.clone();
        refresh.connect_clicked(move |_| refresh_home(&ui));
    }
    {
        // F5 refreshes, Ctrl+F goes to the search box, space pauses (outside the search box).
        let ui = ui.clone();
        let search = search.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if key == gdk::Key::F5 {
                refresh_home(&ui);
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::f && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                search.grab_focus();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
    }
    {
        // The bar's cover opens the lyrics (it never skips the song).
        let ui2 = ui.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| ui2.borrow().w.lyrics_panel.set_reveal_child(true));
        let s = ui.borrow();
        s.w.bar_cover.add_controller(click);
        s.w.bar_cover.set_cursor_from_name(Some("pointer"));
        s.w.bar_cover.set_tooltip_text(Some(tr("Lyrics")));
    }
    {
        let ui2 = ui.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            if let Some(t) = now_playing(&ui2) {
                go_artist(&ui2, &t);
            }
        });
        let s = ui.borrow();
        s.w.bar_artist.add_controller(click);
        s.w.bar_artist.set_cursor_from_name(Some("pointer"));
    }
    {
        let ui = ui.clone();
        bar_heart.connect_clicked(move |_| {
            if let Some(t) = now_playing(&ui) {
                toggle_like(&ui, &t);
            }
        });
    }
    {
        let ui = ui.clone();
        play.connect_clicked(move |_| toggle_playback(&ui));
    }
    {
        let ui = ui.clone();
        prev.connect_clicked(move |_| previous(&ui));
    }
    {
        let ui = ui.clone();
        next.connect_clicked(move |_| skip(&ui));
    }
    {
        // Only the user's own drags and clicks seek (the ticker sets the value too).
        let ui = ui.clone();
        progress.connect_change_value(move |_, _, v| {
            if let Some(a) = &ui.borrow().audio {
                a.seek(v.max(0.0) as f32);
            }
            glib::Propagation::Proceed
        });
    }
    {
        let ui = ui.clone();
        volume.connect_value_changed(move |s| {
            if let Ok(st) = ui.try_borrow()
                && let Some(a) = &st.audio
            {
                a.set_volume(s.value() as f32);
            }
        });
    }
    {
        let ui = ui.clone();
        lyrics_toggle.connect_clicked(move |b| {
            let st = ui.borrow();
            let open = !st.w.lyrics_panel.reveals_child();
            st.w.lyrics_panel.set_reveal_child(open);
            if open {
                b.remove_css_class("dim");
            } else {
                b.add_css_class("dim");
            }
        });
    }

    let poll = ui.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        let started = Instant::now();
        poll_events(&poll);
        let took = started.elapsed();
        if took > Duration::from_millis(40) {
            log(&format!("slow turn: {} ms (page {})", took.as_millis(), page_name(&poll.borrow().page)));
        }
        glib::ControlFlow::Continue
    });
    {
        // Which renderer GTK picked (GL on the graphics card, or cairo on the processor).
        let window = window.clone();
        glib::timeout_add_local_once(Duration::from_secs(2), move || {
            let renderer = window.native().and_then(|n| n.renderer()).map(|r| r.type_().name().to_string()).unwrap_or_default();
            log(&format!("started: renderer {renderer}, GSK_RENDERER={:?}", std::env::var("GSK_RENDERER").ok()));
        });
    }

    // Start: the last home feed at once, then sign in and fetch a fresh one.
    match std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty()) {
        Some(cookie) => {
            ui.borrow_mut().home = page_cache::load_home();
            post(&ui, async move { Ev::Auth(AuthMode::Saved, Api::with_cookie(cookie).await) });
        }
        None => post(&ui, async { Ev::Auth(AuthMode::Guest, Api::guest().await) }),
    }
    ui.borrow_mut().home_loading = true;
    render(&ui);
    window.present();
}

// ---------------------------------------------------------------- navigation

fn go(ui: &Shared, page: Page) {
    {
        let mut s = ui.borrow_mut();
        s.page = page.clone();
        s.page_id += 1;
    }
    match page {
        Page::Liked => {
            if let Some(api) = api_of(ui).filter(Api::logged_in) {
                let id = ui.borrow().page_id;
                ui.borrow_mut().tracks_loading = true;
                post(ui, async move { Ev::Tracks(id, api.playlist_tracks(LIKED_ID.into()).await) });
            }
            let liked = ui.borrow().liked_tracks.clone();
            ui.borrow_mut().tracks = liked;
        }
        Page::History => {
            if let Some(api) = api_of(ui).filter(Api::logged_in) {
                let id = ui.borrow().page_id;
                let mut s = ui.borrow_mut();
                s.tracks.clear();
                s.tracks_loading = true;
                spawn(&s, async move { Ev::Tracks(id, api.history().await) });
            }
        }
        _ => {}
    }
    render(ui);
}

/// Opens a list page and loads its songs.
fn open_songs(ui: &Shared, title: String, load: impl Future<Output = anyhow::Result<Vec<Track>>> + Send + 'static) {
    {
        let mut s = ui.borrow_mut();
        s.from_search = false;
        s.page = Page::Songs(title);
        s.page_id += 1;
        s.tracks.clear();
        s.tracks_loading = true;
        let id = s.page_id;
        spawn(&s, async move { Ev::Tracks(id, load.await) });
    }
    render(ui);
}

fn open_link(ui: &Shared, t: &Track) {
    let Some(api) = api_of(ui) else { return };
    match t.link.clone() {
        Some(Link::Playlist(id)) => open_songs(ui, t.title.clone(), api.playlist_tracks(id)),
        Some(Link::Album(id)) => open_songs(ui, t.title.clone(), api.album_tracks(id)),
        Some(Link::Mix(id)) => open_songs(ui, t.title.clone(), api.mix_tracks(id)),
        None => {}
    }
}

fn search_songs(ui: &Shared, q: String) {
    let Some(api) = api_of(ui) else {
        toast(ui, tr("Still connecting to YouTube Music"));
        return;
    };
    open_songs(ui, format!("{} \"{q}\"", tr("Results for")), api.search(q));
    ui.borrow_mut().from_search = true;
}

/// A fresh home feed: new songs on top, liked songs read on the way.
fn refresh_home(ui: &Shared) {
    let Some(api) = api_of(ui) else { return };
    {
        let mut s = ui.borrow_mut();
        if s.home_loading && !s.home.is_empty() {
            return;
        }
        s.home_loading = true;
        spawn(&s, async move { Ev::Home(feed::refresh(api).await) });
    }
    if ui.borrow().page == Page::Home {
        render(ui);
    } else {
        go(ui, Page::Home);
    }
}

// ---------------------------------------------------------------- pages

fn page_name(p: &Page) -> &'static str {
    match p {
        Page::Home => "home",
        Page::Liked => "liked",
        Page::Library => "library",
        Page::History => "history",
        Page::Settings => "settings",
        Page::Songs(_) => "songs",
    }
}

fn render(ui: &Shared) {
    let started = Instant::now();
    render_page(ui);
    let took = started.elapsed();
    if took > Duration::from_millis(30) {
        log(&format!("render {}: {} ms", page_name(&ui.borrow().page), took.as_millis()));
    }
}

fn render_page(ui: &Shared) {
    let mut wants = Wants::new();
    {
        let mut s = ui.borrow_mut();
        s.hearts.clear();
        s.rows.clear();
    }
    let page = {
        let s = ui.borrow();
        for (p, b) in &s.w.rail {
            if *p == s.page {
                b.add_css_class("active");
            } else {
                b.remove_css_class("active");
            }
        }
        clear(&s.w.content);
        s.w.scroll.vadjustment().set_value(0.0);
        s.page.clone()
    };
    let body: gtk::Widget = match &page {
        Page::Home => home_page(ui, &mut wants),
        Page::Liked => list_page(ui, tr("Liked Music"), true, &mut wants),
        Page::History => list_page(ui, tr("History"), false, &mut wants),
        Page::Songs(title) => list_page(ui, title, false, &mut wants),
        Page::Library => library_page(ui, &mut wants),
        Page::Settings => settings_page(ui),
    };
    ui.borrow().w.content.append(&body);
    want_covers(ui, wants);
    refresh_hearts(ui);
    update_batch(ui);
}

fn page_box() -> gtk::Box {
    let b = gtk::Box::new(Orientation::Vertical, 14);
    b.set_margin_top(22);
    b.set_margin_bottom(22);
    b.set_margin_start(26);
    b.set_margin_end(26);
    b
}

fn note(text: &str) -> gtk::Widget {
    let l = Label::new(Some(text));
    l.add_css_class("dim");
    l.set_margin_top(60);
    l.upcast()
}

fn home_page(ui: &Shared, wants: &mut Wants) -> gtk::Widget {
    let page = page_box();
    let (home, loading) = {
        let s = ui.borrow();
        (s.home.clone(), s.home_loading)
    };
    let top = gtk::Box::new(Orientation::Horizontal, 8);
    let h = label(tr("Home"), &["heading"]);
    h.set_hexpand(true);
    top.append(&h);
    let refresh = Button::with_label(if loading { tr("Refreshing…") } else { tr("Refresh") });
    refresh.add_css_class("ghost");
    refresh.set_sensitive(!loading);
    refresh.set_tooltip_text(Some(tr("New songs on top (F5)")));
    {
        let ui = ui.clone();
        refresh.connect_clicked(move |_| refresh_home(&ui));
    }
    top.append(&refresh);
    page.append(&top);
    if home.is_empty() {
        page.append(&note(if loading { tr("Loading your feed…") } else { tr("Nothing here yet") }));
        return page.upcast();
    }
    // Shelves in YouTube's order ("New for you" first), each a grid of cards.
    let mut shelves: Vec<(String, Vec<Track>)> = Vec::new();
    for t in home {
        match shelves.iter_mut().find(|(name, _)| *name == t.album) {
            Some((_, items)) => items.push(t),
            None => shelves.push((t.album.clone(), vec![t])),
        }
    }
    for (name, items) in shelves {
        page.append(&label(&name, &["shelf"]));
        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_homogeneous(true);
        grid.set_min_children_per_line(2);
        grid.set_max_children_per_line(5);
        grid.set_column_spacing(6);
        grid.set_row_spacing(6);
        for t in items {
            grid.insert(&home_card(ui, &t, wants), -1);
        }
        page.append(&grid);
    }
    page.upcast()
}

/// A home card: cover, title, and what it is. Songs play (with radio after), the rest open.
fn home_card(ui: &Shared, t: &Track, wants: &mut Wants) -> gtk::Widget {
    let b = Button::new();
    b.add_css_class("card");
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.append(&cover(&ui.borrow(), t.thumb.as_ref(), CARD_PX, 72, "cover", wants));
    let text = gtk::Box::new(Orientation::Vertical, 3);
    text.set_valign(Align::Center);
    text.set_hexpand(true);
    let kind = match &t.link {
        Some(Link::Playlist(_)) => tr("Playlist"),
        Some(Link::Album(_)) => tr("Album"),
        Some(Link::Mix(_)) => tr("Mix"),
        None => tr("Song"),
    };
    let title = label(&t.title, &["title"]);
    title.set_max_width_chars(28);
    text.append(&title);
    text.append(&label(&format!("{kind} · {}", t.artist), &["dim", "small"]));
    row.append(&text);
    b.set_child(Some(&row));
    b.set_size_request(260, -1);
    let track = t.clone();
    {
        let ui = ui.clone();
        let track = track.clone();
        b.connect_clicked(move |_| {
            feed::opened(&track);
            if track.link.is_some() {
                open_link(&ui, &track);
            } else {
                play_with_radio(&ui, track.clone());
            }
        });
    }
    if t.link.is_none() {
        song_menu_on(ui, &b, track);
    }
    b.upcast()
}

fn list_page(ui: &Shared, title: &str, liked_page: bool, wants: &mut Wants) -> gtk::Widget {
    let page = page_box();
    let (tracks, loading, signed) = {
        let s = ui.borrow();
        (s.tracks.clone(), s.tracks_loading, s.api.as_ref().is_some_and(Api::logged_in))
    };
    // Header: cover (or the heart tile), title, count, play button.
    let head = gtk::Box::new(Orientation::Horizontal, 20);
    let art: gtk::Widget = if liked_page {
        let tile = gtk::Box::new(Orientation::Vertical, 0);
        tile.add_css_class("tile");
        tile.set_size_request(140, 140);
        let img = heart_glyph(56);
        img.set_vexpand(true);
        tile.append(&img);
        tile.upcast()
    } else {
        cover(&ui.borrow(), tracks.first().and_then(|t| t.thumb.as_ref()), ART_PX, 140, "cover-big", wants).upcast()
    };
    art.set_valign(Align::End);
    head.append(&art);
    let info = gtk::Box::new(Orientation::Vertical, 8);
    info.set_valign(Align::End);
    info.append(&label(title, &["heading"]));
    let count = if loading && tracks.is_empty() { tr("Loading…").to_string() } else { format!("{} {}", tracks.len(), tr("songs")) };
    info.append(&label(&count, &["dim"]));
    let play_all = Button::with_label(tr("Play all"));
    play_all.add_css_class("pill");
    play_all.set_halign(Align::Start);
    play_all.set_sensitive(!tracks.is_empty());
    let songs: Rc<Vec<Track>> = Rc::new(tracks.iter().filter(|t| t.link.is_none()).cloned().collect());
    {
        let ui = ui.clone();
        let songs = songs.clone();
        play_all.connect_clicked(move |_| play_list(&ui, &songs, 0));
    }
    let buttons = gtk::Box::new(Orientation::Horizontal, 10);
    buttons.append(&play_all);
    if liked_page && signed {
        let count = gtk::Entry::new();
        count.set_placeholder_text(Some(tr("How many (empty = all)")));
        count.set_input_purpose(gtk::InputPurpose::Digits);
        count.set_width_chars(18);
        count.add_css_class("search");
        let all = Button::with_label(tr("Download all"));
        all.add_css_class("ghost");
        let start = {
            let ui = ui.clone();
            let count = count.clone();
            move || download_all(&ui, count.text().trim().parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(5000))
        };
        {
            let start = start.clone();
            all.connect_clicked(move |_| start());
        }
        count.connect_activate(move |_| start());
        buttons.append(&count);
        buttons.append(&all);
    }
    info.append(&buttons);
    if liked_page && signed {
        let words = label("", &["dim", "small"]);
        let bar = gtk::ProgressBar::new();
        bar.set_size_request(280, -1);
        bar.set_halign(Align::Start);
        info.append(&words);
        info.append(&bar);
        let mut s = ui.borrow_mut();
        s.batch_label = Some(words.downgrade());
        s.batch_bar = Some(bar.downgrade());
    }
    head.append(&info);
    page.append(&head);

    if tracks.is_empty() {
        let why = if loading {
            tr("Loading…")
        } else if !signed && matches!(ui.borrow().page, Page::Liked | Page::History) {
            tr("Sign in (Settings) to see this")
        } else {
            tr("No songs here")
        };
        page.append(&note(why));
        return page.upcast();
    }
    let list = gtk::Box::new(Orientation::Vertical, 2);
    let playing = now_playing(ui).map(|t| t.video_id);
    for (i, t) in tracks.iter().enumerate() {
        list.append(&song_row(ui, i, t, songs.clone(), playing.as_deref(), wants));
    }
    page.append(&list);
    page.upcast()
}

fn song_row(ui: &Shared, i: usize, t: &Track, songs: Rc<Vec<Track>>, playing: Option<&str>, wants: &mut Wants) -> gtk::Widget {
    let b = Button::new();
    b.add_css_class("flat-row");
    let row = gtk::Box::new(Orientation::Horizontal, 14);
    let n = Label::new(Some(&(i + 1).to_string()));
    n.add_css_class("dim");
    n.set_width_chars(3);
    n.set_xalign(1.0);
    row.append(&n);
    row.append(&cover(&ui.borrow(), t.thumb.as_ref(), ROW_PX, 42, "cover", wants));
    let names = gtk::Box::new(Orientation::Vertical, 2);
    names.set_valign(Align::Center);
    names.set_hexpand(true);
    names.append(&label(&t.title, &["title"]));
    names.append(&artist_label(ui, t, &["dim", "small"]));
    row.append(&names);
    let album = label(&t.album, &["dim", "small"]);
    album.set_width_chars(22);
    album.set_max_width_chars(22);
    row.append(&album);
    let heart = heart_button(tr("Like"));
    heart.set_valign(Align::Center);
    {
        let ui = ui.clone();
        let t = t.clone();
        heart.connect_clicked(move |_| toggle_like(&ui, &t));
    }
    row.append(&heart);
    let d = Label::new(Some(&t.duration));
    d.add_css_class("dim");
    d.add_css_class("small");
    d.set_width_chars(6);
    d.set_xalign(1.0);
    row.append(&d);
    b.set_child(Some(&row));
    if playing == Some(t.video_id.as_str()) {
        b.add_css_class("playing");
    }
    {
        let ui = ui.clone();
        let t = t.clone();
        b.connect_clicked(move |_| {
            if t.link.is_some() {
                open_link(&ui, &t);
            } else if ui.borrow().from_search {
                // A search result: the song, then a radio of songs like it.
                play_with_radio(&ui, t.clone());
            } else if let Some(at) = songs.iter().position(|s| s.video_id == t.video_id) {
                play_list(&ui, &songs, at);
            }
        });
    }
    song_menu_on(ui, &b, t.clone());
    {
        let mut s = ui.borrow_mut();
        s.hearts.push((t.video_id.clone(), heart.downgrade()));
        s.rows.push((t.video_id.clone(), b.downgrade()));
    }
    b.upcast()
}

fn library_page(ui: &Shared, wants: &mut Wants) -> gtk::Widget {
    let page = page_box();
    page.append(&label(tr("Library"), &["heading"]));
    let (playlists, liked_count, signed) = {
        let s = ui.borrow();
        (s.playlists.clone(), s.liked.len(), s.api.as_ref().is_some_and(Api::logged_in))
    };
    if !signed {
        page.append(&note(tr("Sign in (Settings) to see your playlists")));
        return page.upcast();
    }
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_max_children_per_line(6);
    grid.set_min_children_per_line(2);
    let card = |art: gtk::Widget, title: &str, sub: &str| {
        let b = Button::new();
        b.add_css_class("card");
        let col = gtk::Box::new(Orientation::Vertical, 6);
        col.append(&art);
        let t = label(title, &["title"]);
        t.set_max_width_chars(18);
        col.append(&t);
        col.append(&label(sub, &["dim", "small"]));
        b.set_child(Some(&col));
        b
    };
    let tile = gtk::Box::new(Orientation::Vertical, 0);
    tile.add_css_class("tile");
    tile.set_size_request(150, 150);
    let img = heart_glyph(52);
    img.set_vexpand(true);
    tile.append(&img);
    let liked = card(tile.upcast(), tr("Liked Music"), &format!("{liked_count} {}", tr("songs")));
    {
        let ui = ui.clone();
        liked.connect_clicked(move |_| go(&ui, Page::Liked));
    }
    grid.insert(&liked, -1);
    for p in playlists {
        let art = cover(&ui.borrow(), p.thumb.as_ref(), CARD_PX, 150, "cover-big", wants);
        let b = card(art.upcast(), &p.title, tr("Playlist"));
        let ui = ui.clone();
        b.connect_clicked(move |_| {
            if let Some(api) = api_of(&ui) {
                open_songs(&ui, p.title.clone(), api.playlist_tracks(p.id.clone()));
            }
        });
        grid.insert(&b, -1);
    }
    page.append(&grid);
    page.upcast()
}

fn setting_row(title: &str, about: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let row = gtk::Box::new(Orientation::Horizontal, 16);
    let text = gtk::Box::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.append(&label(title, &["title"]));
    let a = label(about, &["dim", "small"]);
    a.set_wrap(true);
    text.append(&a);
    row.append(&text);
    control.set_valign(Align::Center);
    row.append(control);
    row.upcast()
}

fn settings_page(ui: &Shared) -> gtk::Widget {
    let page = page_box();
    page.set_spacing(20);
    page.append(&label(tr("Settings"), &["heading"]));
    let config = ui.borrow().config.clone();

    let language = gtk::DropDown::from_strings(&["English", "العربية"]);
    language.set_selected(config.arabic as u32);
    {
        let ui = ui.clone();
        language.connect_selected_notify(move |d| {
            {
                let mut s = ui.borrow_mut();
                s.config.arabic = d.selected() == 1;
                rymplayer::i18n::set_arabic(s.config.arabic);
                s.config.save();
            }
            toast(&ui, tr("Restart the app to see everything in the new language"));
        });
    }
    page.append(&setting_row(tr("Language"), tr("The words in the apps"), &language));

    let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
    let theme = gtk::DropDown::from_strings(&names);
    theme.set_selected((config.gui_theme % THEMES.len()) as u32);
    {
        let ui = ui.clone();
        theme.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.gui_theme = d.selected() as usize;
            s.config.save();
            apply_theme(&s.w.provider, s.config.gui_theme);
        });
    }
    page.append(&setting_row(tr("Theme"), tr("Shared with RyM Legacy Player"), &theme));

    let source = gtk::DropDown::from_strings(&[tr("Auto"), "yt-dlp", "InnerTube"]);
    source.set_selected(match config.source {
        Source::Auto => 0,
        Source::YtDlp => 1,
        Source::InnerTube => 2,
    });
    {
        let ui = ui.clone();
        source.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.source = match d.selected() {
                1 => Source::YtDlp,
                2 => Source::InnerTube,
                _ => Source::Auto,
            };
            stream::use_source(s.config.source);
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Audio source"), tr("Auto: YouTube's player API, yt-dlp when a song is refused"), &source));

    let quality = gtk::DropDown::from_strings(&[tr("Best (yt-dlp)"), tr("Fast (playback file)")]);
    quality.set_selected(if config.download_best { 0 } else { 1 });
    {
        let ui = ui.clone();
        quality.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.download_best = d.selected() == 0;
            stream::download_best(s.config.download_best);
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Download quality"), tr("Songs saved with right-click > Download go to Music/RymPlayer"), &quality));

    let steps = ["1", "2", "4", "6", "8", "10", "12", "16"];
    let connections = gtk::DropDown::from_strings(&steps);
    connections.set_selected(steps.iter().position(|s| *s == config.download_connections.to_string()).unwrap_or(4) as u32);
    {
        let ui = ui.clone();
        connections.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.download_connections = steps[d.selected() as usize].parse().unwrap_or(8);
            stream::download_parallel(s.config.download_connections, s.config.download_piece_mb);
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Download connections"), tr("Pieces of a song downloaded at once"), &connections));

    if vlc::find().is_some() {
        let player = gtk::DropDown::from_strings(&["RyM", "VLC"]);
        player.set_selected(config.play_in_vlc as u32);
        {
            let ui = ui.clone();
            player.connect_selected_notify(move |d| {
                let mut s = ui.borrow_mut();
                s.config.play_in_vlc = d.selected() == 1;
                s.config.save();
                if !s.config.play_in_vlc {
                    vlc::stop(&mut s.vlc);
                }
            });
        }
        page.append(&setting_row(tr("Play songs in"), tr("VLC: each song opens in VLC"), &player));
    }

    let signed = signed_in(ui);
    let account = Button::with_label(if signed { tr("Sign out") } else { tr("Sign in with Google") });
    account.add_css_class(if signed { "ghost" } else { "pill" });
    {
        let ui = ui.clone();
        account.connect_clicked(move |_| sign_in_or_out(&ui));
    }
    page.append(&setting_row(
        tr("Account"),
        if signed { tr("Signed in: likes and playlists sync with your phone") } else { tr("Sign in for your likes, playlists and history") },
        &account,
    ));
    page.upcast()
}

/// Right-click (or a long press on touch screens) on a song: play next, add to queue, radio,
/// like, download, go to the artist.
fn song_menu_on(ui: &Shared, widget: &impl IsA<gtk::Widget>, t: Track) {
    let anchor = widget.clone().upcast::<gtk::Widget>();
    let click = gtk::GestureClick::new();
    click.set_button(3);
    {
        let (weak, anchor, t) = (Rc::downgrade(ui), anchor.clone(), t.clone());
        click.connect_pressed(move |_, _, x, y| {
            if let Some(ui) = weak.upgrade() {
                song_menu(&ui, &anchor, x, y, &t);
            }
        });
    }
    widget.add_controller(click);
    let hold = gtk::GestureLongPress::new();
    let weak = Rc::downgrade(ui);
    hold.connect_pressed(move |g, x, y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        if let Some(ui) = weak.upgrade() {
            song_menu(&ui, &anchor, x, y, &t);
        }
    });
    widget.add_controller(hold);
}

fn song_menu(ui: &Shared, anchor: &gtk::Widget, x: f64, y: f64, t: &Track) {
    let pop = gtk::Popover::new();
    pop.set_parent(anchor);
    pop.set_has_arrow(false);
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let col = gtk::Box::new(Orientation::Vertical, 0);
    let liked = ui.borrow().liked.contains(&t.video_id);
    let mut items: Vec<(&str, Box<dyn Fn(&Shared, &Track)>)> = vec![
        (tr("Play next"), Box::new(|ui: &Shared, t: &Track| play_next(ui, t.clone()))),
        (tr("Add to queue"), Box::new(|ui: &Shared, t: &Track| add_to_queue(ui, t.clone()))),
        (tr("Start radio"), Box::new(|ui: &Shared, t: &Track| play_with_radio(ui, t.clone()))),
        (if liked { tr("Remove from liked songs") } else { tr("Add to liked songs") }, Box::new(|ui: &Shared, t: &Track| toggle_like(ui, t))),
        (tr("Download"), Box::new(|ui: &Shared, t: &Track| download(ui, t.clone()))),
    ];
    if !t.artist.is_empty() && !rymplayer::local::is_local(&t.video_id) {
        items.push((tr("Go to artist"), Box::new(|ui: &Shared, t: &Track| go_artist(ui, t))));
    }
    for (text, act) in items {
        let b = Button::with_label(text);
        if let Some(l) = b.child().and_downcast::<Label>() {
            l.set_xalign(0.0);
        }
        let (ui, t, pop2) = (ui.clone(), t.clone(), pop.clone());
        b.connect_clicked(move |_| {
            pop2.popdown();
            act(&ui, &t);
        });
        col.append(&b);
    }
    pop.set_child(Some(&col));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

/// An artist's name that opens their page when clicked.
fn artist_label(ui: &Shared, t: &Track, classes: &[&str]) -> Label {
    let l = label(&t.artist, classes);
    if t.link.is_none() && !t.artist.is_empty() && !rymplayer::local::is_local(&t.video_id) {
        l.set_cursor_from_name(Some("pointer"));
        let click = gtk::GestureClick::new();
        let (weak, t) = (Rc::downgrade(ui), t.clone());
        click.connect_released(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(ui) = weak.upgrade() {
                go_artist(&ui, &t);
            }
        });
        l.add_controller(click);
    }
    l
}

/// The artist's page: every song, then albums and singles. Songs without the artist's id find
/// it by name first.
fn go_artist(ui: &Shared, t: &Track) {
    let Some(api) = api_of(ui) else { return };
    let name = t.artist.split(", ").next().unwrap_or(&t.artist).to_string();
    match t.artist_id.clone() {
        Some(id) => open_songs(ui, name, api.artist_page(id)),
        None => {
            toast(ui, tr("Finding the artist..."));
            post(ui, async move { Ev::ArtistFound(name.clone(), api.artist_id_for(name).await) });
        }
    }
}

/// Downloads the liked songs (the first `limit`), one after another, skipping ones already in
/// Music/RymPlayer; the Liked page shows how it goes.
fn download_all(ui: &Shared, limit: usize) {
    let Some(api) = api_of(ui).filter(Api::logged_in) else { return };
    let s = ui.borrow();
    if s.batch.is_some_and(|b| !b.finished()) {
        return;
    }
    let tx = s.tx.clone();
    s.runtime.spawn(async move {
        let songs = api.playlist_all(rymplayer::api::LIKED_ID.into(), limit).await.unwrap_or_default();
        stream::download_many(songs, |b| {
            let _ = tx.send(Ev::Batch(b));
        })
        .await;
    });
}

fn update_batch(ui: &Shared) {
    let s = ui.borrow();
    let Some(b) = s.batch else { return };
    let mut words = format!("{} {} / {}", tr("Downloading songs"), b.done, b.total);
    for (n, what) in [(b.saved, tr("saved")), (b.skipped, tr("already there")), (b.failed, tr("failed"))] {
        if n > 0 {
            words.push_str(&format!(" · {n} {what}"));
        }
    }
    if let Some(l) = s.batch_label.as_ref().and_then(glib::WeakRef::upgrade) {
        l.set_text(&words);
    }
    if let Some(p) = s.batch_bar.as_ref().and_then(glib::WeakRef::upgrade) {
        p.set_fraction(if b.total > 0 { b.done as f64 / b.total as f64 } else { 0.0 });
    }
}

// ---------------------------------------------------------------- likes

fn refresh_hearts(ui: &Shared) {
    let s = ui.borrow();
    for (id, b) in &s.hearts {
        if let Some(b) = b.upgrade() {
            if s.liked.contains(id) {
                b.add_css_class("on");
            } else {
                b.remove_css_class("on");
            }
        }
    }
    let on = s.current.and_then(|i| s.queue.get(i)).is_some_and(|t| s.liked.contains(&t.video_id));
    if on {
        s.w.bar_heart.add_css_class("on");
    } else {
        s.w.bar_heart.remove_css_class("on");
    }
}

fn sync_liked(ui: &Shared, tracks: Vec<Track>) {
    let rerender = {
        let mut s = ui.borrow_mut();
        s.liked.sync(tracks.iter().map(|t| t.video_id.clone()));
        s.likes_synced = Instant::now();
        s.liked_tracks = tracks.clone();
        if s.page == Page::Liked && !s.tracks_loading {
            let changed = s.tracks.len() != tracks.len() || s.tracks.iter().zip(&tracks).any(|(a, b)| a.video_id != b.video_id);
            s.tracks = tracks;
            changed
        } else {
            false
        }
    };
    if rerender {
        render(ui);
    } else {
        refresh_hearts(ui);
    }
}

fn toggle_like(ui: &Shared, t: &Track) {
    let Some(api) = api_of(ui).filter(Api::logged_in) else {
        toast(ui, tr("Sign in to like songs"));
        return;
    };
    let vid = t.video_id.clone();
    let (like, on_liked) = {
        let mut s = ui.borrow_mut();
        let like = !s.liked.contains(&vid);
        s.liked.set(&vid, like);
        if like {
            if !s.liked_tracks.iter().any(|x| x.video_id == vid) {
                s.liked_tracks.insert(0, t.clone());
            }
        } else {
            s.liked_tracks.retain(|x| x.video_id != vid);
        }
        let on_liked = s.page == Page::Liked;
        if on_liked {
            s.tracks = s.liked_tracks.clone();
        }
        (like, on_liked)
    };
    // The Liked page follows at once.
    if on_liked {
        render(ui);
    } else {
        refresh_hearts(ui);
    }
    post(ui, async move { Ev::Rated(vid.clone(), like, api.rate(vid, like).await) });
}

fn sync_likes(ui: &Shared) {
    let Some(api) = api_of(ui).filter(Api::logged_in) else { return };
    ui.borrow_mut().likes_synced = Instant::now();
    post(ui, async move { Ev::Liked(api.playlist_tracks(LIKED_ID.into()).await) });
}

// ---------------------------------------------------------------- playback

/// Plays `songs` from `i`; the queue becomes them.
fn play_list(ui: &Shared, songs: &[Track], i: usize) {
    if songs.is_empty() {
        return;
    }
    ui.borrow_mut().queue = songs.to_vec();
    play_at(ui, i.min(songs.len() - 1));
}

/// One song, then YouTube Music's radio after it.
fn play_with_radio(ui: &Shared, t: Track) {
    ui.borrow_mut().queue = vec![t];
    play_at(ui, 0);
    continue_with_radio(ui);
}

fn play_next(ui: &Shared, t: Track) {
    let empty = {
        let mut s = ui.borrow_mut();
        match s.current {
            Some(i) => {
                s.queue.insert(i + 1, t);
                false
            }
            None => {
                s.queue = vec![t];
                true
            }
        }
    };
    if empty {
        play_at(ui, 0);
    } else {
        toast(ui, tr("Plays next"));
    }
}

fn add_to_queue(ui: &Shared, t: Track) {
    let start = {
        let mut s = ui.borrow_mut();
        s.queue.push(t);
        s.current.is_none().then_some(s.queue.len() - 1)
    };
    match start {
        Some(i) => play_at(ui, i),
        None => toast(ui, tr("Added to the queue")),
    }
}

fn download(ui: &Shared, t: Track) {
    toast(ui, &format!("Downloading \"{}\"…", t.title));
    post(ui, async move { Ev::Downloaded(stream::save_to_music(&t.video_id, &t.artist, &t.title).await) });
}

fn play_at(ui: &Shared, i: usize) {
    let mut wants = Wants::new();
    let (track, id, api, in_vlc) = {
        let mut s = ui.borrow_mut();
        let Some(track) = s.queue.get(i).cloned() else { return };
        s.request_id += 1;
        s.current = Some(i);
        s.loading = true;
        s.lyrics = None;
        s.lyric_line = None;
        if let Some(a) = &mut s.audio {
            a.stop();
        }
        s.w.bar_title.set_text(&track.title);
        s.w.bar_artist.set_text(&track.artist);
        s.w.play.set_icon_name("content-loading-symbolic");
        s.shown_playing = None;
        s.shown_len = 0.0;
        s.shown_pos = None;
        s.shown_slider = -1.0;
        s.w.progress.set_value(0.0);
        s.w.pos.set_text("0:00");
        s.w.len.set_text(if track.duration.is_empty() { "0:00" } else { &track.duration });
        s.w.window.set_title(Some(&format!("{} · {} — RyM", track.title, track.artist)));
        let bar = cover(&s, track.thumb.as_ref(), ROW_PX, 52, "cover", &mut wants);
        match bar.paintable() {
            Some(p) => s.w.bar_cover.set_paintable(Some(&p)),
            None => {
                s.w.bar_cover.set_paintable(gdk::Paintable::NONE);
                if let Some((url, px, _)) = wants.pop() {
                    wants.push((url, px, s.w.bar_cover.clone()));
                }
            }
        }
        for (vid, b) in &s.rows {
            if let Some(b) = b.upgrade() {
                if *vid == track.video_id {
                    b.add_css_class("playing");
                } else {
                    b.remove_css_class("playing");
                }
            }
        }
        clear(&s.w.lyrics_box);
        s.w.lyrics_box.append(&label(tr("Looking for lyrics…"), &["lyrics-note"]));
        let in_vlc = s.config.play_in_vlc && vlc::find().is_some();
        if !in_vlc {
            vlc::stop(&mut s.vlc);
        }
        (track, s.request_id, s.api.clone(), in_vlc)
    };
    want_covers(ui, wants);
    refresh_hearts(ui);

    log(&format!("play {} ({}){}", track.title, track.video_id, if in_vlc { " in VLC" } else { "" }));
    {
        let (vid, thumb) = (track.video_id.clone(), track.thumb.clone());
        post(ui, async move { Ev::LyricsBg(vid, rymplayer::colors::for_cover(thumb).await) });
    }
    ui.borrow_mut().play_asked = Instant::now();
    let vid = track.video_id.clone();
    if in_vlc {
        post(ui, async move { Ev::InVlc(id, stream::link(&vid).await) });
    } else {
        post(ui, async move { Ev::Fetched(id, stream::open(vid).await) });
    }
    if let Some(api) = api.clone() {
        // Tells YouTube Music it was played (history, recommendations); nothing waits for it.
        let vid = track.video_id.clone();
        post(ui, async move {
            let _ = api.report_play(vid).await;
            Ev::Done
        });
    }
    post(ui, async move {
        let vid = track.video_id.clone();
        Ev::Lyrics(vid, rymplayer::lyrics::for_track(api, track).await)
    });
}

/// The next song (and its lyrics) download while this one plays.
fn prefetch_next(ui: &Shared) {
    let (next, api) = {
        let s = ui.borrow();
        (s.current.and_then(|i| s.queue.get(i + 1)).cloned(), s.api.clone())
    };
    if let Some(t) = next.clone() {
        post(ui, async move {
            rymplayer::lyrics::prefetch(api, t).await;
            Ev::Done
        });
    }
    if let Some(id) = next.map(|t| t.video_id).filter(|id| stream::cached(id).is_none()) {
        post(ui, async move {
            let _ = stream::fetch(id).await;
            Ev::Done
        });
    }
}

fn toggle_playback(ui: &Shared) {
    let (loaded, current) = {
        let s = ui.borrow();
        (s.audio.as_ref().is_some_and(Audio::is_loaded), s.current)
    };
    if loaded {
        if let Some(a) = &ui.borrow().audio {
            a.toggle();
        }
    } else if let Some(i) = current {
        play_at(ui, i);
    } else {
        let songs: Vec<Track> = {
            let s = ui.borrow();
            let list = if s.page == Page::Home { &s.home } else { &s.tracks };
            list.iter().filter(|t| t.link.is_none()).cloned().collect()
        };
        play_list(ui, &songs, 0);
    }
}

fn skip(ui: &Shared) {
    let (current, len) = {
        let s = ui.borrow();
        (s.current, s.queue.len())
    };
    match current {
        Some(i) if i + 1 < len => play_at(ui, i + 1),
        Some(_) => continue_with_radio(ui),
        None => {}
    }
}

/// Previous: back to the start of the song after its first 3 seconds; before that (or pressed
/// twice in a row), the song before it.
fn previous(ui: &Shared) {
    let (current, pos, again) = {
        let mut s = ui.borrow_mut();
        let again = s.last_prev.elapsed() < Duration::from_millis(1500);
        s.last_prev = Instant::now();
        (s.current, s.audio.as_ref().filter(|a| a.is_loaded()).map_or(0.0, Audio::position), again)
    };
    match current {
        Some(i) if (pos < 3.0 || again) && i > 0 => play_at(ui, i - 1),
        Some(_) => {
            if let Some(a) = &ui.borrow().audio {
                a.seek(0.0);
            }
        }
        None => {}
    }
}

/// The queue ran out (or a single song plays): YouTube Music's radio keeps it going.
fn continue_with_radio(ui: &Shared) {
    let (Some(api), Some(t)) = (api_of(ui), now_playing(ui)) else { return };
    {
        let mut s = ui.borrow_mut();
        if s.radio_pending {
            return;
        }
        s.radio_pending = true;
    }
    post(ui, async move { Ev::Radio(api.radio(t.video_id).await) });
}

fn show_lyrics(ui: &Shared, lyrics: Option<Lyrics>) {
    let mut s = ui.borrow_mut();
    clear(&s.w.lyrics_box);
    s.lyric_line = None;
    let Some(lyrics) = lyrics else {
        s.w.lyrics_box.append(&label(tr("No lyrics for this song"), &["lyrics-note"]));
        s.lyrics = None;
        return;
    };
    let lines: Vec<String> = if lyrics.synced.is_empty() {
        lyrics.plain.clone()
    } else {
        lyrics.synced.iter().map(|(_, l)| if l.is_empty() { "♪".to_string() } else { l.clone() }).collect()
    };
    if lyrics.synced.is_empty() {
        let note = label(tr("These lyrics aren't synced to the song"), &["lyrics-note"]);
        note.set_margin_bottom(10);
        s.w.lyrics_box.append(&note);
    }
    for (i, line) in lines.into_iter().enumerate() {
        let l = Label::new(Some(&line));
        if lyrics.synced.is_empty() {
            l.add_css_class("now");
        }
        l.set_xalign(0.0);
        l.set_wrap(true);
        l.add_css_class("lyric");
        // Synced lines seek there when clicked.
        if let Some(&(at, _)) = lyrics.synced.get(i) {
            let click = gtk::GestureClick::new();
            let weak = Rc::downgrade(ui);
            click.connect_released(move |_, _, _, _| {
                if let Some(ui) = weak.upgrade()
                    && let Some(a) = &ui.borrow().audio
                {
                    a.seek(at);
                }
            });
            l.add_controller(click);
        }
        s.w.lyrics_box.append(&l);
    }
    let src = label(&format!("{} {}", tr("Lyrics from"), lyrics.source), &["lyrics-note", "small"]);
    src.set_margin_top(18);
    s.w.lyrics_box.append(&src);
    s.lyrics = Some(lyrics);
}

/// Scrolls to `target` over a third of a second, easing out, in step with the screen's frames.
fn glide(scroll: &gtk::ScrolledWindow, target: f64) {
    let adj = scroll.vadjustment();
    let from = adj.value();
    let started = Rc::new(std::cell::Cell::new(None::<i64>));
    scroll.add_tick_callback(move |_, clock| {
        let now = clock.frame_time();
        let t0 = *started.get().get_or_insert(now);
        started.set(Some(t0));
        let t = ((now - t0) as f64 / 330_000.0).min(1.0);
        let k = 1.0 - (1.0 - t).powi(3);
        adj.set_value(from + (target - from) * k);
        if t >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
    });
}

fn tick(ui: &Shared) {
    let mut s = ui.borrow_mut();
    let Some((pos, dur, loaded, playing, finished)) =
        s.audio.as_ref().map(|a| (a.position(), a.duration().unwrap_or(0.0), a.is_loaded(), a.is_playing(), a.finished()))
    else {
        return;
    };
    if s.loading {
        return;
    }
    // Widgets are touched only when what they show changed: each change makes GTK lay out again.
    if s.shown_playing != Some(playing) {
        s.shown_playing = Some(playing);
        s.w.play.set_icon_name(if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
    }
    if loaded && dur > 0.0 {
        if s.shown_len != dur {
            s.shown_len = dur;
            s.w.progress.set_range(0.0, dur as f64);
            s.w.len.set_text(&fmt_time(dur));
        }
        // The slider moves only when it would move by a whole pixel.
        let px_per_sec = s.w.progress.width().max(1) as f32 / dur;
        if (pos - s.shown_slider).abs() * px_per_sec >= 1.0 {
            s.shown_slider = pos;
            s.w.progress.set_value(pos.min(dur) as f64);
        }
        let sec = pos as u32;
        if s.shown_pos != Some(sec) {
            s.shown_pos = Some(sec);
            s.w.pos.set_text(&fmt_time(pos));
        }
    }
    let line = s.lyrics.as_ref().and_then(|l| rymplayer::lyrics::current_line(&l.synced, pos));
    if line != s.lyric_line {
        let mut child = s.w.lyrics_box.first_child();
        let mut i = 0;
        let mut target = None;
        while let Some(c) = child {
            // Sung lines soft white, the sung one white, the rest dark (colors in lyrics_css).
            match line {
                Some(n) if i < n => {
                    c.add_css_class("sung");
                    c.remove_css_class("now");
                }
                Some(n) if i == n => {
                    c.remove_css_class("sung");
                    c.add_css_class("now");
                    target = Some(c.clone());
                }
                _ => {
                    c.remove_css_class("sung");
                    c.remove_css_class("now");
                }
            }
            child = c.next_sibling();
            i += 1;
        }
        // Keep the sung line about a third of the way down.
        if let Some(c) = target {
            let adj = s.w.lyrics_scroll.vadjustment();
            if let Some(p) = c.compute_point(&s.w.lyrics_box, &gtk::graphene::Point::new(0.0, 0.0)) {
                let y = p.y() as f64 - adj.page_size() / 3.0;
                glide(&s.w.lyrics_scroll, y.clamp(adj.lower(), (adj.upper() - adj.page_size()).max(adj.lower())));
            }
        }
        s.lyric_line = line;
    }
    if loaded && finished && s.current.is_some() {
        if let Some(a) = &mut s.audio {
            a.stop();
        }
        drop(s);
        skip(ui);
    }
}

// ---------------------------------------------------------------- background results

fn sign_in_or_out(ui: &Shared) {
    if signed_in(ui) {
        let _ = std::fs::remove_file(cookie_path());
        page_cache::clear();
        {
            let mut s = ui.borrow_mut();
            if let Some(a) = &mut s.audio {
                a.stop();
            }
            s.liked.clear();
            s.liked_tracks.clear();
            s.playlists.clear();
            s.home.clear();
            s.home_loading = true;
        }
        post(ui, async { Ev::Auth(AuthMode::Guest, Api::guest().await) });
        go(ui, Page::Home);
        return;
    }
    toast(ui, tr("Finish signing in in the Google window"));
    post(ui, async {
        let r = async {
            let cookie = login::sign_in().await?;
            Api::with_cookie(cookie).await
        }
        .await;
        Ev::Auth(AuthMode::NewLogin, r)
    });
}

fn poll_events(ui: &Shared) {
    loop {
        let ev = ui.borrow().rx.try_recv();
        let Ok(ev) = ev else { break };
        match ev {
            Ev::Auth(mode, Ok(api)) => {
                let user = api.logged_in();
                ui.borrow_mut().api = Some(api.clone());
                if user {
                    post(ui, async move { Ev::Playlists(api.library_playlists().await) });
                }
                if matches!(mode, AuthMode::NewLogin) {
                    toast(ui, tr("Signed in"));
                }
                ui.borrow_mut().home_loading = false;
                refresh_home(ui);
                if ui.borrow().page == Page::Settings {
                    render(ui);
                }
            }
            Ev::Auth(mode, Err(e)) => match mode {
                AuthMode::Saved => {
                    toast(ui, tr("Your saved sign-in ended: continuing as a guest"));
                    post(ui, async { Ev::Auth(AuthMode::Guest, Api::guest().await) });
                }
                AuthMode::NewLogin => toast(ui, &format!("{}: {e}", tr("Sign-in failed"))),
                AuthMode::Guest => {
                    ui.borrow_mut().home_loading = false;
                    toast(ui, &format!("Can't reach YouTube Music: {e}"));
                    render(ui);
                }
            },
            Ev::Home(result) => {
                ui.borrow_mut().home_loading = false;
                match result {
                    Ok(fresh) => {
                        if let Some(liked) = fresh.liked {
                            sync_liked(ui, liked);
                        }
                        page_cache::save_home(&fresh.home);
                        let ids: Vec<String> = fresh.home.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect();
                        ui.borrow_mut().home = fresh.home;
                        // The first songs' links are looked up now, so a click starts at once.
                        post(ui, async move {
                            stream::look_ahead(ids).await;
                            Ev::Done
                        });
                    }
                    Err(e) => toast(ui, &format!("{}: {e}", tr("Home"))),
                }
                if ui.borrow().page == Page::Home {
                    render(ui);
                }
            }
            Ev::Tracks(id, result) => {
                if id != ui.borrow().page_id {
                    continue;
                }
                ui.borrow_mut().tracks_loading = false;
                match result {
                    Ok(tracks) => {
                        if ui.borrow().page == Page::Liked {
                            sync_liked(ui, tracks.clone());
                        }
                        let ids: Vec<String> = tracks.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect();
                        ui.borrow_mut().tracks = tracks;
                        post(ui, async move {
                            stream::look_ahead(ids).await;
                            Ev::Done
                        });
                    }
                    Err(e) => toast(ui, &e.to_string()),
                }
                render(ui);
            }
            Ev::Playlists(Ok(p)) => {
                ui.borrow_mut().playlists = p;
                if ui.borrow().page == Page::Library {
                    render(ui);
                }
            }
            Ev::Playlists(Err(e)) => toast(ui, &format!("Playlists: {e}")),
            Ev::Liked(Ok(t)) => sync_liked(ui, t),
            Ev::Liked(Err(_)) => {}
            Ev::Cover(url, pixels) => {
                let texture = pixels.map(|(w, h, data)| -> gdk::Texture {
                    gdk::MemoryTexture::new(w as i32, h as i32, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(data), w as usize * 4)
                        .upcast()
                });
                let mut s = ui.borrow_mut();
                let waiting = s.waiting.remove(&url).unwrap_or_default();
                match texture {
                    Some(t) => {
                        for p in waiting.iter().filter_map(glib::WeakRef::upgrade) {
                            p.set_paintable(Some(&t));
                        }
                        s.covers.insert(url, t);
                    }
                    // Let a later page ask again.
                    None => {
                        s.requested.remove(&url);
                    }
                }
            }
            Ev::Fetched(id, result) => {
                let mut s = ui.borrow_mut();
                if id != s.request_id {
                    continue;
                }
                s.loading = false;
                let Some(t) = s.current.and_then(|i| s.queue.get(i).cloned()) else { continue };
                let known = parse_duration(&t.duration).or_else(|| stream::cached_duration(&t.video_id));
                let started = match (result, &mut s.audio) {
                    (Ok(song), Some(a)) => a.play(&song, known).map_err(|e| e.to_string()),
                    (Ok(_), None) => Err("no audio device".into()),
                    (Err(e), _) => Err(e.to_string()),
                };
                let ms = s.play_asked.elapsed().as_millis();
                drop(s);
                match &started {
                    Ok(()) => log(&format!("  playing after {ms} ms")),
                    Err(e) => log(&format!("  can't play after {ms} ms: {e}")),
                }
                match started {
                    Ok(()) => prefetch_next(ui),
                    Err(e) => {
                        ui.borrow().w.play.set_icon_name("media-playback-start-symbolic");
                        toast(ui, &format!("Can't play \"{}\": {e}", t.title));
                    }
                }
            }
            Ev::InVlc(id, result) => {
                let mut s = ui.borrow_mut();
                if id != s.request_id {
                    continue;
                }
                s.loading = false;
                let Some(t) = s.current.and_then(|i| s.queue.get(i).cloned()) else { continue };
                let msg = match (result, vlc::find()) {
                    (Ok((url, headers)), Some(exe)) => {
                        vlc::stop(&mut s.vlc);
                        match vlc::play(&exe, &url, &headers, &format!("{} - {}", t.artist, t.title)) {
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
                toast(ui, &msg);
            }
            Ev::LyricsBg(vid, bg) => {
                if now_playing(ui).is_some_and(|t| t.video_id == vid) {
                    ui.borrow().w.lyrics_provider.load_from_string(&lyrics_css(bg));
                }
            }
            Ev::Lyrics(vid, found) => {
                log(&format!("  lyrics for {vid}: {}", found.as_ref().map_or("none".to_string(), |l| format!("{} synced lines, from {}", l.synced.len(), l.source))));
                if now_playing(ui).is_some_and(|t| t.video_id == vid) {
                    show_lyrics(ui, found);
                }
            }
            Ev::Rated(vid, like, Err(e)) => {
                // Take back the heart that was shown right away.
                ui.borrow_mut().liked.set(&vid, !like);
                refresh_hearts(ui);
                toast(ui, &format!("{}: {e}", tr("Like failed")));
            }
            Ev::Rated(..) => {}
            Ev::Radio(result) => {
                ui.borrow_mut().radio_pending = false;
                if let Ok(tracks) = result {
                    let mut s = ui.borrow_mut();
                    let fresh: Vec<Track> = tracks.into_iter().filter(|t| !s.queue.iter().any(|q| q.video_id == t.video_id)).collect();
                    let at_end = s.current.is_some_and(|i| i + 1 >= s.queue.len()) && !s.loading && s.audio.as_ref().is_some_and(|a| !a.is_loaded());
                    let next = s.queue.len();
                    s.queue.extend(fresh);
                    drop(s);
                    if at_end && next < ui.borrow().queue.len() {
                        play_at(ui, next);
                    } else {
                        prefetch_next(ui);
                    }
                }
            }
            Ev::Downloaded(Ok(p)) => toast(ui, &format!("{} {}", tr("Saved to"), p.display())),
            Ev::Downloaded(Err(e)) => toast(ui, &format!("{}: {e}", tr("Download failed"))),
            Ev::ArtistFound(name, Ok(Some(id))) => {
                if let Some(api) = api_of(ui) {
                    open_songs(ui, name, api.artist_page(id));
                }
            }
            Ev::ArtistFound(..) => toast(ui, tr("Artist not found")),
            Ev::Batch(b) => {
                ui.borrow_mut().batch = Some(b);
                update_batch(ui);
            }
            Ev::Done => {}
        }
    }
    let due = {
        let s = ui.borrow();
        s.api.as_ref().is_some_and(Api::logged_in) && s.likes_synced.elapsed() > SYNC_EVERY
    };
    if due {
        // Likes made on the phone show here too.
        sync_likes(ui);
    }
    tick(ui);
}
