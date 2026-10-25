//! Thin wrapper over ytmapi-rs that hides the auth type and flattens results.

use std::fmt;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::{LikeStatus, PlaylistID, VideoID, YoutubeID};
use ytmapi_rs::parse::PlaylistItem;
use ytmapi_rs::query::GetLibraryPlaylistsQuery;

/// Liked Music is a regular playlist with a fixed id.
pub const LIKED_ID: &str = "LM";

#[derive(Debug, Clone)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
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
            })
            .collect())
    }

    /// Parsed by hand: ytmapi-rs fails the whole list when one playlist lacks a subtitle field.
    pub async fn library_playlists(self) -> Result<Vec<Playlist>> {
        let Api::User(y) = &self else { return Ok(Vec::new()) };
        let raw = y.raw_json_query(GetLibraryPlaylistsQuery).await.map_err(err)?;
        let json: serde_json::Value = serde_json::from_str(&raw)?;
        let items = json
            .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/gridRenderer/items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("unexpected library response"))?;
        Ok(items
            .iter()
            .filter_map(|item| {
                let r = item.get("musicTwoRowItemRenderer")?;
                let id = r.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?;
                let title = r.pointer("/title/runs/0/text")?.as_str()?;
                // Skips the "New playlist" tile and Liked Music (shown separately).
                (id.starts_with("VL") && id != "VLLM").then(|| Playlist {
                    id: id.to_string(),
                    title: title.to_string(),
                })
            })
            .collect())
    }

    pub async fn playlist_tracks(self, id: String) -> Result<Vec<Track>> {
        // ytmapi-rs sends the id as the browseId, which needs the "VL" prefix.
        let id = if id.starts_with("VL") { id } else { format!("VL{id}") };
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
                }),
                PlaylistItem::Video(v) if v.is_available => Some(Track {
                    video_id: v.video_id.get_raw().to_string(),
                    title: v.title,
                    artist: v.channel_name,
                    album: String::new(),
                    duration: v.duration,
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
