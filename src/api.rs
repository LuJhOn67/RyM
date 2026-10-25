//! Thin wrapper over ytmapi-rs that hides the auth type and flattens results.

use std::fmt;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::{LikeStatus, PlaylistID, Thumbnail, VideoID, YoutubeID};
use ytmapi_rs::parse::PlaylistItem;

/// Liked Music is a regular playlist with a fixed id.
pub const LIKED_ID: &str = "LM";

#[derive(Debug, Clone)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub thumb: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: String,
    pub title: String,
}

#[derive(Clone)]
pub enum Api {
    Guest(Arc<YtMusic<NoAuthToken>>),
    User(Arc<YtMusic<BrowserToken>>),
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.logged_in() { "Api(User)" } else { "Api(Guest)" })
    }
}

fn err(e: impl fmt::Debug) -> anyhow::Error {
    anyhow!("{e:?}")
}

/// Largest thumbnail, upscaled: YTM image URLs accept a size suffix.
fn best_thumb(thumbs: &[Thumbnail]) -> Option<String> {
    let t = thumbs.iter().max_by_key(|t| t.width)?;
    Some(match t.url.find("=w") {
        Some(i) => format!("{}=w544-h544-l90-rj", &t.url[..i]),
        None => t.url.clone(),
    })
}

impl Api {
    pub async fn guest() -> Result<Api> {
        Ok(Api::Guest(Arc::new(YtMusic::new_unauthenticated().await.map_err(err)?)))
    }

    pub async fn with_cookie(cookie: String) -> Result<Api> {
        Ok(Api::User(Arc::new(YtMusic::from_cookie(cookie.trim()).await.map_err(err)?)))
    }

    pub fn logged_in(&self) -> bool {
        matches!(self, Api::User(_))
    }

    pub async fn search(self, query: String) -> Result<Vec<Track>> {
        let songs = match &self {
            Api::Guest(y) => y.search_songs(query.as_str()).await,
            Api::User(y) => y.search_songs(query.as_str()).await,
        }
        .map_err(err)?;
        Ok(songs
            .into_iter()
            .map(|s| Track {
                video_id: s.video_id.get_raw().to_string(),
                title: s.title,
                artist: s.artist,
                album: s.album.map(|a| a.name).unwrap_or_default(),
                duration: s.duration,
                thumb: best_thumb(&s.thumbnails),
            })
            .collect())
    }

    pub async fn library_playlists(self) -> Result<Vec<Playlist>> {
        let Api::User(y) = &self else { return Ok(Vec::new()) };
        let lists = y.get_library_playlists().await.map_err(err)?;
        Ok(lists
            .into_iter()
            .filter(|p| p.playlist_id.get_raw() != LIKED_ID)
            .map(|p| Playlist {
                id: p.playlist_id.get_raw().to_string(),
                title: p.title,
            })
            .collect())
    }

    pub async fn playlist_tracks(self, id: String) -> Result<Vec<Track>> {
        let pid = PlaylistID::from_raw(id);
        let items = match &self {
            Api::Guest(y) => y.get_playlist_tracks(pid).await,
            Api::User(y) => y.get_playlist_tracks(pid).await,
        }
        .map_err(err)?;
        Ok(items
            .into_iter()
            .filter_map(|item| match item {
                PlaylistItem::Song(s) if s.is_available => Some(Track {
                    video_id: s.video_id.get_raw().to_string(),
                    title: s.title,
                    artist: s.artists.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
                    album: s.album.name,
                    duration: s.duration,
                    thumb: best_thumb(&s.thumbnails),
                }),
                PlaylistItem::Video(v) if v.is_available => Some(Track {
                    video_id: v.video_id.get_raw().to_string(),
                    title: v.title,
                    artist: v.channel_name,
                    album: String::new(),
                    duration: v.duration,
                    thumb: best_thumb(&v.thumbnails),
                }),
                _ => None,
            })
            .collect())
    }

    pub async fn rate(self, video_id: String, liked: bool) -> Result<()> {
        let Api::User(y) = &self else { return Err(anyhow!("sign in to like songs")) };
        let status = if liked { LikeStatus::Liked } else { LikeStatus::Indifferent };
        y.rate_song(VideoID::from_raw(video_id), status).await.map_err(err)
    }

    /// Tells YouTube Music the song was played, so history and recommendations stay in sync.
    pub async fn report_play(self, video_id: String) -> Result<()> {
        let Api::User(y) = &self else { return Ok(()) };
        let url = y.get_song_tracking_url(VideoID::from_raw(video_id)).await.map_err(err)?;
        y.add_history_item(url).await.map_err(err)?;
        Ok(())
    }
}
