//! Synced lyrics that light up as they're sung.
//!
//! The line being sung glows in the cover's color and a shine runs across it when it's reached
//! (right to left for Arabic and other right-to-left text); the lyrics glide to keep it in view.
//! The widget moves itself between app updates from the playback position it was given, so the
//! rest of the window isn't rebuilt every frame. Clicking a line jumps there; the mouse wheel
//! looks around and the view drifts back after a moment.

use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use iced::advanced::graphics::text::Paragraph;
use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as core_text, Paragraph as _, Renderer as _};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::renderer::Renderer as _;
use iced::advanced::{Clipboard, Shell, Widget, mouse, renderer};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector, alignment, window};

const GAP: f32 = 20.0;
const SIDE_PAD: f32 = 14.0;
/// Where the sung line sits, as a share of the height from the top.
const ANCHOR: f32 = 0.38;
const GLIDE_MS: f32 = 650.0;
const SHINE_MS: f32 = 1100.0;
const FLASH_MS: f32 = 700.0;
/// After scrolling by hand, the view waits this long before drifting back.
const LOOK_AROUND: Duration = Duration::from_millis(2500);

pub struct Colors {
    /// The sung line.
    pub lit: Color,
    /// Other lines (made fainter the further they are).
    pub dim: Color,
    /// The halo around the sung line.
    pub glow: Color,
    /// The shine running across it.
    pub shine: Color,
}

pub struct LyricsView<'a, Message> {
    lines: &'a [(f32, String)],
    /// Playback position, when it was read, and whether time is moving.
    pos: f32,
    at: Instant,
    playing: bool,
    colors: Colors,
    size: f32,
    font: Font,
    on_seek: fn(f32) -> Message,
}

pub fn lyrics_view<'a, Message>(
    lines: &'a [(f32, String)],
    pos: f32,
    at: Instant,
    playing: bool,
    colors: Colors,
    size: f32,
    font: Font,
    on_seek: fn(f32) -> Message,
) -> LyricsView<'a, Message> {
    LyricsView { lines, pos, at, playing, colors, size, font, on_seek }
}

/// One shaped lyric line and its visual rows (a long line wraps into several).
struct Line {
    paragraph: Paragraph,
    /// Horizontal middle of the text inside the paragraph.
    middle: f32,
    height: f32,
    /// Each row: top, height, left edge, width, right-to-left.
    rows: Vec<(f32, f32, f32, f32, bool)>,
}

struct State {
    key: u64,
    width: f32,
    lines: Vec<Line>,
    /// Top of each line in content coordinates.
    tops: Vec<f32>,
    current: Option<usize>,
    changed: Instant,
    glide_from: f32,
    glide_to: f32,
    glide_since: Instant,
    manual: f32,
    manual_at: Instant,
    hovered: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        let long_ago = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now);
        State {
            key: 0,
            width: 0.0,
            lines: Vec::new(),
            tops: Vec::new(),
            current: None,
            changed: long_ago,
            glide_from: 0.0,
            glide_to: 0.0,
            glide_since: long_ago,
            manual: 0.0,
            manual_at: long_ago,
            hovered: None,
        }
    }
}

fn ease(since: Instant, ms: f32) -> f32 {
    let t = (since.elapsed().as_secs_f32() * 1000.0 / ms).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn fade(c: Color, a: f32) -> Color {
    Color { a: c.a * a.clamp(0.0, 1.0), ..c }
}

impl State {
    fn glide(&self) -> f32 {
        self.glide_from + (self.glide_to - self.glide_from) * ease(self.glide_since, GLIDE_MS)
    }

    fn content_height(&self) -> f32 {
        self.tops.last().zip(self.lines.last()).map_or(0.0, |(t, l)| t + l.height)
    }

    /// Scroll offset that puts line `i` at the anchor.
    fn target(&self, i: Option<usize>, height: f32) -> f32 {
        let i = i.unwrap_or(0).min(self.lines.len().saturating_sub(1));
        match (self.tops.get(i), self.lines.get(i)) {
            (Some(top), Some(l)) => top + l.height / 2.0 - height * ANCHOR,
            _ => 0.0,
        }
    }

    fn offset(&self) -> f32 {
        self.glide() + self.manual
    }

    fn line_at(&self, y: f32) -> Option<usize> {
        self.tops.iter().zip(&self.lines).position(|(top, l)| y >= top - GAP / 2.0 && y < top + l.height + GAP / 2.0)
    }
}

impl<Message> LyricsView<'_, Message> {
    fn now_pos(&self) -> f32 {
        if self.playing { self.pos + self.at.elapsed().as_secs_f32() } else { self.pos }
    }

    fn key(&self) -> u64 {
        let mut h = std::hash::DefaultHasher::new();
        for (t, s) in self.lines {
            t.to_bits().hash(&mut h);
            s.hash(&mut h);
        }
        self.size.to_bits().hash(&mut h);
        h.finish()
    }

    fn shape(&self, state: &mut State, width: f32) {
        let wrap = (width - 2.0 * SIDE_PAD).max(40.0);
        state.lines = self
            .lines
            .iter()
            .map(|(_, s)| {
                let content = if s.trim().is_empty() { "♪" } else { s.as_str() };
                let paragraph = Paragraph::with_text(core_text::Text {
                    content,
                    bounds: Size::new(wrap, f32::INFINITY),
                    size: Pixels(self.size),
                    line_height: LineHeight::Relative(1.25),
                    font: self.font,
                    align_x: core_text::Alignment::Center,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::WordOrGlyph,
                });
                let rows: Vec<(f32, f32, f32, f32, bool)> = paragraph
                    .buffer()
                    .layout_runs()
                    .map(|run| {
                        let left = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                        let right = run.glyphs.iter().map(|g| g.x + g.w).fold(f32::NEG_INFINITY, f32::max);
                        let (left, w) = if left.is_finite() { (left, right - left) } else { (0.0, 0.0) };
                        (run.line_top, run.line_height, left, w, run.rtl)
                    })
                    .collect();
                let height = paragraph.min_bounds().height;
                // Middle of the letters themselves: right-to-left rows don't start at 0.
                let lo = rows.iter().map(|r| r.2).fold(f32::INFINITY, f32::min);
                let hi = rows.iter().map(|r| r.2 + r.3).fold(f32::NEG_INFINITY, f32::max);
                let middle = if lo.is_finite() { (lo + hi) / 2.0 } else { 0.0 };
                Line { paragraph, middle, height, rows }
            })
            .collect();
        let mut y = 0.0;
        state.tops = state
            .lines
            .iter()
            .map(|l| {
                let top = y;
                y += l.height + GAP;
                top
            })
            .collect();
        state.width = width;
    }
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for LyricsView<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        let size = limits.max();
        let state = tree.state.downcast_mut::<State>();
        let key = self.key();
        if key != state.key || (size.width - state.width).abs() > 0.5 {
            let fresh = key != state.key;
            state.key = key;
            self.shape(state, size.width);
            if fresh {
                // New song: place it without gliding.
                state.current = rymplayer::lyrics::current_line(self.lines, self.now_pos());
                let t = state.target(state.current, size.height);
                state.glide_from = t;
                state.glide_to = t;
                state.manual = 0.0;
            } else {
                state.glide_to = state.target(state.current, size.height);
            }
        }
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let pos = self.now_pos();
                let cur = rymplayer::lyrics::current_line(self.lines, pos);
                if cur != state.current {
                    state.current = cur;
                    state.changed = Instant::now();
                    state.glide_from = state.glide();
                    state.glide_to = state.target(cur, bounds.height);
                    state.glide_since = Instant::now();
                }
                if state.manual != 0.0 && state.manual_at.elapsed() > LOOK_AROUND {
                    state.glide_from = state.offset();
                    state.glide_since = Instant::now();
                    state.manual = 0.0;
                }
                let since = state.changed.elapsed().as_secs_f32() * 1000.0;
                let moving = since < SHINE_MS.max(GLIDE_MS) || state.glide_since.elapsed().as_secs_f32() * 1000.0 < GLIDE_MS;
                if moving {
                    shell.request_redraw();
                } else if self.playing {
                    // Sleep until the next line starts.
                    let next = cur.map_or(0, |c| c + 1);
                    if let Some((start, _)) = self.lines.get(next) {
                        let wait = (start - pos).max(0.0) + 0.005;
                        shell.request_redraw_at(window::RedrawRequest::At(Instant::now() + Duration::from_secs_f32(wait)));
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                let max = (state.content_height() - bounds.height * ANCHOR).max(0.0);
                let wanted = (state.offset() - dy).clamp(-bounds.height * ANCHOR, max);
                state.manual = wanted - state.glide();
                state.manual_at = Instant::now();
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let hovered = cursor.position_in(bounds).and_then(|p| state.line_at(p.y + state.offset()));
                if hovered != state.hovered {
                    state.hovered = hovered;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds)
                    && let Some(i) = state.line_at(p.y + state.offset())
                    && let Some((start, _)) = self.lines.get(i)
                {
                    state.manual = 0.0;
                    shell.publish((self.on_seek)(*start));
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, _viewport: &Rectangle, _renderer: &iced::Renderer) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        match cursor.position_in(layout.bounds()) {
            Some(p) if state.line_at(p.y + state.offset()).is_some() => mouse::Interaction::Pointer,
            _ => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let offset = state.offset();
        let cur = state.current;
        let lit_k = ease(state.changed, 280.0);
        // The halo flares when the line is reached, then settles.
        let flash = (state.changed.elapsed().as_secs_f32() * 1000.0 / FLASH_MS).clamp(0.0, 1.0);
        let glow = 0.6 + 0.4 * (1.0 - flash) * (1.0 - flash);
        let shine_t = (state.changed.elapsed().as_secs_f32() * 1000.0 / SHINE_MS).clamp(0.0, 1.0);
        let c = &self.colors;

        renderer.with_layer(bounds, |r| {
            for (i, (line, top)) in state.lines.iter().zip(&state.tops).enumerate() {
                let y = bounds.y + top - offset;
                if y > bounds.y + bounds.height || y + line.height < bounds.y {
                    continue;
                }
                // Lines melt away near the top and bottom edges.
                let center = y + line.height / 2.0;
                let edge = ((center - bounds.y).min(bounds.y + bounds.height - center) / 70.0).clamp(0.0, 1.0);
                let left_edge = bounds.center_x() - line.middle;
                let at = Point::new(left_edge, y);
                let paint = |r: &mut iced::Renderer, color: Color, clip: Rectangle| r.fill_paragraph(&line.paragraph, at, color, clip);

                if Some(i) != cur {
                    let far = cur.map_or(2, |c| c.abs_diff(i)) as f32;
                    let mut a = (0.62 - 0.1 * far).max(0.28);
                    if state.hovered == Some(i) {
                        a += 0.3;
                    }
                    // The line just left fades out instead of switching off.
                    if cur.is_some_and(|c| c == i + 1) {
                        a += (1.0 - lit_k) * (1.0 - a);
                    }
                    paint(r, fade(c.dim, a * edge), bounds);
                    continue;
                }

                // Glow: the text drawn faintly in rings around itself.
                for (radius, alpha) in [(1.5f32, 0.16f32), (3.0, 0.09), (5.0, 0.05)] {
                    for k in 0..8 {
                        let angle = k as f32 * std::f32::consts::FRAC_PI_4 + radius * 0.3;
                        let shift = Vector::new(angle.cos() * radius, angle.sin() * radius);
                        r.fill_paragraph(&line.paragraph, at + shift, fade(c.glow, alpha * glow * lit_k * edge), bounds);
                    }
                }
                paint(r, fade(crate::style::mix(c.dim, c.lit, lit_k), edge), bounds);

                // Shine: a soft bright band crossing the rows in reading order.
                if shine_t < 1.0 {
                    let total: f32 = line.rows.iter().map(|row| row.3).sum();
                    let band = 70.0;
                    let head = shine_t * (total + band * 2.0) - band;
                    let mut before = 0.0;
                    for &(row_top, row_h, left, w, rtl) in &line.rows {
                        let row_y = y + row_top;
                        for (k, alpha) in [0.25f32, 0.55, 0.9, 0.55, 0.25].iter().enumerate() {
                            let from = head + k as f32 * band / 5.0 - before;
                            let (a, b) = (from.max(0.0), (from + band / 5.0).min(w));
                            if b <= a {
                                continue;
                            }
                            let x = if rtl { left_edge + left + w - b } else { left_edge + left + a };
                            let clip = Rectangle { x, y: row_y, width: b - a, height: row_h };
                            if let Some(clip) = clip.intersection(&bounds) {
                                paint(r, fade(c.shine, alpha * edge * (1.0 - shine_t * 0.3)), clip);
                            }
                        }
                        before += w;
                    }
                }
            }
        });
    }
}

impl<'a, Message: 'a, Theme: 'a> From<LyricsView<'a, Message>> for Element<'a, Message, Theme, iced::Renderer> {
    fn from(v: LyricsView<'a, Message>) -> Self {
        Element::new(v)
    }
}
