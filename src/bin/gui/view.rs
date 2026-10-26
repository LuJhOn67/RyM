//! Everything drawn on screen.

use iced::widget::text::Wrapping;
use iced::widget::{
    Column, button, center, column, container, image, opaque, row, scrollable, slider, space, stack, text, text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Fill, FillPortion, Font, Length, font};

use rymplayer::api::{self, Link, Track};
use rymplayer::{fmt_time, lyrics};

use crate::icons::{Icon, icon};
use crate::style::{self, Palette};
use crate::{ART_PX, CARD_PX, Dialog, Gui, Message, Page, ROW_PX, Tab};

pub const FONT: Font = if cfg!(windows) { Font::with_name("Segoe UI") } else { Font::DEFAULT };
const SEMI: Font = Font { weight: font::Weight::Semibold, ..FONT };
const BOLD: Font = Font { weight: font::Weight::Bold, ..FONT };

const SIDEBAR_W: f32 = 248.0;
const CARD_W: f32 = 176.0;

/// One line, cut at the container edge instead of wrapping.
fn line<'a>(s: impl text::IntoFragment<'a>, size: u32, font: Font, color: Color) -> iced::widget::Text<'a> {
    text(s).size(size).font(font).color(color).wrapping(Wrapping::None)
}

fn divider<'a>(p: Palette) -> Element<'a, Message> {
    container(space()).width(Fill).height(1).style(style::bg(p.divider)).into()
}

impl Gui {
    pub fn view(&self) -> Element<'_, Message> {
        let p = self.palette();
        if !self.signed_in_screen {
            return self.login(p);
        }
        let spotify = p.panel_radius > 0.0;
        let side = container(self.sidebar(p)).width(SIDEBAR_W).height(Fill);
        let main = container(self.main_area(p)).width(Fill).height(Fill);
        let body: Element<'_, Message> = if spotify {
            row![side.style(style::panel(p)), main.style(style::panel(p))].spacing(8).padding(8).into()
        } else {
            row![side.style(style::bg(p.bg)), container(space()).width(1).height(Fill).style(style::bg(p.divider)), main.style(style::bg(p.surface))]
                .into()
        };
        let base = container(column![container(body).height(Fill), self.player_bar(p)])
            .width(Fill)
            .height(Fill)
            .style(style::bg(p.bg));
        match &self.dialog {
            None => base.into(),
            Some(d) => {
                let backdrop = center(self.dialog(p, d)).style(style::bg(Color::from_rgba8(0, 0, 0, 0.6)));
                stack![base, opaque(backdrop)].into()
            }
        }
    }

    fn login(&self, p: Palette) -> Element<'_, Message> {
        let mut google = button(line("Sign in with Google", 16, BOLD, p.pill_text)).padding([12, 28]).style(style::pill(p));
        let mut guest = button(line("Continue without an account", 14, SEMI, p.text)).padding([10, 22]).style(style::chip(p));
        if !self.busy {
            google = google.on_press(Message::SignIn);
            guest = guest.on_press(Message::Guest);
        }
        let card = column![
            self.logo(p, 34),
            text("YouTube Music, your way").size(16).color(p.dim),
            space().height(8),
            google,
            guest,
            text(&self.status).size(13).color(p.dim),
        ]
        .spacing(16)
        .align_x(Alignment::Center);
        center(container(card).padding(48).style(move |_| container::Style {
            background: Some(Background::Color(p.surface)),
            border: Border { radius: 16.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .style(style::bg(p.bg))
        .into()
    }

    fn logo(&self, p: Palette, size: u32) -> Element<'_, Message> {
        let badge = container(icon(Icon::Play, size as f32 * 0.45, Color::WHITE))
            .padding(size as f32 * 0.18)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.accent)),
                border: Border { radius: 999.0.into(), ..Default::default() },
                ..Default::default()
            });
        row![badge, line("RymPlayer", size, BOLD, p.text)].spacing(10).align_y(Alignment::Center).into()
    }

    fn sidebar(&self, p: Palette) -> Element<'_, Message> {
        let nav = |ic: Icon, label: &'static str, page: Page, title: &'static str| {
            let active = self.page == page && !self.now_playing_open;
            button(
                row![icon(ic, 22.0, if active { p.text } else { p.dim }), line(label, 15, if active { BOLD } else { SEMI }, p.text)]
                    .spacing(18)
                    .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([10, 14])
            .style(style::ghost(p, active))
            .on_press(Message::Open(page, title.into()))
        };
        let mut top = column![container(self.logo(p, 22)).padding([8, 12]), space().height(10), nav(Icon::Home, "For you", Page::Home, "For you")]
            .spacing(4);
        if self.logged_in() {
            top = top.push(nav(Icon::History, "History", Page::History, "History"));
        }

        let mut lib = Column::new().spacing(2);
        if self.logged_in() {
            let item = |title: String, sub: &'static str, id: String| {
                let page = Page::Playlist(id);
                let active = self.page == page && !self.now_playing_open;
                button(column![line(title.clone(), 14, SEMI, p.text), line(sub, 12, FONT, p.dim)].spacing(2))
                    .width(Fill)
                    .padding([7, 12])
                    .style(style::ghost(p, active))
                    .on_press(Message::Open(page, title))
            };
            lib = lib.push(item("Liked Music".into(), "Auto playlist", api::LIKED_ID.into()));
            for pl in &self.playlists {
                lib = lib.push(item(pl.title.clone(), "Playlist", pl.id.clone()));
            }
        }

        let mut col = column![top, space().height(8), divider(p), space().height(8)];
        if self.logged_in() {
            col = col.push(
                button(row![icon(Icon::Add, 18.0, p.text), line("New playlist", 14, SEMI, p.text)].spacing(8).align_y(Alignment::Center))
                    .width(Fill)
                    .padding([9, 16])
                    .style(style::chip(p))
                    .on_press(Message::NewPlaylist(None)),
            );
            col = col.push(space().height(8));
        }
        col.push(scrollable(lib).height(Fill).style(style::scroll(p))).padding([12, 10]).into()
    }

    fn main_area(&self, p: Palette) -> Element<'_, Message> {
        let input = text_input("Search songs, albums, artists", &self.search)
            .on_input(Message::SearchChanged)
            .on_submit(Message::SearchSubmit)
            .padding([11, 6])
            .size(15)
            .style(style::bare_input(p));
        let search = container(row![icon(Icon::Search, 20.0, p.dim), input].spacing(6).align_y(Alignment::Center))
            .padding([0, 14])
            .style(style::search_box(p));
        let theme_btn = button(row![icon(Icon::Palette, 18.0, p.dim), line(p.name, 13, SEMI, p.dim)].spacing(8).align_y(Alignment::Center))
            .padding([8, 12])
            .style(style::ghost(p, false))
            .on_press(Message::CycleTheme);
        let account = if self.logged_in() {
            button(line("Sign out", 13, SEMI, p.dim)).padding([8, 12]).style(style::ghost(p, false)).on_press(Message::SignOut)
        } else {
            button(line("Sign in", 13, BOLD, p.pill_text)).padding([8, 16]).style(style::pill(p)).on_press(Message::SignIn)
        };
        let status = container(line(self.status.clone(), 13, FONT, p.dim)).width(Fill).clip(true);
        let top = row![status, container(search).max_width(560).width(Fill), row![theme_btn, account].spacing(4).width(Fill)]
            .spacing(16)
            .padding([14, 24])
            .align_y(Alignment::Center);
        let top = row![top].width(Fill);

        let content = if self.now_playing_open { self.player_page(p) } else if self.page == Page::Home { self.home(p) } else { self.list(p) };
        column![top, content].into()
    }

    /// Cover art (or a note placeholder) at `size` px, rounded.
    fn art(&self, url: Option<&String>, px: u32, size: f32, radius: f32, p: Palette) -> Element<'_, Message> {
        match self.image(url, px) {
            Some(t) => image(t.handle).width(size).height(size).border_radius(radius).into(),
            None => container(center(icon(Icon::Note, size * 0.4, p.dim)))
                .width(size)
                .height(size)
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.hover)),
                    border: Border { radius: radius.into(), ..Default::default() },
                    ..Default::default()
                })
                .into(),
        }
    }

    fn loading(&self, p: Palette) -> Element<'_, Message> {
        let msg = if self.busy { "Loading..." } else if self.status.is_empty() { "Nothing here yet" } else { self.status.as_str() };
        center(text(msg).size(15).color(p.dim)).into()
    }

    /// Spotify-style shelves: a heading and a row of cards per section.
    fn home(&self, p: Palette) -> Element<'_, Message> {
        if self.tracks.is_empty() {
            return self.loading(p);
        }
        let mut sections: Vec<(&str, Vec<(usize, &Track)>)> = Vec::new();
        for (i, t) in self.tracks.iter().enumerate() {
            match sections.iter_mut().find(|(name, _)| *name == t.album) {
                Some((_, items)) => items.push((i, t)),
                None => sections.push((&t.album, vec![(i, t)])),
            }
        }
        let mut col = Column::new().spacing(28).padding([8, 28]);
        for (name, items) in sections {
            let cards = items.into_iter().map(|(i, t)| {
                let kind = match &t.link {
                    Some(Link::Playlist(_)) => "Playlist · ",
                    Some(Link::Album(_)) => "Album · ",
                    Some(Link::Mix(_)) => "Mix · ",
                    None => "",
                };
                let card = column![
                    self.art(t.thumb.as_ref(), CARD_PX, CARD_W, 6.0, p),
                    container(line(&t.title, 14, SEMI, p.text)).width(CARD_W).clip(true),
                    container(line(format!("{kind}{}", t.artist), 13, FONT, p.dim)).width(CARD_W).clip(true),
                ]
                .spacing(6);
                let msg = if t.link.is_none() { Message::PlaySong(t.clone()) } else { Message::PlayRow(i) };
                button(card).padding(10).style(style::row(p, false)).on_press(msg).into()
            });
            let shelf = scrollable(row(cards).spacing(4).padding([0, 0]))
                .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4)))
                .style(style::scroll(p));
            col = col.push(column![line(name, 24, BOLD, p.text), shelf].spacing(12));
        }
        scrollable(col).height(Fill).style(style::scroll(p)).into()
    }

    /// Playlist, album, search, history: a header and song rows.
    fn list(&self, p: Palette) -> Element<'_, Message> {
        if self.tracks.is_empty() {
            return self.loading(p);
        }
        let kind = match self.page {
            Page::Search(_) => "SEARCH",
            Page::History => "HISTORY",
            Page::Album(_) => "ALBUM",
            Page::Mix(_) | Page::Radio(_) => "MIX",
            _ => "PLAYLIST",
        };
        let songs = self.tracks.iter().filter(|t| t.link.is_none()).count();
        let header = row![
            self.art(self.tracks.first().and_then(|t| t.thumb.as_ref()), CARD_PX, 184.0, 8.0, p),
            column![
                line(kind, 12, BOLD, p.dim),
                container(line(&self.page_title, 40, BOLD, p.text)).clip(true),
                line(format!("{songs} songs"), 14, FONT, p.dim),
                space().height(6),
                button(row![icon(Icon::Play, 20.0, p.pill_text), line("Play", 15, BOLD, p.pill_text)].spacing(6).align_y(Alignment::Center))
                    .padding([10, 22])
                    .style(style::pill(p))
                    .on_press(Message::PlayAll),
            ]
            .spacing(6)
        ]
        .spacing(24)
        .align_y(Alignment::End);

        let playing = self.now_playing().map(|t| t.video_id.as_str());
        let third = if self.page == Page::History { "When" } else { "Album" };
        let head = row![
            container(line("#", 12, SEMI, p.dim)).width(40),
            container(line("Title", 12, SEMI, p.dim)).width(FillPortion(5)),
            container(line(third, 12, SEMI, p.dim)).width(FillPortion(3)),
            container(line("", 12, SEMI, p.dim)).width(76),
            container(line("Time", 12, SEMI, p.dim)).width(48),
        ]
        .spacing(16)
        .padding([0, 10]);
        let mut rows = Column::new().spacing(2);
        for (i, t) in self.tracks.iter().enumerate() {
            let current = playing == Some(t.video_id.as_str()) && !t.video_id.is_empty();
            let liked = self.liked.contains(&t.video_id);
            let title_color = if current { p.accent } else { p.text };
            let mut actions = row![].spacing(2).align_y(Alignment::Center);
            if liked {
                actions = actions.push(icon(Icon::HeartFilled, 16.0, p.accent));
            }
            if self.logged_in() && t.link.is_none() {
                actions = actions.push(
                    button(icon(Icon::PlaylistAdd, 18.0, p.dim)).padding(6).style(style::icon_button(p, false)).on_press(Message::AddTo(t.clone())),
                );
                actions = actions.push(
                    button(icon(Icon::Radio, 16.0, p.dim)).padding(7).style(style::icon_button(p, false)).on_press(Message::StartRadio(t.clone())),
                );
            }
            let time = match &t.link {
                Some(Link::Playlist(_)) => "list".to_string(),
                Some(Link::Album(_)) => "album".to_string(),
                Some(Link::Mix(_)) => "mix".to_string(),
                None => t.duration.clone(),
            };
            let r = row![
                self.art(t.thumb.as_ref(), ROW_PX, 40.0, 4.0, p),
                container(column![line(&t.title, 14, SEMI, title_color), line(&t.artist, 13, FONT, p.dim)].spacing(2))
                    .width(FillPortion(5))
                    .clip(true),
                container(line(&t.album, 13, FONT, p.dim)).width(FillPortion(3)).clip(true),
                container(actions).width(76).align_x(Alignment::End),
                container(line(time, 13, FONT, p.dim)).width(48),
            ]
            .spacing(16)
            .align_y(Alignment::Center);
            rows = rows.push(button(r).width(Fill).padding([6, 10]).style(style::row(p, current)).on_press(Message::PlayRow(i)));
        }
        scrollable(column![header, space().height(12), head, divider(p), rows].spacing(10).padding([8, 28]))
            .height(Fill)
            .style(style::scroll(p))
            .into()
    }

    /// YouTube Music's player page: big cover, "Up next" and lyrics on the right.
    fn player_page(&self, p: Palette) -> Element<'_, Message> {
        let Some(t) = self.now_playing() else { return self.loading(p) };
        let cover = center(column![
            self.art(t.thumb.as_ref(), ART_PX, 420.0, 8.0, p),
            space().height(18),
            line(&t.title, 26, BOLD, p.text),
            line(&t.artist, 16, FONT, p.dim),
        ]
        .align_x(Alignment::Center))
        .width(FillPortion(3));

        let tab = |label: &'static str, which: Tab| {
            let active = self.tab == which;
            let underline = container(space()).width(Fill).height(2).style(style::bg(if active { p.text } else { Color::TRANSPARENT }));
            button(column![container(line(label, 13, BOLD, if active { p.text } else { p.dim })).center_x(Fill).padding([10, 0]), underline])
                .width(Fill)
                .padding(0)
                .style(style::ghost(p, false))
                .on_press(Message::ShowTab(which))
        };
        let tabs = row![tab("UP NEXT", Tab::UpNext), tab("LYRICS", Tab::Lyrics)];
        let body = match self.tab {
            Tab::UpNext => self.up_next(p),
            Tab::Lyrics => self.lyrics_view(p),
        };
        let side = column![tabs, divider(p), body].width(420).height(Fill);
        row![cover, side].spacing(24).padding([8, 28]).height(Fill).into()
    }

    fn up_next(&self, p: Palette) -> Element<'_, Message> {
        let mut col = Column::new().spacing(2);
        for (i, t) in self.queue.iter().enumerate() {
            let current = self.current == Some(i);
            let r = row![
                self.art(t.thumb.as_ref(), ROW_PX, 40.0, 4.0, p),
                container(column![line(&t.title, 14, SEMI, p.text), line(&t.artist, 13, FONT, p.dim)].spacing(2)).width(Fill).clip(true),
                line(&t.duration, 13, FONT, p.dim),
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            col = col.push(button(r).width(Fill).padding([6, 8]).style(style::row(p, current)).on_press(Message::PlayQueue(i)));
        }
        if self.radio_pending {
            col = col.push(container(text("Finding more songs...").size(13).color(p.dim)).padding(10));
        }
        scrollable(col.padding([8, 0])).height(Fill).style(style::scroll(p)).into()
    }

    fn lyrics_view(&self, p: Palette) -> Element<'_, Message> {
        let Some(l) = &self.lyrics else {
            return center(text(&self.lyrics_status).size(15).color(p.dim)).into();
        };
        let source = line(format!("Source: {}", l.source), 12, FONT, p.dim);
        if l.synced.is_empty() {
            let lines = Column::with_children(l.plain.iter().map(|s| text(s).size(18).color(p.text).into())).spacing(8);
            return scrollable(column![lines, space().height(8), source].padding([16, 8])).height(Fill).style(style::scroll(p)).into();
        }
        // A window around the sung line keeps it in view without scrolling state.
        let cur = lyrics::current_line(&l.synced, self.pos);
        let start = cur.unwrap_or(0).saturating_sub(2);
        let mut col = Column::new().spacing(16).padding([24, 8]);
        for (i, (_, s)) in l.synced.iter().enumerate().skip(start).take(14) {
            let s = if s.is_empty() { "♪" } else { s.as_str() };
            let (size, font, color) = match cur {
                Some(c) if c == i => (26, BOLD, p.text),
                Some(c) if i < c => (21, SEMI, Color { a: 0.35, ..p.text }),
                _ => (21, SEMI, p.dim),
            };
            col = col.push(text(s).size(size).font(font).color(color));
        }
        column![container(col).height(Fill).clip(true), source].into()
    }

    fn player_bar(&self, p: Palette) -> Element<'_, Message> {
        let dur = self.duration();
        let shown = self.seeking.unwrap_or(self.pos).min(dur.max(0.0));
        let seek = slider(0.0..=dur.max(1.0), shown, Message::Seek).on_release(Message::SeekDone).step(0.5f32).height(12).style(style::seek(p));

        let play_icon = if self.is_playing() { Icon::Pause } else { Icon::Play };
        let ctrl = |ic: Icon, msg: Message| button(icon(ic, 24.0, p.text)).padding(8).style(style::icon_button(p, false)).on_press(msg);
        let left = row![
            ctrl(Icon::Prev, Message::Prev),
            button(icon(play_icon, 26.0, p.pill_text)).padding(10).style(style::icon_button(p, true)).on_press(Message::Toggle),
            ctrl(Icon::Next, Message::Next),
            line(format!("{} / {}", fmt_time(shown), fmt_time(dur)), 12, FONT, p.dim),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(FillPortion(2));

        let middle: Element<'_, Message> = match self.now_playing() {
            Some(t) => {
                let liked = self.liked.contains(&t.video_id);
                let mut r = row![
                    button(self.art(t.thumb.as_ref(), ROW_PX, 48.0, 4.0, p)).padding(0).style(style::row(p, false)).on_press(Message::ToggleNowPlaying),
                    container(column![line(&t.title, 15, SEMI, p.text), line(&t.artist, 13, FONT, p.dim)].spacing(2)).max_width(320).clip(true),
                ]
                .spacing(14)
                .align_y(Alignment::Center);
                if self.logged_in() {
                    r = r.push(
                        button(icon(if liked { Icon::HeartFilled } else { Icon::Heart }, 20.0, if liked { p.accent } else { p.dim }))
                            .padding(8)
                            .style(style::icon_button(p, false))
                            .on_press(Message::Like(t.clone())),
                    );
                    r = r.push(button(icon(Icon::PlaylistAdd, 20.0, p.dim)).padding(8).style(style::icon_button(p, false)).on_press(Message::AddTo(t.clone())));
                }
                if self.loading {
                    r = r.push(line("loading...", 12, FONT, p.dim));
                }
                r.into()
            }
            None => line("Pick something to play", 14, FONT, p.dim).into(),
        };

        let right = row![
            space::horizontal(),
            icon(Icon::Volume, 20.0, p.dim),
            slider(0.0..=1.0, self.volume, Message::Volume).step(0.01f32).width(110).style(style::seek(p)),
            button(icon(if self.now_playing_open { Icon::Down } else { Icon::Up }, 24.0, p.text))
                .padding(6)
                .style(style::icon_button(p, false))
                .on_press(Message::ToggleNowPlaying),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .width(FillPortion(2));

        let controls = row![left, container(middle).width(FillPortion(3)).center_x(FillPortion(3)), right]
            .spacing(12)
            .padding([6, 16])
            .align_y(Alignment::Center);
        container(column![seek, controls]).width(Fill).height(Length::Shrink).style(style::bg(p.bar)).into()
    }

    fn dialog<'a>(&'a self, p: Palette, d: &'a Dialog) -> Element<'a, Message> {
        let body: Column<'a, Message> = match d {
            Dialog::NewPlaylist(name, song) => {
                let mut c = column![line("New playlist", 22, BOLD, p.text)];
                if let Some(t) = song {
                    c = c.push(line(format!("with \"{}\"", t.title), 13, FONT, p.dim));
                }
                c.push(
                    text_input("Title", name)
                        .on_input(Message::NameChanged)
                        .on_submit(Message::CreatePlaylist)
                        .padding([11, 16])
                        .size(15)
                        .style(style::search(p)),
                )
                .push(
                    row![
                        space::horizontal(),
                        button(line("Cancel", 14, SEMI, p.text)).padding([9, 18]).style(style::ghost(p, false)).on_press(Message::CloseDialog),
                        button(line("Create", 14, BOLD, p.pill_text)).padding([9, 20]).style(style::pill(p)).on_press(Message::CreatePlaylist),
                    ]
                    .spacing(8),
                )
            }
            Dialog::AddTo(t) => {
                let mut list = Column::new().spacing(2);
                for pl in &self.playlists {
                    list = list.push(
                        button(row![icon(Icon::Library, 20.0, p.dim), line(&pl.title, 14, SEMI, p.text)].spacing(12).align_y(Alignment::Center))
                            .width(Fill)
                            .padding([9, 12])
                            .style(style::ghost(p, false))
                            .on_press(Message::Pick(pl.clone())),
                    );
                }
                column![
                    line("Save to playlist", 22, BOLD, p.text),
                    line(&t.title, 13, FONT, p.dim),
                    button(row![icon(Icon::Add, 18.0, p.text), line("New playlist", 14, SEMI, p.text)].spacing(8).align_y(Alignment::Center))
                        .padding([9, 16])
                        .style(style::chip(p))
                        .on_press(Message::NewPlaylist(Some(t.clone()))),
                    scrollable(list).height(Length::Fixed(300.0)).style(style::scroll(p)),
                    row![space::horizontal(), button(line("Close", 14, SEMI, p.text)).padding([9, 18]).style(style::ghost(p, false)).on_press(Message::CloseDialog)],
                ]
            }
        };
        container(body.spacing(14))
            .width(440)
            .padding(24)
            .style(move |_| container::Style {
                // Spotify's bar is pure black; its menus are a lifted grey.
                background: Some(Background::Color(if p.panel_radius > 0.0 { Color::from_rgb8(40, 40, 40) } else { p.bar })),
                border: Border { radius: 12.0.into(), ..Default::default() },
                ..Default::default()
            })
            .into()
    }
}
