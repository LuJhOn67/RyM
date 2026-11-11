//! Discord Rich Presence: the playing song (title, artist, cover, time left) shown on the user's
//! Discord profile, through the Discord app running on this computer (its local IPC socket).
//! Needs an application id from Discord's developer portal (Settings: Discord). A thread of its
//! own talks to Discord, so a missing or closed Discord never holds up the app.
//! Two buttons go with it (others see them on the profile): the song on YouTube Music (it opens
//! in RyM on a phone that has it), and where to get RyM.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};

use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps};

/// Where the "Get RyM" button goes.
const GET_RYM: &str = "https://github.com/LuJhOn67/RyM/releases/latest";
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

/// What Discord should show.
#[derive(Debug, Clone, PartialEq)]
pub enum Show {
    /// A song: its id, title, artist, cover link, seconds played, its length (0 = unknown); paused or not.
    Song { video_id: String, title: String, artist: String, cover: Option<String>, at: f32, length: f32, paused: bool },
    Nothing,
    /// Turned off: the connection is closed.
    Off,
}

/// The thread that talks to Discord: its application id, and where to send what to show.
type Talker = Mutex<Option<(String, Sender<Show>)>>;

fn sender() -> &'static Talker {
    static S: OnceLock<Talker> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Shows `what` on Discord for the application `app_id` (empty id: nothing happens).
pub fn show(app_id: &str, what: Show) {
    let Ok(mut s) = sender().lock() else { return };
    if app_id.trim().is_empty() {
        if let Some((_, tx)) = s.take() {
            let _ = tx.send(Show::Off);
        }
        return;
    }
    if s.as_ref().is_none_or(|(id, _)| id != app_id) {
        if let Some((_, tx)) = s.take() {
            let _ = tx.send(Show::Off);
        }
        let (tx, rx) = channel::<Show>();
        let id = app_id.trim().to_string();
        std::thread::spawn(move || run(id, rx));
        *s = Some((app_id.to_string(), tx));
    }
    if let Some((_, tx)) = s.as_ref() {
        let _ = tx.send(what);
    }
}

/// Talks to Discord: connects when there's something to show (again after Discord restarts).
fn run(app_id: String, rx: std::sync::mpsc::Receiver<Show>) {
    let mut client: Option<DiscordIpcClient> = None;
    while let Ok(mut what) = rx.recv() {
        // Only the latest wish matters.
        while let Ok(newer) = rx.try_recv() {
            what = newer;
        }
        if what == Show::Off {
            if let Some(mut c) = client.take() {
                let _ = c.close();
            }
            return;
        }
        if client.is_none() {
            let mut c = DiscordIpcClient::new(&app_id);
            if c.connect().is_err() {
                continue;
            }
            client = Some(c);
        }
        let Some(c) = client.as_mut() else { continue };
        let sent = match &what {
            Show::Song { video_id, title, artist, cover, at, length, paused } => {
                let now = crate::updates::now() as i64;
                let mut assets = Assets::new().large_text(title.as_str());
                if let Some(url) = cover {
                    assets = assets.large_image(url.as_str());
                }
                let mut a = Activity::new()
                    .activity_type(ActivityType::Listening)
                    .status_display_type(StatusDisplayType::Details)
                    .details(title.as_str())
                    .state(artist.as_str())
                    .assets(assets);
                let song_link = crate::is_youtube(video_id).then(|| format!("https://music.youtube.com/watch?v={video_id}"));
                let mut buttons = Vec::new();
                if let Some(link) = &song_link {
                    buttons.push(Button::new("Play this song", link.as_str()));
                }
                buttons.push(Button::new("Get RyM", GET_RYM));
                a = a.buttons(buttons);
                if !paused {
                    let start = now - *at as i64;
                    let mut ts = Timestamps::new().start(start);
                    if *length > 0.0 {
                        ts = ts.end(start + *length as i64);
                    }
                    a = a.timestamps(ts);
                }
                c.set_activity(a)
            }
            _ => c.clear_activity(),
        };
        // Discord closed: connect again next time.
        if sent.is_err() {
            client = None;
        }
    }
}
