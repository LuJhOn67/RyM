#![windows_subsystem = "windows"]

mod api;
mod audio;
mod lyrics;
mod stream;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{
    button, center, column, container, image, row, rule, scrollable, slider, space, text,
    text_input,
};
use iced::{Alignment, Color, Element, Fill, Font, Length, Subscription, Task, Theme, font};

use api::{Api, Playlist, Track};
use audio::Audio;
use lyrics::Lyrics;

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ytm-player")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cookie_path() -> PathBuf {
    data_dir().join("cookie.txt")
}

fn ytdlp_cookie_path() -> PathBuf {
    data_dir().join("cookies-ytdlp.txt")
}

const ACCENT: Color = Color::from_rgb(1.0, 0.0, 0.2);
const DIM: Color = Color::from_rgb(0.62, 0.62, 0.62);
const BOLD: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .window_size((1280.0, 800.0))
        .centered()
        .run()
}

#[derive(Debug, Clone, PartialEq)]
enum Screen {
    Login,
    Main,
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Search,
    Playlist(String),
}

struct App {
    screen: Screen,
    cookie_input: String,
    busy: bool,
    status: String,
    api: Option<Api>,

    playlists: Vec<Playlist>,
    page: Page,
    page_title: String,
    tracks: Vec<Track>,
    search: String,

    queue: Vec<Track>,
    current: Option<usize>,
    loading: bool,
    audio: Option<Audio>,
    pos: f32,
    seeking: Option<f32>,
    volume: f32,

    lyrics: Option<Lyrics>,
    lyrics_status: String,
    art: Option<image::Handle>,
    liked: HashSet<String>,
}

#[derive(Debug, Clone)]
enum Message {
    CookieChanged(String),
    Login,
    Guest,
    LoggedIn(Result<Api, String>),
    Logout,

    Playlists(Result<Vec<Playlist>, String>),
    OpenSearch,
    OpenPlaylist(String, String),
    Tracks(Page, Result<Vec<Track>, String>),
    SearchChanged(String),
    SearchSubmit,

    Play(usize),
    Fetched(String, Result<PathBuf, String>),
    Prefetched,
    Toggle,
    Next,
    Prev,
    SeekChanged(f32),
    SeekReleased,
    Volume(f32),
    Tick,

    Lyrics(String, Result<Option<Lyrics>, String>),
    Art(String, Result<Vec<u8>, String>),
    Like,
    Rated(String, bool, Result<(), String>),
    Ignore,
}

fn s<T>(r: anyhow::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

fn parse_duration(d: &str) -> Option<f32> {
    let mut secs = 0.0;
    for part in d.split(':') {
        secs = secs * 60.0 + part.trim().parse::<f32>().ok()?;
    }
    Some(secs)
}

fn fmt_time(secs: f32) -> String {
    let t = secs.max(0.0) as u32;
    format!("{}:{:02}", t / 60, t % 60)
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let saved = std::fs::read_to_string(cookie_path()).ok().filter(|c| !c.trim().is_empty());
        let mut app = App {
            screen: Screen::Login,
            cookie_input: String::new(),
            busy: false,
            status: String::new(),
            api: None,
            playlists: Vec::new(),
            page: Page::Search,
            page_title: "Search".into(),
            tracks: Vec::new(),
            search: String::new(),
            queue: Vec::new(),
            current: None,
            loading: false,
            audio: None,
            pos: 0.0,
            seeking: None,
            volume: 0.8,
            lyrics: None,
            lyrics_status: String::new(),
            art: None,
            liked: HashSet::new(),
        };
        match Audio::new() {
            Ok(a) => {
                a.set_volume(app.volume);
                app.audio = Some(a);
            }
            Err(e) => app.status = format!("No audio device: {e}"),
        }
        let task = match saved {
            Some(cookie) => {
                app.busy = true;
                app.status = "Signing in...".into();
                Task::perform(Api::with_cookie(cookie), |r| Message::LoggedIn(s(r)))
            }
            None => Task::none(),
        };
        (app, task)
    }

    fn title(&self) -> String {
        match self.now_playing() {
            Some(t) => format!("{} - {} | YTM Player", t.title, t.artist),
            None => "YTM Player".into(),
        }
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "YTM",
            iced::theme::Palette {
                background: Color::from_rgb8(0x0f, 0x0f, 0x0f),
                text: Color::WHITE,
                primary: ACCENT,
                success: Color::from_rgb8(0x2e, 0xcc, 0x71),
                warning: Color::from_rgb8(0xf3, 0x9c, 0x12),
                danger: Color::from_rgb8(0xe7, 0x4c, 0x3c),
            },
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.current.is_some() {
            iced::time::every(Duration::from_millis(200)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn now_playing(&self) -> Option<&Track> {
        self.current.and_then(|i| self.queue.get(i))
    }

    fn api(&self) -> Option<Api> {
        self.api.clone()
    }

    fn cookie_file(&self) -> Option<PathBuf> {
        let p = ytdlp_cookie_path();
        (self.api.as_ref().is_some_and(Api::logged_in) && p.exists()).then_some(p)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CookieChanged(c) => self.cookie_input = c,
            Message::Login => {
                let cookie = self.cookie_input.trim().to_string();
                if cookie.is_empty() {
                    self.status = "Paste your cookie first.".into();
                    return Task::none();
                }
                self.busy = true;
                self.status = "Signing in...".into();
                let dir = data_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(cookie_path(), &cookie);
                let _ = stream::write_cookie_file(&cookie, &ytdlp_cookie_path());
                return Task::perform(Api::with_cookie(cookie), |r| Message::LoggedIn(s(r)));
            }
            Message::Guest => {
                self.busy = true;
                self.status = "Connecting...".into();
                return Task::perform(Api::guest(), |r| Message::LoggedIn(s(r)));
            }
            Message::LoggedIn(Ok(api)) => {
                self.busy = false;
                self.status.clear();
                self.cookie_input.clear();
                let user = api.logged_in();
                self.api = Some(api.clone());
                self.screen = Screen::Main;
                if user {
                    if let Ok(c) = std::fs::read_to_string(cookie_path()) {
                        let _ = stream::write_cookie_file(&c, &ytdlp_cookie_path());
                    }
                    return Task::batch([
                        Task::perform(api.clone().library_playlists(), |r| Message::Playlists(s(r))),
                        self.open_playlist(api::LIKED_ID.into(), "Liked songs".into()),
                    ]);
                }
            }
            Message::LoggedIn(Err(e)) => {
                self.busy = false;
                self.screen = Screen::Login;
                self.status = format!("Sign-in failed: {e}");
            }
            Message::Logout => {
                let _ = std::fs::remove_file(cookie_path());
                let _ = std::fs::remove_file(ytdlp_cookie_path());
                if let Some(a) = &mut self.audio {
                    a.stop();
                }
                self.api = None;
                self.playlists.clear();
                self.tracks.clear();
                self.queue.clear();
                self.current = None;
                self.lyrics = None;
                self.art = None;
                self.screen = Screen::Login;
            }

            Message::Playlists(Ok(p)) => self.playlists = p,
            Message::Playlists(Err(e)) => self.status = format!("Playlists: {e}"),
            Message::OpenSearch => {
                self.page = Page::Search;
                self.page_title = "Search".into();
                self.tracks.clear();
            }
            Message::OpenPlaylist(id, title) => return self.open_playlist(id, title),
            Message::Tracks(page, result) => {
                if page == self.page {
                    self.busy = false;
                    match result {
                        Ok(t) => {
                            if page == Page::Playlist(api::LIKED_ID.into()) {
                                self.liked.extend(t.iter().map(|t| t.video_id.clone()));
                            }
                            self.status.clear();
                            self.tracks = t;
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            Message::SearchChanged(q) => self.search = q,
            Message::SearchSubmit => {
                let q = self.search.trim().to_string();
                if q.is_empty() {
                    return Task::none();
                }
                let Some(api) = self.api() else { return Task::none() };
                self.page = Page::Search;
                self.page_title = format!("Results for \"{q}\"");
                self.busy = true;
                self.tracks.clear();
                return Task::perform(api.search(q), |r| Message::Tracks(Page::Search, s(r)));
            }

            Message::Play(i) => {
                self.queue = self.tracks.clone();
                return self.play(i);
            }
            Message::Fetched(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) != Some(&vid) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(path) => {
                        if let Some(a) = &mut self.audio {
                            match a.play_file(&path) {
                                Ok(()) => {
                                    a.set_volume(self.volume);
                                    self.status.clear();
                                }
                                Err(e) => self.status = format!("Can't play: {e}"),
                            }
                        }
                        return self.prefetch_next();
                    }
                    Err(e) => self.status = format!("Download failed: {e}"),
                }
            }
            Message::Prefetched | Message::Ignore => {}
            Message::Toggle => {
                if let Some(a) = &self.audio {
                    a.toggle();
                }
            }
            Message::Next => {
                if let Some(i) = self.current
                    && i + 1 < self.queue.len()
                {
                    return self.play(i + 1);
                }
            }
            Message::Prev => {
                if let Some(i) = self.current {
                    // Like most players: restart the song unless we're at its start.
                    if self.pos > 3.0 || i == 0 {
                        if let Some(a) = &self.audio {
                            a.seek(0.0);
                        }
                    } else {
                        return self.play(i - 1);
                    }
                }
            }
            Message::SeekChanged(v) => self.seeking = Some(v),
            Message::SeekReleased => {
                if let (Some(v), Some(a)) = (self.seeking.take(), &self.audio) {
                    a.seek(v);
                    self.pos = v;
                }
            }
            Message::Volume(v) => {
                self.volume = v;
                if let Some(a) = &self.audio {
                    a.set_volume(v);
                }
            }
            Message::Tick => {
                if let Some(a) = &self.audio {
                    self.pos = a.position();
                    if !self.loading && a.finished() {
                        let next = self.current.map(|i| i + 1).filter(|&n| n < self.queue.len());
                        return match next {
                            Some(n) => self.play(n),
                            None => {
                                if let Some(a) = &mut self.audio {
                                    a.stop();
                                }
                                Task::none()
                            }
                        };
                    }
                }
            }

            Message::Lyrics(vid, result) => {
                if self.now_playing().map(|t| &t.video_id) == Some(&vid) {
                    match result {
                        Ok(Some(l)) => {
                            self.lyrics = Some(l);
                            self.lyrics_status.clear();
                        }
                        Ok(None) => self.lyrics_status = "No lyrics found".into(),
                        Err(e) => self.lyrics_status = format!("Lyrics: {e}"),
                    }
                }
            }
            Message::Art(vid, Ok(bytes)) => {
                if self.now_playing().map(|t| &t.video_id) == Some(&vid) {
                    self.art = Some(image::Handle::from_bytes(bytes));
                }
            }
            Message::Art(_, Err(_)) => {}
            Message::Like => {
                let (Some(t), Some(api)) = (self.now_playing(), self.api()) else {
                    return Task::none();
                };
                let vid = t.video_id.clone();
                let like = !self.liked.contains(&vid);
                // Optimistic: flip now, roll back if the request fails.
                if like {
                    self.liked.insert(vid.clone());
                } else {
                    self.liked.remove(&vid);
                }
                return Task::perform(api.rate(vid.clone(), like), move |r| {
                    Message::Rated(vid.clone(), like, s(r))
                });
            }
            Message::Rated(vid, like, Err(e)) => {
                if like {
                    self.liked.remove(&vid);
                } else {
                    self.liked.insert(vid);
                }
                self.status = format!("Like failed: {e}");
            }
            Message::Rated(_, _, Ok(())) => {}
        }
        Task::none()
    }

    fn open_playlist(&mut self, id: String, title: String) -> Task<Message> {
        let Some(api) = self.api() else { return Task::none() };
        let page = Page::Playlist(id.clone());
        self.page = page.clone();
        self.page_title = title;
        self.busy = true;
        self.tracks.clear();
        Task::perform(api.playlist_tracks(id), move |r| Message::Tracks(page.clone(), s(r)))
    }

    fn play(&mut self, i: usize) -> Task<Message> {
        let Some(track) = self.queue.get(i).cloned() else { return Task::none() };
        self.current = Some(i);
        self.pos = 0.0;
        self.seeking = None;
        self.lyrics = None;
        self.lyrics_status = "Loading lyrics...".into();
        self.art = None;
        self.loading = true;
        if let Some(a) = &mut self.audio {
            a.stop();
        }

        let vid = track.video_id.clone();
        let mut tasks = vec![Task::perform(
            stream::fetch(vid.clone(), self.cookie_file()),
            {
                let vid = vid.clone();
                move |r| Message::Fetched(vid.clone(), s(r))
            },
        )];
        let dur = parse_duration(&track.duration);
        tasks.push(Task::perform(
            lyrics::lrclib(track.title.clone(), track.artist.clone(), track.album.clone(), dur),
            {
                let vid = vid.clone();
                move |r| Message::Lyrics(vid.clone(), s(r))
            },
        ));
        if let Some(url) = track.thumb.clone() {
            let vid = vid.clone();
            tasks.push(Task::perform(
                async move {
                    let bytes = reqwest::get(url).await?.error_for_status()?.bytes().await?;
                    anyhow::Ok(bytes.to_vec())
                },
                move |r| Message::Art(vid.clone(), s(r)),
            ));
        }
        if let Some(api) = self.api() {
            tasks.push(Task::perform(api.report_play(vid), |_| Message::Ignore));
        }
        Task::batch(tasks)
    }

    fn prefetch_next(&self) -> Task<Message> {
        let next = self.current.and_then(|i| self.queue.get(i + 1));
        match next {
            Some(t) if stream::cached(&t.video_id).is_none() => {
                Task::perform(stream::fetch(t.video_id.clone(), self.cookie_file()), |_| {
                    Message::Prefetched
                })
            }
            _ => Task::none(),
        }
    }

    // ---------------------------------------------------------------- view

    fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Login => self.view_login(),
            Screen::Main => self.view_main(),
        }
    }

    fn view_login(&self) -> Element<'_, Message> {
        let steps = column![
            text("1. Open music.youtube.com in your browser and sign in."),
            text("2. Press F12, open the Network tab and refresh the page."),
            text("3. Click any request to music.youtube.com (for example \"browse\")."),
            text("4. Under Request Headers, copy the whole value of \"cookie\" and paste it below."),
        ]
        .spacing(6);
        let mut login = button(text("Sign in").font(BOLD)).padding([10, 24]);
        let mut guest = button("Continue without account").style(button::secondary).padding([10, 24]);
        if !self.busy {
            login = login.on_press(Message::Login);
            guest = guest.on_press(Message::Guest);
        }
        let card = column![
            text("YTM Player").size(36).font(BOLD).color(ACCENT),
            text("Sign in with your YouTube Music account").size(18),
            steps,
            text_input("Paste cookie here", &self.cookie_input)
                .on_input(Message::CookieChanged)
                .on_submit(Message::Login)
                .secure(true)
                .padding(10),
            row![login, guest].spacing(12),
            text(&self.status).color(DIM),
            text("Your cookie is stored only on this computer.").size(12).color(DIM),
        ]
        .spacing(16)
        .max_width(640);
        center(container(card).padding(32).style(container::rounded_box)).into()
    }

    fn view_main(&self) -> Element<'_, Message> {
        let body = row![self.view_sidebar(), self.view_tracks(), self.view_lyrics()];
        column![
            container(body).height(Fill),
            rule::horizontal(1),
            self.view_player_bar()
        ]
        .into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let nav = |label: String, active: bool, msg: Message| {
            button(text(label).font(if active { BOLD } else { Font::DEFAULT }))
                .width(Fill)
                .style(if active { button::secondary } else { button::text })
                .on_press(msg)
        };
        let logged_in = self.api.as_ref().is_some_and(Api::logged_in);
        let mut items = column![
            text("YTM Player").size(22).font(BOLD).color(ACCENT),
            space().height(8),
            nav("Search".into(), self.page == Page::Search, Message::OpenSearch),
        ]
        .spacing(4);
        if logged_in {
            items = items.push(nav(
                "Liked songs".into(),
                self.page == Page::Playlist(api::LIKED_ID.into()),
                Message::OpenPlaylist(api::LIKED_ID.into(), "Liked songs".into()),
            ));
            items = items.push(space().height(12));
            items = items.push(text("PLAYLISTS").size(12).color(DIM));
            for p in &self.playlists {
                items = items.push(nav(
                    p.title.clone(),
                    self.page == Page::Playlist(p.id.clone()),
                    Message::OpenPlaylist(p.id.clone(), p.title.clone()),
                ));
            }
        }
        let account = button(text(if logged_in { "Sign out" } else { "Sign in" }).size(13))
            .style(button::text)
            .on_press(Message::Logout);
        container(column![scrollable(items).height(Fill), account].spacing(8))
            .width(240)
            .height(Fill)
            .padding(16)
            .style(|t: &Theme| container::background(t.extended_palette().background.weak.color))
            .into()
    }

    fn view_tracks(&self) -> Element<'_, Message> {
        let search = text_input("Search songs, artists...", &self.search)
            .on_input(Message::SearchChanged)
            .on_submit(Message::SearchSubmit)
            .padding(10);

        let playing = self.now_playing().map(|t| t.video_id.as_str());
        let mut list = column![].spacing(2);
        for (i, t) in self.tracks.iter().enumerate() {
            let is_current = playing == Some(t.video_id.as_str());
            let color = if is_current { ACCENT } else { Color::WHITE };
            let line = row![
                text(format!("{}", i + 1)).width(36).color(DIM),
                column![
                    text(&t.title).color(color).font(BOLD),
                    text(&t.artist).size(13).color(DIM),
                ]
                .width(Length::FillPortion(3)),
                text(&t.album).size(13).color(DIM).width(Length::FillPortion(2)),
                text(&t.duration).size(13).color(DIM).width(56),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            list = list.push(button(line).width(Fill).padding([8, 10]).style(button::text).on_press(Message::Play(i)));
        }

        let header = row![
            text(&self.page_title).size(28).font(BOLD),
            space::horizontal(),
            text(if self.busy { "Loading..." } else { "" }).color(DIM),
        ]
        .align_y(Alignment::Center);

        let mut col = column![search, header].spacing(16);
        if !self.status.is_empty() {
            col = col.push(text(&self.status).color(ACCENT));
        }
        if self.tracks.is_empty() && !self.busy && self.page == Page::Search {
            col = col.push(text("Search for something to play.").color(DIM));
        }
        col = col.push(scrollable(list).height(Fill));
        container(col).width(Fill).height(Fill).padding(20).into()
    }

    fn view_lyrics(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(10);
        if let Some(art) = &self.art {
            col = col.push(image(art.clone()).width(Fill));
        }
        if let Some(t) = self.now_playing() {
            col = col.push(column![text(&t.title).size(18).font(BOLD), text(&t.artist).color(DIM)]);
        }
        col = col.push(rule::horizontal(1));

        match &self.lyrics {
            Some(l) if !l.synced.is_empty() => {
                let cur = lyrics::current_line(&l.synced, self.pos);
                let start = cur.unwrap_or(0).saturating_sub(2);
                for (i, (_, line)) in l.synced.iter().enumerate().skip(start).take(14) {
                    let active = Some(i) == cur;
                    let past = cur.is_some_and(|c| i < c);
                    let line = if line.is_empty() { "♪" } else { line.as_str() };
                    col = col.push(
                        text(line)
                            .size(if active { 22 } else { 17 })
                            .font(if active { BOLD } else { Font::DEFAULT })
                            .color(if active {
                                Color::WHITE
                            } else if past {
                                Color::from_rgb(0.4, 0.4, 0.4)
                            } else {
                                DIM
                            }),
                    );
                }
                col = col.push(text(format!("Lyrics: {}", l.source)).size(11).color(DIM));
            }
            Some(l) => {
                let plain: Vec<Element<'_, Message>> =
                    l.plain.iter().map(|line| text(line).size(16).into()).collect();
                col = col.push(scrollable(column(plain).spacing(6)).height(Fill));
            }
            None => col = col.push(text(&self.lyrics_status).color(DIM)),
        }
        container(col)
            .width(340)
            .height(Fill)
            .padding(16)
            .clip(true)
            .style(|t: &Theme| container::background(t.extended_palette().background.weak.color))
            .into()
    }

    fn view_player_bar(&self) -> Element<'_, Message> {
        let now = self.now_playing();
        let info: Element<'_, Message> = match now {
            Some(t) => column![
                text(&t.title).font(BOLD),
                text(&t.artist).size(13).color(DIM),
            ]
            .width(280)
            .into(),
            None => text("Nothing playing").color(DIM).width(280).into(),
        };

        let audio_dur = self.audio.as_ref().and_then(Audio::duration);
        let dur = audio_dur.or_else(|| now.and_then(|t| parse_duration(&t.duration))).unwrap_or(0.0);
        let shown = self.seeking.unwrap_or(self.pos).min(dur.max(0.0));
        let paused = self.audio.as_ref().is_none_or(Audio::is_paused);
        let play_label = if self.loading {
            "..."
        } else if paused {
            "▶"
        } else {
            "⏸"
        };
        let ctrl = |label: &'static str, msg: Message| {
            button(text(label).size(18).center()).width(44).style(button::text).on_press(msg)
        };
        let mut seek = slider(0.0..=dur.max(1.0), shown, Message::SeekChanged).step(0.5f32);
        if audio_dur.is_some() {
            seek = seek.on_release(Message::SeekReleased);
        }
        let controls = column![
            row![
                ctrl("⏮", Message::Prev),
                button(text(play_label).size(20).center())
                    .width(52)
                    .style(button::primary)
                    .on_press(Message::Toggle),
                ctrl("⏭", Message::Next),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                text(fmt_time(shown)).size(12).color(DIM),
                seek,
                text(fmt_time(dur)).size(12).color(DIM),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(6)
        .align_x(Alignment::Center)
        .width(Fill);

        let liked = now.is_some_and(|t| self.liked.contains(&t.video_id));
        let like = button(text(if liked { "♥" } else { "♡" }).size(20).color(if liked { ACCENT } else { Color::WHITE }))
            .style(button::text)
            .on_press(Message::Like);
        let volume = row![
            like,
            text("Vol").size(12).color(DIM),
            slider(0.0..=1.0, self.volume, Message::Volume).step(0.01f32).width(120),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .width(280);

        container(row![info, controls, volume].spacing(24).align_y(Alignment::Center))
            .padding([12, 20])
            .width(Fill)
            .into()
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the network: `cargo test -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn search_fetch_decode_lyrics() {
        let api = Api::guest().await.expect("guest api");
        let tracks = api.search("Rick Astley Never Gonna Give You Up".into()).await.expect("search");
        let t = tracks.first().expect("results").clone();
        println!("first hit: {} - {} [{}] {} {:?}", t.title, t.artist, t.album, t.duration, t.thumb);

        let path = stream::fetch(t.video_id.clone(), None).await.expect("fetch");
        println!("audio: {}", path.display());
        let secs = audio::decode_seconds(&path).expect("decode");
        println!("decoded {secs:.1}s of audio");
        assert!(secs > 60.0);

        let l = lyrics::lrclib(t.title, t.artist, t.album, parse_duration(&t.duration)).await.expect("lyrics");
        let l = l.expect("lyrics found");
        println!("lyrics: {} synced lines, first: {:?}", l.synced.len(), l.synced.first());
        assert!(!l.synced.is_empty());
    }
}
