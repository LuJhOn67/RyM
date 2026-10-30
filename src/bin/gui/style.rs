//! Colors and widget styles. Light themes follow a clean desktop-player look; the dark ones
//! include YouTube Music and Spotify styles. Every theme can be tinted by the cover art.

use iced::widget::{button, container, scrollable, slider, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    pub light: bool,
    /// Sidebar rail and window background.
    pub bg: Color,
    /// Main content area.
    pub surface: Color,
    /// Rounded content panel (Spotify); 0 means flat.
    pub panel_radius: f32,
    pub bar: Color,
    pub hover: Color,
    pub selected: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    pub pill_bg: Color,
    pub pill_text: Color,
    pub input_bg: Color,
    pub divider: Color,
    pub menu: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// A light theme around one accent color.
const fn light(name: &'static str, accent: Color, bg: Color, selected: Color) -> Palette {
    Palette {
        name,
        light: true,
        bg,
        surface: rgb(255, 255, 255),
        panel_radius: 0.0,
        bar: rgb(255, 255, 255),
        hover: rgba(0, 0, 0, 0.045),
        selected,
        text: rgb(40, 44, 52),
        dim: rgb(128, 134, 145),
        accent,
        pill_bg: accent,
        pill_text: rgb(255, 255, 255),
        input_bg: rgba(0, 20, 60, 0.06),
        divider: rgba(0, 0, 0, 0.07),
        menu: rgb(255, 255, 255),
    }
}

pub const PALETTES: [Palette; 6] = [
    light("Sky", rgb(66, 133, 196), rgb(236, 242, 248), rgba(66, 133, 196, 0.13)),
    light("Mint", rgb(56, 166, 117), rgb(236, 246, 240), rgba(56, 166, 117, 0.13)),
    light("Sakura", rgb(222, 94, 140), rgb(250, 240, 244), rgba(222, 94, 140, 0.12)),
    Palette {
        name: "Midnight",
        light: false,
        bg: rgb(14, 18, 25),
        surface: rgb(20, 26, 35),
        panel_radius: 0.0,
        bar: rgb(17, 22, 30),
        hover: rgba(255, 255, 255, 0.06),
        selected: rgba(90, 160, 255, 0.16),
        text: rgb(230, 236, 244),
        dim: rgb(140, 152, 170),
        accent: rgb(96, 166, 255),
        pill_bg: rgb(96, 166, 255),
        pill_text: rgb(10, 14, 20),
        input_bg: rgba(255, 255, 255, 0.07),
        divider: rgba(255, 255, 255, 0.07),
        menu: rgb(30, 38, 50),
    },
    Palette {
        name: "YouTube Music",
        light: false,
        bg: rgb(3, 3, 3),
        surface: rgb(15, 14, 13),
        panel_radius: 0.0,
        bar: rgb(33, 33, 33),
        hover: rgba(255, 255, 255, 0.10),
        selected: rgba(255, 255, 255, 0.16),
        text: rgb(255, 255, 255),
        dim: rgb(170, 170, 170),
        accent: rgb(255, 0, 0),
        pill_bg: rgb(255, 255, 255),
        pill_text: rgb(3, 3, 3),
        input_bg: rgba(255, 255, 255, 0.10),
        divider: rgba(255, 255, 255, 0.10),
        menu: rgb(58, 52, 49),
    },
    Palette {
        name: "Spotify",
        light: false,
        bg: rgb(0, 0, 0),
        surface: rgb(18, 18, 18),
        panel_radius: 8.0,
        bar: rgb(0, 0, 0),
        hover: rgb(31, 31, 31),
        selected: rgb(42, 42, 42),
        text: rgb(255, 255, 255),
        dim: rgb(179, 179, 179),
        accent: rgb(30, 215, 96),
        pill_bg: rgb(30, 215, 96),
        pill_text: rgb(0, 0, 0),
        input_bg: rgb(36, 36, 36),
        divider: rgb(40, 40, 40),
        menu: rgb(40, 40, 40),
    },
];

pub fn palette(i: usize) -> Palette {
    PALETTES[i % PALETTES.len()]
}

pub fn theme(p: Palette) -> Theme {
    Theme::custom(
        p.name,
        iced::theme::Palette {
            background: p.bg,
            text: p.text,
            primary: p.accent,
            success: rgb(46, 204, 113),
            warning: rgb(243, 156, 18),
            danger: rgb(231, 76, 60),
        },
    )
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

/// Background washed with the cover's color: soft, never loud. `strength` 0..1.
pub fn tinted(p: Palette, base: Color, tint: Option<Color>, strength: f32) -> Color {
    match tint {
        Some(t) => mix(base, t, (if p.light { 0.16 } else { 0.24 }) * strength),
        None => base,
    }
}

/// Wash behind the content and the player page. A flat color: gradients are costly to paint
/// in software on large screens, and every scroll repaints the content area.
pub fn wash(p: Palette, tint: Option<Color>, strength: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(tinted(p, p.surface, tint, strength * 0.7))),
        border: Border { radius: p.panel_radius.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn bg(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(Background::Color(color)), ..Default::default() }
}

pub fn card_bg(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Text-only button that lights up on hover; `active` keeps it lit (current page).
pub fn ghost(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            _ if active => Some(Background::Color(p.selected)),
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.hover)),
            _ => None,
        };
        button::Style {
            background,
            text_color: p.text,
            border: Border { radius: 8.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Sidebar rail icon: accent tint when it's the current page.
pub fn rail(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            _ if active => Some(Background::Color(p.selected)),
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.hover)),
            _ => None,
        };
        button::Style {
            background,
            text_color: p.text,
            border: Border { radius: 10.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Accent rounded "Play" button.
pub fn pill(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let mut bg = p.pill_bg;
        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            bg = Color { a: 0.85, ..bg };
        }
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.pill_text,
            border: Border { radius: 999.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Soft rounded button (chips, "New playlist").
pub fn chip(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.selected,
            _ => p.input_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text,
            border: Border { radius: 999.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Round icon button.
pub fn icon_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(p.hover)),
        text_color: p.text,
        border: Border { radius: 999.0.into(), ..Default::default() },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// List rows and cards: transparent, hover tint; `current` marks the playing song.
pub fn row(p: Palette, current: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.hover)),
            _ if current => Some(Background::Color(p.selected)),
            _ => None,
        };
        button::Style {
            background,
            text_color: p.text,
            border: Border { radius: 8.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Right-click menu row: square, lighter on hover.
pub fn menu_item(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(p.hover)),
        text_color: p.text,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Rounded search field background; the input inside it is bare (see [`bare_input`]).
pub fn search_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.input_bg)),
        border: Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn bare_input(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, _| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.dim,
        placeholder: p.dim,
        value: p.text,
        selection: Color { a: 0.3, ..p.accent },
    }
}

pub fn field(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: Background::Color(p.input_bg),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: if matches!(status, text_input::Status::Focused { .. }) { p.accent } else { Color::TRANSPARENT },
        },
        icon: p.dim,
        placeholder: p.dim,
        value: p.text,
        selection: Color { a: 0.3, ..p.accent },
    }
}

/// Thin progress / volume bar with a small round handle.
pub fn seek(p: Palette) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let active = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        let rest = if p.light { rgba(0, 0, 0, 0.10) } else { rgba(255, 255, 255, 0.22) };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (Background::Color(p.accent), Background::Color(rest)),
                width: if active { 5.0 } else { 3.0 },
                border: Border { radius: 2.0.into(), ..Default::default() },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: if active { 7.0 } else { 0.0 } },
                background: Background::Color(p.accent),
                border_width: if p.light { 2.0 } else { 0.0 },
                border_color: rgb(255, 255, 255),
            },
        }
    }
}

/// Slim scrollbars that only show their thumb.
pub fn scroll(p: Palette) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut s = scrollable::default(theme, status);
        let thumb = if p.light { rgba(0, 0, 0, 0.16) } else { rgba(255, 255, 255, 0.18) };
        for rail in [&mut s.vertical_rail, &mut s.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(thumb);
            rail.scroller.border = Border { radius: 4.0.into(), ..Default::default() };
        }
        s.container.background = None;
        s
    }
}

pub fn shadow(p: Palette) -> Shadow {
    Shadow {
        color: if p.light { rgba(30, 50, 80, 0.18) } else { rgba(0, 0, 0, 0.55) },
        offset: iced::Vector::new(0.0, 8.0),
        blur_radius: 24.0,
    }
}

/// Scrollbars on a colored panel (the lyrics).
pub fn scroll_on(_bg: Color) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut s = scrollable::default(theme, status);
        for rail in [&mut s.vertical_rail, &mut s.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(rgba(255, 255, 255, 0.3));
            rail.scroller.border = Border { radius: 4.0.into(), ..Default::default() };
        }
        s.container.background = None;
        s
    }
}

/// A button that looks like its content alone (clickable lyrics lines).
pub fn invisible() -> impl Fn(&Theme, button::Status) -> button::Style {
    |_, _| button::Style { background: None, ..button::Style::default() }
}
