//! Terminal drawing. Reads `App` state; all input handling lives in main.rs.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, HighlightSpacing, LineGauge, List, ListItem, Paragraph, Row, Table,
};

use crate::api::Link;
use crate::config::{BarPlace, Config, LyricsPlace, SETTINGS, Side, Theme};
use crate::{App, Focus, Input, Mode, NavItem, Page, fmt_time, lyrics};
use rymplayer::i18n;

/// Interface text in the app's language, ready to print.
fn t(en: &'static str) -> String {
    show(i18n::tr(en))
}

/// Any text (song titles, lyrics, messages) ready to print: Windows terminals don't lay out
/// Arabic themselves, so its letters are joined and put in visual order here.
pub fn show(s: &str) -> String {
    if cfg!(windows) { i18n::for_terminal(s) } else { s.to_string() }
}

/// First row of the add-to-playlist picker.
pub const NEW_PLAYLIST_ROW: &str = "＋ New playlist";

/// Something on screen that does a thing when clicked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Click {
    ForYou,
    History,
    Search,
    Settings,
    /// A row of the settings screen.
    Setting(usize),
    /// A row of the add-to-playlist picker.
    Pick(usize),
    /// A row of the song menu.
    MenuItem(usize),
}

/// Screen areas the mouse handler needs, filled in while drawing.
#[derive(Debug, Default, Clone)]
pub struct Areas {
    pub nav: Rect,
    pub tracks: Rect,
    pub gauge: Rect,
    /// Width of the time label at the start of the gauge.
    pub gauge_label: u16,
    pub buttons: Vec<(Rect, Click)>,
    /// The open popup (settings or playlist picker); clicks outside it close it.
    pub popup: Rect,
}

/// Selected row: accent-tinted in the pane you're in, grey in the other one.
fn cursor_style(th: &Theme, focused: bool) -> Style {
    if focused {
        Style::new().bg(th.cursor).fg(th.text).bold()
    } else {
        Style::new().bg(th.cursor_dim)
    }
}

fn pane(th: &Theme, title: &str, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { th.focus } else { th.faint }))
        .title(Span::styled(format!(" {} ", show(title)), Style::new().fg(th.text).bold()))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    if let Some(bg) = app.config.theme().bg {
        f.render_widget(Block::default().style(Style::new().bg(bg)), f.area());
    }
    match app.mode {
        Mode::Login => draw_login(f, app),
        Mode::Main => draw_main(f, app),
    }
}

fn draw_login(f: &mut Frame, app: &App) {
    let th = app.config.theme();
    let area = centered(f.area(), 64, 14);
    let key = |k: &'static str| format!("  {k} ").fg(th.accent).bold();
    let mut lines = vec![
        Line::from("RymPlayer".fg(th.accent).bold()),
        Line::from(t("YouTube Music in your terminal").fg(th.dim)),
        Line::from(""),
        Line::from(vec![key("g"), t("Sign in with Google (opens a sign-in window)").fg(th.text)]),
        Line::from(vec![key("c"), t("Paste a browser cookie").fg(th.text)]),
        Line::from(vec![key("s"), t("Continue without an account").fg(th.text)]),
        Line::from(vec![key("m"), t("Songs on this device (no account needed)").fg(th.text)]),
        Line::from(vec![key("q"), t("Quit").fg(th.text)]),
        Line::from(""),
    ];
    if app.input == Some(Input::Cookie) {
        let shown = if app.buffer.is_empty() {
            t("paste with Ctrl+V, then Enter (Esc cancels)")
        } else {
            format!("{} characters pasted, press Enter", app.buffer.chars().count())
        };
        lines.push(Line::from(vec!["Cookie: ".fg(th.text).bold(), shown.fg(th.dim)]));
    }
    lines.push(Line::from(show(&app.status).fg(th.dim)));
    f.render_widget(Paragraph::new(lines).block(pane(th, i18n::tr("Sign in"), true)), area);
}

fn draw_main(f: &mut Frame, app: &mut App) {
    let th = app.config.theme();
    let cfg = app.config.clone();
    app.areas.buttons.clear();
    app.areas.popup = Rect::default();

    let bar_h = if cfg.help { 5 } else { 4 };
    let [top, rest] = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).areas(f.area());
    let (body, bar) = match cfg.player {
        BarPlace::Bottom => {
            let [b, p] = Layout::vertical([Constraint::Min(6), Constraint::Length(bar_h)]).areas(rest);
            (b, p)
        }
        BarPlace::Top => {
            let [p, b] = Layout::vertical([Constraint::Length(bar_h), Constraint::Min(6)]).areas(rest);
            (b, p)
        }
    };

    let w = Constraint::Length(cfg.library_width);
    let (nav, main) = match cfg.library {
        Side::Left => {
            let [n, m] = Layout::horizontal([w, Constraint::Min(30)]).areas(body);
            (Some(n), m)
        }
        Side::Right => {
            let [m, n] = Layout::horizontal([Constraint::Min(30), w]).areas(body);
            (Some(n), m)
        }
        Side::Hidden => (None, body),
    };

    let size = Constraint::Percentage(cfg.lyrics_size);
    let (tracks, lyr) = match cfg.lyrics {
        LyricsPlace::Right => {
            let [t, l] = Layout::horizontal([Constraint::Min(30), size]).areas(main);
            (t, Some(l))
        }
        LyricsPlace::Left => {
            let [l, t] = Layout::horizontal([size, Constraint::Min(30)]).areas(main);
            (t, Some(l))
        }
        LyricsPlace::Bottom => {
            let [t, l] = Layout::vertical([Constraint::Min(5), size]).areas(main);
            (t, Some(l))
        }
        LyricsPlace::Hidden => (main, None),
    };

    app.areas.nav = nav.unwrap_or_default();
    app.areas.tracks = tracks;
    draw_top(f, app, th, top);
    if let Some(n) = nav {
        draw_nav(f, app, th, n);
    }
    draw_tracks(f, app, th, tracks);
    if let Some(l) = lyr {
        draw_lyrics(f, app, th, l);
    }
    draw_bar(f, app, th, bar);
    if let Some(sel) = app.settings {
        draw_settings(f, app, th, sel);
    }
    if let Some(sel) = app.picker {
        draw_picker(f, app, th, sel);
    }
    if let Some(sel) = app.song_menu {
        draw_song_menu(f, app, th, sel);
    }
    if app.input == Some(Input::DownloadCount) {
        let area = centered(f.area(), 64, 3);
        popup_bg(f, th, area);
        let text = Line::from(vec![app.buffer.as_str().fg(th.text), "█".fg(th.accent)]);
        f.render_widget(Paragraph::new(text).block(pane(th, i18n::tr("Download how many liked songs? (empty = all, Enter / Esc)"), true)), area);
    }
    if app.input == Some(Input::NewPlaylist) {
        let area = centered(f.area(), 56, 3);
        popup_bg(f, th, area);
        let text = Line::from(vec![app.buffer.as_str().fg(th.text), "█".fg(th.accent)]);
        f.render_widget(Paragraph::new(text).block(pane(th, i18n::tr("New playlist name (Enter / Esc)"), true)), area);
    }
}

/// Clears a popup's area to the theme background.
fn popup_bg(f: &mut Frame, th: &Theme, area: Rect) {
    f.render_widget(Clear, area);
    if let Some(bg) = th.bg {
        f.render_widget(Block::default().style(Style::new().bg(bg)), area);
    }
}

/// The song menu (`o` or a right-click on a song).
fn draw_song_menu(f: &mut Frame, app: &mut App, th: &Theme, sel: usize) {
    let rows = app.song_menu_rows();
    let area = centered(f.area(), 44, rows.len() as u16 + 2);
    popup_bg(f, th, area);
    let title = app.menu_song.as_ref().map(|t| t.title.clone()).unwrap_or_default();
    let block = pane(th, &title, true).title_bottom(Line::from(format!(" {} ", t("Enter choose  Esc close")).fg(th.dim)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.areas.popup = area;
    for (i, (name, _)) in rows.iter().enumerate() {
        let r = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let style = if i == sel { cursor_style(th, true) } else { Style::new() };
        f.render_widget(Paragraph::new(Line::from(format!(" {}", show(name)).fg(th.text))).style(style), r);
        app.areas.buttons.push((r, Click::MenuItem(i)));
    }
}

/// "Add to playlist": a new one first, then the user's playlists.
fn draw_picker(f: &mut Frame, app: &mut App, th: &Theme, sel: usize) {
    let rows = app.picker_rows();
    let h = (rows.len() as u16 + 4).min(f.area().height.saturating_sub(4)).max(6);
    let area = centered(f.area(), 52, h);
    popup_bg(f, th, area);
    let song = app.pick_target().map(|t| t.title.clone()).unwrap_or_default();
    let title = format!("{}: {song}", i18n::tr("Add to playlist"));
    let block = pane(th, &title, true).title_bottom(Line::from(format!(" {} ", t("Enter add  Esc close")).fg(th.dim)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.areas.popup = area;
    // Scroll so the selected row stays visible.
    let first = sel.saturating_sub(inner.height.saturating_sub(1) as usize);
    for (row, (i, name)) in rows.iter().enumerate().skip(first).take(inner.height as usize).enumerate() {
        let r = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
        let style = if i == sel { cursor_style(th, true) } else { Style::new() };
        let color = if i == 0 { th.accent } else { th.text };
        f.render_widget(Paragraph::new(Line::from(format!(" {}", show(name)).fg(color))).style(style), r);
        app.areas.buttons.push((r, Click::Pick(i)));
    }
}

/// Logo and page buttons on the left, the search box in the middle, settings on the right.
fn draw_top(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let [left, mid, right] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(20), Constraint::Length(16)]).areas(area);
    let y = left.y + 1;

    let mut x = left.x + 1;
    let logo = "RymPlayer";
    f.render_widget(Paragraph::new(logo.fg(th.accent).bold()), Rect::new(x, y, logo.len() as u16, 1));
    x += logo.len() as u16 + 3;
    let buttons = [(t("★ For you"), Click::ForYou, Page::ForYou), (t("⏱ History"), Click::History, Page::History)];
    for (label, click, page) in buttons {
        let w = label.chars().count() as u16;
        if x + w > left.x + left.width {
            break;
        }
        let r = Rect::new(x, y, w, 1);
        let style = if app.page == page { Style::new().fg(th.accent).bold() } else { Style::new().fg(th.dim) };
        f.render_widget(Paragraph::new(Span::styled(label, style)), r);
        app.areas.buttons.push((r, click));
        x += w + 2;
    }

    // Search box, centered, at most 60 wide.
    let sw = mid.width.min(60);
    let search = Rect::new(mid.x + (mid.width - sw) / 2, mid.y, sw, 3);
    let active = app.input == Some(Input::Search);
    let text = if active {
        Line::from(vec![app.buffer.as_str().fg(th.text), "█".fg(th.accent)])
    } else {
        Line::from(vec![" ".into(), t("Search songs, artists...").fg(th.faint), "  /".fg(th.faint)])
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if active { th.accent } else { th.faint }));
    f.render_widget(Paragraph::new(text).block(block), search);
    app.areas.buttons.push((search, Click::Search));

    let label = t("⚙ Settings ,");
    let r = Rect::new(right.x + right.width.saturating_sub(label.chars().count() as u16 + 1), y, label.chars().count() as u16, 1);
    let style = if app.settings.is_some() { Style::new().fg(th.accent).bold() } else { Style::new().fg(th.dim) };
    f.render_widget(Paragraph::new(Span::styled(label.clone(), style)), r);
    app.areas.buttons.push((r, Click::Settings));
}

fn draw_nav(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let items: Vec<ListItem> = app
        .nav
        .iter()
        .map(|item| match item {
            NavItem::ForYou => ListItem::new(Line::from(vec!["★ ".fg(th.accent), t("For you").fg(th.text)])),
            NavItem::History => ListItem::new(Line::from(vec!["⏱ ".fg(th.accent), t("History").fg(th.text)])),
            NavItem::Local => ListItem::new(Line::from(vec!["♫ ".fg(th.accent), t("On this device").fg(th.text)])),
            NavItem::NewPlaylist => ListItem::new(Line::from(vec!["＋ ".fg(th.accent), t("New playlist").fg(th.dim)])),
            NavItem::Playlist(p) if p.id == crate::api::LIKED_ID => {
                ListItem::new(Line::from(vec!["♥ ".fg(th.accent), t("Liked songs").fg(th.text)]))
            }
            NavItem::Playlist(p) => ListItem::new(show(&p.title).fg(th.text)),
        })
        .collect();
    let focused = app.focus == Focus::Nav;
    let list = List::new(items)
        .block(pane(th, i18n::tr("Library"), focused))
        .highlight_style(cursor_style(th, focused))
        .highlight_symbol(if focused { "▶ " } else { "› " })
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(list, area, &mut app.nav_state);
}

fn draw_tracks(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let playing = app.now_playing().map(|t| t.video_id.clone());
    let rows: Vec<Row> = app
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_current = playing.as_deref() == Some(t.video_id.as_str());
            let title_style = if is_current { Style::new().fg(th.accent).bold() } else { Style::new().fg(th.text) };
            let mark = if app.liked.contains(&t.video_id) { "♥" } else { "" };
            // Home feed rows that open something show what they are instead of a length.
            let time = match &t.link {
                Some(Link::Playlist(_)) => "list".to_string(),
                Some(Link::Album(_)) => "album".to_string(),
                Some(Link::Mix(_)) => "mix".to_string(),
                None => t.duration.clone(),
            };
            Row::new(vec![
                if is_current {
                    Cell::from("♪").fg(th.accent).bold()
                } else {
                    Cell::from(format!("{}", i + 1)).fg(th.dim)
                },
                Cell::from(show(&t.title)).style(title_style),
                Cell::from(show(&t.artist)).fg(th.dim),
                Cell::from(show(&t.album)).fg(th.dim),
                Cell::from(mark).fg(th.accent),
                Cell::from(time).fg(th.dim),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(4),
        Constraint::Percentage(38),
        Constraint::Percentage(26),
        Constraint::Percentage(26),
        Constraint::Length(1),
        Constraint::Length(6),
    ];
    let focused = app.focus == Focus::Tracks;
    let mut block = pane(th, &app.page_title, focused);
    if app.busy {
        block = block.title_bottom(Line::from(format!(" {} ", t("Loading...")).fg(th.dim)));
    } else if !app.status.is_empty() {
        block = block.title_bottom(Line::from(format!(" {} ", show(&app.status)).fg(th.accent)));
    } else if app.tracks.is_empty() {
        block = block.title_bottom(Line::from(format!(" {} ", t("Press / to search")).fg(th.dim)));
    }
    let third = match app.page {
        Page::ForYou => t("Section"),
        Page::History => t("When"),
        _ => t("Album"),
    };
    let header = Row::new(["#".to_string(), t("Title"), t("Artist"), third, String::new(), t("Time")])
        .style(Style::new().fg(th.dim).add_modifier(Modifier::BOLD));
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(cursor_style(th, focused))
        .highlight_symbol(if focused { "▶ " } else { "› " })
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(table, area, &mut app.track_state);
}

/// Lyrics on the cover's color: lines to come dark, the sung line bright white and bold, sung
/// lines soft white; a blank line between lines so each stands apart.
fn draw_lyrics(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let rgb = |(r, g, b): (u8, u8, u8)| ratatui::style::Color::Rgb(r, g, b);
    let bg = app.lyrics_bg;
    let [sung, now, next] = rymplayer::colors::lyric_colors(bg).map(rgb);
    let inner_h = area.height.saturating_sub(2);
    let (lines, scroll): (Vec<Line>, u16) = match &app.lyrics {
        Some(l) if !l.synced.is_empty() => {
            let cur = lyrics::current_line(&l.synced, app.pos);
            let mut lines = Vec::new();
            for (i, (_, text)) in l.synced.iter().enumerate() {
                let text = if text.is_empty() { "♪".to_string() } else { show(text) };
                lines.push(match cur {
                    Some(c) if c == i => Line::from(text.bold().fg(now)),
                    Some(c) if i < c => Line::from(text.bold().fg(sung)),
                    _ => Line::from(text.bold().fg(next)),
                });
                lines.push(Line::default());
            }
            // Keep the sung line about a third of the way down.
            let scroll = (cur.unwrap_or(0) * 2).saturating_sub(inner_h as usize / 3) as u16;
            (lines, scroll)
        }
        Some(l) => {
            let mut lines = vec![Line::from(t("These lyrics aren't synced to the song").italic().fg(sung)), Line::default()];
            lines.extend(l.plain.iter().map(|line| Line::from(show(line).bold().fg(now))));
            (lines, 0)
        }
        None => (vec![Line::from(show(&app.lyrics_status).fg(sung))], 0),
    };
    let mut block = pane(th, i18n::tr("Lyrics"), false).style(Style::new().bg(rgb(bg)));
    if let Some(l) = &app.lyrics {
        block = block.title_bottom(Line::from(format!(" {} ", show(&format!("{} {}", i18n::tr("Lyrics from"), l.source))).fg(sung)));
    }
    let align = if app.config.lyrics_centered { Alignment::Center } else { Alignment::Left };
    f.render_widget(Paragraph::new(lines).block(block).alignment(align).scroll((scroll, 0)), area);
}

fn draw_bar(f: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let block = pane(th, i18n::tr("Now playing"), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let [info, gauge, help] = Layout::vertical([Constraint::Length(1); 3]).areas(inner);

    let icon = if app.loading {
        "…"
    } else if app.is_playing() {
        "▶"
    } else {
        "⏸"
    };
    let mut spans = match app.now_playing() {
        Some(t) => vec![
            format!(" {icon} ").fg(th.accent).bold(),
            show(&t.title).fg(th.text).bold(),
            "  ".into(),
            show(&t.artist).fg(th.dim),
        ],
        None => vec![format!(" {}", t("Nothing playing")).fg(th.dim)],
    };
    if let Some(t) = app.now_playing()
        && app.liked.contains(&t.video_id)
    {
        spans.push("  ♥".fg(th.accent));
    }
    if app.loading {
        spans.push(format!("  {}", t("downloading...")).fg(th.dim));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), info);

    let dur = app.duration();
    let ratio = if dur > 0.0 { (app.pos / dur).clamp(0.0, 1.0) as f64 } else { 0.0 };
    let label = format!("{} / {}", fmt_time(app.pos.min(dur)), fmt_time(dur));
    app.areas.gauge = gauge;
    app.areas.gauge_label = label.chars().count() as u16 + 1;
    f.render_widget(
        LineGauge::default()
            .ratio(ratio)
            .label(label.fg(th.text))
            .filled_style(Style::new().fg(th.accent))
            .unfilled_style(Style::new().fg(th.faint)),
        gauge,
    );

    if !app.config.help {
        return;
    }
    let vol = format!("{} {:.0}%", t("vol"), app.volume * 100.0);
    let keys = [
        ("space", "pause"),
        ("n/p", "next/prev"),
        ("←→ 0-9", "seek"),
        ("+/-", vol.as_str()),
        ("l", "like"),
        ("r", "radio"),
        ("d", "download"),
        ("/", "search"),
        ("h", "history"),
        ("F5/u", "refresh"),
        ("a", "add to playlist"),
        ("o", "song menu"),
        ("A", "artist"),
        ("D", "download all"),
        ("tab", "switch"),
        (",", "settings"),
        ("q", "quit"),
    ];
    let mut help_spans = Vec::new();
    for (k, v) in keys {
        let v = if v == vol { vol.clone() } else { show(i18n::tr(word(v))) };
        help_spans.push(format!(" {k} ").fg(th.accent));
        help_spans.push(format!("{v} ").fg(th.dim));
    }
    f.render_widget(Paragraph::new(Line::from(help_spans)), help);
}

fn draw_settings(f: &mut Frame, app: &mut App, th: &Theme, sel: usize) {
    let h = SETTINGS.len() as u16 + 6;
    let area = centered(f.area(), 58, h);
    popup_bg(f, th, area);
    let block = pane(th, i18n::tr("Settings"), true)
        .title_bottom(Line::from(format!(" {} ", t("↑↓ choose  ←→ change  click to change  Esc close")).fg(th.dim)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.areas.popup = area;

    // Theme swatches across the top: the accents of every theme, current one marked.
    let mut swatch = vec![" ".into()];
    for (i, t) in crate::config::THEMES.iter().enumerate() {
        let mark = if i == app.config.theme { "◆" } else { "●" };
        swatch.push(format!("{mark} ").fg(t.accent));
    }
    f.render_widget(Paragraph::new(Line::from(swatch)), Rect::new(inner.x, inner.y, inner.width, 1));

    for (i, s) in SETTINGS.iter().enumerate() {
        let r = Rect::new(inner.x, inner.y + 2 + i as u16, inner.width, 1);
        let selected = i == sel;
        let value = show(&app.config.value(*s));
        let label = format!(" {:<20}", show(Config::label(*s)));
        let line = Line::from(vec![
            label.fg(th.text),
            "‹ ".fg(th.faint),
            value.fg(th.accent).bold(),
            " ›".fg(th.faint),
        ]);
        let style = if selected { cursor_style(th, true) } else { Style::new() };
        f.render_widget(Paragraph::new(line).style(style), r);
        app.areas.buttons.push((r, Click::Setting(i)));
    }
}

/// The key hints' words as the static text the translations are keyed on.
fn word(v: &str) -> &'static str {
    ["pause", "next/prev", "seek", "like", "radio", "download", "search", "history", "refresh", "add to playlist", "song menu", "artist", "download all", "switch", "settings", "quit"]
        .into_iter()
        .find(|w| *w == v)
        .unwrap_or("")
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Playlist, Track};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn app() -> App {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(tx)
    }

    /// The main screen in Arabic, printed: `cargo test --bin RyM arabic_screen -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn arabic_screen() {
        rymplayer::i18n::set_arabic(true);
        let mut a = main_app();
        a.tracks[0].title = "أغنية عربية".into();
        a.tracks[0].artist = "فنان".into();
        let out = render(&mut a, 120, 24);
        rymplayer::i18n::set_arabic(false);
        println!("{out}");
        assert!(out.contains(&show("الرئيسية")) || out.contains(&show("المكتبة")));
    }

    fn main_app() -> App {
        let mut a = app();
        a.mode = Mode::Main;
        a.nav.push(NavItem::Playlist(Playlist { id: "LM".into(), title: "Liked songs".into(), thumb: None }));
        a.nav.push(NavItem::Playlist(Playlist { id: "VLx".into(), title: "Gym mix".into(), thumb: None }));
        a.page_title = "Liked songs".into();
        a.tracks = (1..=5)
            .map(|i| Track {
                video_id: format!("v{i}"),
                title: format!("Song number {i}"),
                artist: "Some Artist".into(),
                album: "Album".into(),
                duration: format!("3:3{i}"),
                thumb: None,
                album_id: None,
                artist_id: None,
                link: None,
            })
            .collect();
        a.track_state.select(Some(1));
        a.queue = a.tracks.clone();
        a.current = Some(1);
        a.liked.set("v2", true);
        a.lyrics = Some(lyrics::Lyrics {
            synced: lyrics::parse_lrc("[00:00.00]first line\n[00:05.00]second line\n[00:10.00]third line"),
            plain: vec![],
            source: "LRCLIB".into(),
        });
        a.pos = 6.0;
        a
    }

    #[test]
    fn login_screen() {
        let mut a = app();
        let out = render(&mut a, 100, 30);
        println!("{out}");
        assert!(out.contains("Sign in with Google"));
    }

    #[test]
    fn main_screen() {
        let mut a = main_app();
        let out = render(&mut a, 140, 30);
        println!("{out}");
        assert!(out.contains("Song number 2"));
        assert!(out.contains("second line"));
        assert!(out.contains("0:06 / 3:32"));
        assert!(out.contains("Search songs"));
        assert!(out.contains("History"));
    }

    #[test]
    fn playlist_picker_with_youtube_theme() {
        let mut a = main_app();
        a.config.theme = 1;
        a.playlists = vec![
            Playlist { id: "VLa".into(), title: "Gym mix".into(), thumb: None },
            Playlist { id: "VLb".into(), title: "Night drive".into(), thumb: None },
        ];
        a.pick_song = a.tracks.get(1).cloned();
        a.picker = Some(2);
        let mut term = Terminal::new(TestBackend::new(140, 30)).unwrap();
        term.draw(|f| draw(f, &mut a)).unwrap();
        let buf = term.backend().buffer().clone();
        let out = render(&mut a, 140, 30);
        println!("{out}");
        assert_eq!(a.config.theme().name, "YouTube Music");
        assert!(out.contains("Add to playlist: Song number 2"));
        assert!(out.contains("New playlist"));
        assert!(out.contains("Night drive"));
        // The theme paints its own background.
        assert_eq!(buf[(0, 29)].bg, a.config.theme().bg.unwrap());
    }

    #[test]
    fn rearranged_layout_and_settings() {
        let mut a = main_app();
        a.config.library = Side::Right;
        a.config.lyrics = LyricsPlace::Bottom;
        a.config.player = BarPlace::Top;
        a.config.help = false;
        a.settings = Some(3);
        let out = render(&mut a, 140, 36);
        println!("{out}");
        let lines: Vec<&str> = out.lines().collect();
        // Player bar right under the top bar, lyrics below the songs.
        assert!(lines[3].contains("Now playing"));
        let songs = lines.iter().position(|l| l.contains("Liked songs")).unwrap();
        let lyr = lines.iter().position(|l| l.contains("Lyrics")).unwrap();
        assert!(lyr > songs);
        assert!(out.contains("Lyrics panel"));
        assert!(out.contains("‹ Bottom ›"));
    }
}
