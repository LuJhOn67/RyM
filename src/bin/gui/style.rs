//! Colors and widget styles for the two looks: YouTube Music and Spotify.

use iced::widget::{button, container, scrollable, slider, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    /// Window background (sidebar and top bar sit on it).
    pub bg: Color,
    /// Main content area.
    pub surface: Color,
    /// Rounded panels around the sidebar and content (Spotify); 0 radius means flat (YouTube).
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
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

pub const PALETTES: [Palette; 2] = [
    Palette {
        name: "YouTube Music",
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
    },
    Palette {
        name: "Spotify",
        bg: rgb(0, 0, 0),
        surface: rgb(18, 18, 18),
        panel_radius: 8.0,
        bar: rgb(0, 0, 0),
        hover: rgb(31, 31, 31),
        selected: rgb(42, 42, 42),
        text: rgb(255, 255, 255),
        dim: rgb(179, 179, 179),
        accent: rgb(30, 215, 96),
        pill_bg: rgb(255, 255, 255),
        pill_text: rgb(0, 0, 0),
        input_bg: rgb(36, 36, 36),
        divider: rgb(40, 40, 40),
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

pub fn bg(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(Background::Color(color)), ..Default::default() }
}

pub fn panel(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface)),
        border: Border { radius: p.panel_radius.into(), ..Default::default() },
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

/// White rounded "Play" / "New playlist" button.
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

/// Dark rounded button (sidebar "New playlist", chips).
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

/// Round icon button; the big play button uses `filled`.
pub fn icon_button(p: Palette, filled: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if filled {
            Some(Background::Color(if hovered { Color { a: 0.85, ..p.pill_bg } } else { p.pill_bg }))
        } else if hovered {
            Some(Background::Color(p.hover))
        } else {
            None
        };
        button::Style {
            background,
            text_color: if filled { p.pill_text } else { p.text },
            border: Border { radius: 999.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
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
            border: Border { radius: 6.0.into(), ..Default::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn search(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: Background::Color(p.input_bg),
            border: Border {
                radius: if p.panel_radius > 0.0 { 999.0 } else { 8.0 }.into(),
                width: 1.0,
                color: if focused { p.dim } else { p.divider },
            },
            icon: p.dim,
            placeholder: p.dim,
            value: p.text,
            selection: Color { a: 0.4, ..p.accent },
        }
    }
}

/// Rounded search field background; the input inside it is bare (see [`bare_input`]).
pub fn search_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.input_bg)),
        border: Border {
            radius: if p.panel_radius > 0.0 { 999.0 } else { 8.0 }.into(),
            width: 1.0,
            color: p.divider,
        },
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
        selection: Color { a: 0.4, ..p.accent },
    }
}

/// Thin progress / volume bar with a small round handle, YouTube-red or Spotify-green.
pub fn seek(p: Palette) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let active = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        slider::Style {
            rail: slider::Rail {
                backgrounds: (Background::Color(p.accent), Background::Color(rgba(255, 255, 255, 0.25))),
                width: if active { 5.0 } else { 3.0 },
                border: Border { radius: 2.0.into(), ..Default::default() },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: if active { 7.0 } else { 0.0 } },
                background: Background::Color(if p.panel_radius > 0.0 { p.text } else { p.accent }),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    }
}

/// Slim, dark scrollbars that only show their thumb.
pub fn scroll(p: Palette) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut s = scrollable::default(theme, status);
        for rail in [&mut s.vertical_rail, &mut s.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(rgba(255, 255, 255, 0.18));
            rail.scroller.border = Border { radius: 4.0.into(), ..Default::default() };
        }
        s.container.background = None;
        let _ = p;
        s
    }
}
