//! Terminal drawing. Reads `App` state; all input handling lives in main.rs.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, HighlightSpacing, LineGauge, List, ListItem, Paragraph, Row, Table};

use crate::{App, Focus, Input, Mode, NavItem, fmt_time, lyrics};

const ACCENT: Color = Color::Rgb(255, 0, 51);
const DIM: Color = Color::Rgb(140, 140, 140);
const FAINT: Color = Color::Rgb(90, 90, 90);

/// Selected row: bright red in the pane you're in, grey in the other one.
fn cursor_style(focused: bool) -> Style {
    if focused {
        Style::new().bg(Color::Rgb(150, 0, 30)).fg(Color::White).bold()
    } else {
        Style::new().bg(Color::Rgb(50, 50, 50))
    }
}

fn pane(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { FAINT }))
        .title(Span::styled(format!(" {title} "), Style::new().bold()))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    match app.mode {
        Mode::Login => draw_login(f, app),
        Mode::Main => draw_main(f, app),
    }
}

fn draw_login(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 64, 13);
    let mut lines = vec![
        Line::from("YTM Player".fg(ACCENT).bold()),
        Line::from("YouTube Music in your terminal".fg(DIM)),
        Line::from(""),
        Line::from(vec!["  g ".fg(ACCENT).bold(), "Sign in with Google (opens a sign-in window)".into()]),
        Line::from(vec!["  c ".fg(ACCENT).bold(), "Paste a browser cookie".into()]),
        Line::from(vec!["  s ".fg(ACCENT).bold(), "Continue without an account".into()]),
        Line::from(vec!["  q ".fg(ACCENT).bold(), "Quit".into()]),
        Line::from(""),
    ];
    if app.input == Some(Input::Cookie) {
        let shown = if app.buffer.is_empty() {
            "paste with Ctrl+V, then Enter (Esc cancels)".to_string()
        } else {
            format!("{} characters pasted, press Enter", app.buffer.chars().count())
        };
        lines.push(Line::from(vec!["Cookie: ".bold(), shown.fg(DIM)]));
    }
    lines.push(Line::from(app.status.clone().fg(DIM)));
    f.render_widget(Paragraph::new(lines).block(pane("Sign in", true)), area);
}

fn draw_main(f: &mut Frame, app: &mut App) {
    let [body, bar] = Layout::vertical([Constraint::Min(8), Constraint::Length(5)]).areas(f.area());
    let [nav, tracks, lyr] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(40), Constraint::Percentage(30)]).areas(body);
    draw_nav(f, app, nav);
    draw_tracks(f, app, tracks);
    draw_lyrics(f, app, lyr);
    draw_bar(f, app, bar);
    if app.input == Some(Input::Search) {
        let area = centered(f.area(), 60, 3);
        f.render_widget(Clear, area);
        let text = Line::from(vec![app.buffer.as_str().into(), "█".fg(ACCENT)]);
        f.render_widget(Paragraph::new(text).block(pane("Search (Enter / Esc)", true)), area);
    }
}

fn draw_nav(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .nav
        .iter()
        .map(|item| match item {
            NavItem::Search => ListItem::new("Search"),
            NavItem::Playlist(p) if p.id == crate::api::LIKED_ID => ListItem::new(Line::from(vec!["♥ ".fg(ACCENT), p.title.clone().into()])),
            NavItem::Playlist(p) => ListItem::new(p.title.clone()),
        })
        .collect();
    let list = List::new(items)
        .block(pane("Library", app.focus == Focus::Nav))
        .highlight_style(cursor_style(app.focus == Focus::Nav))
        .highlight_symbol(if app.focus == Focus::Nav { "▶ " } else { "› " })
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(list, area, &mut app.nav_state);
}

fn draw_tracks(f: &mut Frame, app: &mut App, area: Rect) {
    let playing = app.now_playing().map(|t| t.video_id.clone());
    let rows: Vec<Row> = app
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_current = playing.as_deref() == Some(t.video_id.as_str());
            let title_style = if is_current { Style::new().fg(ACCENT).bold() } else { Style::new() };
            let mark = if app.liked.contains(&t.video_id) { "♥" } else { "" };
            Row::new(vec![
                if is_current { Cell::from("♪").fg(ACCENT).bold() } else { Cell::from(format!("{}", i + 1)).fg(DIM) },
                Cell::from(t.title.clone()).style(title_style),
                Cell::from(t.artist.clone()).fg(DIM),
                Cell::from(t.album.clone()).fg(DIM),
                Cell::from(mark).fg(ACCENT),
                Cell::from(t.duration.clone()).fg(DIM),
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
    let mut block = pane(&app.page_title, app.focus == Focus::Tracks);
    if app.busy {
        block = block.title_bottom(Line::from(" Loading... ".fg(DIM)));
    } else if !app.status.is_empty() {
        block = block.title_bottom(Line::from(format!(" {} ", app.status).fg(ACCENT)));
    } else if app.tracks.is_empty() {
        block = block.title_bottom(Line::from(" Press / to search ".fg(DIM)));
    }
    let header = Row::new(["#", "Title", "Artist", "Album", "", "Time"]).style(Style::new().fg(DIM).add_modifier(Modifier::BOLD));
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(cursor_style(app.focus == Focus::Tracks))
        .highlight_symbol(if app.focus == Focus::Tracks { "▶ " } else { "› " })
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(table, area, &mut app.track_state);
}

fn draw_lyrics(f: &mut Frame, app: &App, area: Rect) {
    let block = pane("Lyrics", false);
    let inner_h = area.height.saturating_sub(2);
    let (lines, scroll): (Vec<Line>, u16) = match &app.lyrics {
        Some(l) if !l.synced.is_empty() => {
            let cur = lyrics::current_line(&l.synced, app.pos);
            let lines = l
                .synced
                .iter()
                .enumerate()
                .map(|(i, (_, text))| {
                    let text = if text.is_empty() { "♪".to_string() } else { text.clone() };
                    match cur {
                        Some(c) if c == i => Line::from(text.bold().fg(Color::White)),
                        Some(c) if i < c => Line::from(text.fg(FAINT)),
                        _ => Line::from(text.fg(DIM)),
                    }
                })
                .collect();
            // Keep the sung line about a third of the way down.
            let scroll = cur.unwrap_or(0).saturating_sub(inner_h as usize / 3) as u16;
            (lines, scroll)
        }
        Some(l) => (l.plain.iter().map(|t| Line::from(t.clone())).collect(), 0),
        None => (vec![Line::from(app.lyrics_status.clone().fg(DIM))], 0),
    };
    let mut block = block;
    if let Some(l) = &app.lyrics {
        block = block.title_bottom(Line::from(format!(" {} ", l.source).fg(FAINT)));
    }
    f.render_widget(Paragraph::new(lines).block(block).alignment(Alignment::Center).scroll((scroll, 0)), area);
}

fn draw_bar(f: &mut Frame, app: &App, area: Rect) {
    let block = pane("Now playing", false);
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
            format!(" {icon} ").fg(ACCENT).bold(),
            t.title.clone().bold(),
            "  ".into(),
            t.artist.clone().fg(DIM),
        ],
        None => vec![" Nothing playing".fg(DIM)],
    };
    if let Some(t) = app.now_playing()
        && app.liked.contains(&t.video_id)
    {
        spans.push("  ♥".fg(ACCENT));
    }
    if app.loading {
        spans.push("  downloading...".fg(DIM));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), info);

    let dur = app.duration();
    let ratio = if dur > 0.0 { (app.pos / dur).clamp(0.0, 1.0) as f64 } else { 0.0 };
    let label = format!("{} / {}", fmt_time(app.pos.min(dur)), fmt_time(dur));
    f.render_widget(
        LineGauge::default()
            .ratio(ratio)
            .label(label)
            .filled_style(Style::new().fg(ACCENT))
            .unfilled_style(Style::new().fg(FAINT)),
        gauge,
    );

    let vol = format!("vol {:.0}%", app.volume * 100.0);
    let keys = [
        ("space", "pause"),
        ("n/p", "next/prev"),
        ("←→", "seek"),
        ("+/-", vol.as_str()),
        ("l", "like"),
        ("/", "search"),
        ("tab", "switch"),
        ("g", "google sign-in"),
        ("O", "sign out"),
        ("q", "quit"),
    ];
    let mut help_spans = Vec::new();
    for (k, v) in keys {
        help_spans.push(format!(" {k} ").fg(ACCENT));
        help_spans.push(format!("{v} ").fg(DIM));
    }
    f.render_widget(Paragraph::new(Line::from(help_spans)), help);
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

    #[test]
    fn login_screen() {
        let mut a = app();
        let out = render(&mut a, 100, 30);
        println!("{out}");
        assert!(out.contains("Sign in with Google"));
    }

    #[test]
    fn main_screen() {
        let mut a = app();
        a.mode = Mode::Main;
        a.nav.push(NavItem::Playlist(Playlist { id: "LM".into(), title: "Liked songs".into() }));
        a.nav.push(NavItem::Playlist(Playlist { id: "VLx".into(), title: "Gym mix".into() }));
        a.page_title = "Liked songs".into();
        a.tracks = (1..=5)
            .map(|i| Track {
                video_id: format!("v{i}"),
                title: format!("Song number {i}"),
                artist: "Some Artist".into(),
                album: "Album".into(),
                duration: "3:3{i}".replace("{i}", &i.to_string()),
            })
            .collect();
        a.track_state.select(Some(1));
        a.queue = a.tracks.clone();
        a.current = Some(1);
        a.liked.insert("v2".into());
        a.lyrics = Some(lyrics::Lyrics {
            synced: lyrics::parse_lrc("[00:00.00]first line\n[00:05.00]second line\n[00:10.00]third line"),
            plain: vec![],
            source: "LRCLIB".into(),
        });
        a.pos = 6.0;
        let out = render(&mut a, 140, 30);
        println!("{out}");
        assert!(out.contains("Song number 2"));
        assert!(out.contains("second line"));
        assert!(out.contains("0:06 / 3:32"));
    }
}
