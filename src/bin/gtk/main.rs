#![windows_subsystem = "windows"]
//! The GTK 4 window app, laid out like the iced one: an icon rail on the left, the page in the
//! middle (home shelves of cards, song lists with covers, library, history, settings), lyrics
//! that slide in on the right, and a slim player bar with the cover at the bottom. Same themes
//! as the iced app (shared `gui_theme` setting).
//! Build: `cargo build --release --features gtk-ui --bin RymPlayer-GTK`.

use std::cell::{Cell, RefCell};
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
use rymplayer::player_core as core;

mod fx_view;
use rymplayer::config::{Config, Source};
use rymplayer::likes::{Likes, SYNC_EVERY};
use rymplayer::lyrics::Lyrics;
use rymplayer::{feed, fmt_time, login, page_cache, parse_duration, stream, vlc};

const APP_ID: &str = "com.rymplayer.Gtk";
/// Cover sizes requested from Google (pixels; drawn at about half for sharp HiDPI).
const ROW_PX: u32 = 104;
const CARD_PX: u32 = 160;
const ART_PX: u32 = 300;
/// The big cover on the playing song's page.
const BIG_PX: u32 = 720;

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

const THEMES: [Theme; 7] = [
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
    Theme {
        name: "RyM YTM",
        bg: "#030303",
        surface: "#030303",
        bar: "#212121",
        hover: "rgba(255,255,255,0.10)",
        selected: "rgba(255,255,255,0.14)",
        text: "#ffffff",
        dim: "#aaaaaa",
        accent: "#ff3048",
        pill_bg: "#ffffff",
        pill_text: "#030303",
        input: "rgba(255,255,255,0.10)",
        divider: "rgba(255,255,255,0.10)",
        menu: "#212121",
    },
];

/// The "RyM YTM" theme, which also has its own layout: a side menu with the playlists, big cover
/// shelves, the playing song's own page (its cover zooms in, lyrics and Up next beside it), the
/// progress line across the top of the bar, and colors from the playing song's cover.
const YTM: usize = 6;

fn ytm_layout(theme: usize) -> bool {
    theme % THEMES.len() == YTM
}

const YTM_STYLE: &str = r#"
headerbar.ytm-head { background: @bg@; min-height: 60px; border-bottom: 1px solid @divider@; }
.logo { font-size: 21px; font-weight: 800; color: @text@; }
.logo-dot { background: @accent@; color: #ffffff; border-radius: 999px; min-width: 26px; min-height: 26px; font-size: 11px; }
.ytm .search { background: rgba(255,255,255,0.10); border: 1px solid rgba(255,255,255,0.14); border-radius: 8px; min-height: 40px; }
.side-nav { background: @bg@; padding: 8px; border-right: 1px solid @divider@; }
.side-nav button.nav { background: transparent; border: none; box-shadow: none; border-radius: 8px; padding: 11px 16px; color: @text@; }
.side-nav button.nav:hover { background: @hover@; }
.side-nav button.nav.active { background: @selected@; }
.nav-label { font-size: 15px; }
.side-nav button.pl { background: transparent; border: none; box-shadow: none; border-radius: 8px; padding: 8px 16px; color: @text@; }
.side-nav button.pl:hover { background: @hover@; }
.nav-line { background: @divider@; margin: 10px 16px; }
.ytm .page { background: @bg@; border-radius: 0; }
.ytm .heading { font-size: 30px; }
.ytm .shelf { font-size: 26px; font-weight: 800; margin-top: 22px; }
button.big-card { background: transparent; border: none; box-shadow: none; border-radius: 8px; padding: 0 0 6px 0; color: @text@; }
button.big-card:hover { background: @hover@; }
.ytm stackswitcher button { border-radius: 0; padding: 12px 10px; color: @dim@; font-size: 13px; }
.ytm stackswitcher button:checked { background: transparent; color: @text@; box-shadow: inset 0 -2px @text@; }
.ytm .bar { border-top: none; padding: 0 0 8px 0; }
scale.ytm-progress { padding: 0; margin: 0 0 6px 0; }
scale.ytm-progress trough { min-height: 3px; border-radius: 0; }
scale.ytm-progress highlight { border-radius: 0; }
scale.ytm-progress slider { min-width: 0; min-height: 0; margin: -3px; opacity: 0; transition: all 150ms ease; }
scale.ytm-progress:hover slider { min-width: 13px; min-height: 13px; margin: -6px; opacity: 1; background: @accent@; }
.ytm button.play { background: transparent; color: @text@; min-width: 46px; min-height: 46px; }
.ytm button.play:hover { background: @hover@; }
.ytm .qrow { border-radius: 0; padding: 8px; border-bottom: 1px solid @divider@; }
.ytm .qrow.now { background: @hover@; }
.ytm .qrow.now .title { color: @text@; }
"#;

const STYLE: &str = r#"
window, .rym { background: @bg@; color: @text@; }
/* No shadow around the window: on Windows its room is drawn black, a dark frame around the app. */
window.csd, window.csd:backdrop, window.solid-csd { box-shadow: none; margin: 0; border-radius: 0; outline: none; }
.avatar { border-radius: 999px; }
window.overlay-win, .overlay-win { background: rgba(14,14,18,0.72); border-radius: 16px; }
window.mini-win { border-radius: 14px; border: 1px solid @divider@; }
.overlay-now { color: #ffffff; font-size: 22px; font-weight: 800; }
.overlay-next { color: rgba(255,255,255,0.6); font-size: 15px; font-weight: 600; }
button.src { background: @input@; color: @dim@; border: none; box-shadow: none; border-radius: 14px; padding: 2px 12px; min-height: 0; font-size: 12px; font-weight: 700; }
button.src.on { background: @text@; color: @bg@; }
button.src:disabled { opacity: 0.35; }
button.recent { background: transparent; border: none; box-shadow: none; border-radius: 10px; padding: 6px; color: @text@; }
button.recent:hover { background: @input@; }
menubutton.account > button { background: transparent; border: none; box-shadow: none; border-radius: 999px; padding: 3px; min-height: 0; min-width: 0; }
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
.lyric-tr { font-size: 15px; font-weight: 600; opacity: 0.8; }
.qrow { border-radius: 10px; padding: 6px 8px; transition: background-color 180ms ease, box-shadow 200ms ease, padding 200ms ease; }
.qrow:hover { background: @hover@; }
.qrow.now { background: @selected@; }
.qrow.now .title { color: @accent@; }
.qrow.past { opacity: 0.55; }
.qrow.lifted { background: @menu@; padding: 10px 12px; box-shadow: 0 8px 22px rgba(0,0,0,0.45); }
.qrow.drop-here { box-shadow: inset 0 3px @accent@; }
stackswitcher button { background: transparent; border: none; box-shadow: none; color: @dim@; font-weight: 700; }
stackswitcher button:checked { color: @text@; background: @input@; }
.store-card { background: @input@; border-radius: 12px; padding: 14px 16px; }
.toast { background: @menu@; color: @text@; border-radius: 10px; padding: 10px 16px; margin: 14px; box-shadow: 0 4px 16px rgba(0,0,0,0.3); }
popover > contents { background: @menu@; color: @text@; border-radius: 10px; padding: 6px; }
popover button { background: transparent; border: none; box-shadow: none; color: @text@; padding: 8px 12px; border-radius: 6px; }
popover button:hover { background: @hover@; }
scale trough { background: @divider@; min-height: 4px; border-radius: 3px; }
scale highlight { background: @accent@; border-radius: 3px; }
scale slider { background: @text@; min-width: 12px; min-height: 12px; margin: -4px; border: none; box-shadow: none; }
dropdown button { background: @input@; color: @text@; border: none; box-shadow: none; border-radius: 8px; }
scrollbar slider { background: @dim@; border-radius: 8px; min-width: 6px; }
.np-view { background: @bg@; }
.np-art { border-radius: 6px; box-shadow: 0 20px 60px rgba(0,0,0,0.55); }
.np-art.zoom { animation: np-zoom 450ms cubic-bezier(0.2, 0.8, 0.2, 1); }
@keyframes np-zoom { from { transform: scale(0.8); opacity: 0.3; } to { transform: scale(1); opacity: 1; } }
button.np-back { background: rgba(0,0,0,0.35); color: #ffffff; border-radius: 22px; min-width: 40px; min-height: 40px; }
button.np-back:hover { background: rgba(0,0,0,0.55); }
.now-icon { background: rgba(0,0,0,0.55); color: #ffffff; border-radius: 6px; }
"#;

fn css(theme: usize) -> String {
    let t = &THEMES[theme % THEMES.len()];
    let style = if ytm_layout(theme) { format!("{STYLE}{YTM_STYLE}") } else { STYLE.to_string() };
    style
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
/// sung lines soft white (see rymplayer::colors). It also lights the song's page (and, in the
/// RyM YTM layout, the top of the pages).
fn lyrics_css(bg: (u8, u8, u8), ytm: bool) -> String {
    let [sung, _, next] = rymplayer::colors::lyric_colors(bg);
    let c = |(r, g, b): (u8, u8, u8)| format!("rgb({r},{g},{b})");
    let mut css = format!(
        ".lyrics {{ background: {}; }} .lyric {{ color: {}; }} .lyric.sung {{ color: {}; }} .lyric.now {{ color: #ffffff; }} .lyrics-note {{ color: {}; font-weight: 600; }}",
        c(bg),
        c(next),
        c(sung),
        c(sung)
    );
    // The cover's color through the player: the progress bar, the play button, and a wash on
    // the bar, each easing into the next song's.
    if bg != rymplayer::colors::PLAIN {
        let accent = rymplayer::colors::mix(bg, (255, 255, 255), 0.35);
        let wash = rymplayer::colors::mix(bg, (12, 12, 16), 0.82);
        css.push_str(&format!(
            " scale.progress highlight {{ background: {a}; transition: background 800ms ease; }} .bar {{ background: {w}; transition: background 800ms ease; }} .overlay-win {{ background: rgba({r},{g},{b},0.72); }}",
            a = c(accent),
            w = c(wash),
            r = wash.0,
            g = wash.1,
            b = wash.2
        ));
        let glow = rymplayer::colors::mix(bg, (3, 3, 3), 0.45);
        let edge = rymplayer::colors::mix(bg, (3, 3, 3), 0.9);
        css.push_str(&format!(
            " .np-view {{ background-image: radial-gradient(circle at 32% 48%, {g} 0%, {e} 72%); }} .page-glow {{ background-image: linear-gradient(180deg, rgba({r},{gr},{b},0.32) 0px, rgba(3,3,3,0) 460px); }}",
            g = c(glow),
            e = c(edge),
            r = bg.0,
            gr = bg.1,
            b = bg.2
        ));
        if !ytm {
            css.push_str(&format!(" button.play {{ background: {}; transition: background 800ms ease; }}", c(accent)));
        }
    }
    css
}

thread_local! {
    /// The "Update parsers" row's status line, while the settings page shows.
    static PARSERS_LABEL: RefCell<Option<glib::WeakRef<Label>>> = const { RefCell::new(None) };
}

/// Settings > Network & Parsers: which engine finds songs (with the rollback to the built-in
/// one), and "Update parsers" (yt-dlp and ytmusicapi to their newest, saved answers cleared).
fn network_settings(ui: &Shared, page: &gtk::Box, config: &Config) {
    use rymplayer::ytmusicapi::Engine;
    page.append(&label(tr("Network & Parsers"), &["shelf"]));
    let names: Vec<&str> = (0..3).map(|i| tr(Engine::from_index(i).label())).collect();
    let engine = gtk::DropDown::from_strings(&names);
    engine.set_selected(config.music_engine.min(2) as u32);
    {
        let ui = ui.clone();
        engine.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.music_engine = d.selected().min(2) as u8;
            rymplayer::ytmusicapi::use_engine(Engine::from_index(s.config.music_engine));
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Song finder"), tr("Search, radio and mixes; the built-in one is the rollback if a YouTube change breaks ytmusicapi"), &engine));
    let status = rymplayer::ytmusicapi::last_failure().map_or_else(|| tr("yt-dlp and ytmusicapi to their newest, saved answers cleared").to_string(), |why| format!("{}: {why}", tr("Last fallback")));
    let about = label(&status, &["dim", "small"]);
    about.set_wrap(true);
    let update = Button::with_label(tr("Update"));
    update.add_css_class("pill");
    update.set_valign(Align::Center);
    {
        let (ui, about) = (ui.clone(), about.clone());
        update.connect_clicked(move |_| {
            about.set_text(tr("Updating…"));
            let nightly = ui.borrow().config.ytdlp_nightly;
            post(&ui, async move { Ev::Parsers(rymplayer::ytmusicapi::update_parsers(nightly).await) });
        });
    }
    PARSERS_LABEL.with(|l| *l.borrow_mut() = Some(about.downgrade()));
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(tr("Update parsers"), &["title"]));
    text.append(&about);
    row.append(&text);
    row.append(&update);
    page.append(&row);
}

thread_local! {
    static LYRICS_FONT: std::cell::OnceCell<CssProvider> = const { std::cell::OnceCell::new() };
}

/// The lyrics' size (Settings), on its own style layer so it changes at once.
fn apply_lyrics_font(px: u8) {
    LYRICS_FONT.with(|cell| {
        let provider = cell.get_or_init(|| {
            let p = CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
            }
            p
        });
        let px = px.clamp(14, 48) as u32;
        provider.load_from_string(&format!(".lyric {{ font-size: {px}px; }} .lyric-tr {{ font-size: {}px; }}", (px * 3 / 5).max(11)));
    });
}

/// A color theme (Settings > Colors, or a .json file) over the layout: its colors in the CSS,
/// its corners, and its pattern behind the window.
fn colors_css(theme: usize, t: &rymplayer::themes::Theme) -> String {
    let c = |hex: &str| {
        let (r, g, b, a) = rymplayer::themes::rgba(hex).unwrap_or((128, 128, 128, 255));
        format!("rgba({r},{g},{b},{:.3})", a as f32 / 255.0)
    };
    let soft = |hex: &str, alpha: f32| {
        let (r, g, b, _) = rymplayer::themes::rgba(hex).unwrap_or((128, 128, 128, 255));
        format!("rgba({r},{g},{b},{alpha})")
    };
    let style = if ytm_layout(theme) { format!("{STYLE}{YTM_STYLE}") } else { STYLE.to_string() };
    let mut css = style
        .replace("@bg@", &c(&t.bg))
        .replace("@surface@", &c(&t.raised))
        .replace("@bar@", &c(&t.raised))
        .replace("@hover@", &soft(&t.text, 0.07))
        .replace("@selected@", &soft(&t.accent, 0.22))
        .replace("@text@", &c(&t.text))
        .replace("@dim@", &c(&t.dim))
        .replace("@accent@", &c(&t.accent))
        .replace("@pill_bg@", &c(&t.accent))
        .replace("@pill_text@", &c(&t.bg))
        .replace("@input@", &soft(&t.text, 0.08))
        .replace("@divider@", &c(&t.border))
        .replace("@menu@", &c(&t.raised));
    if t.dot_clock {
        css.push_str(" .clock { font-family: \"Doto\"; font-weight: 800; font-size: 1.15em; }");
    }
    if t.glow {
        css.push_str(&format!(" .np-art {{ box-shadow: 0 0 34px {}; }}", soft(&t.accent, 0.55)));
    }
    if t.muted {
        css.push_str(" .np-art { filter: grayscale(85%); }");
    }
    if t.cover_border {
        css.push_str(&format!(" .np-art {{ border: 2px solid {}; }}", c(&t.accent)));
    }
    // Corners: sharp themes square everything off.
    if t.round < 0.5 {
        let px = (t.round * 12.0).round();
        css.push_str(&format!(" button, entry, .card, .tile, popover > contents, .pill, image, picture {{ border-radius: {px}px; }}"));
    }
    // The pattern, as a picture repeated behind the window, and the darkened edges (Remorse).
    let mut layers = Vec::new();
    if let Some((w, h, px)) = t.tile() {
        let file = rymplayer::data_dir().join("theme-pattern.png");
        if image::RgbaImage::from_raw(w, h, px).is_some_and(|img| img.save(&file).is_ok()) {
            let url = file.to_string_lossy().replace('\\', "/");
            layers.push((format!("url(\"file://{url}\")"), "repeat"));
        }
    }
    if t.vignette {
        layers.push(("radial-gradient(circle, rgba(0,0,0,0) 40%, rgba(0,0,0,0.66) 100%)".to_string(), "no-repeat"));
    }
    if !layers.is_empty() {
        let images: Vec<&str> = layers.iter().map(|(i, _)| i.as_str()).collect();
        let repeats: Vec<&str> = layers.iter().map(|(_, r)| *r).collect();
        css.push_str(&format!(" window.background {{ background-image: {}; background-repeat: {}; }}", images.join(", "), repeats.join(", ")));
    }
    css
}

/// The color theme picked in the settings (None: the layout's own colors).
fn current_colors(c: &Config) -> Option<rymplayer::themes::Theme> {
    match c.palette.as_str() {
        "" => None,
        "Custom" => c.custom_theme.clone(),
        name => rymplayer::themes::preset(name),
    }
}

/// The record or the cover, Liquid Aurora, and the meters, as the settings and colors say.
fn apply_extras(s: &Ui) {
    let colors = current_colors(&s.config);
    s.w.art_stack.set_visible_child_name(if s.config.vinyl { "vinyl" } else { "cover" });
    s.w.aurora.set_visible(s.config.aurora);
    s.w.meters.set_visible(colors.as_ref().is_some_and(|t| t.vu));
    let bg = colors.as_ref().and_then(|t| rymplayer::themes::rgba(&t.bg)).map_or((0.07, 0.07, 0.08), |(r, g, b, _)| (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0));
    s.w.live.bg.set(bg);
    s.w.live.dark.set(colors.as_ref().is_none_or(|t| t.dark));
}

fn apply_theme(provider: &CssProvider, theme: usize, colors: Option<&rymplayer::themes::Theme>) {
    match colors {
        Some(t) => provider.load_from_string(&colors_css(theme, t)),
        None => provider.load_from_string(&css(theme)),
    }
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
    /// Plugin store.
    Plugins,
    /// "Your recap": listening stats.
    Recap,
    /// A playlist, album, mix, radio or search results.
    Songs(String),
}

/// When the sleep timer stops the music.
#[derive(Clone, Copy, PartialEq)]
enum Sleep {
    At(Instant),
    EndOfSong,
}

enum Ev {
    Auth(AuthMode, anyhow::Result<Api>),
    /// "Play in RyM": a link handed over by another copy (see `open_incoming`).
    Incoming(String),
    /// The songs of a link from outside, to play now (picked by hand).
    Handed(anyhow::Result<Vec<Track>>),
    /// The signed-in Google account, then its picture's bytes.
    Account(anyhow::Result<rymplayer::api::Account>),
    Avatar(String, anyhow::Result<Vec<u8>>),
    Home(anyhow::Result<feed::Refreshed>),
    /// A page's songs; the number is the page it was asked for (older answers are dropped).
    Tracks(u64, anyhow::Result<Vec<Track>>),
    /// A list's first part and where it goes on.
    Paged(u64, anyhow::Result<rymplayer::api::Paged>),
    /// The next part of the open page.
    More(u64, anyhow::Result<rymplayer::api::Paged>),
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
    /// A newer RyM on GitHub (None: up to date); true when asked from the settings.
    Update(anyhow::Result<Option<rymplayer::updates::Release>>, bool),
    /// The plugin store's list.
    Store(anyhow::Result<Vec<rymplayer::plugins::Plugin>>),
    /// A video's picture and sound links, to watch it.
    VideoLinks(Track, anyhow::Result<(String, Option<String>)>),
    /// Every source's lyrics for a song (the source switcher).
    LyricsSources(String, Vec<Lyrics>),
    /// Up next saved as a playlist: what to say, and the library's playlists after it.
    Saved(String, anyhow::Result<Vec<Playlist>>),
    /// A media key, or the system's media controls (Windows' volume flyout, MPRIS on Linux).
    MediaKey(souvlaki::MediaControlEvent),
    /// English for the lyrics of a song (Japanese songs), lined up with their lines.
    Translated(String, String, Vec<String>),
    /// A song's Japanese lines in Latin letters (its id, the lyrics' source, one per line).
    Romaji(String, String, Vec<String>),
    /// SponsorBlock's non-music parts of a song (its id).
    Segments(String, rymplayer::sponsorblock::Segments),
    /// Search suggestions for what was typed (the account's search history first).
    Suggestions(String, anyhow::Result<Vec<rymplayer::api::Suggestion>>),
    /// The update downloaded and put in place: the program to start again.
    Installed(anyhow::Result<PathBuf>),
    /// RyM Link: a device's note on the Wi-Fi, and the music taken over from a phone.
    Peer(rymplayer::link::Announce, std::net::IpAddr),
    TookOver(String, anyhow::Result<rymplayer::link::State>),
    /// "Update parsers" finished: what happened.
    Parsers(String),
    Done,
}

/// Widgets the code changes after building them.
struct Widgets {
    window: ApplicationWindow,
    provider: CssProvider,
    /// The lyrics panel's colors, changed for each song.
    lyrics_provider: CssProvider,
    rail: Vec<(Page, Button)>,
    /// The pages, and the playing song's page over them (its big cover; lyrics and Up next beside it).
    stack: gtk::Stack,
    np_art: gtk::Picture,
    /// Theme extras: what they follow, the cover or the record, Liquid Aurora, the meters.
    live: Rc<fx_view::Live>,
    art_stack: gtk::Stack,
    aurora: gtk::DrawingArea,
    meters: gtk::DrawingArea,
    /// The RyM YTM layout, and its side menu's playlists.
    ytm: bool,
    nav_lists: gtk::Box,
    content: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toast: gtk::Revealer,
    toast_text: Label,
    account: gtk::MenuButton,
    sign_in: Button,
    /// The search box, and the list of suggestions under it.
    search: gtk::Entry,
    suggest: gtk::Popover,
    suggest_box: gtk::Box,
    lyrics_box: gtk::Box,
    lyrics_scroll: gtk::ScrolledWindow,
    /// The side panel: lyrics or the queue ("Up next").
    side: gtk::Stack,
    queue_box: gtk::Box,
    /// The lyrics' source pills.
    pills: gtk::Box,
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
    /// Sleep timer: when it ends (or at the end of the playing song).
    sleep: Option<Sleep>,
    /// The next song starts while this one fades out (set for its play_at).
    crossfade_next: bool,
    /// The song (request) whose crossfade already started.
    crossfade_for: u64,
    /// Listening stats: when time was last added, and last written.
    listened_at: Instant,
    stats_written: Instant,
    /// What Discord was last told: song, playing, position, when.
    discord_sent: Option<(String, bool, f32, Instant)>,
    /// The floating lyrics window, and its two lines.
    overlay: Option<(gtk::Window, Label, Label)>,
    /// Songs started since memory was last cleaned up (every 5 it is).
    songs_since_cleanup: u32,
    volume: f32,
    /// The song left off last time and the second to go on from, until it plays.
    resume_at: Option<(String, f32)>,
    /// What the session file last got (song, second, volume, queue length): saved again only on a change.
    session_key: Option<(String, u32, u32, usize)>,
    /// The signed-in Google account (name, handle, picture link).
    account: Option<rymplayer::api::Account>,
    /// The lyrics / Up next panel was closed by hand (songs starting don't open it then).
    side_closed: bool,
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

    /// What plays: the songs, the playing one, and the ones added to play after it.
    q: rymplayer::queue::Queue,
    /// Songs played this session (Up next leaves them out).
    played: HashSet<String>,
    /// The song (request) whose end already moved the queue on, so it moves only once.
    ended_for: u64,
    /// Every source's lyrics for a song, once looked up (the source switcher).
    alt_lyrics: Option<(String, Vec<Lyrics>)>,
    /// A source pill pressed before the sources were in: shown once they are.
    wanted_source: Option<&'static str>,
    /// The song whose sources are being looked up.
    sources_for: Option<String>,
    /// Where the open page goes on (the home feed, or a list: search results, a playlist).
    home_more: Option<rymplayer::api::More>,
    list_more: Option<rymplayer::api::More>,
    loading_more: bool,
    /// Where the next part of the open page goes, and the spinner under it.
    more_box: Option<glib::WeakRef<gtk::Box>>,
    more_spinner: Option<glib::WeakRef<gtk::Spinner>>,
    /// A queue row was held for 2 s: it's lifted and can be dragged.
    armed: Option<usize>,
    held: bool,
    request_id: u64,
    /// The lyrics shown in romaji: for which song and source, and the lines.
    romaji: (String, Vec<String>),
    /// SponsorBlock, Last.fm and Seamless Music's rules (shared with RyM Slint and the terminal).
    non_music: core::NonMusic,
    scrobbler: core::Scrobbler,
    peers: core::PeerWatch,
    loading: bool,
    radio_pending: bool,
    /// Liquid Aurora: the loudest bass lately; the theme extras last shown (vinyl, aurora, colors).
    bass_peak: f32,
    extras_key: Option<(bool, bool, String)>,
    /// When the scheduled points reset was last looked at.
    reset_checked: Option<Instant>,
    /// Word by word: the line filling, and how much (sung and left lengths) was last shown.
    word_fill: Option<(usize, usize, usize)>,
    /// Battery saver or a low battery, looked at every 30 s (animations stop then).
    saving: bool,
    saving_at: Option<Instant>,
    /// The next song was picked by hand (RaM points: its minus points go).
    user_pick: bool,
    /// A link from outside that came before the sign-in was ready: opened once it is.
    pending_link: Option<String>,
    audio: Option<Audio>,
    vlc: Option<std::process::Child>,
    lyrics: Option<Lyrics>,
    lyric_line: Option<usize>,

    covers: HashMap<String, gdk::Texture>,
    /// The covers in the order they came: past the newest 250, the oldest are let go (each one
    /// is a picture on the graphics card; kept forever they pile up over a long session).
    cover_order: std::collections::VecDeque<String>,
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
    /// The plugin store's list once loaded.
    store: Option<Vec<rymplayer::plugins::Plugin>>,
    /// What the player bar shows now, so the ticker only changes what changed.
    shown_playing: Option<bool>,
    shown_len: f32,
    shown_pos: Option<u32>,
    shown_slider: f32,
    /// When the playing song was asked for (for the log).
    play_asked: Instant,
    /// What the suggestions were last asked for, and the wait before asking (typing goes on).
    suggest_for: Option<String>,
    suggest_wait: Option<glib::SourceId>,
    /// Up next changed while it wasn't on screen: it's built when it shows.
    queue_dirty: bool,
    /// The mini player: its window, cover, names and play button; and the song it shows.
    mini: Option<(gtk::Window, gtk::Picture, Label, Label, Button)>,
    mini_for: Option<(String, bool)>,
    /// The system's media controls, and what they were last told (song, playing).
    media: Option<souvlaki::MediaControls>,
    media_sent: Option<(String, bool)>,
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

/// When the program started (for the start-up times in rym-gtk.log).
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn since_start() -> u128 {
    STARTED.get().map_or(0, |t| t.elapsed().as_millis())
}

fn main() {
    STARTED.get_or_init(Instant::now);
    // Linux: the Japanese and dot-digit fonts that come with RyM.
    rymplayer::fonts::add_bundled();
    // The window app has no console: a crash's message and place go to rym-gtk.log.
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        log(&format!("CRASH at {at}: {msg}"));
        rymplayer::crash::record("RyM GTK", &at, &msg);
    }));
    configure_gtk_runtime();
    // What the last update put aside goes now.
    rymplayer::updates::clean_old();
    // GTK's own warnings and errors (a lost graphics context, say) go to rym-gtk.log too: the
    // window app has no console to print them to.
    glib::log_set_writer_func(|level, fields| {
        if matches!(level, glib::LogLevel::Error | glib::LogLevel::Critical | glib::LogLevel::Warning) {
            let text = fields.iter().find(|f| f.key() == "MESSAGE").and_then(|f| f.value_str()).unwrap_or_default();
            let domain = fields.iter().find(|f| f.key() == "GLIB_DOMAIN").and_then(|f| f.value_str()).unwrap_or_default();
            log(&format!("{level:?} {domain}: {text}"));
        }
        glib::LogWriterOutput::Handled
    });
    if std::env::args().any(|arg| arg == "--login") {
        login::run();
    }
    // "Play in RyM": a link on the command line (a rym:// link from the browser too) goes to the
    // RyM already open, which plays it; this copy then quits.
    let args: Vec<String> = std::env::args().collect();
    let incoming = args.iter().skip(1).find(|a| rymplayer::links::is_incoming(a)).cloned();
    if let Some(link) = &incoming {
        if rymplayer::link::hand_over(link) {
            return;
        }
        let _ = INCOMING.set(link.clone());
    }
    // Each start is its own window (like the other apps), not a second window of the first.
    let app = Application::builder().application_id(APP_ID).flags(gtk::gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(build);
    // GTK would take the link for a file to open (and refuse it): it's left out of what GTK reads.
    let gtk_args: Vec<String> = args.into_iter().filter(|a| Some(a) != incoming.as_ref()).collect();
    app.run_with_args(&gtk_args);
}

/// The link this copy was started with (and no other RyM took), opened once the window is up.
static INCOMING: std::sync::OnceLock<String> = std::sync::OnceLock::new();

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
    s.q.current.and_then(|i| s.q.songs.get(i).cloned())
}

// ---------------------------------------------------------------- window

fn build(app: &Application) {
    log(&format!("start: GTK ready after {} ms", since_start()));
    let config = Config::load();
    rymplayer::i18n::set_arabic(config.arabic);
    // Arabic: the whole window reads right to left.
    gtk::Widget::set_default_direction(if config.arabic { gtk::TextDirection::Rtl } else { gtk::TextDirection::Ltr });
    stream::use_source(config.source);
    rymplayer::ytmusicapi::use_engine(rymplayer::ytmusicapi::Engine::from_index(config.music_engine));
    rymplayer::ytmusicapi::use_language(config.arabic);
    stream::prefer_lossless(config.prefer_lossless);
    stream::download_best(config.download_best);
    stream::download_parallel(config.download_connections, config.download_piece_mb);

    let provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    apply_theme(&provider, config.gui_theme, current_colors(&config).as_ref());
    let lyrics_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &lyrics_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
    lyrics_provider.load_from_string(&lyrics_css(rymplayer::colors::PLAIN, ytm_layout(config.gui_theme)));
    apply_lyrics_font(config.lyrics_font);

    let window = ApplicationWindow::builder().application(app).title("RyM").default_width(1240).default_height(800).build();
    window.add_css_class("rym");
    let ytm = ytm_layout(config.gui_theme);
    if ytm {
        window.add_css_class("ytm");
    }

    // Header: the system's title buttons, search in the middle, refresh at the end.
    let header = HeaderBar::new();
    let search = gtk::Entry::new();
    search.add_css_class("search");
    search.set_placeholder_text(Some(tr("Search songs, albums, artists")));
    search.set_primary_icon_name(Some("system-search-symbolic"));
    search.set_width_chars(46);
    let search_in = gtk::DropDown::from_strings(&["YouTube Music", "SoundCloud"]);
    search_in.set_tooltip_text(Some(tr("Search on")));
    let search_row = gtk::Box::new(Orientation::Horizontal, 6);
    search_row.append(&search_in);
    search_row.append(&search);
    header.set_title_widget(Some(&search_row));
    // Suggestions under the search box: the account's own search history (the same on the
    // phone), then YouTube Music's suggestions. It never takes the focus from the box.
    let suggest_box = gtk::Box::new(Orientation::Vertical, 0);
    let suggest = gtk::Popover::new();
    suggest.set_parent(&search);
    suggest.set_autohide(false);
    suggest.set_has_arrow(false);
    suggest.set_can_focus(false);
    suggest.set_position(gtk::PositionType::Bottom);
    suggest.set_halign(Align::Start);
    suggest.set_child(Some(&suggest_box));
    // RyM YTM: the menu button (folds the side menu) and the name on the left.
    let menu_button = icon_button("open-menu-symbolic", tr("Menu"), &[]);
    if ytm {
        header.add_css_class("ytm-head");
        header.pack_start(&menu_button);
        let logo = gtk::Box::new(Orientation::Horizontal, 6);
        let dot = Label::new(Some("▶"));
        dot.add_css_class("logo-dot");
        dot.set_valign(Align::Center);
        logo.append(&dot);
        logo.append(&label("RyM", &["logo"]));
        logo.set_margin_start(6);
        header.pack_start(&logo);
    }
    // The Google account: its picture (a menu with the name and Sign out), or "Sign in".
    let account = gtk::MenuButton::new();
    account.add_css_class("account");
    account.set_visible(false);
    let sign_in = Button::with_label(tr("Sign in"));
    sign_in.add_css_class("pill");
    sign_in.set_valign(Align::Center);
    sign_in.set_visible(false);
    header.pack_end(&account);
    header.pack_end(&sign_in);
    let refresh = icon_button("view-refresh-symbolic", tr("Refresh: new songs on top (F5)"), &[]);
    header.pack_end(&refresh);
    let from_link = icon_button("folder-download-symbolic", tr("Download from a link"), &[]);
    header.pack_end(&from_link);
    window.set_titlebar(Some(&header));

    // Rail (RyM YTM: a side menu with the pages' names, and the playlists under them).
    let rail_box = gtk::Box::new(Orientation::Vertical, 4);
    rail_box.add_css_class(if ytm { "side-nav" } else { "rail" });
    let mut rail = Vec::new();
    let mut nav_labels = Vec::new();
    for (page, icon, text) in [
        (Page::Home, "go-home-symbolic", tr("Home")),
        (Page::Liked, "", tr("Liked")),
        (Page::Library, "view-grid-symbolic", tr("Library")),
        (Page::History, "document-open-recent-symbolic", tr("History")),
        (Page::Settings, "emblem-system-symbolic", tr("Settings")),
    ] {
        let b = Button::new();
        if ytm {
            b.add_css_class("nav");
        }
        let col = if ytm { gtk::Box::new(Orientation::Horizontal, 18) } else { gtk::Box::new(Orientation::Vertical, 3) };
        let img: gtk::Widget = if page == Page::Liked {
            heart_glyph(20).upcast()
        } else {
            let i = gtk::Image::from_icon_name(icon);
            i.set_pixel_size(22);
            i.upcast()
        };
        let l = Label::new(Some(text));
        l.add_css_class(if ytm { "nav-label" } else { "rail-label" });
        if ytm {
            l.set_xalign(0.0);
            nav_labels.push(l.clone());
        }
        col.append(&img);
        col.append(&l);
        b.set_child(Some(&col));
        rail_box.append(&b);
        rail.push((page, b));
    }
    let nav_lists = gtk::Box::new(Orientation::Vertical, 0);
    if ytm {
        let line = gtk::Separator::new(Orientation::Horizontal);
        line.add_css_class("nav-line");
        rail_box.append(&line);
        let lists_scroll = gtk::ScrolledWindow::new();
        lists_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        lists_scroll.set_child(Some(&nav_lists));
        lists_scroll.set_vexpand(true);
        rail_box.append(&lists_scroll);
        rail_box.set_size_request(240, -1);
        // The menu button folds it to its icons and back.
        let rail_box = rail_box.clone();
        menu_button.connect_clicked(move |_| {
            let open = !lists_scroll.is_visible();
            lists_scroll.set_visible(open);
            for l in &nav_labels {
                l.set_visible(open);
            }
            rail_box.set_size_request(if open { 240 } else { -1 }, -1);
        });
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
    overlay.add_overlay(&toast_rev);
    // RyM YTM: the pages, and the playing song's page over them.
    let main_stack = gtk::Stack::new();
    let np_art = picture(280, "np-art");
    let np_back = icon_button("go-down-symbolic", tr("Back"), &[]);

    // Lyrics on the right.
    let lyrics_box = gtk::Box::new(Orientation::Vertical, 4);
    let lyrics_scroll = gtk::ScrolledWindow::new();
    lyrics_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    lyrics_scroll.set_child(Some(&lyrics_box));
    lyrics_scroll.set_vexpand(true);
    lyrics_scroll.set_size_request(440, -1);
    lyrics_scroll.add_css_class("lyrics");
    // Up next: the queue, reordered by holding a row for 2 s and dragging it.
    let queue_box = gtk::Box::new(Orientation::Vertical, 2);
    queue_box.set_margin_start(10);
    queue_box.set_margin_end(10);
    let queue_scroll = gtk::ScrolledWindow::new();
    queue_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    queue_scroll.set_child(Some(&queue_box));
    queue_scroll.set_vexpand(true);
    let side = gtk::Stack::new();
    side.set_transition_type(gtk::StackTransitionType::Crossfade);
    side.set_hhomogeneous(false);
    side.set_vhomogeneous(false);
    // The lyrics page: one pill per source over the lines (LRCLIB | NetEase | KuGou | Genius | YouTube Music).
    let pills = gtk::Box::new(Orientation::Horizontal, 6);
    pills.set_margin_top(8);
    pills.set_margin_bottom(4);
    pills.set_margin_start(12);
    pills.set_margin_end(12);
    let pills_scroll = gtk::ScrolledWindow::new();
    pills_scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
    pills_scroll.set_child(Some(&pills));
    let lyrics_page = gtk::Box::new(Orientation::Vertical, 0);
    lyrics_page.append(&pills_scroll);
    lyrics_page.append(&lyrics_scroll);
    if ytm {
        side.add_titled(&queue_scroll, Some("queue"), &tr("Up next").to_uppercase());
        side.add_titled(&lyrics_page, Some("lyrics"), &tr("Lyrics").to_uppercase());
    } else {
        side.add_titled(&lyrics_page, Some("lyrics"), tr("Lyrics"));
        side.add_titled(&queue_scroll, Some("queue"), tr("Up next"));
    }
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&side));
    switcher.set_hexpand(true);
    let side_head = gtk::Box::new(Orientation::Horizontal, 6);
    side_head.set_margin_top(10);
    side_head.set_margin_start(10);
    side_head.set_margin_end(10);
    side_head.append(&switcher);
    let save_queue = icon_button("document-save-symbolic", tr("Save Up next as a playlist"), &["dim"]);
    save_queue.set_valign(Align::Center);
    side_head.append(&save_queue);
    let side_box = gtk::Box::new(Orientation::Vertical, 0);
    side_box.set_size_request(440, -1);
    side_box.append(&side_head);
    side_box.append(&side);
    lyrics_scroll.set_size_request(-1, -1);
    let middle = gtk::Box::new(Orientation::Horizontal, 0);
    middle.append(&rail_box);
    if ytm {
        content.add_css_class("page-glow");
    }
    let live = Rc::new(fx_view::Live::default());
    let art_stack = gtk::Stack::new();
    art_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    let aurora = fx_view::aurora(live.clone());
    let meters = fx_view::meters(live.clone());
    meters.set_margin_bottom(8);
    meters.set_visible(false);
    {
        // Liquid Aurora takes the cover's colors.
        let (live, aurora) = (live.clone(), aurora.clone());
        np_art.connect_paintable_notify(move |p| {
            if let Some(lights) = p.paintable().and_then(|p| p.downcast::<gdk::Texture>().ok()).and_then(|t| fx_view::cover_lights(&t)) {
                *live.lights.borrow_mut() = lights;
                aurora.queue_draw();
            }
        });
    }
    {
        // The cover as big as the room allows, always square (or the record in its place).
        np_art.set_halign(Align::Fill);
        np_art.set_valign(Align::Fill);
        let art_box = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
        art_stack.add_named(&np_art, Some("cover"));
        art_stack.add_named(&fx_view::vinyl(&np_art, live.clone()), Some("vinyl"));
        art_box.set_child(Some(&art_stack));
        side_box.prepend(&meters);
        art_box.set_hexpand(true);
        art_box.set_vexpand(true);
        art_box.set_margin_top(40);
        art_box.set_margin_bottom(40);
        art_box.set_margin_start(40);
        side_box.set_margin_top(14);
        side_box.set_margin_end(24);
        let np_view = gtk::Box::new(Orientation::Horizontal, 24);
        np_view.add_css_class("np-view");
        np_view.append(&art_box);
        np_view.append(&side_box);
        // Back to the page it came from (Esc too).
        let np_page = gtk::Overlay::new();
        // Liquid Aurora under the page.
        np_page.set_child(Some(&aurora));
        np_page.add_overlay(&np_view);
        np_page.set_measure_overlay(&np_view, true);
        aurora.set_visible(false);
        np_back.add_css_class("np-back");
        np_back.set_halign(Align::Start);
        np_back.set_valign(Align::Start);
        np_back.set_margin_top(14);
        np_back.set_margin_start(14);
        np_page.add_overlay(&np_back);
        main_stack.add_named(&scroll, Some("page"));
        main_stack.add_named(&np_page, Some("now"));
        main_stack.set_transition_duration(380);
        // Only the page on screen is measured (resizing measured the hidden one too).
        main_stack.set_hhomogeneous(false);
        main_stack.set_vhomogeneous(false);
        overlay.set_child(Some(&main_stack));
    }
    middle.append(&overlay);
    middle.set_vexpand(true);

    // Player bar: song on the left, controls in the middle, lyrics and volume on the right.
    let bar_cover = picture(if ytm { 44 } else { 52 }, "cover");
    let bar_title = label(tr("Nothing playing"), &["title"]);
    let bar_artist = label("", &["dim", "small"]);
    let names = gtk::Box::new(Orientation::Vertical, 2);
    names.set_valign(Align::Center);
    names.append(&bar_title);
    names.append(&bar_artist);
    names.set_hexpand(true);
    let bar_heart = heart_button(tr("Like"));
    let bar_share = icon_button("send-to-symbolic", tr("Share"), &["dim"]);
    bar_share.set_valign(Align::Center);
    bar_heart.set_valign(Align::Center);
    // RyM YTM: the playing song's menu beside it.
    let bar_more = icon_button("view-more-symbolic", tr("More"), &["dim"]);
    bar_more.set_valign(Align::Center);

    let prev = icon_button("media-skip-backward-symbolic", tr("Previous"), &[]);
    let play = Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("icon");
    play.add_css_class("play");
    play.set_tooltip_text(Some(tr("Play / pause")));
    let next = icon_button("media-skip-forward-symbolic", tr("Next"), &[]);
    let pos = label("0:00", &["dim", "small", "clock"]);
    let len = label("0:00", &["dim", "small", "clock"]);
    let progress = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.5);
    progress.add_css_class("progress");
    progress.set_draw_value(false);
    progress.set_hexpand(true);

    // Opens the playing song's page and closes it again (RyM YTM: an arrow).
    let lyrics_toggle = if ytm { icon_button("pan-up-symbolic", tr("Now playing"), &[]) } else { icon_button("format-justify-left-symbolic", tr("Now playing"), &["dim"]) };
    let speed_button = gtk::MenuButton::new();
    speed_button.set_icon_name("media-seek-forward-symbolic");
    speed_button.set_tooltip_text(Some(tr("Speed and pitch")));
    speed_button.add_css_class("flat");
    let sleep_button = gtk::MenuButton::new();
    sleep_button.set_icon_name("alarm-symbolic");
    sleep_button.set_tooltip_text(Some(tr("Sleep timer")));
    sleep_button.add_css_class("flat");
    let overlay_toggle = icon_button("view-pin-symbolic", tr("Floating lyrics"), &["dim"]);
    let mini_toggle = icon_button("view-restore-symbolic", tr("Mini player"), &["dim"]);
    let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    let volume = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    volume.set_draw_value(false);
    // The last session: its volume now, its queue once the window is up.
    let session = rymplayer::session::load();
    let start_volume = session.as_ref().map_or(0.8, |s| s.volume);
    volume.set_value(start_volume as f64);
    volume.set_size_request(110, -1);
    let right = gtk::Box::new(Orientation::Horizontal, 8);
    right.set_size_request(320, -1);
    right.set_halign(Align::End);
    right.set_valign(Align::Center);

    let bar = gtk::Box::new(Orientation::Horizontal, 16);
    let bottom: gtk::Box;
    if ytm {
        // The progress line across the top; under it the controls and the time, the song in the
        // middle, the rest on the right.
        progress.add_css_class("ytm-progress");
        let controls = gtk::Box::new(Orientation::Horizontal, 6);
        controls.set_valign(Align::Center);
        controls.set_size_request(320, -1);
        controls.append(&prev);
        controls.append(&play);
        controls.append(&next);
        let time = gtk::Box::new(Orientation::Horizontal, 0);
        time.set_margin_start(10);
        time.set_valign(Align::Center);
        time.append(&pos);
        time.append(&label(" / ", &["dim", "small"]));
        time.append(&len);
        controls.append(&time);
        let song = gtk::Box::new(Orientation::Horizontal, 12);
        song.set_hexpand(true);
        song.set_halign(Align::Center);
        song.append(&bar_cover);
        song.append(&names);
        song.append(&bar_heart);
        song.append(&bar_more);
        right.append(&volume);
        right.append(&vol_icon);
        right.append(&speed_button);
        right.append(&sleep_button);
        right.append(&overlay_toggle);
        right.append(&mini_toggle);
        right.append(&lyrics_toggle);
        bar.set_margin_start(16);
        bar.set_margin_end(16);
        bar.append(&controls);
        bar.append(&song);
        bar.append(&right);
        bottom = gtk::Box::new(Orientation::Vertical, 0);
        bottom.add_css_class("bar");
        bottom.append(&progress);
        bottom.append(&bar);
    } else {
        let left = gtk::Box::new(Orientation::Horizontal, 12);
        left.set_size_request(320, -1);
        left.append(&bar_cover);
        left.append(&names);
        left.append(&bar_heart);
        left.append(&bar_share);
        let controls = gtk::Box::new(Orientation::Horizontal, 10);
        controls.set_halign(Align::Center);
        controls.append(&prev);
        controls.append(&play);
        controls.append(&next);
        let line = gtk::Box::new(Orientation::Horizontal, 8);
        line.append(&pos);
        line.append(&progress);
        line.append(&len);
        let center = gtk::Box::new(Orientation::Vertical, 2);
        center.set_hexpand(true);
        center.append(&controls);
        center.append(&line);
        right.append(&speed_button);
        right.append(&sleep_button);
        right.append(&overlay_toggle);
        right.append(&mini_toggle);
        right.append(&lyrics_toggle);
        right.append(&vol_icon);
        right.append(&volume);
        bar.add_css_class("bar");
        bar.append(&left);
        bar.append(&center);
        bar.append(&right);
        bottom = bar;
    }

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.append(&middle);
    root.append(&bottom);
    window.set_child(Some(&root));

    let audio = match Audio::new() {
        Ok(a) => Some(a),
        Err(e) => {
            log(&format!("no audio device: {e}"));
            None
        }
    };
    if let Some(a) = &audio {
        a.set_volume(start_volume);
        a.set_speed(config.speed);
    }
    rymplayer::fx::set(config.fx());
    let (tx, rx) = mpsc::channel();
    // A few threads are plenty for its background work (one per processor core made ~100 threads).
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).max_blocking_threads(8).enable_all().build().expect("async runtime");
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
            stack: main_stack.clone(),
            np_art: np_art.clone(),
            live: live.clone(),
            art_stack: art_stack.clone(),
            aurora: aurora.clone(),
            meters: meters.clone(),
            ytm,
            nav_lists: nav_lists.clone(),
            content,
            scroll,
            toast: toast_rev,
            toast_text,
            account: account.clone(),
            sign_in: sign_in.clone(),
            search: search.clone(),
            suggest: suggest.clone(),
            suggest_box: suggest_box.clone(),
            lyrics_box,
            lyrics_scroll,
            side,
            queue_box,
            pills: pills.clone(),
            bar_cover,
            bar_title,
            bar_artist,
            bar_heart: bar_heart.clone(),
            play: play.clone(),
            progress: progress.clone(),
            pos,
            len,
        },
        side_closed: false,
        sleep: None,
        crossfade_next: false,
        crossfade_for: 0,
        listened_at: Instant::now(),
        stats_written: Instant::now(),
        discord_sent: None,
        overlay: None,
        songs_since_cleanup: 0,
        volume: start_volume,
        resume_at: None,
        session_key: None,
        account: None,
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
        q: Default::default(),
        played: HashSet::new(),
        ended_for: 0,
        alt_lyrics: None,
        wanted_source: None,
        sources_for: None,
        home_more: None,
        list_more: None,
        loading_more: false,
        more_box: None,
        more_spinner: None,
        armed: None,
        held: false,
        request_id: 0,
        romaji: (String::new(), Vec::new()),
        non_music: Default::default(),
        scrobbler: Default::default(),
        peers: Default::default(),
        loading: false,
        radio_pending: false,
        bass_peak: 0.05,
        extras_key: None,
        reset_checked: None,
        word_fill: None,
        saving: false,
        saving_at: None,
        user_pick: false,
        pending_link: None,
        audio,
        vlc: None,
        lyrics: None,
        lyric_line: None,
        covers: HashMap::new(),
        cover_order: Default::default(),
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
        store: None,
        shown_playing: None,
        shown_len: 0.0,
        shown_pos: None,
        shown_slider: -1.0,
        play_asked: Instant::now(),
        suggest_for: None,
        suggest_wait: None,
        queue_dirty: false,
        media: None,
        media_sent: None,
        mini: None,
        mini_for: None,
    }));
    if ui.borrow().audio.is_none() {
        toast(&ui, tr("No audio device found"));
    }
    from_link.set_visible(has_video(&ui));
    FROM_LINK.with(|f| *f.borrow_mut() = Some(from_link.downgrade()));
    search_in.set_visible(has_plugin(&ui, rymplayer::plugins::SOUNDCLOUD));
    SEARCH_IN.with(|f| *f.borrow_mut() = Some(search_in.downgrade()));

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
            let pop = ui.borrow().w.suggest.clone();
            pop.popdown();
            if !q.is_empty() {
                search_songs(&ui, q);
            }
        });
    }
    {
        // Typing: suggestions once it pauses for a moment.
        let ui = ui.clone();
        search.connect_changed(move |e| {
            if e.has_focus() || e.first_child().is_some_and(|c| c.has_focus()) {
                ask_suggestions(&ui, 200);
            }
        });
    }
    {
        // Into the box: the history (or suggestions for what's there); out of it: the list goes
        // (a moment later, so a click on it still counts).
        let focus = gtk::EventControllerFocus::new();
        let ui2 = ui.clone();
        focus.connect_enter(move |_| ask_suggestions(&ui2, 0));
        let ui2 = ui.clone();
        focus.connect_leave(move |_| {
            let ui = ui2.clone();
            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                let pop = ui.borrow().w.suggest.clone();
                pop.popdown();
            });
        });
        search.add_controller(focus);
        let keys = gtk::EventControllerKey::new();
        let ui2 = ui.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let pop = ui2.borrow().w.suggest.clone();
            if key == gdk::Key::Escape && pop.is_visible() {
                pop.popdown();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        search.add_controller(keys);
    }
    {
        let ui = ui.clone();
        refresh.connect_clicked(move |_| refresh_home(&ui));
    }
    {
        let ui = ui.clone();
        from_link.connect_clicked(move |_| link_dialog(&ui));
    }
    {
        let ui = ui.clone();
        sign_in.connect_clicked(move |_| sign_in_or_out(&ui));
    }
    {
        // Updates: at start when it's been 12 hours, then checked hourly whether it's time again.
        let ui2 = ui.clone();
        glib::timeout_add_seconds_local(3600, move || {
            auto_update_check(&ui2);
            glib::ControlFlow::Continue
        });
        auto_update_check(&ui);
        // The store's list, for plugin updates.
        post(&ui, async { Ev::Store(rymplayer::plugins::store().await) });
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
            // Esc leaves the song's page.
            if key == gdk::Key::Escape {
                let mut s = ui.borrow_mut();
                if side_shown(&s.w) {
                    s.side_closed = true;
                    show_side(&s.w, false);
                    return glib::Propagation::Stop;
                }
            }
            if key == gdk::Key::f && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                search.grab_focus();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
    }
    fill_pills(&ui);
    {
        // RyM Link: the phones on this Wi-Fi say what they play.
        let tx = ui.borrow().tx.clone();
        rymplayer::link::listen(move |note, ip| {
            let _ = tx.send(Ev::Peer(note, ip));
        });
    }
    speed_button.set_popover(Some(&speed_popover(&ui)));
    sleep_button.set_popover(Some(&sleep_popover(&ui)));
    {
        let ui2 = ui.clone();
        overlay_toggle.connect_clicked(move |b| {
            let on = {
                let mut s = ui2.borrow_mut();
                s.config.lyrics_overlay = !s.config.lyrics_overlay;
                s.config.save();
                s.config.lyrics_overlay
            };
            if on { b.remove_css_class("dim") } else { b.add_css_class("dim") }
            show_overlay(&ui2, on);
        });
        if ui.borrow().config.lyrics_overlay {
            overlay_toggle.remove_css_class("dim");
            let ui3 = ui.clone();
            glib::idle_add_local_once(move || show_overlay(&ui3, true));
        }
    }
    {
        // Near the bottom of a page that goes on, its next part loads.
        let ui2 = ui.clone();
        ui.borrow().w.scroll.vadjustment().connect_value_changed(move |a| {
            if a.value() + a.page_size() >= a.upper() - 700.0 {
                let ui = ui2.clone();
                glib::idle_add_local_once(move || load_more(&ui));
            }
        });
    }
    {
        // The bar's cover opens the lyrics (it never skips the song); RyM YTM: the song's page.
        let ui2 = ui.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let mut s = ui2.borrow_mut();
            s.side_closed = false;
            show_side(&s.w, true);
        });
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
        bar_share.connect_clicked(move |_| {
            if let Some(t) = now_playing(&ui) {
                share(&ui, &t);
            }
        });
    }
    {
        let ui = ui.clone();
        save_queue.connect_clicked(move |_| save_queue_dialog(&ui));
    }
    {
        let ui = ui.clone();
        mini_toggle.connect_clicked(move |_| {
            let open = ui.borrow().mini.is_some();
            show_mini(&ui, !open);
        });
    }
    {
        let ui = ui.clone();
        bar_more.connect_clicked(move |b| {
            if let Some(t) = now_playing(&ui) {
                song_menu(&ui, b.upcast_ref(), b.width() as f64 / 2.0, b.height() as f64 / 2.0, &t);
            }
        });
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
            if let Ok(mut st) = ui.try_borrow_mut() {
                st.volume = s.value() as f32;
                if let Some(a) = &st.audio {
                    a.set_volume(st.volume);
                }
            }
        });
    }
    {
        let ui2 = ui.clone();
        lyrics_toggle.connect_clicked(move |_| {
            let mut st = ui2.borrow_mut();
            let open = !side_shown(&st.w);
            // Closed by hand: it stays closed when the next song starts.
            st.side_closed = !open;
            show_side(&st.w, open);
        });
        // The button is lit while the panel shows, however it opened (RyM YTM: its arrow turns).
        let b = lyrics_toggle.clone();
        let ytm = ui.borrow().w.ytm;
        {
            let ui2 = ui.clone();
            // The page switches inside calls that hold the app's state (playing a song, going to
            // a page): reading it here would stop the app. Looked at a moment later instead.
            let built = move || {
                let ui = ui2.clone();
                glib::idle_add_local_once(move || {
                    let dirty = ui.borrow().queue_dirty;
                    if dirty {
                        render_queue(&ui);
                    }
                });
            };
            let again = built.clone();
            ui.borrow().w.stack.connect_visible_child_name_notify(move |_| built());
            ui.borrow().w.side.connect_visible_child_name_notify(move |_| again());
        }
        ui.borrow().w.stack.connect_visible_child_name_notify(move |s| {
            let open = s.visible_child_name().as_deref() == Some("now");
            if ytm {
                b.set_icon_name(if open { "pan-down-symbolic" } else { "pan-up-symbolic" });
            } else if open {
                b.remove_css_class("dim");
            } else {
                b.add_css_class("dim");
            }
        });
    }
    {
        let ui = ui.clone();
        np_back.connect_clicked(move |_| {
            let mut s = ui.borrow_mut();
            s.side_closed = true;
            show_side(&s.w, false);
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
    match rymplayer::secret::read_cookie_io().ok().filter(|c| !c.trim().is_empty()) {
        Some(cookie) => {
            ui.borrow_mut().home = page_cache::load_home();
            post(&ui, async move { Ev::Auth(AuthMode::Saved, Api::with_cookie(cookie).await) });
        }
        None => post(&ui, async { Ev::Auth(AuthMode::Guest, Api::guest().await) }),
    }
    ui.borrow_mut().home_loading = true;
    if let Some(saved) = session {
        restore_session(&ui, &saved);
    }
    {
        // Over the memory limit: cleaned up at once (checked every 30 seconds).
        let ui = ui.clone();
        glib::timeout_add_seconds_local(30, move || {
            if rymplayer::memory::working_set_mb().is_some_and(|mb| mb > rymplayer::memory::LIMIT_MB) {
                clean_up_memory(&ui, "over the limit");
            }
            glib::ControlFlow::Continue
        });
    }
    {
        // What plays and where, kept every 3 seconds (a crash loses at most that much).
        let ui = ui.clone();
        glib::timeout_add_seconds_local(3, move || {
            save_session(&ui);
            glib::ControlFlow::Continue
        });
    }
    {
        let ui = ui.clone();
        window.connect_close_request(move |_| {
            rymplayer::stats::flush();
            let (overlay, mini) = {
                let mut s = ui.borrow_mut();
                (s.overlay.take(), s.mini.take())
            };
            if let Some((w, ..)) = overlay {
                w.destroy();
            }
            if let Some((w, ..)) = mini {
                w.destroy();
            }
            save_session(&ui);
            glib::Propagation::Proceed
        });
    }
    log(&format!("start: window built after {} ms", since_start()));
    render(&ui);
    watch_frames(&ui);
    {
        let ui2 = ui.clone();
        window.connect_map(move |_| {
            let ui = ui2.clone();
            glib::idle_add_local_once(move || start_media_controls(&ui));
        });
    }
    #[cfg(windows)]
    redraw_after_display_changes(&ui.borrow().w.window);
    window.present();
    offer_crash_report(&ui);
    {
        let tx = ui.borrow().tx.clone();
        rymplayer::link::take_handed_links(move |link| {
            let _ = tx.send(Ev::Incoming(link));
        });
    }
    if let Some(link) = INCOMING.get().cloned() {
        open_incoming(&ui, link);
    }
}

/// Frames that took long to lay out or draw, with the window's size and what was on screen, go
/// to rym-gtk.log (for finding what makes resizing stutter).
fn watch_frames(ui: &Shared) {
    fn count(w: &gtk::Widget) -> usize {
        let mut n = 1;
        let mut child = w.first_child();
        while let Some(c) = child {
            n += count(&c);
            child = c.next_sibling();
        }
        n
    }
    let window = ui.borrow().w.window.clone();
    let ui = Rc::downgrade(ui);
    window.connect_map(move |w| {
        log(&format!("start: window shown after {} ms", since_start()));
        let Some(clock) = w.frame_clock() else { return };
        let first = Rc::new(Cell::new(true));
        let started = Rc::new(Cell::new(None::<Instant>));
        let laid_out = Rc::new(Cell::new(None::<Instant>));
        {
            let started = started.clone();
            clock.connect_before_paint(move |_| started.set(Some(Instant::now())));
        }
        {
            // After GTK's own layout handler: the window has been laid out.
            let laid_out = laid_out.clone();
            clock.connect_layout(move |_| laid_out.set(Some(Instant::now())));
        }
        let (ui, window) = (ui.clone(), w.clone());
        clock.connect_after_paint(move |_| {
            if first.replace(false) {
                log(&format!("start: first frame drawn after {} ms", since_start()));
            }
            let Some(start) = started.take() else { return };
            let total = start.elapsed();
            let layout = laid_out.take().map_or(Duration::ZERO, |t| t.duration_since(start));
            if total < Duration::from_millis(120) {
                return;
            }
            let Some(ui) = ui.upgrade() else { return };
            let Ok(s) = ui.try_borrow() else { return };
            log(&format!(
                "slow frame: {} ms ({} ms layout), window {}x{}, page {}, song page {}, {} widgets in the page, {} in the lyrics, {} in Up next",
                total.as_millis(),
                layout.as_millis(),
                window.width(),
                window.height(),
                page_name(&s.page),
                if side_shown(&s.w) { "open" } else { "shut" },
                count(s.w.content.upcast_ref()),
                count(s.w.lyrics_box.upcast_ref()),
                count(s.w.queue_box.upcast_ref()),
            ));
        });
    });
}

/// The last run crashed: the report is offered (it shows what would be sent); "Open report"
/// opens a GitHub issue filled in with it, and you send it yourself.
fn offer_crash_report(ui: &Shared) {
    let Some(report) = rymplayer::crash::pending() else { return };
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(tr("RyM closed unexpectedly last time"))
        .detail(format!("{}\n\n{report}", tr("OK opens a GitHub issue with this report filled in, and you send it yourself. Nothing is sent otherwise.")))
        .buttons([tr("Not now"), tr("Open report")])
        .cancel_button(0)
        .default_button(1)
        .build();
    let window = ui.borrow().w.window.clone();
    dialog.choose(Some(&window), None::<&gtk::gio::Cancellable>, move |answer| {
        rymplayer::crash::dismiss();
        if answer.is_ok_and(|i| i == 1) {
            let _ = rymplayer::updates::open_link(&rymplayer::crash::issue_url(&report));
        }
    });
}

/// Windows: when the screen's resolution changes (a game running in another resolution, say),
/// the window can lose the surface it draws on through the graphics card and show black from
/// then on. Display changes are noted, and when the window is used again its surface is made anew.
#[cfg(windows)]
fn redraw_after_display_changes(window: &ApplicationWindow) {
    let Some(display) = gdk::Display::default() else { return };
    let changed = Rc::new(Cell::new(false));
    fn watch(m: &gdk::Monitor, changed: &Rc<Cell<bool>>) {
        let c = changed.clone();
        m.connect_geometry_notify(move |m| {
            let g = m.geometry();
            log(&format!("display changed: {}x{}", g.width(), g.height()));
            c.set(true);
        });
        let c = changed.clone();
        m.connect_scale_factor_notify(move |_| c.set(true));
    }
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(m) = monitors.item(i).and_downcast::<gdk::Monitor>() {
            watch(&m, &changed);
        }
    }
    {
        let changed = changed.clone();
        monitors.connect_items_changed(move |list, at, _, added| {
            log("display changed: screens added or removed");
            changed.set(true);
            for i in at..at + added {
                if let Some(m) = list.item(i).and_downcast::<gdk::Monitor>() {
                    watch(&m, &changed);
                }
            }
        });
    }
    // Not while a game holds the screen: only once the window is in use again.
    window.connect_is_active_notify(move |w| {
        if w.is_active() && changed.replace(false) {
            let w = w.clone();
            glib::idle_add_local_once(move || remake_surface(&w));
        }
    });
}

/// Throws the window's drawing surface away and makes a new one (same size, same state).
#[cfg(windows)]
fn remake_surface(w: &ApplicationWindow) {
    log("display changed: making the window's drawing surface anew");
    let (width, height, maximized) = (w.width(), w.height(), w.is_maximized());
    w.set_visible(false);
    WidgetExt::unrealize(w);
    w.set_default_size(width, height);
    if maximized {
        w.maximize();
    }
    w.present();
}

/// The last session's queue, with its song in the bar, paused where it was left: play goes on from there.
fn restore_session(ui: &Shared, saved: &rymplayer::session::Session) {
    let mut wants = Wants::new();
    {
        let mut s = ui.borrow_mut();
        s.q = saved.queue();
        let Some(t) = s.q.now().cloned() else { return };
        s.resume_at = Some((t.video_id.clone(), saved.position));
        s.w.bar_title.set_text(&t.title);
        s.w.bar_artist.set_text(&t.artist);
        s.w.pos.set_text(&fmt_time(saved.position));
        s.w.len.set_text(if t.duration.is_empty() { "0:00" } else { &t.duration });
        show_cover(&s, &t, &mut wants);
    }
    want_covers(ui, wants);
    render_queue(ui);
}

/// Lets go of covers no page is waiting for (the ones on screen keep their own copy) and gives
/// the freed memory back to Windows.
fn clean_up_memory(ui: &Shared, why: &str) {
    let before = rymplayer::memory::working_set_mb();
    {
        let Ok(mut s) = ui.try_borrow_mut() else { return };
        s.songs_since_cleanup = 0;
        let waiting: HashSet<String> = s.waiting.keys().cloned().collect();
        s.covers.retain(|url, _| waiting.contains(url));
        s.cover_order.retain(|url| waiting.contains(url));
        s.requested.retain(|url| waiting.contains(url));
    }
    rymplayer::memory::trim();
    log(&format!("memory cleaned up ({why}): {:?} MB -> {:?} MB", before, rymplayer::memory::working_set_mb()));
}

/// Writes what plays, where in it, and the volume to the session file, when any of it changed.
fn save_session(ui: &Shared) {
    let Ok(mut s) = ui.try_borrow_mut() else { return };
    let now = s.q.now().map(|t| t.video_id.clone()).unwrap_or_default();
    let position = match (&s.resume_at, &s.audio) {
        (Some((id, at)), _) if *id == now => *at,
        (_, Some(a)) if a.is_loaded() => a.position(),
        _ => 0.0,
    };
    let key = (now, position as u32, (s.volume * 100.0) as u32, s.q.songs.len());
    if s.session_key.as_ref() == Some(&key) {
        return;
    }
    rymplayer::session::save(&rymplayer::session::Session::new(&s.q, position, s.volume));
    s.session_key = Some(key);
}

// ---------------------------------------------------------------- navigation

fn go(ui: &Shared, page: Page) {
    to_page(ui);
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
                post(ui, async move { Ev::Paged(id, api.playlist_paged(LIKED_ID.into()).await) });
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

/// Opens a list page that goes on as it's scrolled down (search results, a playlist).
fn open_paged(ui: &Shared, title: String, load: impl Future<Output = anyhow::Result<rymplayer::api::Paged>> + Send + 'static) {
    to_page(ui);
    {
        let mut s = ui.borrow_mut();
        s.from_search = false;
        s.page = Page::Songs(title);
        s.page_id += 1;
        s.tracks.clear();
        s.tracks_loading = true;
        s.list_more = None;
        let id = s.page_id;
        spawn(&s, async move { Ev::Paged(id, load.await) });
    }
    render(ui);
}


/// The sound settings into the players (effects for every song, speed for the playing one) and
/// saved.
fn apply_sound(ui: &Shared) {
    let s = ui.borrow();
    rymplayer::fx::set(s.config.fx());
    if let Some(a) = &s.audio {
        a.set_speed(s.config.speed);
    }
    s.config.save();
}

fn switch(on: bool, ui: &Shared, set: impl Fn(&mut Config, bool) + 'static) -> gtk::Switch {
    let w = gtk::Switch::new();
    w.set_active(on);
    w.set_valign(Align::Center);
    let ui = ui.clone();
    w.connect_active_notify(move |w| {
        set(&mut ui.borrow_mut().config, w.is_active());
        apply_sound(&ui);
    });
    w
}

/// Settings > Sound: crossfade, skip silence, equalizer, even loudness; Discord; floating lyrics.
fn sound_settings(ui: &Shared, page: &gtk::Box, config: &Config) {
    page.append(&label(tr("Sound"), &["shelf"]));
    let fade = Scale::with_range(Orientation::Horizontal, 0.0, 10.0, 1.0);
    fade.set_value(config.crossfade as f64);
    fade.set_digits(0);
    fade.set_draw_value(true);
    fade.set_size_request(220, -1);
    {
        let ui = ui.clone();
        fade.connect_value_changed(move |w| {
            ui.borrow_mut().config.crossfade = w.value().round() as u8;
            apply_sound(&ui);
        });
    }
    page.append(&setting_row(tr("Crossfade"), tr("Seconds the end of a song and the start of the next play together (0 = off)"), &fade));
    page.append(&setting_row(tr("Smart Automix"), tr("Songs blend like a DJ mix: on the fade-out, on the beat, without the basses clashing"), &switch(config.automix, ui, |c, on| c.automix = on)));
    page.append(&setting_row(tr("Skip non-music parts"), tr("SponsorBlock: a music video's intro, talk and outro are skipped"), &switch(config.sponsorblock, ui, |c, on| c.sponsorblock = on)));
    page.append(&setting_row(tr("Romaji"), tr("Japanese lines in Latin letters under them"), &switch(config.romaji, ui, |c, on| c.romaji = on)));
    {
        // The alarm: on or off, and its time (07:30).
        let row = gtk::Box::new(Orientation::Horizontal, 8);
        let time = gtk::Entry::new();
        time.set_text(if config.alarm_time.is_empty() { "07:30" } else { &config.alarm_time });
        time.set_width_chars(6);
        time.set_max_length(5);
        time.set_valign(Align::Center);
        {
            let ui = ui.clone();
            time.connect_changed(move |e| {
                let t = e.text().to_string();
                let ok = t.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?))).filter(|(h, m)| *h < 24 && *m < 60);
                if let Some((h, m)) = ok {
                    let mut s = ui.borrow_mut();
                    s.config.alarm_time = format!("{h:02}:{m:02}");
                    s.config.alarm_rang.clear();
                    s.config.save();
                }
            });
        }
        row.append(&time);
        row.append(&switch(config.alarm_on, ui, |c, on| c.alarm_on = on));
        page.append(&setting_row(tr("Alarm"), tr("Music starts at this time (RyM has to be open)"), &row));
    }
    {
        // Japanese lyrics: English under each line, or not (the lyrics shown again either way).
        let w = gtk::Switch::new();
        w.set_active(config.lyrics_translation);
        w.set_valign(Align::Center);
        let page_ui = ui.clone();
        let ui = ui.clone();
        w.connect_active_notify(move |w| {
            let lyrics = {
                let mut s = ui.borrow_mut();
                s.config.lyrics_translation = w.is_active();
                s.config.save();
                s.lyrics.clone()
            };
            if lyrics.is_some() {
                let shown = lyrics.and_then(|l| now_playing(&ui).map(|t| (t, l)));
                // Turned on: the saved lyrics may already hold the English.
                let l = shown.map(|(t, l)| rymplayer::lyrics::cached(&t.video_id).flatten().filter(|c| c.source == l.source).unwrap_or(l));
                show_lyrics(&ui, l);
            }
        });
        let size = Scale::with_range(Orientation::Horizontal, 14.0, 48.0, 1.0);
        size.set_value(config.lyrics_font as f64);
        size.set_digits(0);
        size.set_draw_value(true);
        size.set_size_request(220, -1);
        {
            let ui = page_ui.clone();
            size.connect_value_changed(move |w| {
                let px = w.value().round() as u8;
                apply_lyrics_font(px);
                let mut s = ui.borrow_mut();
                s.config.lyrics_font = px;
                s.config.save();
            });
        }
        page.append(&setting_row(tr("Lyrics size"), tr("How big the lyrics are on the song's page"), &size));
        page.append(&setting_row(tr("Translate Japanese lyrics"), tr("English in small letters under each line: the lyrics site's own translation, else a machine translation"), &w));
    }
    page.append(&setting_row(
        "Seamless Music",
        tr("Music playing on your phone can move to this PC (same Wi-Fi and Google account); off: the phone is ignored"),
        &switch(config.cross_ears, ui, |c, on| c.cross_ears = on),
    ));
    page.append(&setting_row(
        tr("Take over from the phone"),
        tr("Music playing on your phone (same Wi-Fi and account) moves here when RyM opens; off: it asks first"),
        &switch(config.link_auto, ui, |c, on| c.link_auto = on),
    ));
    page.append(&setting_row(tr("Skip silence"), tr("A quiet start is skipped; a long quiet end finishes the song"), &switch(config.skip_silence, ui, |c, on| c.skip_silence = on)));
    page.append(&setting_row(tr("Even loudness"), tr("Every song at about the same loudness, without sudden loud parts"), &switch(config.normalize, ui, |c, on| c.normalize = on)));

    let names: Vec<&str> = rymplayer::fx::PRESETS.iter().map(|p| tr(p.0)).collect();
    let preset = gtk::DropDown::from_strings(&names);
    preset.set_selected(config.eq_preset as u32);
    let bands = gtk::Box::new(Orientation::Horizontal, 14);
    let mut sliders = Vec::new();
    for (i, hz) in rymplayer::fx::BANDS.iter().enumerate() {
        let col = gtk::Box::new(Orientation::Vertical, 4);
        let sl = Scale::with_range(Orientation::Vertical, -12.0, 12.0, 0.5);
        sl.set_inverted(true);
        sl.set_size_request(-1, 130);
        sl.set_draw_value(false);
        sl.set_value(config.fx().eq[i] as f64);
        col.append(&sl);
        col.append(&label(&if *hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz}") }, &["dim", "small"]));
        bands.append(&col);
        sliders.push(sl);
    }
    {
        let (ui, sliders) = (ui.clone(), sliders.clone());
        preset.connect_selected_notify(move |d| {
            let gains = {
                let mut s = ui.borrow_mut();
                s.config.eq_preset = d.selected() as usize;
                s.config.fx().eq
            };
            apply_sound(&ui);
            // The sliders show the preset (moving one makes it "Custom").
            for (sl, g) in sliders.iter().zip(gains) {
                sl.set_value(g as f64);
            }
        });
    }
    for (i, sl) in sliders.iter().enumerate() {
        let (ui, preset) = (ui.clone(), preset.clone());
        sl.connect_change_value(move |_, _, v| {
            let custom = rymplayer::fx::PRESETS.len() - 1;
            {
                let mut s = ui.borrow_mut();
                if s.config.eq_preset != custom {
                    s.config.eq_custom = s.config.fx().eq;
                }
                s.config.eq_custom[i] = v.clamp(-12.0, 12.0) as f32;
                s.config.eq_preset = custom;
            }
            if preset.selected() as usize != custom {
                preset.set_selected(custom as u32);
            }
            apply_sound(&ui);
            glib::Propagation::Proceed
        });
    }
    page.append(&setting_row(tr("Equalizer"), tr("Flat, Bass Boost, Vocal, Rock, Pop, or your own (move a band)"), &preset));
    page.append(&bands);

    page.append(&label(tr("Suggestions (RaM points)"), &["shelf"]));
    let summary = label(&ram_summary(), &["dim", "small"]);
    summary.set_wrap(true);
    summary.set_halign(Align::Start);
    page.append(&summary);
    let reset_all = Button::with_label(tr("Reset all"));
    reset_all.add_css_class("ghost");
    {
        // Twice to be sure: the first press asks.
        let asked = Rc::new(std::cell::Cell::new(None::<Instant>));
        let summary = summary.clone();
        reset_all.connect_clicked(move |b| {
            if asked.get().is_some_and(|at| at.elapsed() < Duration::from_secs(5)) {
                asked.set(None);
                rymplayer::ram_points::reset_all();
                b.set_label(tr("Reset all"));
                summary.set_text(tr("All points are back to 0"));
            } else {
                asked.set(Some(Instant::now()));
                b.set_label(tr("Press again to reset all points"));
            }
        });
    }
    page.append(&setting_row(tr("Reset all points"), "", &reset_all));
    let reset_neg = Button::with_label(tr("Reset"));
    reset_neg.add_css_class("ghost");
    {
        let summary = summary.clone();
        reset_neg.connect_clicked(move |_| {
            rymplayer::ram_points::reset_negative();
            summary.set_text(tr("Negative points are back to 0"));
        });
    }
    page.append(&setting_row(tr("Reset negative points only"), tr("Skipped songs come back; your favorites stay"), &reset_neg));
    // A day the points reset by themselves (empty: none).
    let date = gtk::Entry::new();
    date.set_text(&config.ram_reset_date);
    date.set_placeholder_text(Some("2026-12-31"));
    date.set_width_chars(12);
    {
        let ui = ui.clone();
        date.connect_changed(move |e| {
            let text = e.text();
            let mut s = ui.borrow_mut();
            if text.trim().is_empty() {
                s.config.ram_reset_date.clear();
            } else if let Some(d) = rymplayer::ram_points::reset_date(&text) {
                s.config.ram_reset_date = d;
            } else {
                e.add_css_class("error");
                return;
            }
            e.remove_css_class("error");
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Reset on a date"), tr("Like 2026-12-31; empty: no date. On that day all points go back to 0"), &date));
    // Every week: off unless turned on, and only after the warning.
    let weekly = gtk::Switch::new();
    weekly.set_active(config.ram_weekly_reset);
    weekly.set_valign(Align::Center);
    {
        let ui = ui.clone();
        weekly.connect_active_notify(move |w| {
            let on = w.is_active();
            if on == ui.borrow().config.ram_weekly_reset {
                return;
            }
            if !on {
                let mut s = ui.borrow_mut();
                s.config.ram_weekly_reset = false;
                s.config.save();
                return;
            }
            w.set_active(false);
            let dialog = gtk::AlertDialog::builder()
                .modal(true)
                .message(tr("Turn on the weekly reset?"))
                .detail(tr("Every 7 days, all the points RyM learned from what you play go back to 0, and the suggestions start learning again from scratch. Your likes, playlists and history stay. You can still reset by hand or pick a date instead."))
                .buttons([tr("Cancel"), tr("Turn on")])
                .cancel_button(0)
                .default_button(1)
                .build();
            let window = ui.borrow().w.window.clone();
            let (ui, w) = (ui.clone(), w.clone());
            dialog.choose(Some(&window), None::<&gtk::gio::Cancellable>, move |answer| {
                if answer.is_ok_and(|i| i == 1) {
                    {
                        let mut s = ui.borrow_mut();
                        s.config.ram_weekly_reset = true;
                        // The first reset comes a week from now.
                        s.config.ram_last_reset = rymplayer::updates::now();
                        s.config.save();
                    }
                    w.set_active(true);
                }
            });
        });
    }
    page.append(&setting_row(tr("Weekly reset"), tr("Every 7 days all points go back to 0 (off unless you turn it on)"), &weekly));

    page.append(&label(tr("On this device"), &["shelf"]));
    let folders = gtk::Entry::new();
    folders.set_text(&config.local_folders.join("; "));
    folders.set_placeholder_text(Some("/home/you/Songs"));
    folders.set_width_chars(28);
    {
        let ui = ui.clone();
        folders.connect_changed(move |e| {
            let mut s = ui.borrow_mut();
            s.config.local_folders = e.text().split(';').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
            s.config.save();
        });
    }
    // The system's folder window: the folder picked joins the list.
    let add_folder = Button::with_label(tr("Add a folder"));
    add_folder.add_css_class("ghost");
    {
        let ui = ui.clone();
        let folders = folders.clone();
        add_folder.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder().title(tr("Pick a music folder")).modal(true).build();
            let window = ui.borrow().w.window.clone();
            let folders = folders.clone();
            dialog.select_folder(Some(&window), None::<&gtk::gio::Cancellable>, move |picked| {
                let Some(dir) = picked.ok().and_then(|f| f.path()) else { return };
                let dir = dir.to_string_lossy().to_string();
                let mut list: Vec<String> = folders.text().split(';').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
                if !list.contains(&dir) {
                    list.push(dir);
                    // The box's own handler saves it.
                    folders.set_text(&list.join("; "));
                }
            });
        });
    }
    let folder_row = gtk::Box::new(Orientation::Horizontal, 8);
    folder_row.append(&folders);
    folder_row.append(&add_folder);
    page.append(&setting_row(tr("Other music folders"), tr("Besides your Music folder; separate them with ;"), &folder_row));

    page.append(&label("Discord", &["shelf"]));
    let app_id = gtk::Entry::new();
    app_id.set_text(&config.discord_app_id);
    app_id.set_placeholder_text(Some(tr("Application ID")));
    app_id.set_width_chars(22);
    {
        let ui = ui.clone();
        app_id.connect_changed(move |e| {
            let mut s = ui.borrow_mut();
            s.config.discord_app_id = e.text().trim().to_string();
            s.discord_sent = None;
            s.config.save();
        });
    }
    let discord = gtk::Switch::new();
    discord.set_active(config.discord);
    discord.set_valign(Align::Center);
    {
        let ui = ui.clone();
        discord.connect_active_notify(move |w| {
            let mut s = ui.borrow_mut();
            s.config.discord = w.is_active();
            s.discord_sent = None;
            s.config.save();
            if !s.config.discord {
                rymplayer::discord::show("", rymplayer::discord::Show::Off);
            }
        });
    }
    page.append(&setting_row(tr("Show on Discord"), tr("The song you're playing on your Discord profile (Discord must be running)"), &discord));
    page.append(&setting_row(tr("Discord application ID"), tr("Make an application at discord.com/developers and paste its ID"), &app_id));

    page.append(&label(tr("Floating lyrics"), &["shelf"]));
    let through = switch(config.overlay_click_through, ui, |c, on| c.overlay_click_through = on);
    {
        let ui = ui.clone();
        through.connect_active_notify(move |_| {
            let on = ui.borrow().config.lyrics_overlay;
            if on {
                show_overlay(&ui, false);
                show_overlay(&ui, true);
            }
        });
    }
    page.append(&setting_row(tr("Clicks go through"), tr("The floating lyrics can't be dragged then, and never get in the way"), &through));
}

/// The bar's speed and pitch: two sliders and the quick picks (Normal, Nightcore, Slowed).
fn speed_popover(ui: &Shared) -> gtk::Popover {
    let pop = gtk::Popover::new();
    let col = gtk::Box::new(Orientation::Vertical, 8);
    col.set_margin_top(12);
    col.set_margin_bottom(12);
    col.set_margin_start(14);
    col.set_margin_end(14);
    let config = ui.borrow().config.clone();
    let slider = |name: &str, v: f32| {
        col.append(&label(name, &["dim", "small"]));
        let sl = Scale::with_range(Orientation::Horizontal, 0.5, 2.0, 0.05);
        sl.set_value(v as f64);
        sl.set_digits(2);
        sl.set_draw_value(true);
        sl.set_size_request(240, -1);
        col.append(&sl);
        sl
    };
    let speed = slider(tr("Speed"), config.speed);
    let pitch = slider(tr("Pitch"), config.pitch);
    {
        let ui = ui.clone();
        speed.connect_value_changed(move |w| {
            ui.borrow_mut().config.speed = w.value() as f32;
            apply_sound(&ui);
        });
    }
    {
        let ui = ui.clone();
        pitch.connect_value_changed(move |w| {
            ui.borrow_mut().config.pitch = w.value() as f32;
            apply_sound(&ui);
        });
    }
    let modes = gtk::Box::new(Orientation::Horizontal, 6);
    for (name, sp, pi, _) in rymplayer::fx::MODES {
        let b = Button::with_label(tr(name));
        b.add_css_class("ghost");
        let (ui, speed, pitch) = (ui.clone(), speed.clone(), pitch.clone());
        b.connect_clicked(move |_| {
            ui.borrow_mut().config.set_mode(name);
            speed.set_value(sp as f64);
            pitch.set_value(pi as f64);
            apply_sound(&ui);
        });
        modes.append(&b);
    }
    col.append(&modes);
    // Smart automix on or off, from the player too (it shows the setting each time it opens).
    let mix_row = gtk::Box::new(Orientation::Horizontal, 10);
    mix_row.set_margin_top(6);
    let mix_name = label(tr("Smart Automix"), &["title"]);
    mix_name.set_hexpand(true);
    mix_row.append(&mix_name);
    let mix = switch(config.automix, ui, |c, on| c.automix = on);
    mix_row.append(&mix);
    col.append(&mix_row);
    // Skip silence too (the quiet starts and ends of songs, in mixes as well).
    let quiet_row = gtk::Box::new(Orientation::Horizontal, 10);
    let quiet_name = label(tr("Skip silence"), &["title"]);
    quiet_name.set_hexpand(true);
    quiet_row.append(&quiet_name);
    let quiet = switch(config.skip_silence, ui, |c, on| c.skip_silence = on);
    quiet_row.append(&quiet);
    col.append(&quiet_row);
    {
        let ui = ui.clone();
        pop.connect_show(move |_| {
            let (automix, skip) = {
                let s = ui.borrow();
                (s.config.automix, s.config.skip_silence)
            };
            mix.set_active(automix);
            quiet.set_active(skip);
        });
    }
    pop.set_child(Some(&col));
    pop
}

/// The bar's sleep timer: 15, 30, 45, 60 minutes or the end of the song; the last 10 seconds fade out.
fn sleep_popover(ui: &Shared) -> gtk::Popover {
    let pop = gtk::Popover::new();
    let col = gtk::Box::new(Orientation::Vertical, 2);
    col.set_margin_top(8);
    col.set_margin_bottom(8);
    let mut choices: Vec<String> = [15, 30, 45, 60].iter().map(|m| format!("{m} {}", tr("minutes"))).collect();
    choices.push(tr("End of this song").to_string());
    choices.push(tr("Off").to_string());
    for (i, text) in choices.iter().enumerate() {
        let b = Button::with_label(text);
        b.add_css_class("flat-row");
        let (ui, pop2) = (ui.clone(), pop.clone());
        b.connect_clicked(move |_| {
            pop2.popdown();
            let sleep = match i {
                0..=3 => Some(Sleep::At(Instant::now() + Duration::from_secs(15 * 60 * (i as u64 + 1)))),
                4 => Some(Sleep::EndOfSong),
                _ => None,
            };
            ui.borrow_mut().sleep = sleep;
            if let Some(a) = &ui.borrow().audio {
                a.set_fade(1.0);
            }
            toast(&ui, match sleep {
                Some(Sleep::At(_)) => tr("Sleep timer set"),
                Some(Sleep::EndOfSong) => tr("The music stops after this song"),
                None => tr("Sleep timer off"),
            });
        });
        col.append(&b);
    }
    pop.set_child(Some(&col));
    pop
}

/// "Your recap": hours listened (in all and this month), top songs, top artists this month.
fn recap_page(ui: &Shared, wants: &mut Wants) -> gtk::Widget {
    let page = page_box();
    page.append(&label(tr("Your recap"), &["heading"]));
    let r = rymplayer::stats::recap();
    let numbers = gtk::Box::new(Orientation::Horizontal, 40);
    for (n, what) in [(r.hours_month, tr("hours this month")), (r.hours_total, tr("hours in all"))] {
        let col = gtk::Box::new(Orientation::Vertical, 2);
        col.append(&label(&format!("{n:.1}"), &["heading"]));
        col.append(&label(what, &["dim"]));
        numbers.append(&col);
    }
    page.append(&numbers);
    page.append(&label(tr("Top songs"), &["shelf"]));
    if r.top_songs.is_empty() {
        page.append(&note(tr("Play some songs and they'll show here")));
    }
    for (t, plays) in &r.top_songs {
        let row = gtk::Box::new(Orientation::Horizontal, 12);
        row.append(&cover(&ui.borrow(), t.thumb.as_ref(), ROW_PX, 42, "cover", wants));
        let names = gtk::Box::new(Orientation::Vertical, 2);
        names.set_hexpand(true);
        names.append(&label(&t.title, &["title"]));
        names.append(&label(&t.artist, &["dim", "small"]));
        row.append(&names);
        row.append(&label(&format!("{plays} {}", tr("plays")), &["dim", "small"]));
        let b = Button::new();
        b.add_css_class("flat-row");
        b.set_child(Some(&row));
        let (ui2, track) = (ui.clone(), t.clone());
        b.connect_clicked(move |_| play_with_radio(&ui2, track.clone()));
        page.append(&b);
    }
    page.append(&label(tr("Top artists this month"), &["shelf"]));
    for (artist, hours) in &r.top_artists {
        let row = gtk::Box::new(Orientation::Horizontal, 12);
        let name = label(artist, &["title"]);
        name.set_hexpand(true);
        row.append(&name);
        row.append(&label(&format!("{:.0} {}", hours * 60.0, tr("minutes")), &["dim", "small"]));
        page.append(&row);
    }
    page.upcast()
}

/// The floating lyrics: a small borderless window above every other one (games too, when
/// they're windowed) with the sung line and the next. Dragged with the mouse, or set so clicks
/// go through it.
fn show_overlay(ui: &Shared, on: bool) {
    if !on {
        if let Some((w, ..)) = ui.borrow_mut().overlay.take() {
            w.destroy();
        }
        return;
    }
    if ui.borrow().overlay.is_some() {
        return;
    }
    let win = gtk::Window::new();
    win.set_title(Some(OVERLAY_TITLE));
    win.set_decorated(false);
    win.set_default_size(760, 96);
    win.set_resizable(false);
    win.add_css_class("rym");
    win.add_css_class("overlay-win");
    let col = gtk::Box::new(Orientation::Vertical, 2);
    col.set_margin_top(10);
    col.set_margin_bottom(10);
    col.set_margin_start(18);
    col.set_margin_end(18);
    let now = label("♪", &["overlay-now"]);
    let next = label("", &["overlay-next"]);
    now.set_wrap(true);
    now.set_justify(gtk::Justification::Center);
    now.set_halign(Align::Center);
    next.set_halign(Align::Center);
    col.append(&now);
    col.append(&next);
    win.set_child(Some(&col));
    let through = ui.borrow().config.overlay_click_through;
    if !through {
        // Dragged anywhere with the left button.
        let press = gtk::GestureClick::new();
        let w = win.clone();
        press.connect_pressed(move |g, _, x, y| {
            if let (Some(surface), Some(device)) = (w.surface(), g.device())
                && let Ok(top) = surface.downcast::<gdk::Toplevel>()
            {
                top.begin_move(&device, g.current_button() as i32, x, y, g.current_event_time());
            }
        });
        win.add_controller(press);
    }
    win.present();
    // Above everything, and (if asked) not in the way of clicks.
    glib::timeout_add_local_once(Duration::from_millis(200), move || {
        #[cfg(windows)]
        overlay_on_top(through);
    });
    if through && let Some(surface) = win.surface() {
        surface.set_input_region(Some(&gtk::cairo::Region::create()));
    }
    ui.borrow_mut().overlay = Some((win, now, next));
}

const OVERLAY_TITLE: &str = "RyM floating lyrics";

/// Windows: the floating lyrics window above every other (and clicks through it, when asked).
#[cfg(windows)]
fn overlay_on_top(click_through: bool) {
    window_on_top(OVERLAY_TITLE, click_through);
}

/// Windows: one of our small windows (by its title) above every other, out of the taskbar.
#[cfg(windows)]
fn window_on_top(window_title: &str, click_through: bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> isize;
        fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    }
    let title: Vec<u16> = window_title.encode_utf16().chain(Some(0)).collect();
    // SAFETY: plain Win32 calls with a NUL-terminated title; a missing window gives 0 and is skipped.
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 {
            return;
        }
        const HWND_TOPMOST: isize = -1;
        const SWP_NOMOVE: u32 = 0x2;
        const SWP_NOSIZE: u32 = 0x1;
        const SWP_NOACTIVATE: u32 = 0x10;
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        // Not in the taskbar or Alt+Tab (a tool window).
        const GWL_EXSTYLE: i32 = -20;
        const WS_EX_TOOLWINDOW: isize = 0x80;
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let _ = click_through;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_TOOLWINDOW);
    }
}

/// The open page's next part, when it goes on and nothing is loading already.
fn load_more(ui: &Shared) {
    let Some(api) = api_of(ui) else { return };
    let (more, id) = {
        let Ok(mut s) = ui.try_borrow_mut() else { return };
        if s.loading_more || s.tracks_loading || s.home_loading {
            return;
        }
        let more = match s.page {
            Page::Home => s.home_more.clone(),
            Page::Songs(_) | Page::Liked => s.list_more.clone(),
            _ => None,
        };
        let Some(more) = more else { return };
        s.loading_more = true;
        if let Some(spin) = s.more_spinner.as_ref().and_then(glib::WeakRef::upgrade) {
            spin.set_visible(true);
            spin.start();
        }
        (more, s.page_id)
    };
    post(ui, async move { Ev::More(id, api.more(more).await) });
}

/// The next part of the open page, put under what's there (nothing already on the page).
fn add_more(ui: &Shared, part: rymplayer::api::Paged) {
    let mut wants = Wants::new();
    let (fresh, home, target) = {
        let mut s = ui.borrow_mut();
        let home = s.page == Page::Home;
        let have: HashSet<String> = if home { s.home.iter().map(item_key).collect() } else { s.tracks.iter().map(|t| t.video_id.clone()).collect() };
        let mut seen = have;
        let fresh: Vec<Track> = part.tracks.into_iter().filter(|t| seen.insert(item_key(t))).collect();
        if home {
            s.home_more = part.more;
            s.home.extend(fresh.iter().cloned());
        } else {
            s.list_more = part.more;
            s.tracks.extend(fresh.iter().cloned());
        }
        (fresh, home, s.more_box.as_ref().and_then(glib::WeakRef::upgrade))
    };
    let Some(target) = target else { return };
    if home {
        for (name, items) in shelves(fresh.clone()) {
            target.append(&label(shelf_name(&name), &["shelf"]));
            let grid = home_grid();
            for t in items {
                grid.insert(&home_card(ui, &t, &mut wants), -1);
            }
            target.append(&grid);
        }
    } else {
        let start = ui.borrow().tracks.len() - fresh.len();
        let playing = now_playing(ui).map(|t| t.video_id);
        for (i, t) in fresh.iter().enumerate() {
            target.append(&song_row(ui, start + i, t, playing.as_deref(), &mut wants));
        }
    }
    want_covers(ui, wants);
    refresh_hearts(ui);
    let ids: Vec<String> = fresh.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect();
    post(ui, async move {
        stream::look_ahead(ids).await;
        Ev::Done
    });
    // Still short of the bottom: the next part comes too.
    let ui = ui.clone();
    glib::idle_add_local_once(move || {
        let a = ui.borrow().w.scroll.vadjustment();
        if a.value() + a.page_size() >= a.upper() - 700.0 {
            load_more(&ui);
        }
    });
}

fn item_key(t: &Track) -> String {
    match &t.link {
        Some(Link::Playlist(id) | Link::Album(id) | Link::Mix(id)) => id.clone(),
        None => t.video_id.clone(),
    }
}

/// Opens a list page and loads its songs.
fn open_songs(ui: &Shared, title: String, load: impl Future<Output = anyhow::Result<Vec<Track>>> + Send + 'static) {
    to_page(ui);
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

/// "Play in RyM": a link from outside (the command line, a `rym://` link, another copy of RyM).
/// A song plays now, as one picked by hand (RaM points: a song below zero comes back to zero),
/// with its radio after it; a playlist, an album, a mix or an artist opens.
fn open_incoming(ui: &Shared, link: String) {
    use rymplayer::links::Target;
    use rymplayer::plugins;
    let Some(target) = rymplayer::links::parse(&link) else {
        toast(ui, tr("Not a YouTube Music link"));
        return;
    };
    let Some(api) = api_of(ui) else {
        ui.borrow_mut().pending_link = Some(link);
        return;
    };
    match target {
        Target::Song(id) => {
            toast(ui, &format!("{}…", tr("Loading")));
            post(ui, async move { Ev::Handed(Ok(core::song_with_radio(api, id).await)) });
        }
        Target::Playlist(id) => open_paged(ui, tr("Playlist").into(), api.playlist_paged(id)),
        Target::Album(id) => open_songs(ui, tr("Album").into(), api.album_tracks(id)),
        Target::Mix(id) => open_songs(ui, tr("Mix").into(), api.mix_tracks(id)),
        Target::Artist(id) => open_songs(ui, tr("Artist").into(), api.artist_page(id)),
        t @ Target::Spotify { one_song: true, .. } if has_plugin(ui, plugins::SPOTIFY) => {
            toast(ui, tr("Finding the Spotify songs on YouTube Music..."));
            post(ui, async move { Ev::Handed(core::found_song(api, t).await) });
        }
        t @ Target::SoundCloud { one_song: true, .. } if has_plugin(ui, plugins::SOUNDCLOUD) => post(ui, async move { Ev::Handed(core::found_song(api, t).await) }),
        // A Spotify or SoundCloud list (or its plugin isn't there: the search says so).
        Target::Spotify { link, .. } | Target::SoundCloud { link, .. } => search_songs(ui, link),
    }
}

fn open_link(ui: &Shared, t: &Track) {
    let Some(api) = api_of(ui) else { return };
    match t.link.clone() {
        Some(Link::Playlist(id)) => open_paged(ui, t.title.clone(), api.playlist_paged(id)),
        Some(Link::Album(id)) => open_songs(ui, t.title.clone(), api.album_tracks(id)),
        Some(Link::Mix(id)) => open_songs(ui, t.title.clone(), api.mix_tracks(id)),
        None => {}
    }
}

fn search_songs(ui: &Shared, q: String) {
    use rymplayer::{plugins, soundcloud, spotify};
    if soundcloud::is_link(&q) || searching_soundcloud(ui) {
        if !has_plugin(ui, plugins::SOUNDCLOUD) {
            toast(ui, tr("Install the SoundCloud plugin to open SoundCloud links"));
            return;
        }
        if soundcloud::is_link(&q) {
            open_songs(ui, "SoundCloud".into(), soundcloud::open_link(q));
        } else {
            open_songs(ui, format!("SoundCloud: \"{q}\""), soundcloud::search(q));
            ui.borrow_mut().from_search = true;
        }
        return;
    }
    let Some(api) = api_of(ui) else {
        toast(ui, tr("Still connecting to YouTube Music"));
        return;
    };
    if spotify::is_link(&q) {
        if !has_plugin(ui, plugins::SPOTIFY) {
            toast(ui, tr("Install the Spotify plugin to open Spotify links"));
            return;
        }
        toast(ui, tr("Finding the Spotify songs on YouTube Music..."));
        open_songs(ui, "Spotify".into(), spotify::import(api, q));
        return;
    }
    open_paged(ui, format!("{} \"{q}\"", tr("Results for")), api.search_paged(q));
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
        Page::Plugins => "plugins",
        Page::Recap => "recap",
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
        drop(s);
        let mut s = ui.borrow_mut();
        s.more_box = None;
        s.more_spinner = None;
        s.page.clone()
    };
    let body: gtk::Widget = match &page {
        Page::Home => home_page(ui, &mut wants),
        Page::Liked => list_page(ui, tr("Liked Music"), true, &mut wants),
        Page::History => list_page(ui, tr("History"), false, &mut wants),
        Page::Songs(title) => list_page(ui, title, false, &mut wants),
        Page::Library => library_page(ui, &mut wants),
        Page::Settings => settings_page(ui),
        Page::Plugins => plugins_page(ui),
        Page::Recap => recap_page(ui, &mut wants),
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
    // "Recently played" as a row, then the shelves ("For you" first), each a grid of cards.
    let ytm = ui.borrow().w.ytm;
    for (name, items) in shelves(home) {
        page.append(&label(shelf_name(&name), &["shelf"]));
        if ytm {
            page.append(&big_row(ui, &items, wants));
            continue;
        }
        if name == feed::RECENTLY_PLAYED {
            page.append(&recent_row(ui, &items, wants));
            continue;
        }
        let grid = home_grid();
        for t in items {
            grid.insert(&home_card(ui, &t, wants), -1);
        }
        page.append(&grid);
    }
    more_tail(ui, &page, None);
    page.upcast()
}

/// Items grouped by their shelf, in order.
fn shelves(items: Vec<Track>) -> Vec<(String, Vec<Track>)> {
    let mut out: Vec<(String, Vec<Track>)> = Vec::new();
    for t in items {
        match out.iter_mut().find(|(name, _)| *name == t.album) {
            Some((_, items)) => items.push(t),
            None => out.push((t.album.clone(), vec![t])),
        }
    }
    out
}

/// The feed's own shelves in the app's language; YouTube's as they come.
fn shelf_name(name: &str) -> &str {
    match name {
        feed::RECENTLY_PLAYED => tr("Recently played"),
        feed::FOR_YOU => tr("For you"),
        feed::MORE_FOR_YOU => tr("More for you"),
        // A second kind's own shelf ("Husseini for you"...).
        other => rymplayer::ram_points::SHELVES.iter().find(|s| **s == other).map_or(other, |s| tr(s)),
    }
}

fn home_grid() -> gtk::FlowBox {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(2);
    grid.set_max_children_per_line(5);
    grid.set_column_spacing(6);
    grid.set_row_spacing(6);
    grid
}

/// "Recently played": one row of covers that scrolls sideways; a click plays the song again.
fn recent_row(ui: &Shared, items: &[Track], wants: &mut Wants) -> gtk::Widget {
    let row = gtk::Box::new(Orientation::Horizontal, 4);
    for t in items {
        let b = Button::new();
        b.add_css_class("recent");
        let col = gtk::Box::new(Orientation::Vertical, 4);
        col.append(&cover(&ui.borrow(), t.thumb.as_ref(), CARD_PX, 116, "cover", wants));
        let title = label(&t.title, &["title", "small"]);
        title.set_max_width_chars(14);
        col.append(&title);
        let artist = label(&t.artist, &["dim", "small"]);
        artist.set_max_width_chars(14);
        col.append(&artist);
        b.set_child(Some(&col));
        let (ui2, track) = (ui.clone(), t.clone());
        b.connect_clicked(move |_| play_with_radio(&ui2, track.clone()));
        song_menu_on(ui, &b, t.clone());
        row.append(&b);
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&row));
    scroll.set_min_content_height(196);
    scroll.upcast()
}

/// RyM YTM: a shelf as one row of big covers that scrolls sideways; songs play (with radio
/// after), the rest open.
fn big_row(ui: &Shared, items: &[Track], wants: &mut Wants) -> gtk::Widget {
    let row = gtk::Box::new(Orientation::Horizontal, 18);
    for t in items {
        let b = Button::new();
        b.add_css_class("big-card");
        let col = gtk::Box::new(Orientation::Vertical, 6);
        col.append(&cover(&ui.borrow(), t.thumb.as_ref(), BIG_PX / 2, 190, "cover", wants));
        let title = label(&t.title, &["title"]);
        title.set_max_width_chars(20);
        col.append(&title);
        let kind = match &t.link {
            Some(Link::Playlist(_)) => tr("Playlist"),
            Some(Link::Album(_)) => tr("Album"),
            Some(Link::Mix(_)) => tr("Mix"),
            None => tr("Song"),
        };
        let sub = label(&format!("{kind} · {}", t.artist), &["dim", "small"]);
        sub.set_max_width_chars(24);
        col.append(&sub);
        b.set_child(Some(&col));
        let (ui2, track) = (ui.clone(), t.clone());
        b.connect_clicked(move |_| {
            feed::opened(&track);
            if track.link.is_some() {
                open_link(&ui2, &track);
            } else {
                play_with_radio(&ui2, track.clone());
            }
        });
        if t.link.is_none() {
            song_menu_on(ui, &b, t.clone());
        }
        row.append(&b);
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&row));
    scroll.set_min_content_height(268);
    scroll.upcast()
}

/// Under a page that goes on: where its next parts go (`list`, or a new box), and a spinner.
fn more_tail(ui: &Shared, page: &gtk::Box, list: Option<&gtk::Box>) {
    let target = match list {
        Some(l) => l.clone(),
        None => {
            let b = gtk::Box::new(Orientation::Vertical, 14);
            page.append(&b);
            b
        }
    };
    let spin = gtk::Spinner::new();
    spin.set_visible(false);
    spin.set_size_request(28, 28);
    spin.set_margin_top(10);
    page.append(&spin);
    let mut s = ui.borrow_mut();
    s.more_box = Some(target.downgrade());
    s.more_spinner = Some(spin.downgrade());
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
        list.append(&song_row(ui, i, t, playing.as_deref(), wants));
    }
    page.append(&list);
    more_tail(ui, &page, Some(&list));
    page.upcast()
}

fn song_row(ui: &Shared, i: usize, t: &Track, playing: Option<&str>, wants: &mut Wants) -> gtk::Widget {
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
            } else {
                // The list as it is now (parts loaded since included).
                let songs: Vec<Track> = ui.borrow().tracks.iter().filter(|s| s.link.is_none()).cloned().collect();
                if let Some(at) = songs.iter().position(|s| s.video_id == t.video_id) {
                    play_list(&ui, &songs, at);
                }
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
    let recap = Button::with_label(tr("Your recap: listening stats"));
    recap.add_css_class("ghost");
    recap.set_halign(Align::Start);
    {
        let ui = ui.clone();
        recap.connect_clicked(move |_| go(&ui, Page::Recap));
    }
    // Songs on this computer (the Music folder and the folders from the settings): no account needed.
    let local = Button::with_label(tr("On this device"));
    local.add_css_class("ghost");
    {
        let ui = ui.clone();
        local.connect_clicked(move |_| {
            let folders = rymplayer::local::folders(&ui.borrow().config.local_folders);
            open_songs(&ui, tr("On this device").into(), async move {
                let songs = tokio::task::spawn_blocking(move || rymplayer::local::scan(&folders)).await?;
                if songs.is_empty() {
                    anyhow::bail!("{}", tr("No songs found in your Music folder"));
                }
                Ok(songs)
            });
        });
    }
    let buttons = gtk::Box::new(Orientation::Horizontal, 8);
    buttons.append(&local);
    buttons.append(&recap);
    page.append(&buttons);
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

/// The reset the user scheduled (a day, or every week), when it's due; looked at once a minute.
fn scheduled_reset(ui: &Shared) {
    let due = {
        let mut s = ui.borrow_mut();
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
        let mut s = ui.borrow_mut();
        s.config.ram_last_reset = rymplayer::updates::now();
        if due == rymplayer::ram_points::Due::Date {
            s.config.ram_reset_date.clear();
        }
        s.config.save();
    }
    toast(ui, if due == rymplayer::ram_points::Due::Date { tr("Points reset: the day you picked came") } else { tr("Points reset: a week passed") });
}

/// The label of lyric line `i` (a line is its label, or a box with the label and its translation).
fn lyric_label(lyrics_box: &gtk::Box, i: usize) -> Option<Label> {
    let mut child = lyrics_box.first_child();
    for _ in 0..i {
        child = child?.next_sibling();
    }
    let c = child?;
    c.clone().downcast::<Label>().ok().or_else(|| c.first_child()?.downcast::<Label>().ok())
}

/// The RaM points line in the settings: songs with points, and the top artists.
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
    page.append(&label(&format!("RyM {}", rymplayer::VERSION), &["dim", "small"]));

    let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
    let theme = gtk::DropDown::from_strings(&names);
    theme.set_selected((config.gui_theme % THEMES.len()) as u32);
    {
        let ui = ui.clone();
        theme.connect_selected_notify(move |d| {
            let (relayout, window) = {
                let mut s = ui.borrow_mut();
                let was = ytm_layout(s.config.gui_theme);
                s.config.gui_theme = d.selected() as usize;
                s.config.save();
                apply_theme(&s.w.provider, s.config.gui_theme, current_colors(&s.config).as_ref());
                (was != ytm_layout(s.config.gui_theme), s.w.window.clone())
            };
            // The RyM YTM layout is built when the window opens: RyM opens again, going on
            // where it was.
            if relayout {
                save_session(&ui);
                if let Ok(exe) = std::env::current_exe()
                    && std::process::Command::new(exe).spawn().is_ok()
                {
                    window.close();
                }
            }
        });
    }
    page.append(&setting_row(tr("Theme"), "", &theme));

    // Colors over the theme: the layout's own, the built-in themes, or an imported one.
    let mut names: Vec<String> = vec![tr("The layout's own").to_string()];
    names.extend(rymplayer::themes::presets().into_iter().map(|t| t.name));
    names.push("Custom".into());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let colors = gtk::DropDown::from_strings(&refs);
    let at = if config.palette.is_empty() { 0 } else { names.iter().position(|n| *n == config.palette).unwrap_or(0) };
    colors.set_selected(at as u32);
    {
        let ui = ui.clone();
        let names = names.clone();
        colors.connect_selected_notify(move |d| {
            let i = d.selected() as usize;
            let mut s = ui.borrow_mut();
            s.config.palette = if i == 0 { String::new() } else { names.get(i).cloned().unwrap_or_default() };
            s.config.save();
            apply_theme(&s.w.provider, s.config.gui_theme, current_colors(&s.config).as_ref());
        });
    }
    page.append(&setting_row(tr("Colors"), "", &colors));
    // Animations' frames a second.
    const FPS: [u32; 7] = [24, 30, 60, 120, 144, 240, 0];
    let fps_names: Vec<String> = FPS.iter().map(|f| if *f == 0 { tr("As fast as the screen").to_string() } else { format!("{f} FPS") }).collect();
    let fps_refs: Vec<&str> = fps_names.iter().map(String::as_str).collect();
    let fps = gtk::DropDown::from_strings(&fps_refs);
    fps.set_selected(FPS.iter().position(|f| *f == config.fps_cap).unwrap_or(2) as u32);
    {
        let ui = ui.clone();
        fps.connect_selected_notify(move |d| {
            let mut s = ui.borrow_mut();
            s.config.fps_cap = FPS[(d.selected() as usize).min(FPS.len() - 1)];
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Animation frame rate"), "", &fps));
    let lossless = gtk::Switch::new();
    lossless.set_active(config.prefer_lossless);
    lossless.set_valign(Align::Center);
    {
        let ui = ui.clone();
        lossless.connect_active_notify(move |w| {
            let mut s = ui.borrow_mut();
            s.config.prefer_lossless = w.is_active();
            stream::prefer_lossless(w.is_active());
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Prefer lossless (FLAC)"), tr("Looks for the song as FLAC on the Internet Archive first (free and legal); YouTube when it isn't there"), &lossless));
    for (title, about, on, set) in [
        (tr("Liquid Aurora"), tr("The cover's colors move behind everything with the bass"), config.aurora, (|c: &mut Config, on| c.aurora = on) as fn(&mut Config, bool)),
        (tr("Vinyl record"), tr("The cover turns like a record: faster with Nightcore, slower with Slowed"), config.vinyl, |c: &mut Config, on| c.vinyl = on),
    ] {
        let sw = gtk::Switch::new();
        sw.set_active(on);
        sw.set_valign(Align::Center);
        let ui = ui.clone();
        sw.connect_active_notify(move |w| {
            let mut s = ui.borrow_mut();
            set(&mut s.config, w.is_active());
            s.config.save();
        });
        page.append(&setting_row(title, about, &sw));
    }
    let path = gtk::Entry::new();
    path.set_placeholder_text(Some(tr("A .json theme file (its path), or the JSON itself")));
    path.set_width_chars(28);
    let import = Button::with_label(tr("Import"));
    import.add_css_class("ghost");
    {
        let ui = ui.clone();
        let path = path.clone();
        let colors = colors.clone();
        let last = names.len() - 1;
        import.connect_clicked(move |_| {
            let text = path.text().trim().to_string();
            let json = if text.starts_with('{') { Ok(text.clone()) } else { std::fs::read_to_string(text.trim_matches('"')).map_err(|e| e.to_string()) };
            match json.and_then(|j| rymplayer::themes::Theme::from_json(&j)) {
                Ok(t) => {
                    {
                        let mut s = ui.borrow_mut();
                        s.config.custom_theme = Some(t);
                        s.config.palette = "Custom".into();
                        s.extras_key = None;
                        s.config.save();
                        apply_theme(&s.w.provider, s.config.gui_theme, current_colors(&s.config).as_ref());
                    }
                    colors.set_selected(last as u32);
                }
                Err(e) => toast(&ui, &format!("{}: {e}", tr("Import colors"))),
            }
        });
    }
    let export = Button::with_label(tr("Export"));
    export.add_css_class("ghost");
    {
        let ui = ui.clone();
        export.connect_clicked(move |_| {
            let theme = current_colors(&ui.borrow().config).unwrap_or_else(|| rymplayer::themes::Theme { name: "RyM".into(), ..Default::default() });
            let dir = directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.to_path_buf())).unwrap_or_else(rymplayer::data_dir);
            let file = dir.join(format!("RyM theme - {}.json", theme.name.replace(['/', '\\', ':'], "-")));
            match std::fs::write(&file, theme.to_json()) {
                Ok(()) => toast(&ui, &format!("{} {}", tr("Saved to"), file.display())),
                Err(e) => toast(&ui, &e.to_string()),
            }
        });
    }
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.append(&path);
    row.append(&import);
    row.append(&export);
    page.append(&setting_row(tr("Import colors"), tr("Saves these colors as a .json file in Downloads"), &row));
    page.append(&setting_row(
        tr("Suggestions"),
        tr("For you and Up next by points: new songs, the same artist and kind (Husseini brings only Husseini, songs only songs)"),
        &label(rymplayer::ram_points::NAME, &["title"]),
    ));

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
    network_settings(ui, &page, &config);
    sound_settings(ui, &page, &config);

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

    page.append(&label(tr("Updates"), &["shelf"]));
    let auto = gtk::Switch::new();
    auto.set_active(config.auto_update_check);
    {
        let ui = ui.clone();
        auto.connect_active_notify(move |w| {
            let mut s = ui.borrow_mut();
            s.config.auto_update_check = w.is_active();
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Check for updates automatically"), tr("Every 12 hours, and when the app opens"), &auto));
    let beta = gtk::Switch::new();
    beta.set_active(config.beta_updates);
    {
        let ui = ui.clone();
        beta.connect_active_notify(move |w| {
            let mut s = ui.borrow_mut();
            s.config.beta_updates = w.is_active();
            s.config.save();
        });
    }
    page.append(&setting_row(tr("Beta updates"), tr("Test versions (pre-releases) count as updates too"), &beta));
    let check = Button::with_label(tr("Check for updates"));
    check.add_css_class("ghost");
    {
        let ui = ui.clone();
        check.connect_clicked(move |_| check_updates(&ui, true));
    }
    page.append(&setting_row(&format!("{} {}", tr("You have"), rymplayer::VERSION), "", &check));

    page.append(&label(tr("Plugins"), &["shelf"]));
    let store = Button::with_label(tr("Plugin store"));
    store.add_css_class("pill");
    {
        let ui = ui.clone();
        store.connect_clicked(move |_| go(&ui, Page::Plugins));
    }
    page.append(&setting_row(tr("Plugins"), tr("Add features to RyM; without them it stays a music player"), &store));

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

thread_local! {
    /// The header's "Download from a link" button: shown while the video plugin is installed.
    static FROM_LINK: RefCell<Option<glib::WeakRef<Button>>> = const { RefCell::new(None) };
    /// Where the search looks (YouTube Music or SoundCloud): shown with the SoundCloud plugin.
    static SEARCH_IN: RefCell<Option<glib::WeakRef<gtk::DropDown>>> = const { RefCell::new(None) };
}

fn has_video(ui: &Shared) -> bool {
    has_plugin(ui, rymplayer::plugins::VIDEO)
}

fn has_plugin(ui: &Shared, id: &str) -> bool {
    ui.borrow().config.plugins.contains_key(id)
}

/// The search box looks on SoundCloud (its plugin installed and picked next to the box).
fn searching_soundcloud(ui: &Shared) -> bool {
    has_plugin(ui, rymplayer::plugins::SOUNDCLOUD) && SEARCH_IN.with(|f| f.borrow().as_ref().and_then(glib::WeakRef::upgrade).is_some_and(|d| d.selected() == 1))
}

// ---------------------------------------------------------------- updates

fn check_updates(ui: &Shared, asked: bool) {
    let beta = {
        let mut s = ui.borrow_mut();
        s.config.last_update_check = rymplayer::updates::now();
        s.config.save();
        s.config.beta_updates
    };
    if asked {
        toast(ui, tr("Checking for updates..."));
    }
    post(ui, async move { Ev::Update(rymplayer::updates::check(beta).await, asked) });
}

fn auto_update_check(ui: &Shared) {
    let due = {
        let s = ui.borrow();
        rymplayer::updates::due(s.config.auto_update_check, s.config.last_update_check)
    };
    if due {
        check_updates(ui, false);
    }
}

/// The new version, its patch notes and a download button.
fn show_update(ui: &Shared, r: &rymplayer::updates::Release) {
    let parent = ui.borrow().w.window.clone();
    let win = gtk::Window::builder().transient_for(&parent).modal(true).default_width(560).default_height(560).title(tr("New update")).build();
    win.add_css_class("rym");
    let col = gtk::Box::new(Orientation::Vertical, 12);
    col.set_margin_top(20);
    col.set_margin_bottom(20);
    col.set_margin_start(22);
    col.set_margin_end(22);
    let kind = if r.beta { tr("New beta") } else { tr("New update") };
    col.append(&label(&format!("{kind}: RyM {}", r.version), &["heading"]));
    col.append(&label(&format!("{} {}", tr("You have"), rymplayer::VERSION), &["dim"]));
    col.append(&label(tr("What's new"), &["shelf"]));
    let notes = gtk::Label::new(Some(&r.plain_notes().join("\n")));
    notes.set_wrap(true);
    notes.set_xalign(0.0);
    notes.set_yalign(0.0);
    notes.set_selectable(true);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&notes));
    scroll.set_vexpand(true);
    col.append(&scroll);
    let buttons = gtk::Box::new(Orientation::Horizontal, 10);
    buttons.set_halign(Align::End);
    let later = Button::with_label(tr("Later"));
    later.add_css_class("ghost");
    // Windows: the update downloads and goes in place here, then RyM opens again on it.
    let get = Button::with_label(if cfg!(windows) { tr("Update now") } else { tr("Download") });
    get.add_css_class("pill");
    let file = if cfg!(windows) { "RyM-GTK-Windows.zip" } else { "RyM-linux-x86_64.tar.gz" };
    let url = r.download_for(file);
    let how = label("", &["dim"]);
    col.append(&how);
    {
        let win = win.clone();
        later.connect_clicked(move |_| win.close());
    }
    {
        let (win, ui, later) = (win.clone(), ui.clone(), later.clone());
        get.connect_clicked(move |b| {
            if !cfg!(windows) || !url.ends_with(".zip") {
                let _ = rymplayer::updates::open_link(&url);
                win.close();
                return;
            }
            b.set_sensitive(false);
            later.set_sensitive(false);
            let done = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            {
                // The download's progress on the dialog, until it's done.
                let (done, how) = (done.clone(), how.downgrade());
                glib::timeout_add_local(Duration::from_millis(200), move || {
                    let Some(how) = how.upgrade() else { return glib::ControlFlow::Break };
                    let p = f32::from_bits(done.load(std::sync::atomic::Ordering::Relaxed));
                    how.set_text(&format!("{} {:.0}%", tr("Downloading the update..."), p * 100.0));
                    if p >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
                });
            }
            let url = url.clone();
            post(&ui, async move { Ev::Installed(rymplayer::updates::install(&url, move |p| done.store(p.to_bits(), std::sync::atomic::Ordering::Relaxed)).await) });
        });
    }
    buttons.append(&later);
    buttons.append(&get);
    col.append(&buttons);
    win.set_child(Some(&col));
    win.present();
}

// ---------------------------------------------------------------- plugins

fn plugins_page(ui: &Shared) -> gtk::Widget {
    let page = page_box();
    page.append(&label(tr("Plugin store"), &["heading"]));
    page.append(&label(tr("Add features to RyM; without them it stays a music player"), &["dim"]));
    let store = ui.borrow().store.clone();
    let Some(list) = store else {
        post(ui, async { Ev::Store(rymplayer::plugins::store().await) });
        page.append(&note(tr("Loading the store...")));
        return page.upcast();
    };
    for p in list {
        let installed = ui.borrow().config.plugins.get(&p.id).cloned();
        let card = gtk::Box::new(Orientation::Horizontal, 16);
        card.add_css_class("store-card");
        let text = gtk::Box::new(Orientation::Vertical, 4);
        text.set_hexpand(true);
        text.append(&label(p.name(), &["title"]));
        let d = label(p.description(), &["dim", "small"]);
        d.set_wrap(true);
        d.set_ellipsize(gtk::pango::EllipsizeMode::None);
        text.append(&d);
        let status = match &installed {
            Some(v) => format!("{} {v}", tr("Installed")),
            None => format!("{} {}", tr("Version"), p.version),
        };
        text.append(&label(&status, &["dim", "small"]));
        card.append(&text);
        let actions = gtk::Box::new(Orientation::Vertical, 6);
        actions.set_valign(Align::Center);
        let newer = installed.as_deref().is_some_and(|v| rymplayer::updates::newer(&p.version, v));
        let main = Button::with_label(match (&installed, newer) {
            (None, _) => tr("Install"),
            (Some(_), true) => tr("Update"),
            (Some(_), false) => tr("Remove"),
        });
        main.add_css_class(if installed.is_some() && !newer { "ghost" } else { "pill" });
        {
            let ui = ui.clone();
            let p = p.clone();
            let remove = installed.is_some() && !newer;
            main.connect_clicked(move |_| set_plugin(&ui, &p, !remove));
        }
        actions.append(&main);
        if installed.is_some() && newer {
            let remove = Button::with_label(tr("Remove"));
            remove.add_css_class("ghost");
            let ui = ui.clone();
            let p = p.clone();
            remove.connect_clicked(move |_| set_plugin(&ui, &p, false));
            actions.append(&remove);
        }
        card.append(&actions);
        page.append(&card);
    }
    page.upcast()
}

/// Installs (or updates) a plugin, or removes it; its features appear or go at once.
fn set_plugin(ui: &Shared, p: &rymplayer::plugins::Plugin, on: bool) {
    {
        let mut s = ui.borrow_mut();
        if on {
            s.config.plugins.insert(p.id.clone(), p.version.clone());
        } else {
            s.config.plugins.remove(&p.id);
        }
        s.config.save();
    }
    let video = has_video(ui);
    FROM_LINK.with(|f| {
        if let Some(b) = f.borrow().as_ref().and_then(glib::WeakRef::upgrade) {
            b.set_visible(video);
        }
    });
    let soundcloud = has_plugin(ui, rymplayer::plugins::SOUNDCLOUD);
    SEARCH_IN.with(|f| {
        if let Some(d) = f.borrow().as_ref().and_then(glib::WeakRef::upgrade) {
            d.set_visible(soundcloud);
            if !soundcloud {
                d.set_selected(0);
            }
        }
    });
    toast(ui, tr(if on { "Plugin installed" } else { "Plugin removed" }));
    render(ui);
}

/// Video plugin: the song's video in VLC (picture and sound streamed together).
fn watch_video(ui: &Shared, t: &Track) {
    toast(ui, tr("Opening the video..."));
    let t = t.clone();
    post(ui, async move {
        let links = rymplayer::video::links(&t.video_id).await;
        Ev::VideoLinks(t, links)
    });
}

fn download_link(ui: &Shared, url: String, video: bool) {
    toast(ui, &format!("{} {url}", tr("Downloading")));
    post(ui, async move { Ev::Downloaded(rymplayer::video::download(&url, video).await) });
}

/// Video plugin: download from YouTube or any site yt-dlp knows, as a video or just the sound.
fn link_dialog(ui: &Shared) {
    let parent = ui.borrow().w.window.clone();
    let win = gtk::Window::builder().transient_for(&parent).modal(true).default_width(520).title(tr("Download from a link")).build();
    win.add_css_class("rym");
    let col = gtk::Box::new(Orientation::Vertical, 12);
    col.set_margin_top(20);
    col.set_margin_bottom(20);
    col.set_margin_start(22);
    col.set_margin_end(22);
    col.append(&label(tr("Download from a link"), &["heading"]));
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some(tr("Paste a link from YouTube or any site")));
    entry.add_css_class("search");
    col.append(&entry);
    let kind = gtk::DropDown::from_strings(&[tr("Video"), tr("Audio only")]);
    col.append(&kind);
    let get = Button::with_label(tr("Download"));
    get.add_css_class("pill");
    get.set_halign(Align::End);
    let go = {
        let (ui, win, entry, kind) = (ui.clone(), win.clone(), entry.clone(), kind.clone());
        move || {
            let url = entry.text().trim().to_string();
            if !url.is_empty() {
                download_link(&ui, url, kind.selected() == 0);
                win.close();
            }
        }
    };
    {
        let go = go.clone();
        get.connect_clicked(move |_| go());
    }
    entry.connect_activate(move |_| go());
    col.append(&get);
    win.set_child(Some(&col));
    win.present();
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

/// What a song menu row does when picked.
type MenuAction = Box<dyn Fn(&Shared, &Track)>;

fn song_menu(ui: &Shared, anchor: &gtk::Widget, x: f64, y: f64, t: &Track) {
    let pop = gtk::Popover::new();
    pop.set_parent(anchor);
    pop.set_has_arrow(false);
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let col = gtk::Box::new(Orientation::Vertical, 0);
    let liked = ui.borrow().liked.contains(&t.video_id);
    let mut items: Vec<(&str, MenuAction)> = vec![
        (tr("Play next"), Box::new(|ui: &Shared, t: &Track| play_next(ui, t.clone()))),
        (tr("Add to queue"), Box::new(|ui: &Shared, t: &Track| add_to_queue(ui, t.clone()))),
        (tr("Start radio"), Box::new(|ui: &Shared, t: &Track| play_with_radio(ui, t.clone()))),
        (tr("Download"), Box::new(|ui: &Shared, t: &Track| download(ui, t.clone()))),
    ];
    if rymplayer::is_youtube(&t.video_id) {
        items.insert(3, (if liked { tr("Remove from liked songs") } else { tr("Add to liked songs") }, Box::new(|ui: &Shared, t: &Track| toggle_like(ui, t))));
    }
    if !rymplayer::local::is_local(&t.video_id) {
        items.push((tr("Share"), Box::new(|ui: &Shared, t: &Track| share(ui, t))));
    }
    if !t.artist.is_empty() && rymplayer::is_youtube(&t.video_id) {
        items.push((tr("Go to artist"), Box::new(|ui: &Shared, t: &Track| go_artist(ui, t))));
    }
    if has_video(ui) && rymplayer::is_youtube(&t.video_id) {
        items.push((tr("Watch video"), Box::new(|ui: &Shared, t: &Track| watch_video(ui, t))));
        items.push((tr("Download video"), Box::new(|ui: &Shared, t: &Track| download_link(ui, rymplayer::video::watch_url(&t.video_id), true))));
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
    if t.link.is_none() && !t.artist.is_empty() && rymplayer::is_youtube(&t.video_id) {
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
    let on = s.q.current.and_then(|i| s.q.songs.get(i)).is_some_and(|t| s.liked.contains(&t.video_id));
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
    if !rymplayer::is_youtube(&t.video_id) {
        toast(ui, tr("Likes are for YouTube Music songs"));
        return;
    }
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

/// Plays `songs` from `i`. What was played stays above them, so Previous goes back to it.
fn play_list(ui: &Shared, songs: &[Track], i: usize) {
    if songs.is_empty() {
        return;
    }
    song_page_on_play(ui);
    ui.borrow_mut().user_pick = true;
    let at = ui.borrow_mut().q.start(songs.to_vec(), i);
    play_at(ui, at);
}

/// One song, then YouTube Music's radio after it (Up next made for it); what was played stays above.
fn play_with_radio(ui: &Shared, t: Track) {
    song_page_on_play(ui);
    ui.borrow_mut().user_pick = true;
    let at = ui.borrow_mut().q.start(vec![t], 0);
    play_at(ui, at);
    continue_with_radio(ui);
}

fn play_next(ui: &Shared, t: Track) {
    let start = {
        let mut s = ui.borrow_mut();
        let idle = s.q.current.is_none();
        s.q.play_next(t);
        idle.then_some(s.q.current).flatten()
    };
    match start {
        Some(i) => play_at(ui, i),
        None => {
            toast(ui, tr("Plays next"));
            render_queue(ui);
        }
    }
}

/// Right after the playing song (after songs added before it), not at the end of the queue.
fn add_to_queue(ui: &Shared, t: Track) {
    let start = {
        let mut s = ui.borrow_mut();
        s.q.add(t).then_some(s.q.current).flatten()
    };
    match start {
        Some(i) => play_at(ui, i),
        None => {
            toast(ui, tr("Added to the queue"));
            render_queue(ui);
        }
    }
}

fn move_in_queue(ui: &Shared, from: usize, to: usize) {
    ui.borrow_mut().q.move_song(from, to);
    render_queue(ui);
}

/// Copies the song's link (with its name) and says so.
fn share(ui: &Shared, t: &Track) {
    let text = format!("{} - {}\n{}", t.title, t.artist, rymplayer::queue::share_link(t));
    ui.borrow().w.window.clipboard().set_text(&text);
    toast(ui, tr("Link copied"));
}

/// The queue in the side panel: played songs dimmed, the playing one marked. A click plays a
/// song; holding a row for 2 s lifts it, and it can be dragged to another place.
/// Songs Up next lists after the playing one (a long queue costs hundreds of widgets otherwise).
const QUEUE_SHOWN: usize = 40;

fn render_queue(ui: &Shared) {
    // Not on screen (the song's page shut, or the lyrics tab showing): built when it shows.
    {
        let mut s = ui.borrow_mut();
        let visible = side_shown(&s.w) && s.w.side.visible_child_name().as_deref() == Some("queue");
        if !visible {
            s.queue_dirty = true;
            return;
        }
        s.queue_dirty = false;
    }
    let mut wants = Wants::new();
    let (songs, current, qb) = {
        let s = ui.borrow();
        (s.q.songs.clone(), s.q.current, s.w.queue_box.clone())
    };
    clear(&qb);
    let first = current.unwrap_or(0);
    let last = (first + QUEUE_SHOWN).min(songs.len().saturating_sub(1));
    if songs.is_empty() {
        qb.append(&note(tr("Nothing playing")));
        return;
    }
    for (i, t) in songs.iter().enumerate() {
        // Up next starts at the playing song (the ones before it aren't listed), 40 after it.
        if current.is_some_and(|c| i < c) {
            continue;
        }
        if i > last {
            qb.append(&note(&format!("+{} {}", songs.len() - i, tr("more songs"))));
            break;
        }
        let row = gtk::Box::new(Orientation::Horizontal, 10);
        row.add_css_class("qrow");
        if Some(i) == current {
            row.add_css_class("now");
        } else if current.is_some_and(|c| i < c) {
            row.add_css_class("past");
        }
        let art = cover(&ui.borrow(), t.thumb.as_ref(), ROW_PX, 36, "cover", &mut wants);
        if Some(i) == current {
            // The playing one: a speaker over its cover.
            let marked = gtk::Overlay::new();
            marked.set_child(Some(&art));
            let speaker = gtk::Image::from_icon_name("audio-volume-high-symbolic");
            speaker.add_css_class("now-icon");
            marked.add_overlay(&speaker);
            row.append(&marked);
        } else {
            row.append(&art);
        }
        let names = gtk::Box::new(Orientation::Vertical, 1);
        names.set_valign(Align::Center);
        names.set_hexpand(true);
        names.append(&label(&t.title, &["title"]));
        names.append(&label(&t.artist, &["dim", "small"]));
        row.append(&names);
        if !t.duration.is_empty() {
            let d = label(&t.duration, &["dim", "small"]);
            d.set_valign(Align::Center);
            row.append(&d);
        }
        if Some(i) != current {
            let x = icon_button("window-close-symbolic", tr("Remove from queue"), &["dim"]);
            x.set_valign(Align::Center);
            let ui = ui.clone();
            x.connect_clicked(move |_| {
                ui.borrow_mut().q.remove(i);
                render_queue(&ui);
            });
            row.append(&x);
        }
        // A click plays it (not when the press was a 2 s hold).
        let click = gtk::GestureClick::new();
        {
            let ui = ui.clone();
            click.connect_pressed(move |_, _, _, _| ui.borrow_mut().held = false);
        }
        {
            let ui = ui.clone();
            click.connect_released(move |_, _, _, _| {
                if !std::mem::take(&mut ui.borrow_mut().held) {
                    play_at(&ui, i);
                }
            });
        }
        row.add_controller(click);
        // Holding 2 s lifts the row ("pops out"); letting go puts it down ("pops in").
        // GTK allows at most twice the system's long press (about 1 s): the second one is waited
        // for here, while the row is still held.
        let hold = gtk::GestureLongPress::new();
        hold.set_delay_factor(2.0);
        {
            let (ui, row) = (ui.clone(), row.clone());
            hold.connect_pressed(move |g, _, _| {
                let (ui, row, g) = (ui.clone(), row.clone(), g.downgrade());
                glib::timeout_add_local_once(Duration::from_millis(1000), move || {
                    if !g.upgrade().is_some_and(|g| g.is_active()) {
                        return;
                    }
                    let mut s = ui.borrow_mut();
                    s.armed = Some(i);
                    s.held = true;
                    row.add_css_class("lifted");
                });
            });
        }
        {
            let row = row.clone();
            hold.connect_end(move |_, _| row.remove_css_class("lifted"));
        }
        row.add_controller(hold);
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        {
            let ui = ui.clone();
            drag.connect_prepare(move |_, _, _| {
                (ui.borrow().armed == Some(i)).then(|| gdk::ContentProvider::for_value(&(i as u32).to_value()))
            });
        }
        {
            let (ui, row) = (ui.clone(), row.clone());
            drag.connect_drag_begin(move |src, _| {
                src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 20, 20);
                let _ = &ui;
            });
        }
        {
            let (ui, row) = (ui.clone(), row.clone());
            drag.connect_drag_end(move |_, _, _| {
                ui.borrow_mut().armed = None;
                row.remove_css_class("lifted");
            });
        }
        row.add_controller(drag);
        let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
        {
            let row = row.clone();
            drop.connect_enter(move |_, _, _| {
                row.add_css_class("drop-here");
                gdk::DragAction::MOVE
            });
        }
        {
            let row = row.clone();
            drop.connect_leave(move |_| row.remove_css_class("drop-here"));
        }
        {
            let (ui, row) = (ui.clone(), row.clone());
            drop.connect_drop(move |_, value, _, _| {
                row.remove_css_class("drop-here");
                if let Ok(from) = value.get::<u32>() {
                    let ui = ui.clone();
                    // After the drag has finished with the old rows.
                    glib::idle_add_local_once(move || move_in_queue(&ui, from as usize, i));
                }
                true
            });
        }
        row.add_controller(drop);
        qb.append(&row);
    }
    want_covers(ui, wants);
}

/// The source pills: one per provider, the one shown lit, the ones that had nothing dimmed once
/// every source was asked; LRCLIB, when it has several versions, says which one shows.
fn fill_pills(ui: &Shared) {
    use rymplayer::lyric_sources::{LRCLIB, PROVIDERS, provider};
    let now = now_playing(ui);
    let (pills, list, shown, looking) = {
        let s = ui.borrow();
        let list = s.alt_lyrics.as_ref().filter(|(id, _)| now.as_ref().is_some_and(|t| t.video_id == *id)).map(|(_, l)| l.clone());
        (s.w.pills.clone(), list, s.lyrics.clone(), s.sources_for.is_some())
    };
    clear(&pills);
    let active = shown.as_ref().map(provider);
    for p in PROVIDERS {
        let versions: Vec<&Lyrics> = list.iter().flatten().filter(|l| provider(l) == p).collect();
        let mut text = p.to_string();
        if p == LRCLIB && versions.len() > 1 && let Some(i) = shown.as_ref().and_then(|sh| versions.iter().position(|v| rymplayer::lyrics::same(v, sh))) {
            text = format!("{p} {}/{}", i + 1, versions.len());
        }
        let b = Button::with_label(&text);
        b.add_css_class("src");
        if active == Some(p) {
            b.add_css_class("on");
        }
        if let Some(first) = versions.first() {
            b.set_tooltip_text(Some(if first.synced.is_empty() { tr("not synced") } else { tr("synced") }));
        }
        // Every source asked and this one had nothing.
        b.set_sensitive(now.is_some() && (list.is_none() || !versions.is_empty()));
        let ui = ui.clone();
        b.connect_clicked(move |_| pick_source(&ui, p));
        pills.append(&b);
    }
    if looking {
        let spin = gtk::Spinner::new();
        spin.start();
        pills.append(&spin);
    }
}

/// A source pill: that provider's lyrics (its next version when it already shows); the sources
/// are looked up first when they aren't in yet.
/// Every source's lyrics for the playing song, asked as soon as its own lyrics are in, so the
/// source pills switch at once.
fn prefetch_sources(ui: &Shared) {
    let Some(t) = now_playing(ui) else { return };
    {
        let mut s = ui.borrow_mut();
        let have = s.alt_lyrics.as_ref().is_some_and(|(id, _)| *id == t.video_id);
        if have || s.sources_for.as_deref() == Some(t.video_id.as_str()) {
            return;
        }
        s.sources_for = Some(t.video_id.clone());
    }
    let (api, shown) = (api_of(ui), ui.borrow().lyrics.clone());
    post(ui, async move {
        let list = rymplayer::lyrics::sources(api, t.clone(), shown).await;
        Ev::LyricsSources(t.video_id, list)
    });
    fill_pills(ui);
}

fn pick_source(ui: &Shared, p: &'static str) {
    use rymplayer::lyric_sources::provider;
    let Some(t) = now_playing(ui) else { return };
    let list = ui.borrow().alt_lyrics.as_ref().filter(|(id, _)| *id == t.video_id).map(|(_, l)| l.clone());
    let Some(list) = list else {
        let ask = {
            let mut s = ui.borrow_mut();
            s.wanted_source = Some(p);
            let ask = s.sources_for.as_deref() != Some(t.video_id.as_str());
            s.sources_for = Some(t.video_id.clone());
            ask
        };
        if ask {
            let (api, shown) = (api_of(ui), ui.borrow().lyrics.clone());
            post(ui, async move {
                let list = rymplayer::lyrics::sources(api, t.clone(), shown).await;
                Ev::LyricsSources(t.video_id, list)
            });
        }
        fill_pills(ui);
        return;
    };
    let versions: Vec<Lyrics> = list.into_iter().filter(|l| provider(l) == p).collect();
    if versions.is_empty() {
        return;
    }
    let shown = ui.borrow().lyrics.clone();
    let at = shown.as_ref().and_then(|sh| versions.iter().position(|v| rymplayer::lyrics::same(v, sh)));
    let pick = versions[at.map_or(0, |i| (i + 1) % versions.len())].clone();
    let side = ui.borrow().w.side.clone();
    side.set_visible_child_name("lyrics");
    rymplayer::lyrics::choose(&t.video_id, &pick);
    show_lyrics(ui, Some(pick));
}

fn download(ui: &Shared, t: Track) {
    rymplayer::ram_points::loved(t.clone(), rymplayer::ram_points::Love::Download);
    toast(ui, &format!("Downloading \"{}\"…", t.title));
    post(ui, async move { Ev::Downloaded(stream::save_to_music(&t.video_id, &t.artist, &t.title).await) });
}

fn play_at(ui: &Shared, i: usize) {
    let mut wants = Wants::new();
    let (track, id, api, in_vlc) = {
        let mut s = ui.borrow_mut();
        let Some(track) = s.q.songs.get(i).cloned() else { return };
        // RaM points: the song that was playing gets its listening time, then this one its play.
        let left = s.q.now().cloned().map(|p| {
            let a = s.audio.as_ref().filter(|a| a.is_loaded());
            (p, a.map_or(0.0, Audio::position), a.and_then(Audio::duration).unwrap_or(0.0))
        });
        let manual = std::mem::take(&mut s.user_pick);
        rymplayer::ram_points::moved_on(left, track.clone(), manual);
        if manual {
            s.peers.played_here();
        }
        let sponsor = s.config.sponsorblock;
        if let Some(ask) = s.non_music.song(&track.video_id, sponsor) {
            spawn(&s, async move {
                let (v, parts) = ask.await;
                Ev::Segments(v, parts)
            });
        }
        s.request_id += 1;
        s.q.went_to(i);
        // The panel opens on the lyrics when a song starts (unless closed by hand).
        if !s.side_closed && !side_shown(&s.w) {
            s.w.side.set_visible_child_name("lyrics");
            show_side(&s.w, true);
        }
        s.played.insert(track.video_id.clone());
        rymplayer::recent::played(&track);
        s.wanted_source = None;
        // Another song than the one left off last time: that one's second no longer counts.
        if s.resume_at.as_ref().is_some_and(|(id, _)| *id != track.video_id) {
            s.resume_at = None;
        }
        s.loading = true;
        s.lyrics = None;
        s.lyric_line = None;
        // A crossfade keeps the old song playing while the new one loads.
        if !s.crossfade_next
            && let Some(a) = &mut s.audio
        {
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
        show_cover(&s, &track, &mut wants);
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
        s.alt_lyrics = None;
        let in_vlc = s.config.play_in_vlc && vlc::find().is_some();
        if !in_vlc {
            vlc::stop(&mut s.vlc);
        }
        (track, s.request_id, s.api.clone(), in_vlc)
    };
    want_covers(ui, wants);
    refresh_hearts(ui);
    render_queue(ui);
    save_session(ui);
    let due = {
        let mut s = ui.borrow_mut();
        s.songs_since_cleanup += 1;
        s.songs_since_cleanup >= 5
    };
    if due {
        let ui = ui.clone();
        glib::idle_add_local_once(move || clean_up_memory(&ui, "5 songs"));
    }

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
        // FLAC first when "Prefer lossless" is on and the Internet Archive has the song.
        let song = track.clone();
        post(ui, async move { Ev::Fetched(id, stream::open_track(song).await) });
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
        (s.q.current.and_then(|i| s.q.songs.get(i + 1)).cloned(), s.api.clone())
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

/// The playing song's id, from the state already borrowed.
fn now_playing_of(s: &Ui) -> String {
    s.q.now().map(|t| t.video_id.clone()).unwrap_or_default()
}

/// The alarm rings: what was playing goes on, else the liked songs (shuffled).
fn ring_alarm(ui: &Shared) {
    toast(ui, tr("Alarm"));
    let (loaded, playing, current) = {
        let s = ui.borrow();
        (s.audio.as_ref().is_some_and(Audio::is_loaded), s.audio.as_ref().is_some_and(Audio::is_playing), s.q.current)
    };
    if playing {
        return;
    }
    if loaded || current.is_some() {
        toggle_playback(ui);
        return;
    }
    let songs = core::shuffled(ui.borrow().liked_tracks.clone());
    if !songs.is_empty() {
        play_list(ui, &songs, 0);
    }
}

fn toggle_playback(ui: &Shared) {
    let (loaded, current) = {
        let s = ui.borrow();
        (s.audio.as_ref().is_some_and(Audio::is_loaded), s.q.current)
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
        (s.q.current, s.q.songs.len())
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
        (s.q.current, s.audio.as_ref().filter(|a| a.is_loaded()).map_or(0.0, Audio::position), again)
    };
    match current {
        Some(i) if (pos < 3.0 || again) && i > 0 => play_at(ui, i - 1),
        Some(_) => {
            if let Some(a) = &ui.borrow().audio {
                a.seek(0.0);
            }
            // Back to its start by hand: a sign it's loved (RaM points).
            let now = ui.borrow().q.now().cloned();
            if let Some(t) = now.filter(|_| pos >= 3.0) {
                rymplayer::ram_points::loved(t, rymplayer::ram_points::Love::BackToStart);
            }
        }
        None => {}
    }
}

/// The queue ran out (or a single song plays): YouTube Music's radio keeps it going.
fn continue_with_radio(ui: &Shared) {
    let (Some(api), Some(t)) = (api_of(ui), now_playing(ui)) else { return };
    if rymplayer::local::is_local(&t.video_id) {
        return;
    }
    {
        let mut s = ui.borrow_mut();
        if s.radio_pending {
            return;
        }
        s.radio_pending = true;
    }
    post(ui, async move { Ev::Radio(rymplayer::radio::with_artist(api, t).await) });
}

fn show_lyrics(ui: &Shared, lyrics: Option<Lyrics>) {
    let mut s = ui.borrow_mut();
    // Translations off: none shown, none looked for.
    let translate = s.config.lyrics_translation;
    let lyrics = lyrics.map(|mut l| {
        if !translate {
            l.translation.clear();
        }
        l
    });
    clear(&s.w.lyrics_box);
    s.lyric_line = None;
    let Some(lyrics) = lyrics else {
        s.w.lyrics_box.append(&label(tr("No lyrics for this song"), &["lyrics-note"]));
        s.lyrics = None;
        drop(s);
        fill_pills(ui);
        return;
    };
    let lines: Vec<String> = if lyrics.synced.is_empty() {
        lyrics.plain.clone()
    } else {
        lyrics.synced.iter().map(|(_, l)| if l.is_empty() { "♪".to_string() } else { l.clone() }).collect()
    };
    // Romaji (the setting): kana by the table at once, then the reading with kanji.
    let romaji_key = format!("{}|{}", s.q.now().map(|t| t.video_id.clone()).unwrap_or_default(), lyrics.source);
    let mut ask_romaji = None;
    let romaji: Vec<String> = if s.config.romaji && lines.iter().any(|l| rymplayer::romaji::has_kana(l)) {
        if s.romaji.0 == romaji_key {
            s.romaji.1.clone()
        } else {
            ask_romaji = Some(lines.clone());
            lines.iter().map(|l| if rymplayer::romaji::has_kana(l) { rymplayer::romaji::kana(l) } else { String::new() }).collect()
        }
    } else {
        Vec::new()
    };
    if lyrics.synced.is_empty() {
        let note = label(tr("These lyrics aren't synced to the song"), &["lyrics-note"]);
        note.set_margin_bottom(10);
        s.w.lyrics_box.append(&note);
    }
    for (i, line) in lines.into_iter().enumerate() {
        let text = Label::new(Some(&line));
        text.set_xalign(0.0);
        text.set_wrap(true);
        // A Japanese song's English, small under the line (one widget a line: the sung line is
        // found by its place).
        let ro = romaji.get(i).filter(|t| !t.is_empty());
        let en = lyrics.translation.get(i).filter(|t| !t.is_empty());
        let l: gtk::Widget = if ro.is_some() || en.is_some() {
            let b = gtk::Box::new(Orientation::Vertical, 2);
            b.append(&text);
            for small in [ro, en].into_iter().flatten() {
                let small = label(small, &["lyric-tr"]);
                small.set_wrap(true);
                small.set_ellipsize(gtk::pango::EllipsizeMode::None);
                b.append(&small);
            }
            b.upcast()
        } else {
            text.upcast()
        };
        if lyrics.synced.is_empty() {
            l.add_css_class("now");
        }
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
    // Japanese without English yet: it's looked for, and added when it comes.
    let wants = translate && lyrics.translation.is_empty() && rymplayer::lyrics::is_japanese(&lyrics);
    let source = lyrics.source.clone();
    s.lyrics = Some(lyrics.clone());
    if let Some(lines) = ask_romaji {
        s.romaji = (romaji_key.clone(), romaji.clone());
        let (vid, src) = (now_playing_of(&s), source.clone());
        spawn(&s, async move { Ev::Romaji(vid, src, rymplayer::romaji::romanize(&lines).await) });
    }
    drop(s);
    fill_pills(ui);
    if wants && let Some(t) = now_playing(ui) {
        post(ui, async move {
            let tr = rymplayer::lyrics::translate(&t, &lyrics).await;
            Ev::Translated(t.video_id, source, tr)
        });
    }
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
    scheduled_reset(ui);
    let mut s = ui.borrow_mut();
    let Some((pos, dur, loaded, playing, finished)) =
        s.audio.as_ref().map(|a| (a.position(), a.duration().unwrap_or(0.0), a.is_loaded(), a.is_playing(), a.finished()))
    else {
        return;
    };
    if let Some(a) = &mut s.audio {
        a.tick();
    }
    // Theme extras: the record turns, Liquid Aurora pulses with the bass, the needles follow.
    {
        let key = (s.config.vinyl, s.config.aurora, s.config.palette.clone());
        if s.extras_key.as_ref() != Some(&key) {
            s.extras_key = Some(key);
            apply_extras(&s);
        }
        let (bass, left, right) = if playing { s.audio.as_ref().map(Audio::bands).unwrap_or_default() } else { (0.0, 0.0, 0.0) };
        s.bass_peak = (s.bass_peak * 0.995).max(bass).max(0.01);
        let live = s.w.live.clone();
        live.playing.set(playing);
        live.spin_rate.set(200.0 * s.config.speed as f64);
        live.energy.set((bass / s.bass_peak).powf(1.5).clamp(0.0, 1.0) as f64);
        let level = |x: f32| ((20.0 * x.max(1e-5).log10() + 30.0) / 30.0).clamp(0.0, 1.0) as f64;
        live.vu.set((level(left), level(right)));
        // The animations move only with the window in front and shown, and no battery saver.
        if s.saving_at.is_none_or(|at| at.elapsed() > Duration::from_secs(30)) {
            s.saving_at = Some(Instant::now());
            s.saving = rymplayer::power::saving();
        }
        // Minimized or hidden stops it; another window having the focus doesn't.
        let shown = !s.w.window.is_suspended() && s.w.window.is_visible();
        fx_view::set_eco(shown && !s.saving, s.config.fps_cap);
    }
    // Listening stats: time while playing, written once a minute.
    let since = s.listened_at.elapsed();
    s.listened_at = Instant::now();
    if playing && let Some(t) = s.q.now() {
        rymplayer::stats::listened(t, since.as_millis().min(1000) as u64);
    }
    if s.stats_written.elapsed() > Duration::from_secs(60) {
        s.stats_written = Instant::now();
        rymplayer::stats::flush();
    }
    // Last.fm: sent once half the song (or 4 minutes) played.
    let now = s.q.now().cloned();
    let cfg = s.config.clone();
    if playing && let Some(send) = s.scrobbler.heard(now.as_ref(), since.as_secs_f32().min(1.0), dur, &cfg) {
        spawn(&s, async move {
            send.await;
            Ev::Done
        });
    }
    // The alarm: once a day at its time (RyM open).
    if core::alarm_rings(&mut s.config) {
        drop(s);
        ring_alarm(ui);
        return;
    }
    if s.loading {
        return;
    }
    // SponsorBlock: a music video's non-music parts are jumped over (each once).
    let now_id = now.map(|t| t.video_id).unwrap_or_default();
    let sponsor = s.config.sponsorblock;
    if playing && let Some(to) = s.non_music.jump(&now_id, pos, sponsor) {
        if let Some(a) = &s.audio {
            a.seek(to);
        }
        drop(s);
        toast(ui, tr("Skipped a part that isn't music (SponsorBlock)"));
        return;
    }
    // Sleep timer: the last 10 seconds fade out, then the music stops.
    if let Some(sleep) = s.sleep {
        let left = match sleep {
            Sleep::At(when) => when.saturating_duration_since(Instant::now()).as_secs_f32(),
            Sleep::EndOfSong if dur > 0.0 => dur - pos,
            Sleep::EndOfSong => f32::MAX,
        };
        if playing && left < 10.0 {
            let fade = (left / 10.0).clamp(0.0, 1.0);
            if let Some(a) = &mut s.audio {
                a.set_fade(fade);
                if left <= 0.25 {
                    a.pause();
                    a.set_fade(1.0);
                }
            }
            if left <= 0.25 {
                s.sleep = None;
                drop(s);
                toast(ui, tr("Sleep timer: music stopped"));
                return;
            }
        }
    }
    // Discord: told when the song, play/pause, or the position (a seek) changes.
    if s.config.discord && !s.config.discord_app_id.is_empty() && let Some(t) = s.q.now().cloned() {
        let expected = s.discord_sent.as_ref().map(|(_, was_playing, at, when)| at + if *was_playing { when.elapsed().as_secs_f32() } else { 0.0 });
        let changed = s.discord_sent.as_ref().is_none_or(|(id, was_playing, ..)| *id != t.video_id || *was_playing != playing)
            || expected.is_some_and(|e| (e - pos).abs() > 3.0);
        if changed && loaded {
            s.discord_sent = Some((t.video_id.clone(), playing, pos, Instant::now()));
            let cover = t.thumb.as_ref().map(|u| rymplayer::api::thumb_sized(u, 512));
            rymplayer::discord::show(&s.config.discord_app_id, rymplayer::discord::Show::Song { video_id: t.video_id.clone(), title: t.title.clone(), artist: t.artist.clone(), cover, at: pos, length: dur, paused: !playing });
        }
    }
    // Crossfade: the next song starts that many seconds before this one ends. Smart automix:
    // also as soon as it has faded out in its last 15 seconds.
    let automix = s.config.automix;
    let fade_secs = if automix { rymplayer::automix::mix_secs(s.config.crossfade) } else { s.config.crossfade as f32 };
    let next_index = s.q.current.map(|c| c + 1).filter(|n| *n < s.q.songs.len());
    let lead = if automix { rymplayer::automix::LOAD_LEAD } else { 0.0 };
    let due = pos >= dur - fade_secs - lead
        || (automix && pos >= dur - rymplayer::automix::WINDOW_SECS && s.audio.as_ref().is_some_and(|a| a.faded_out()));
    if fade_secs > 0.0
        && playing
        && dur > fade_secs + 10.0
        && due
        && s.crossfade_for != s.request_id
        && s.sleep != Some(Sleep::EndOfSong)
        && let Some(n) = next_index
    {
        s.crossfade_for = s.request_id;
        s.crossfade_next = true;
        drop(s);
        play_at(ui, n);
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
    // Word by word: the sung line fills as it's sung (sources with word times): the sung part in
    // the line's color, the rest faded; the line before goes back to plain text.
    {
        let fill = line.and_then(|i| {
            let l = s.lyrics.as_ref()?;
            let words = l.words.get(i).filter(|w| !w.is_empty())?;
            let end = l.synced.get(i + 1).map_or(words.last().map_or(pos, |w| w.0) + 3.0, |n| n.0);
            Some((i, rymplayer::lyrics::sung_split(words, end, pos)))
        });
        let key = fill.as_ref().map(|(i, (a, b))| (*i, a.len(), b.len()));
        if key != s.word_fill {
            if let Some((old, _, _)) = s.word_fill
                && Some(old) != line
                && let (Some(label), Some(text)) = (lyric_label(&s.w.lyrics_box, old), s.lyrics.as_ref().and_then(|l| l.synced.get(old)).map(|x| x.1.clone()))
            {
                label.set_text(&text);
            }
            if let Some((i, (sung, left))) = &fill
                && let Some(label) = lyric_label(&s.w.lyrics_box, *i)
            {
                label.set_markup(&format!("{}<span alpha=\"35%\">{}</span>", glib::markup_escape_text(sung), glib::markup_escape_text(left)));
            }
            s.word_fill = key;
        }
    }
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
        if let Some((_, now, next)) = &s.overlay
            && let Some(l) = &s.lyrics
            && !l.synced.is_empty()
        {
            now.set_text(line.and_then(|i| l.synced.get(i)).map_or("♪", |x| if x.1.is_empty() { "♪" } else { x.1.as_str() }));
            next.set_text(line.map_or(0, |i| i + 1).min(l.synced.len()).checked_sub(0).and_then(|i| l.synced.get(i)).map_or("", |x| x.1.as_str()));
        }
    }
    if loaded && finished && s.q.current.is_some() && s.ended_for != s.request_id {
        s.ended_for = s.request_id;
        if let Some(a) = &mut s.audio {
            a.stop();
        }
        drop(s);
        skip(ui);
    }
}

// ---------------------------------------------------------------- background results

/// The header's account: the picture (a menu with the name, the handle and Sign out) when
/// signed in; a "Sign in" button when not (`guest`); nothing while it isn't known yet.
fn show_account(ui: &Shared, who: Option<&rymplayer::api::Account>, photo: Option<&gdk::Texture>, guest: bool) {
    let (account, sign_in) = {
        let s = ui.borrow();
        (s.w.account.clone(), s.w.sign_in.clone())
    };
    sign_in.set_visible(guest && who.is_none());
    let Some(a) = who else {
        account.set_visible(false);
        return;
    };
    let picture = |size: i32| -> gtk::Image {
        let img = match photo {
            Some(t) => gtk::Image::from_paintable(Some(t)),
            None => gtk::Image::from_icon_name("avatar-default-symbolic"),
        };
        img.set_pixel_size(size);
        img.add_css_class("avatar");
        img.set_overflow(gtk::Overflow::Hidden);
        img
    };
    account.set_child(Some(&picture(30)));
    account.set_tooltip_text(Some(&a.name));
    let col = gtk::Box::new(Orientation::Vertical, 6);
    col.set_margin_top(14);
    col.set_margin_bottom(12);
    col.set_margin_start(16);
    col.set_margin_end(16);
    let big = picture(72);
    big.set_halign(Align::Center);
    col.append(&big);
    let name = label(&a.name, &["title"]);
    name.set_halign(Align::Center);
    col.append(&name);
    if !a.handle.is_empty() {
        let handle = label(&a.handle, &["dim", "small"]);
        handle.set_halign(Align::Center);
        col.append(&handle);
    }
    let out = Button::with_label(tr("Sign out"));
    out.add_css_class("ghost");
    out.set_margin_top(8);
    let pop = gtk::Popover::new();
    {
        let (ui, pop) = (ui.clone(), pop.clone());
        out.connect_clicked(move |_| {
            pop.popdown();
            sign_in_or_out(&ui);
        });
    }
    col.append(&out);
    pop.set_child(Some(&col));
    account.set_popover(Some(&pop));
    account.set_visible(true);
}

const MINI_TITLE: &str = "RyM mini player";

/// The mini player: a small window above the others with the cover, the song, and
/// previous / play / next (dragged by the cover or the names).
fn show_mini(ui: &Shared, on: bool) {
    if !on {
        let old = ui.borrow_mut().mini.take();
        if let Some((w, ..)) = old {
            w.destroy();
        }
        return;
    }
    if ui.borrow().mini.is_some() {
        return;
    }
    let win = gtk::Window::new();
    win.set_title(Some(MINI_TITLE));
    win.set_decorated(false);
    win.set_default_size(380, 76);
    win.set_resizable(false);
    win.add_css_class("rym");
    win.add_css_class("mini-win");
    let row = gtk::Box::new(Orientation::Horizontal, 10);
    row.set_margin_top(8);
    row.set_margin_bottom(8);
    row.set_margin_start(8);
    row.set_margin_end(6);
    let art = picture(60, "cover");
    let names = gtk::Box::new(Orientation::Vertical, 2);
    names.set_hexpand(true);
    names.set_valign(Align::Center);
    let title = label(tr("Nothing playing"), &["title"]);
    let artist = label("", &["dim", "small"]);
    names.append(&title);
    names.append(&artist);
    let prev = icon_button("media-skip-backward-symbolic", tr("Previous"), &[]);
    let play = icon_button("media-playback-start-symbolic", tr("Play / pause"), &[]);
    let next = icon_button("media-skip-forward-symbolic", tr("Next"), &[]);
    let close = icon_button("window-close-symbolic", tr("Close"), &["dim"]);
    for b in [&prev, &play, &next, &close] {
        b.set_valign(Align::Center);
    }
    row.append(&art);
    row.append(&names);
    row.append(&prev);
    row.append(&play);
    row.append(&next);
    row.append(&close);
    win.set_child(Some(&row));
    // Dragged by the cover or the names (the buttons keep their clicks).
    for handle in [art.upcast_ref::<gtk::Widget>(), names.upcast_ref::<gtk::Widget>()] {
        let press = gtk::GestureClick::new();
        let w = win.clone();
        press.connect_pressed(move |g, _, x, y| {
            if let (Some(surface), Some(device)) = (w.surface(), g.device())
                && let Ok(top) = surface.downcast::<gdk::Toplevel>()
            {
                top.begin_move(&device, g.current_button() as i32, x, y, g.current_event_time());
            }
        });
        handle.add_controller(press);
    }
    {
        let ui = ui.clone();
        prev.connect_clicked(move |_| previous(&ui));
    }
    {
        let ui = ui.clone();
        play.connect_clicked(move |_| toggle_playback(&ui));
    }
    {
        let ui = ui.clone();
        next.connect_clicked(move |_| skip(&ui));
    }
    {
        let ui = ui.clone();
        close.connect_clicked(move |_| show_mini(&ui, false));
    }
    win.present();
    #[cfg(windows)]
    glib::timeout_add_local_once(Duration::from_millis(200), || window_on_top(MINI_TITLE, false));
    let mut s = ui.borrow_mut();
    s.mini = Some((win, art, title, artist, play));
    s.mini_for = None;
}

/// The mini player follows the song and play / pause (touched only when they change).
fn mini_sync(ui: &Shared) {
    let mut wants = Wants::new();
    {
        let Ok(mut s) = ui.try_borrow_mut() else { return };
        let Some((_, art, title, artist, play)) = s.mini.clone() else { return };
        let Some(t) = s.q.now().cloned() else { return };
        let playing = s.audio.as_ref().is_some_and(|a| a.is_playing());
        let key = (t.video_id.clone(), playing);
        if s.mini_for.as_ref() == Some(&key) {
            return;
        }
        let new_song = s.mini_for.as_ref().is_none_or(|(id, _)| *id != t.video_id);
        s.mini_for = Some(key);
        play.set_icon_name(if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
        if new_song {
            title.set_text(&t.title);
            artist.set_text(&t.artist);
            let pic = cover(&s, t.thumb.as_ref(), ROW_PX, 60, "cover", &mut wants);
            match pic.paintable() {
                Some(p) => art.set_paintable(Some(&p)),
                None => {
                    art.set_paintable(gdk::Paintable::NONE);
                    if let Some((url, px, _)) = wants.pop() {
                        wants.push((url, px, art.clone()));
                    }
                }
            }
        }
    }
    want_covers(ui, wants);
}

/// "Save Up next as a playlist": a name, then a new playlist in the account with the playing song
/// and every song after it.
fn save_queue_dialog(ui: &Shared) {
    let songs: Vec<Track> = {
        let s = ui.borrow();
        let from = s.q.current.unwrap_or(0);
        s.q.songs.iter().skip(from).filter(|t| t.link.is_none() && rymplayer::is_youtube(&t.video_id)).cloned().collect()
    };
    if songs.is_empty() || !signed_in(ui) {
        toast(ui, tr(if songs.is_empty() { "Nothing to save" } else { "Sign in to save playlists" }));
        return;
    }
    let parent = ui.borrow().w.window.clone();
    let win = gtk::Window::builder().transient_for(&parent).modal(true).default_width(380).title(tr("Save Up next as a playlist")).build();
    win.add_css_class("rym");
    let col = gtk::Box::new(Orientation::Vertical, 10);
    col.set_margin_top(18);
    col.set_margin_bottom(18);
    col.set_margin_start(20);
    col.set_margin_end(20);
    col.append(&label(&format!("{} {}", songs.len(), tr("songs")), &["dim"]));
    let name = gtk::Entry::new();
    name.add_css_class("search");
    name.set_text(&format!("RyM · {}", songs[0].title));
    col.append(&name);
    let buttons = gtk::Box::new(Orientation::Horizontal, 10);
    buttons.set_halign(Align::End);
    let cancel = Button::with_label(tr("Cancel"));
    cancel.add_css_class("ghost");
    let save = Button::with_label(tr("Save"));
    save.add_css_class("pill");
    {
        let win = win.clone();
        cancel.connect_clicked(move |_| win.close());
    }
    {
        let (win, ui, name) = (win.clone(), ui.clone(), name.clone());
        save.connect_clicked(move |_| {
            let title = name.text().trim().to_string();
            if title.is_empty() {
                return;
            }
            win.close();
            let Some(api) = api_of(&ui) else { return };
            let songs = songs.clone();
            toast(&ui, tr("Saving the playlist..."));
            post(&ui, async move {
                let made = api.clone().create_playlist(title.clone()).await;
                let msg = match made {
                    Ok(p) => {
                        let mut added = 0;
                        for t in &songs {
                            if api.clone().add_to_playlist(p.id.clone(), t.video_id.clone()).await.is_ok() {
                                added += 1;
                            }
                        }
                        format!("{} \"{title}\" ({added} {})", tr("Saved"), tr("songs"))
                    }
                    Err(e) => format!("{}: {e}", tr("Couldn't save the playlist")),
                };
                Ev::Saved(msg, api.library_playlists().await)
            });
        });
    }
    buttons.append(&cancel);
    buttons.append(&save);
    col.append(&buttons);
    win.set_child(Some(&col));
    win.present();
}

/// The media keys and the system's media controls (Windows: the volume flyout with the song and
/// its cover; Linux: MPRIS), started once the window is up.
fn start_media_controls(ui: &Shared) {
    if ui.borrow().media.is_some() {
        return;
    }
    #[cfg(windows)]
    let hwnd = main_window_handle();
    #[cfg(not(windows))]
    let hwnd = None;
    if cfg!(windows) && hwnd.is_none() {
        return;
    }
    let config = souvlaki::PlatformConfig { display_name: "RyM", dbus_name: "rym_player", hwnd };
    let Ok(mut controls) = souvlaki::MediaControls::new(config) else {
        log("media controls: not available");
        return;
    };
    let tx = ui.borrow().tx.clone();
    if controls.attach(move |e| {
        let _ = tx.send(Ev::MediaKey(e));
    }).is_err() {
        log("media controls: couldn't attach");
        return;
    }
    let mut s = ui.borrow_mut();
    s.media = Some(controls);
    s.media_sent = None;
}

/// This app's main window (Windows): its visible top window that isn't the floating lyrics.
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
            if title.is_empty() || title == OVERLAY_TITLE {
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

/// A media key pressed (or the system's controls used).
fn media_key(ui: &Shared, key: souvlaki::MediaControlEvent) {
    use souvlaki::MediaControlEvent as K;
    let playing = ui.borrow().audio.as_ref().is_some_and(|a| a.is_playing());
    match key {
        K::Toggle => toggle_playback(ui),
        K::Play if !playing => toggle_playback(ui),
        K::Pause | K::Stop if playing => toggle_playback(ui),
        K::Next => skip(ui),
        K::Previous => previous(ui),
        K::SetPosition(souvlaki::MediaPosition(at)) => {
            if let Some(a) = &ui.borrow().audio {
                a.seek(at.as_secs_f32());
            }
        }
        _ => {}
    }
}

/// The system's media controls follow the song and play/pause (told only when those change).
fn media_sync(ui: &Shared) {
    let Ok(mut s) = ui.try_borrow_mut() else { return };
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

/// Seamless Music (RyM Link): a phone of the same account plays while this app is idle: the music moves here
/// (or, with "ask" chosen, a question first). Once per song and phone.
fn heard_peer(ui: &Shared, note: rymplayer::link::Announce, ip: std::net::IpAddr) {
    let (mine, auto) = {
        let mut s = ui.borrow_mut();
        let mine = s.account.as_ref().map(|a| rymplayer::link::account_key(&a.name, &a.handle));
        let idle = !s.loading && !s.audio.as_ref().is_some_and(|a| a.is_playing());
        // Seamless Music off: the phones are ignored.
        let on = s.config.cross_ears;
        if !s.peers.wants(&note, mine.as_deref(), idle, on) {
            return;
        }
        (mine, s.config.link_auto)
    };
    let Some(key) = mine else { return };
    let take = {
        let ui = ui.clone();
        let (device, port, key) = (note.device.clone(), note.port, key.clone());
        move || {
            let device = device.clone();
            let key = key.clone();
            post(&ui, async move { Ev::TookOver(device, rymplayer::link::take_over(ip, port, &key).await) });
        }
    };
    if auto {
        take();
        return;
    }
    // Asked first: what plays on the phone, and "Play here".
    let parent = ui.borrow().w.window.clone();
    let win = gtk::Window::builder().transient_for(&parent).modal(true).default_width(420).title("RyM").build();
    win.add_css_class("rym");
    let col = gtk::Box::new(Orientation::Vertical, 10);
    col.set_margin_top(18);
    col.set_margin_bottom(18);
    col.set_margin_start(20);
    col.set_margin_end(20);
    col.append(&label(&format!("{} {}", tr("Playing on"), note.device), &["dim"]));
    col.append(&label(&note.title, &["heading"]));
    col.append(&label(&note.artist, &["dim"]));
    let buttons = gtk::Box::new(Orientation::Horizontal, 10);
    buttons.set_halign(Align::End);
    let no = Button::with_label(tr("Not now"));
    no.add_css_class("ghost");
    let yes = Button::with_label(tr("Play here"));
    yes.add_css_class("pill");
    {
        let win = win.clone();
        no.connect_clicked(move |_| win.close());
    }
    {
        let win = win.clone();
        yes.connect_clicked(move |_| {
            take();
            win.close();
        });
    }
    buttons.append(&no);
    buttons.append(&yes);
    col.append(&buttons);
    win.set_child(Some(&col));
    win.present();
}

/// Asks for suggestions for the search box's text after `wait_ms` (each keystroke starts the
/// wait again, so only the last one asks).
fn ask_suggestions(ui: &Shared, wait_ms: u64) {
    // Called from focus changes, which can happen inside other calls: if the state is in use,
    // a moment later.
    if ui.try_borrow_mut().is_err() {
        let ui = ui.clone();
        glib::idle_add_local_once(move || ask_suggestions(&ui, wait_ms));
        return;
    }
    let waiting = ui.borrow_mut().suggest_wait.take();
    if let Some(id) = waiting {
        id.remove();
    }
    let ui2 = ui.clone();
    let id = glib::timeout_add_local_once(Duration::from_millis(wait_ms), move || {
        ui2.borrow_mut().suggest_wait = None;
        let Some(api) = api_of(&ui2) else { return };
        let text = ui2.borrow().w.search.text().trim().to_string();
        ui2.borrow_mut().suggest_for = Some(text.clone());
        post(&ui2, async move { Ev::Suggestions(text.clone(), api.search_suggestions(text).await) });
    });
    ui.borrow_mut().suggest_wait = Some(id);
}

/// The suggestions under the search box: a click searches it; a past search can be taken off the
/// account's history (✕).
fn show_suggestions(ui: &Shared, list: Vec<rymplayer::api::Suggestion>) {
    let (pop, rows, entry) = {
        let s = ui.borrow();
        (s.w.suggest.clone(), s.w.suggest_box.clone(), s.w.search.clone())
    };
    clear(&rows);
    if list.is_empty() || !entry.has_focus() && !entry.first_child().is_some_and(|c| c.has_focus()) {
        pop.popdown();
        return;
    }
    rows.set_size_request(entry.width().max(320), -1);
    for sug in list.into_iter().take(10) {
        let row = gtk::Box::new(Orientation::Horizontal, 10);
        let pick = Button::new();
        pick.add_css_class("flat-row");
        pick.set_hexpand(true);
        let inner = gtk::Box::new(Orientation::Horizontal, 10);
        inner.append(&gtk::Image::from_icon_name(if sug.from_history() { "document-open-recent-symbolic" } else { "system-search-symbolic" }));
        let l = label(&sug.text, &[]);
        l.set_hexpand(true);
        inner.append(&l);
        pick.set_child(Some(&inner));
        {
            let (ui, text, pop, entry) = (ui.clone(), sug.text.clone(), pop.clone(), entry.clone());
            pick.connect_clicked(move |_| {
                entry.set_text(&text);
                pop.popdown();
                search_songs(&ui, text.clone());
            });
        }
        row.append(&pick);
        if let Some(token) = sug.forget.clone().filter(|t| !t.is_empty()) {
            let x = icon_button("window-close-symbolic", tr("Remove from search history"), &["dim"]);
            let (ui, row2, rows2) = (ui.clone(), row.clone(), rows.clone());
            x.connect_clicked(move |_| {
                rows2.remove(&row2);
                if let Some(api) = api_of(&ui) {
                    let token = token.clone();
                    post(&ui, async move {
                        let _ = api.forget_search(token).await;
                        Ev::Done
                    });
                }
            });
            row.append(&x);
        }
        rows.append(&row);
    }
    pop.popup();
}

/// Opens or closes the playing song's page: it rises over the pages, its cover zooming in, with
/// the lyrics and Up next beside it.
fn show_side(w: &Widgets, open: bool) {
    if open == side_shown(w) {
        return;
    }
    w.stack.set_transition_type(if open { gtk::StackTransitionType::OverUp } else { gtk::StackTransitionType::UnderDown });
    w.stack.set_visible_child_name(if open { "now" } else { "page" });
    if open {
        // The zoom runs again each time (the class is put back once it was off).
        w.np_art.remove_css_class("zoom");
        let art = w.np_art.clone();
        glib::idle_add_local_once(move || art.add_css_class("zoom"));
    }
}

fn side_shown(w: &Widgets) -> bool {
    w.stack.visible_child_name().as_deref() == Some("now")
}

/// Going to a page leaves the song's page (and songs starting on their own don't bring it back;
/// playing one from a list does).
fn to_page(ui: &Shared) {
    let mut s = ui.borrow_mut();
    s.side_closed = true;
    show_side(&s.w, false);
}

/// A song played from a list or a card opens its page.
fn song_page_on_play(ui: &Shared) {
    ui.borrow_mut().side_closed = false;
}

/// RyM YTM: the library's playlists in the side menu; each opens.
fn fill_nav_lists(ui: &Shared) {
    let (lists, playlists) = {
        let s = ui.borrow();
        if !s.w.ytm {
            return;
        }
        (s.w.nav_lists.clone(), s.playlists.clone())
    };
    clear(&lists);
    for p in playlists {
        let b = Button::new();
        b.add_css_class("pl");
        b.set_child(Some(&label(&p.title, &["title", "small"])));
        let ui = ui.clone();
        b.connect_clicked(move |_| {
            if let Some(api) = api_of(&ui) {
                open_songs(&ui, p.title.clone(), api.playlist_tracks(p.id.clone()));
            }
        });
        lists.append(&b);
    }
}

/// The playing song's cover on the bar, and the big one on its page.
fn show_cover(s: &Ui, t: &Track, wants: &mut Wants) {
    for (px, size, target) in [(ROW_PX, 52, s.w.bar_cover.clone()), (BIG_PX, 480, s.w.np_art.clone())] {
        let pic = cover(s, t.thumb.as_ref(), px, size, "cover", wants);
        match pic.paintable() {
            Some(p) => target.set_paintable(Some(&p)),
            None => {
                target.set_paintable(gdk::Paintable::NONE);
                if let Some((url, px, _)) = wants.pop() {
                    wants.push((url, px, target));
                }
            }
        }
    }
}

fn sign_in_or_out(ui: &Shared) {
    if signed_in(ui) {
        rymplayer::secret::forget_cookie();
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
            s.account = None;
        }
        show_account(ui, None, None, true);
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
                show_account(ui, None, None, !user);
                if user {
                    let api2 = api.clone();
                    post(ui, async move { Ev::Account(api2.account().await) });
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
                let waiting = ui.borrow_mut().pending_link.take();
                if let Some(link) = waiting {
                    open_incoming(ui, link);
                }
            }
            Ev::Incoming(link) => {
                let window = ui.borrow().w.window.clone();
                window.present();
                open_incoming(ui, link);
            }
            Ev::Handed(Ok(songs)) => {
                let songs: Vec<Track> = songs.into_iter().filter(|t| !t.video_id.is_empty() && t.link.is_none()).collect();
                if songs.is_empty() {
                    toast(ui, tr("No songs in"));
                } else {
                    play_list(ui, &songs, 0);
                }
            }
            Ev::Handed(Err(e)) => toast(ui, &e.to_string()),
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
            Ev::Account(Ok(a)) => {
                show_account(ui, Some(&a), None, false);
                if let Some(url) = a.photo.clone() {
                    post(ui, async move {
                        let bytes = async { Ok::<_, anyhow::Error>(reqwest::get(&url).await?.error_for_status()?.bytes().await?.to_vec()) }.await;
                        Ev::Avatar(url, bytes)
                    });
                }
                ui.borrow_mut().account = Some(a);
            }
            // Signed in, but the account menu didn't answer: the picture's place shows a plain icon.
            Ev::Account(Err(e)) => {
                log(&format!("account: {e}"));
                let a = rymplayer::api::Account { name: tr("Signed in").into(), handle: String::new(), photo: None };
                show_account(ui, Some(&a), None, false);
            }
            Ev::Avatar(url, Ok(bytes)) => {
                let a = ui.borrow().account.clone().filter(|a| a.photo.as_deref() == Some(url.as_str()));
                if let (Some(a), Ok(t)) = (a, gdk::Texture::from_bytes(&glib::Bytes::from(&bytes))) {
                    show_account(ui, Some(&a), Some(&t), false);
                }
            }
            Ev::Avatar(_, Err(e)) => log(&format!("account picture: {e}")),
            Ev::Home(result) => {
                ui.borrow_mut().home_loading = false;
                match result {
                    Ok(fresh) => {
                        if let Some(liked) = fresh.liked {
                            sync_liked(ui, liked);
                        }
                        page_cache::save_home(&fresh.home);
                        let ids: Vec<String> = fresh.home.iter().filter(|t| t.link.is_none()).map(|t| t.video_id.clone()).collect();
                        let mut s = ui.borrow_mut();
                        s.home = fresh.home;
                        s.home_more = fresh.more;
                        drop(s);
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
            Ev::Paged(id, result) => {
                if id != ui.borrow().page_id {
                    continue;
                }
                let tracks = result.map(|p| {
                    ui.borrow_mut().list_more = p.more;
                    p.tracks
                });
                ui.borrow().tx.send(Ev::Tracks(id, tracks)).ok();
            }
            Ev::More(id, result) => {
                let current = id == ui.borrow().page_id;
                {
                    let mut s = ui.borrow_mut();
                    s.loading_more = false;
                    if let Some(spin) = s.more_spinner.as_ref().and_then(glib::WeakRef::upgrade) {
                        spin.stop();
                        spin.set_visible(false);
                    }
                }
                match result {
                    Ok(part) if current => add_more(ui, part),
                    Ok(_) => {}
                    Err(e) => log(&format!("more: {e}")),
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
                fill_nav_lists(ui);
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
                        if s.covers.insert(url.clone(), t).is_none() {
                            s.cover_order.push_back(url);
                        }
                        while s.cover_order.len() > 250 {
                            if let Some(old) = s.cover_order.pop_front() {
                                s.covers.remove(&old);
                                // Asked again if a page wants it later.
                                s.requested.remove(&old);
                            }
                        }
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
                let Some(t) = s.q.current.and_then(|i| s.q.songs.get(i).cloned()) else { continue };
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
                let ms = s.play_asked.elapsed().as_millis();
                drop(s);
                match &started {
                    Ok(()) => log(&format!("  playing after {ms} ms")),
                    Err(e) => log(&format!("  can't play after {ms} ms: {e}")),
                }
                match started {
                    Ok(()) => {
                        prefetch_next(ui);
                        // Last.fm: "now playing", and its listening counts from now.
                        let mut s = ui.borrow_mut();
                        let dur = s.audio.as_ref().and_then(Audio::duration).unwrap_or(0.0);
                        let cfg = s.config.clone();
                        if let Some(send) = s.scrobbler.start(&t, dur, &cfg) {
                            spawn(&s, async move {
                                send.await;
                                Ev::Done
                            });
                        }
                    }
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
                let Some(t) = s.q.current.and_then(|i| s.q.songs.get(i).cloned()) else { continue };
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
                    let s = ui.borrow();
                    s.w.lyrics_provider.load_from_string(&lyrics_css(bg, s.w.ytm));
                }
            }
            Ev::Suggestions(asked, Ok(list)) => {
                if ui.borrow().suggest_for.as_deref() == Some(asked.as_str()) {
                    show_suggestions(ui, list);
                }
            }
            Ev::Suggestions(..) => {}
            Ev::Romaji(vid, source, lines) => {
                let current = ui.borrow().lyrics.clone().filter(|l| l.source == source && now_playing(ui).is_some_and(|t| t.video_id == vid));
                if let Some(l) = current.filter(|_| lines.iter().any(|x| !x.is_empty())) {
                    ui.borrow_mut().romaji = (format!("{vid}|{source}"), lines);
                    let line = ui.borrow().lyric_line;
                    show_lyrics(ui, Some(l));
                    ui.borrow_mut().lyric_line = line.map(|_| usize::MAX);
                }
            }
            Ev::Segments(vid, parts) => ui.borrow_mut().non_music.got(&vid, parts),
            Ev::Translated(vid, source, tr) => {
                let current = ui.borrow().lyrics.clone().filter(|l| l.source == source && now_playing(ui).is_some_and(|t| t.video_id == vid));
                if let Some(mut l) = current.filter(|_| !tr.is_empty()) {
                    l.translation = tr;
                    rymplayer::lyrics::remember(&vid, &l);
                    let line = ui.borrow().lyric_line;
                    show_lyrics(ui, Some(l));
                    // The sung line is marked again on the next tick.
                    ui.borrow_mut().lyric_line = line.map(|_| usize::MAX);
                }
            }
            Ev::Installed(Ok(exe)) => {
                // The new version opens where this one leaves off.
                save_session(ui);
                rymplayer::stats::flush();
                if std::process::Command::new(&exe).spawn().is_ok() {
                    // Closing runs the close handler, which uses the app's state: not held here.
                    let window = ui.borrow().w.window.clone();
                    window.close();
                    // It goes for good (closing may only hide it to the tray), so the old
                    // version doesn't stay open beside the new one.
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_secs(3));
                        std::process::exit(0);
                    });
                } else {
                    toast(ui, tr("Updated: open RyM again to use the new version"));
                }
            }
            Ev::Installed(Err(e)) => toast(ui, &format!("{}: {e}", tr("Couldn't update"))),
            Ev::Peer(note, ip) => heard_peer(ui, note, ip),
            Ev::MediaKey(key) => media_key(ui, key),
            Ev::Saved(msg, lists) => {
                toast(ui, &msg);
                if let Ok(p) = lists {
                    ui.borrow_mut().playlists = p;
                    fill_nav_lists(ui);
                }
            }
            Ev::TookOver(device, Ok(state)) => {
                let current = state.current;
                let songs: Vec<Track> = state.songs.into_iter().map(Track::from).collect();
                {
                    let mut s = ui.borrow_mut();
                    let vid = songs.get(current).map(|t| t.video_id.clone()).unwrap_or_default();
                    s.q = rymplayer::queue::Queue { songs, current: Some(current), added: 0 };
                    // The song goes on from the phone's second.
                    s.resume_at = Some((vid, state.position));
                    s.side_closed = false;
                }
                play_at(ui, current);
                toast(ui, &format!("{} {device}", tr("Playing here, moved from")));
            }
            Ev::TookOver(_, Err(e)) => toast(ui, &format!("{}: {e}", tr("Couldn't move the music from the phone"))),
            Ev::LyricsSources(vid, list) => {
                let wanted = {
                    let mut s = ui.borrow_mut();
                    if s.sources_for.as_deref() == Some(vid.as_str()) {
                        s.sources_for = None;
                    }
                    s.alt_lyrics = Some((vid.clone(), list));
                    s.wanted_source.take()
                };
                match wanted {
                    Some(p) if now_playing(ui).is_some_and(|t| t.video_id == vid) => pick_source(ui, p),
                    _ => fill_pills(ui),
                }
            }
            Ev::Lyrics(vid, found) => {
                log(&format!("  lyrics for {vid}: {}", found.as_ref().map_or("none".to_string(), |l| format!("{} synced lines, from {}", l.synced.len(), l.source))));
                if now_playing(ui).is_some_and(|t| t.video_id == vid) {
                    show_lyrics(ui, found);
                    prefetch_sources(ui);
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
                    // RaM points: only the playing song's kind, its artist and songs like it first,
                    // nothing played or queued already, no repeats.
                    let fresh: Vec<Track> = match s.q.now().cloned() {
                        Some(seed) => {
                            let queued: HashSet<String> = s.q.songs.iter().map(|t| t.video_id.clone()).collect();
                            let heard = rymplayer::radio::artists_of(s.q.songs.iter().take(s.q.current.map_or(0, |c| c + 1)));
                            let skip: HashSet<String> = s.played.union(&queued).cloned().collect();
                            rymplayer::ram_points::up_next(tracks, &seed, &s.liked_tracks, &skip, &heard, rymplayer::updates::now() | 1)
                        }
                        None => tracks,
                    };
                    let at_end = s.q.current.is_some_and(|i| i + 1 >= s.q.songs.len()) && !s.loading && s.audio.as_ref().is_some_and(|a| !a.is_loaded());
                    let next = s.q.songs.len();
                    s.q.songs.extend(fresh);
                    drop(s);
                    if at_end && next < ui.borrow().q.songs.len() {
                        play_at(ui, next);
                    } else {
                        prefetch_next(ui);
                        render_queue(ui);
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
            Ev::Update(result, asked) => match result {
                Ok(Some(r)) => show_update(ui, &r),
                Ok(None) if asked => toast(ui, tr("You have the latest version")),
                Err(e) if asked => toast(ui, &format!("{}: {e}", tr("Couldn't check for updates"))),
                _ => {}
            },
            Ev::Store(result) => {
                match result {
                    Ok(list) => {
                        let news: Vec<String> = {
                            let s = ui.borrow();
                            rymplayer::plugins::updates(&s.config.plugins, &list).iter().map(|p| p.name().to_string()).collect()
                        };
                        if !news.is_empty() {
                            toast(ui, &format!("{}: {}", tr("Plugin updates"), news.join(", ")));
                        }
                        ui.borrow_mut().store = Some(list);
                    }
                    Err(e) => toast(ui, &e.to_string()),
                }
                if ui.borrow().page == Page::Plugins {
                    render(ui);
                }
            }
            Ev::VideoLinks(t, result) => {
                let msg = match (result, vlc::find()) {
                    (Ok((video, audio)), Some(exe)) => {
                        // The song pauses while the video plays.
                        if let Some(a) = &ui.borrow().audio
                            && a.is_playing()
                        {
                            a.toggle();
                        }
                        match rymplayer::video::watch_in_vlc(&exe, &video, audio.as_deref(), &format!("{} - {}", t.artist, t.title)) {
                            Ok(child) => {
                                ui.borrow_mut().vlc = Some(child);
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
                    toast(ui, &m);
                }
            }
            Ev::Batch(b) => {
                ui.borrow_mut().batch = Some(b);
                update_batch(ui);
            }
            Ev::Done => {}
            Ev::Parsers(text) => {
                toast(ui, &format!("{}: {text}", tr("Parsers")));
                PARSERS_LABEL.with(|l| {
                    if let Some(label) = l.borrow().as_ref().and_then(|w| w.upgrade()) {
                        label.set_text(&format!("{} · {text}", tr("Up to date")));
                    }
                });
            }
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
    media_sync(ui);
    mini_sync(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every theme's style, and the cover colors over it, is CSS GTK takes without an error.
    #[test]
    fn styles_parse() {
        if gtk::init().is_err() {
            eprintln!("no display: skipped");
            return;
        }
        let errors = Rc::new(std::cell::Cell::new(0));
        for theme in 0..THEMES.len() {
            for style in [css(theme), lyrics_css((180, 60, 90), ytm_layout(theme)), lyrics_css(rymplayer::colors::PLAIN, ytm_layout(theme))] {
                let provider = CssProvider::new();
                let e = errors.clone();
                provider.connect_parsing_error(move |_, section, err| {
                    eprintln!("theme {theme}: {section}: {err}");
                    e.set(e.get() + 1);
                });
                provider.load_from_string(&style);
            }
        }
        assert_eq!(errors.get(), 0);
    }
}
