//! Everything drawn on screen: an icon rail, a search bar, card grids, a slim player bar,
//! and a full player page. The content background is washed with the cover's color.

use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Row, button, center, column, container, image, mouse_area, opaque, pin, row, scrollable, slider, space, stack,
    text, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Fill, FillPortion, Font, Length, font};

use rymplayer::api::{self, Link, Track};
use rymplayer::{fmt_time, lyrics};

use crate::icons::{Icon, icon};
use crate::style::{self, Palette, mix, tinted};
use crate::{ART_PX, CARD_PX, Dialog, Gui, MenuFor, Message, Page, ROW_PX, Tab, ease};

pub const FONT: Font = if cfg!(windows) { Font::with_name("Segoe UI") } else { Font::DEFAULT };
const SEMI: Font = Font { weight: font::Weight::Semibold, ..FONT };
const BOLD: Font = Font { weight: font::Weight::Bold, ..FONT };

pub const RAIL_W: f32 = 76.0;
const CARD_MIN: f32 = 300.0;
const CARD_ART: f32 = 104.0;
pub const MENU_W: f32 = 272.0;
const MENU_ITEM_H: f32 = 44.0;

/// One line, cut at the container edge instead of wrapping.
fn line<'a>(s: impl text::IntoFragment<'a>, size: u32, font: Font, color: Color) -> iced::widget::Text<'a> {
    text(s).size(size).font(font).color(color).wrapping(Wrapping::None)
}

fn divider<'a>(p: Palette) -> Element<'a, Message> {
    container(space()).width(Fill).height(1).style(style::bg(p.divider)).into()
}

/// A rounded square with an icon, for things without cover art (Liked Music, New playlist).
fn tile<'a>(ic: Icon, size: f32, fg: Color, bg: Color) -> Element<'a, Message> {
    container(center(icon(ic, size * 0.42, fg))).width(size).height(size).style(style::card_bg(bg, 6.0)).into()
}

fn heading<'a>(s: impl text::IntoFragment<'a>, p: Palette) -> Element<'a, Message> {
    line(s, 20, BOLD, p.text).into()
}

impl Gui {
    pub fn view(&self) -> Element<'_, Message> {
        let p = self.palette();
        if !self.signed_in_screen {
            return self.login(p);
        }
        let (tint, strength) = self.tint();
        let page_strength = if self.now_playing_open { 1.7 } else { 1.0 } * strength;
        let content = column![self.top_bar(p), self.page_view(p)];
        let main = container(content).width(Fill).height(Fill).style(style::wash(p, tint, page_strength));
        let rail = container(self.rail(p)).width(RAIL_W).height(Fill).style(style::bg(tinted(p, p.bg, tint, 0.5 * strength)));
        let body: Element<'_, Message> = if p.panel_radius > 0.0 {
            row![rail, container(main).padding([8, 8])].into()
        } else {
            row![rail, main].into()
        };
        let base = container(column![container(body).height(Fill), self.player_bar(p, tint, strength)])
            .width(Fill)
            .height(Fill)
            .style(style::bg(p.bg));

        let mut layers = stack![base];
        if let Some(d) = &self.dialog {
            layers = layers.push(opaque(center(self.dialog(p, d)).style(style::bg(Color::from_rgba8(0, 0, 0, 0.45)))));
        }
        if let Some(m) = &self.menu {
            // A click anywhere else closes the menu.
            let catcher = mouse_area(container(space()).width(Fill).height(Fill))
                .on_press(Message::CloseMenu)
                .on_right_press(Message::CloseMenu);
            layers = layers.push(catcher).push(pin(self.menu(p, m)).position(self.menu_pos));
        }
        crate::right_click::tracker(layers, Message::Cursor).into()
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
        let shadow = style::shadow(p);
        center(container(card).padding(48).style(move |_| container::Style {
            background: Some(Background::Color(p.surface)),
            border: Border { radius: 16.0.into(), ..Default::default() },
            shadow,
            ..Default::default()
        }))
        .style(style::bg(p.bg))
        .into()
    }

    fn logo(&self, p: Palette, size: u32) -> Element<'_, Message> {
        let badge = container(icon(Icon::Play, size as f32 * 0.45, Color::WHITE))
            .padding(size as f32 * 0.18)
            .style(style::card_bg(p.accent, 999.0));
        row![badge, line("RymPlayer", size, BOLD, p.text)].spacing(10).align_y(Alignment::Center).into()
    }

    // ---------------------------------------------------------------- chrome

    /// Narrow icon column; labels show as tooltips.
    fn rail(&self, p: Palette) -> Element<'_, Message> {
        let item = |ic: Icon, label: &'static str, page: Page, title: &'static str| {
            let active = self.page == page && !self.now_playing_open;
            let b = button(center(icon(ic, 24.0, if active { p.accent } else { p.dim })))
                .width(52)
                .height(48)
                .style(style::rail(p, active))
                .on_press(Message::Open(page, title.into()));
            tooltip(b, container(line(label, 13, SEMI, p.text)).padding([5, 10]).style(style::card_bg(p.menu, 6.0)), tooltip::Position::Right)
        };
        let logo = container(icon(Icon::Play, 16.0, Color::WHITE)).padding(8).style(style::card_bg(p.accent, 999.0));
        let mut col = column![container(logo).padding([18, 0]), item(Icon::Home, "For you", Page::Home, "For you")]
            .spacing(8)
            .align_x(Alignment::Center);
        col = col.push(item(Icon::Search, "Search", Page::Search(self.search.clone()), "Search"));
        if self.logged_in() {
            col = col
                .push(item(Icon::Heart, "Liked music", Page::Playlist(api::LIKED_ID.into()), "Liked Music"))
                .push(item(Icon::Library, "Library", Page::Library, "Library"))
                .push(item(Icon::History, "History", Page::History, "History"));
        }
        col.push(space().height(Fill))
            .push(item(Icon::Settings, "Settings", Page::Settings, "Settings"))
            .push(space().height(12))
            .width(Fill)
            .into()
    }

    fn top_bar(&self, p: Palette) -> Element<'_, Message> {
        let input = text_input("Search songs, albums, artists", &self.search)
            .on_input(Message::SearchChanged)
            .on_submit(Message::SearchSubmit)
            .padding([9, 6])
            .size(14)
            .style(style::bare_input(p));
        let search = container(row![input, icon(Icon::Search, 20.0, p.dim)].spacing(6).align_y(Alignment::Center))
            .padding([0, 12])
            .width(380)
            .style(style::search_box(p));
        let status = container(line(self.status.clone(), 13, FONT, p.dim)).width(Fill).align_x(Alignment::End).clip(true);
        let mut r = row![search, status].spacing(20).padding([16, 28]).align_y(Alignment::Center);
        if !self.logged_in() {
            r = r.push(button(line("Sign in", 13, BOLD, p.pill_text)).padding([7, 16]).style(style::pill(p)).on_press(Message::SignIn));
        }
        r.into()
    }

    fn page_view(&self, p: Palette) -> Element<'_, Message> {
        if self.now_playing_open {
            return self.player_page(p);
        }
        match self.page {
            Page::Home => self.home(p),
            Page::Library => self.library(p),
            Page::Settings => self.settings(p),
            Page::Search(ref q) if q.is_empty() => center(text("Type in the search box above").size(15).color(p.dim)).into(),
            _ => self.list(p),
        }
    }

    fn pad(&self) -> iced::Padding {
        iced::Padding { top: 4.0 + self.slide_in(), right: 28.0, bottom: 16.0, left: 28.0 }
    }

    /// How many cards fit across the content area.
    fn columns(&self) -> usize {
        let avail = self.window.width - RAIL_W - 56.0;
        (((avail + 12.0) / (CARD_MIN + 12.0)).floor() as usize).max(1)
    }

    fn grid<'a>(&self, cards: Vec<Element<'a, Message>>) -> Element<'a, Message> {
        let cols = self.columns();
        let mut rows = Column::new().spacing(6);
        let mut cards = cards.into_iter().peekable();
        while cards.peek().is_some() {
            let mut r = Row::new().spacing(12);
            for i in 0..cols {
                r = match cards.next() {
                    Some(c) => r.push(container(c).width(FillPortion(1))),
                    // Keep the last row's cards the same width as the others.
                    None if i > 0 => r.push(space().width(FillPortion(1))),
                    None => r,
                };
            }
            rows = rows.push(r);
        }
        rows.into()
    }

    /// Cover on the left, title (two lines), subtitle and a detail line on the right.
    fn card<'a>(&'a self, p: Palette, art: Element<'a, Message>, title: &'a str, sub: String, meta: String, msg: Message) -> button::Button<'a, Message> {
        let text_col = column![
            container(text(title).size(14).font(SEMI).color(p.text)).height(40).clip(true),
            container(line(sub, 12, FONT, p.dim)).clip(true),
            space().height(Fill),
            container(line(meta, 12, FONT, p.dim)).clip(true),
        ]
        .spacing(4)
        .height(CARD_ART)
        .width(Fill);
        button(row![art, text_col].spacing(14)).padding(8).width(Fill).style(style::row(p, false)).on_press(msg)
    }

    // ---------------------------------------------------------------- pages

    /// Cover art (or a note placeholder) at `size` px, rounded; fades in when it arrives.
    fn art(&self, url: Option<&String>, px: u32, size: f32, radius: f32, p: Palette) -> Element<'_, Message> {
        match self.image(url, px) {
            Some(t) => image(t.handle).width(size).height(size).border_radius(radius).opacity(ease(t.loaded, 320.0)).into(),
            None => container(center(icon(Icon::Note, size * 0.4, p.dim)))
                .width(size)
                .height(size)
                .style(style::card_bg(p.input_bg, radius))
                .into(),
        }
    }

    fn loading(&self, p: Palette) -> Element<'_, Message> {
        let msg = if self.busy { "Loading..." } else if self.status.is_empty() { "Nothing here yet" } else { self.status.as_str() };
        center(text(msg).size(15).color(p.dim)).into()
    }

    /// The home feed: each section is a heading and a grid of cards.
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
        let mut col = Column::new().spacing(26).padding(self.pad());
        for (name, items) in sections {
            let cards = items
                .into_iter()
                .map(|(i, t)| {
                    let kind = match &t.link {
                        Some(Link::Playlist(_)) => "Playlist",
                        Some(Link::Album(_)) => "Album",
                        Some(Link::Mix(_)) => "Mix",
                        None => "Song",
                    };
                    let msg = if t.link.is_none() { Message::PlaySong(t.clone()) } else { Message::PlayRow(i) };
                    let art = self.art(t.thumb.as_ref(), CARD_PX, CARD_ART, 6.0, p);
                    let card = self.card(p, art, &t.title, t.artist.clone(), kind.to_string(), msg);
                    if t.link.is_none() {
                        mouse_area(card).on_right_press(Message::Menu(MenuFor { track: t.clone(), queue_index: None })).into()
                    } else {
                        card.into()
                    }
                })
                .collect();
            col = col.push(column![heading(name, p), self.grid(cards)].spacing(12));
        }
        scrollable(col).id(crate::HOME_SCROLL).height(Fill).style(style::scroll(p)).into()
    }

    fn library(&self, p: Palette) -> Element<'_, Message> {
        let mut cards: Vec<Element<'_, Message>> = vec![
            self.card(
                p,
                tile(Icon::HeartFilled, CARD_ART, Color::WHITE, p.accent),
                "Liked Music",
                "Auto playlist".into(),
                format!("{} songs", self.liked.len()),
                Message::Open(Page::Playlist(api::LIKED_ID.into()), "Liked Music".into()),
            )
            .into(),
            self.card(p, tile(Icon::Add, CARD_ART, p.dim, p.input_bg), "New playlist", "Make an empty one".into(), String::new(), Message::NewPlaylist(None))
                .into(),
        ];
        for pl in &self.playlists {
            let art = self.art(pl.thumb.as_ref(), CARD_PX, CARD_ART, 6.0, p);
            cards.push(self.card(p, art, &pl.title, "Playlist".into(), String::new(), Message::Open(Page::Playlist(pl.id.clone()), pl.title.clone())).into());
        }
        scrollable(column![heading("Library", p), self.grid(cards)].spacing(12).padding(self.pad())).height(Fill).style(style::scroll(p)).into()
    }

    fn settings(&self, p: Palette) -> Element<'_, Message> {
        let swatch = |c: Color| container(space()).width(34).height(34).style(style::card_bg(c, 6.0));
        let themes: Vec<Element<'_, Message>> = style::PALETTES
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let active = i == self.config.gui_theme % style::PALETTES.len();
                let preview = row![swatch(t.bg), swatch(t.surface), swatch(t.accent)].spacing(6);
                let label = row![line(t.name, 14, SEMI, p.text), space().width(Fill)]
                    .push(if active { Element::from(icon(Icon::Play, 14.0, p.accent)) } else { space().into() });
                button(column![preview, label].spacing(10))
                    .width(Fill)
                    .padding(12)
                    .style(style::row(p, active))
                    .on_press(Message::SetTheme(i))
                    .into()
            })
            .collect();

        let on = self.config.gui_tint;
        let toggle = button(line(if on { "On" } else { "Off" }, 13, BOLD, if on { p.pill_text } else { p.text }))
            .padding([6, 16])
            .style(move |theme: &iced::Theme, status| if on { style::pill(p)(theme, status) } else { style::chip(p)(theme, status) })
            .on_press(Message::ToggleTint);
        let tint_row = row![
            column![line("Cover colors", 15, SEMI, p.text), line("Wash the background with a soft shade of the song's cover", 13, FONT, p.dim)]
                .spacing(4)
                .width(Fill),
            toggle,
        ]
        .align_y(Alignment::Center);

        let renderer = self.config.gui_renderer;
        let gpu = renderer != rymplayer::config::Renderer::Cpu;
        let gpu_toggle = button(line(renderer.label(), 13, BOLD, if gpu { p.pill_text } else { p.text }))
            .padding([6, 16])
            .style(move |theme: &iced::Theme, status| if gpu { style::pill(p)(theme, status) } else { style::chip(p)(theme, status) })
            .on_press(Message::ToggleGpu);
        let gpu_row = row![
            column![
                line("Drawing", 15, SEMI, p.text),
                line("Vulkan: works on most PCs. DirectX 12: same speed, less RAM. Processor: least RAM, less smooth. Applies after a restart.", 13, FONT, p.dim)
            ]
            .spacing(4)
            .width(Fill),
            gpu_toggle,
        ]
        .align_y(Alignment::Center);

        let source = self.config.source;
        let source_row = row![
            column![
                line("Audio source", 15, SEMI, p.text),
                line("InnerTube + yt-dlp: fast, and yt-dlp takes over when YouTube refuses. yt-dlp: slower start, Premium gets 256 kbps (needs yt-dlp and Node.js). InnerTube: straight from YouTube only.", 13, FONT, p.dim)
            ]
            .spacing(4)
            .width(Fill),
            button(line(source.label(), 13, BOLD, p.pill_text)).padding([6, 16]).style(style::pill(p)).on_press(Message::ToggleSource),
        ]
        .align_y(Alignment::Center);

        let best = self.config.download_best;
        let download_row = row![
            column![
                line("Download quality", 15, SEMI, p.text),
                line("Best: yt-dlp with your account, 256 kbps for Premium, a few seconds more. Fast: the playback file, about 160 kbps.", 13, FONT, p.dim)
            ]
            .spacing(4)
            .width(Fill),
            button(line(if best { "Best" } else { "Fast" }, 13, BOLD, p.pill_text)).padding([6, 16]).style(style::pill(p)).on_press(Message::ToggleDownload),
        ]
        .align_y(Alignment::Center);

        let parallel_row = row![
            column![
                line("Download connections", 15, SEMI, p.text),
                line("How many connections a song downloads over at once. YouTube limits each one, so more use more of your line.", 13, FONT, p.dim)
            ]
            .spacing(4)
            .width(Fill),
            button(line("-", 15, BOLD, p.text)).padding([4, 14]).style(style::chip(p)).on_press(Message::Connections(-1)),
            container(line(format!("{}", self.config.download_connections), 15, BOLD, p.text)).width(36).center_x(36),
            button(line("+", 15, BOLD, p.text)).padding([4, 14]).style(style::chip(p)).on_press(Message::Connections(1)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let piece_row = row![
            column![
                line("Download piece size", 15, SEMI, p.text),
                line("Open: the song is split evenly over the connections, no fixed size.", 13, FONT, p.dim)
            ]
            .spacing(4)
            .width(Fill),
            button(line(self.config.piece_label(), 13, BOLD, p.pill_text)).padding([6, 16]).style(style::pill(p)).on_press(Message::NextPiece),
        ]
        .align_y(Alignment::Center);

        let vlc_found = rymplayer::vlc::find().is_some();
        let vlc_row = row![
            column![
                line("Play songs in", 15, SEMI, p.text),
                line(
                    if vlc_found { "VLC: each song opens in VLC. RyM: the built-in player." } else { "VLC isn't installed on this PC, so songs play here." },
                    13,
                    FONT,
                    p.dim
                )
            ]
            .spacing(4)
            .width(Fill),
            button(line(if self.config.play_in_vlc { "VLC" } else { "RyM" }, 13, BOLD, p.pill_text)).padding([6, 16]).style(style::pill(p)).on_press(Message::ToggleVlc),
        ]
        .align_y(Alignment::Center);

        let account: Element<'_, Message> = if self.logged_in() {
            button(line("Sign out", 13, SEMI, p.text)).padding([7, 16]).style(style::chip(p)).on_press(Message::SignOut).into()
        } else {
            button(line("Sign in with Google", 13, BOLD, p.pill_text)).padding([7, 16]).style(style::pill(p)).on_press(Message::SignIn).into()
        };

        let col = column![
            heading("Theme", p),
            self.grid(themes),
            space().height(8),
            heading("Look", p),
            tint_row,
            gpu_row,
            source_row,
            download_row,
            parallel_row,
            piece_row,
            vlc_row,
            space().height(8),
            heading("Account", p),
            row![account],
        ]
        .spacing(14)
        .padding(self.pad());
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
            Page::Artist(_) => "ARTIST",
            _ => "PLAYLIST",
        };
        let songs = self.tracks.iter().filter(|t| t.link.is_none()).count();
        let cover: Element<'_, Message> = if self.page == Page::Playlist(api::LIKED_ID.into()) {
            tile(Icon::HeartFilled, 150.0, Color::WHITE, p.accent)
        } else {
            self.art(self.tracks.first().and_then(|t| t.thumb.as_ref()), CARD_PX, 150.0, 8.0, p)
        };
        let shadow = style::shadow(p);
        let header = row![
            container(cover).style(move |_| container::Style { shadow, border: Border { radius: 8.0.into(), ..Default::default() }, ..Default::default() }),
            column![
                line(kind, 12, BOLD, p.dim),
                container(line(&self.page_title, 30, BOLD, p.text)).clip(true),
                line(format!("{songs} songs"), 13, FONT, p.dim),
                space().height(4),
                button(row![icon(Icon::Play, 18.0, p.pill_text), line("Play all", 14, BOLD, p.pill_text)].spacing(6).align_y(Alignment::Center))
                    .padding([8, 20])
                    .style(style::pill(p))
                    .on_press(Message::PlayAll),
            ]
            .spacing(6)
        ]
        .spacing(22)
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
                actions = actions.push(button(icon(Icon::PlaylistAdd, 18.0, p.dim)).padding(6).style(style::icon_button(p)).on_press(Message::AddTo(t.clone())));
                actions = actions.push(button(icon(Icon::Radio, 16.0, p.dim)).padding(7).style(style::icon_button(p)).on_press(Message::StartRadio(t.clone())));
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
            let b = button(r).width(Fill).padding([6, 10]).style(style::row(p, current)).on_press(Message::PlayRow(i));
            rows = rows.push(if t.link.is_none() {
                Element::from(mouse_area(b).on_right_press(Message::Menu(MenuFor { track: t.clone(), queue_index: None })))
            } else {
                b.into()
            });
        }
        scrollable(column![header, space().height(10), head, divider(p), rows].spacing(10).padding(self.pad()))
            .height(Fill)
            .style(style::scroll(p))
            .into()
    }

    /// Full player: big cover on the left, lyrics and "Up next" on the right.
    fn player_page(&self, p: Palette) -> Element<'_, Message> {
        let Some(t) = self.now_playing() else { return self.loading(p) };
        let shadow = style::shadow(p);
        let cover = center(
            column![
                container(self.art(t.thumb.as_ref(), ART_PX, 340.0, 10.0, p))
                    .style(move |_| container::Style { shadow, border: Border { radius: 10.0.into(), ..Default::default() }, ..Default::default() }),
                space().height(22),
                line(&t.title, 24, BOLD, p.text),
                line(&t.artist, 15, FONT, p.dim),
            ]
            .align_x(Alignment::Center),
        )
        .width(FillPortion(3));

        let tab = |label: &'static str, which: Tab| {
            let active = self.tab == which;
            let underline = container(space()).width(Fill).height(2).style(style::bg(if active { p.accent } else { Color::TRANSPARENT }));
            button(column![container(line(label, 13, BOLD, if active { p.text } else { p.dim })).center_x(Fill).padding([10, 0]), underline])
                .width(Fill)
                .padding(0)
                .style(style::ghost(p, false))
                .on_press(Message::ShowTab(which))
        };
        let tabs = row![tab("LYRICS", Tab::Lyrics), tab("UP NEXT", Tab::UpNext)];
        let body = match self.tab {
            Tab::UpNext => self.up_next(p),
            Tab::Lyrics => self.lyrics_view(p),
        };
        let side = column![tabs, divider(p), body].width(FillPortion(2)).height(Fill);
        row![cover, side].spacing(24).padding(self.pad()).height(Fill).into()
    }

    fn up_next(&self, p: Palette) -> Element<'_, Message> {
        let mut col = Column::new().spacing(2);
        for (i, t) in self.queue.iter().enumerate() {
            let current = self.current == Some(i);
            let r = row![
                self.art(t.thumb.as_ref(), ROW_PX, 40.0, 4.0, p),
                container(column![line(&t.title, 14, SEMI, if current { p.accent } else { p.text }), line(&t.artist, 13, FONT, p.dim)].spacing(2))
                    .width(Fill)
                    .clip(true),
                line(&t.duration, 13, FONT, p.dim),
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            let b = button(r).width(Fill).padding([6, 8]).style(style::row(p, current)).on_press(Message::PlayQueue(i));
            col = col.push(mouse_area(b).on_right_press(Message::Menu(MenuFor { track: t.clone(), queue_index: Some(i) })));
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
            let lines = Column::with_children(l.plain.iter().map(|s| text(s).size(17).color(p.text).center().width(Fill).into())).spacing(8);
            return scrollable(column![lines, space().height(8), source].padding([16, 8])).height(Fill).style(style::scroll(p)).into();
        }
        // A window around the sung line keeps it in view without scrolling state. When the line
        // changes, the window moves down one line and starts one line lower, then slides up.
        let cur = lyrics::current_line(&l.synced, self.pos);
        let k = ease(self.lyric_changed, 420.0);
        let start = cur.unwrap_or(0).saturating_sub(3);
        let slide = if cur.is_some_and(|c| c >= 3) { (1.0 - k) * LYRIC_STEP } else { 0.0 };
        let mut col = Column::new().spacing(14).padding(iced::Padding { top: 28.0 + slide, right: 8.0, bottom: 24.0, left: 8.0 });
        let lit = if p.light { p.accent } else { p.text };
        let faded = Color { a: 0.4, ..p.dim };
        for (i, (_, s)) in l.synced.iter().enumerate().skip(start).take(16) {
            let s = if s.is_empty() { "♪" } else { s.as_str() };
            // The sung line grows and lights up; the one before it shrinks and dims.
            let (size, font, color) = match cur {
                Some(c) if c == i => (19.0 + 5.0 * k, BOLD, mix(p.dim, lit, k)),
                Some(c) if i + 1 == c => (24.0 - 5.0 * k, SEMI, mix(lit, faded, k)),
                Some(c) if i < c => (19.0, SEMI, faded),
                _ => (19.0, SEMI, p.dim),
            };
            col = col.push(text(s).size(size).font(font).color(color).center().width(Fill));
        }
        column![container(col).height(Fill).clip(true), container(source).center_x(Fill)].into()
    }

    /// Slim bar: cover, song and the line being sung on the left; controls on the right.
    fn player_bar(&self, p: Palette, tint: Option<Color>, strength: f32) -> Element<'_, Message> {
        let dur = self.duration();
        let shown = self.seeking.unwrap_or(self.pos).min(dur.max(0.0));
        let seek = slider(0.0..=dur.max(1.0), shown, Message::Seek).on_release(Message::SeekDone).step(0.5f32).height(10).style(style::seek(p));
        let small = |ic: Icon, color: Color, msg: Message| button(icon(ic, 20.0, color)).padding(7).style(style::icon_button(p)).on_press(msg);

        let left: Element<'_, Message> = match self.now_playing() {
            Some(t) => {
                let sung = self
                    .lyrics
                    .as_ref()
                    .and_then(|l| lyrics::current_line(&l.synced, self.pos).map(|i| l.synced[i].1.as_str()))
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&t.artist);
                let menu = Message::Menu(MenuFor { track: t.clone(), queue_index: self.current });
                row![
                    mouse_area(button(self.art(t.thumb.as_ref(), ROW_PX, 48.0, 6.0, p)).padding(0).style(style::row(p, false)).on_press(Message::ToggleNowPlaying))
                        .on_right_press(menu),
                    container(column![line(format!("{} - {}", t.title, t.artist), 12, FONT, p.dim), line(sung, 14, SEMI, p.text)].spacing(3)).width(Fill).clip(true),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into()
            }
            None => line("Pick something to play", 14, FONT, p.dim).into(),
        };

        let mut right = row![line(format!("{} / {}", fmt_time(shown), fmt_time(dur)), 12, FONT, p.dim), space().width(6)]
            .spacing(2)
            .align_y(Alignment::Center);
        if let Some(t) = self.now_playing()
            && self.logged_in()
        {
            let liked = self.liked.contains(&t.video_id);
            let disliked = self.disliked.contains(&t.video_id);
            right = right
                .push(small(if liked { Icon::ThumbUpFilled } else { Icon::ThumbUp }, if liked { p.accent } else { p.dim }, Message::Like(t.clone())))
                .push(small(if disliked { Icon::ThumbDownFilled } else { Icon::ThumbDown }, if disliked { p.accent } else { p.dim }, Message::Dislike(t.clone())))
                .push(small(Icon::PlaylistAdd, p.dim, Message::AddTo(t.clone())));
        }
        let lrc_active = self.now_playing_open && self.tab == Tab::Lyrics;
        right = right
            .push(
                button(line("LRC", 12, BOLD, if lrc_active { p.accent } else { p.dim }))
                    .padding([8, 6])
                    .style(style::icon_button(p))
                    .on_press(Message::ShowLyrics),
            )
            .push(icon(Icon::Volume, 20.0, p.dim))
            .push(slider(0.0..=1.0, self.volume, Message::Volume).step(0.01f32).width(90).style(style::seek(p)))
            .push(space().width(10));
        let play_icon = if self.is_playing() { Icon::Pause } else { Icon::Play };
        let big = |ic: Icon, size: f32, msg: Message| button(icon(ic, size, p.accent)).padding(4).style(style::icon_button(p)).on_press(msg);
        right = right.push(big(Icon::Prev, 30.0, Message::Prev)).push(big(play_icon, 38.0, Message::Toggle)).push(big(Icon::Next, 30.0, Message::Next));

        let controls = row![container(left).width(FillPortion(5)), right].spacing(16).padding([6, 18]).align_y(Alignment::Center);
        let bar_color = tinted(p, p.bar, tint, 0.45 * strength);
        container(column![seek, controls]).width(Fill).height(Length::Shrink).style(style::bg(bar_color)).into()
    }

    fn dialog<'a>(&'a self, p: Palette, d: &'a Dialog) -> Element<'a, Message> {
        let body: Column<'a, Message> = match d {
            Dialog::NewPlaylist(name, song) => {
                let mut c = column![line("New playlist", 22, BOLD, p.text)];
                if let Some(t) = song {
                    c = c.push(line(format!("with \"{}\"", t.title), 13, FONT, p.dim));
                }
                c.push(text_input("Title", name).on_input(Message::NameChanged).on_submit(Message::CreatePlaylist).padding([11, 16]).size(15).style(style::field(p)))
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
                        button(row![self.art(pl.thumb.as_ref(), CARD_PX, 36.0, 4.0, p), line(&pl.title, 14, SEMI, p.text)].spacing(12).align_y(Alignment::Center))
                            .width(Fill)
                            .padding([6, 10])
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
        let shadow = style::shadow(p);
        container(body.spacing(14))
            .width(440)
            .padding(24)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.menu)),
                border: Border { radius: 12.0.into(), ..Default::default() },
                shadow,
                ..Default::default()
            })
            .into()
    }

    /// The song menu.
    fn menu<'a>(&'a self, p: Palette, m: &'a MenuFor) -> Element<'a, Message> {
        let mut col = Column::new().padding([6, 0]);
        for (ic, label, msg) in menu_items(self, m) {
            col = col.push(
                button(row![icon(ic, 20.0, p.dim), line(label, 14, FONT, p.text)].spacing(16).align_y(Alignment::Center))
                    .width(Fill)
                    .height(MENU_ITEM_H)
                    .padding([0, 16])
                    .style(style::menu_item(p))
                    .on_press(msg),
            );
        }
        let shadow = style::shadow(p);
        opaque(container(col).width(MENU_W).style(move |_| container::Style {
            background: Some(Background::Color(p.menu)),
            border: Border { radius: 8.0.into(), width: if p.light { 1.0 } else { 0.0 }, color: p.divider },
            shadow,
            ..Default::default()
        }))
    }
}

/// Height of one lyrics line plus the gap, for the slide.
const LYRIC_STEP: f32 = 40.0;

/// The song menu's rows. Only what RymPlayer can do is listed.
fn menu_items(gui: &Gui, m: &MenuFor) -> Vec<(Icon, &'static str, Message)> {
    let t = &m.track;
    let mut items = vec![
        (Icon::Radio, "Start mix", Message::StartRadio(t.clone())),
        (Icon::PlayNext, "Play next", Message::PlayNext(t.clone())),
        (Icon::Queue, "Add to queue", Message::AddToQueue(t.clone())),
    ];
    if gui.logged_in() {
        let liked = gui.liked.contains(&t.video_id);
        items.push((Icon::ThumbUp, if liked { "Remove from liked songs" } else { "Add to liked songs" }, Message::Like(t.clone())));
    }
    items.push((Icon::Download, "Download", Message::Download(t.clone())));
    if gui.logged_in() {
        items.push((Icon::PlaylistAdd, "Save to playlist", Message::AddTo(t.clone())));
    }
    if let Some(i) = m.queue_index {
        items.push((Icon::Remove, "Remove from queue", Message::RemoveFromQueue(i)));
    }
    if t.album_id.is_some() {
        items.push((Icon::Album, "Go to album", Message::GoAlbum(t.clone())));
    }
    if t.artist_id.is_some() {
        items.push((Icon::Artist, "Go to artist", Message::GoArtist(t.clone())));
    }
    items.push((Icon::Share, "Share", Message::Share(t.clone())));
    if m.queue_index.is_some() && gui.queue.len() > 1 {
        items.push((Icon::ClearQueue, "Dismiss queue", Message::DismissQueue));
    }
    items
}

pub fn menu_height(gui: &Gui, m: &MenuFor) -> f32 {
    menu_items(gui, m).len() as f32 * MENU_ITEM_H + 12.0
}

impl Gui {
    /// Pages rise into place when they open.
    fn slide_in(&self) -> f32 {
        24.0 * (1.0 - ease(self.page_shown, 380.0))
    }
}
