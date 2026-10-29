//! Thin wrapper over ytmapi-rs that hides the auth type and flattens results.

use std::fmt;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use serde_json::Value;
use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::AuthToken;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::{
    AlbumID, ArtistChannelID, LikeStatus, PlaylistID, Thumbnail, VideoID, YoutubeID,
};
use ytmapi_rs::parse::{HistoryItem, ParseFrom, PlaylistItem, ProcessedResult, WatchPlaylistTrack};
use ytmapi_rs::query::playlist::{CreatePlaylistQuery, PrivacyStatus};
use ytmapi_rs::query::{
    GetLibraryPlaylistsQuery, GetWatchPlaylistQuery, PostMethod, PostQuery, Query,
};

/// Liked Music is a regular playlist with a fixed id.
pub const LIKED_ID: &str = "LM";

#[derive(Debug, Clone)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    /// Cover art URL (any size; see [`thumb_sized`]).
    pub thumb: Option<String>,
    /// For "Go to album" / "Go to artist", when YouTube Music says.
    pub album_id: Option<String>,
    pub artist_id: Option<String>,
    /// Set for rows that open something instead of playing (home feed playlists, albums, mixes).
    pub link: Option<Link>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    Playlist(String),
    Album(String),
    /// Radio-style mix, played through the watch queue.
    Mix(String),
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub thumb: Option<String>,
}

#[derive(Clone)]
pub enum Api {
    Guest(Arc<YtMusic<NoAuthToken>>),
    User(Arc<YtMusic<BrowserToken>>),
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.logged_in() {
            "Api(User)"
        } else {
            "Api(Guest)"
        })
    }
}

fn err(e: impl fmt::Debug) -> anyhow::Error {
    anyhow!("{e:?}")
}

impl Api {
    pub async fn guest() -> Result<Api> {
        Ok(Api::Guest(Arc::new(
            YtMusic::new_unauthenticated().await.map_err(err)?,
        )))
    }

    pub async fn with_cookie(cookie: String) -> Result<Api> {
        let cookie = normalize_sapisid_cookie(cookie.trim());
        Ok(Api::User(Arc::new(
            YtMusic::from_cookie(&cookie).await.map_err(err)?,
        )))
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
                album_id: s.album.as_ref().map(|a| a.id.get_raw().to_string()),
                artist_id: None,
                title: s.title,
                artist: s.artist,
                album: s.album.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
                duration: s.duration,
                link: None,
                thumb: best_thumb(&s.thumbnails),
            })
            .collect())
    }

    /// Parsed by hand: ytmapi-rs fails the whole list when one playlist lacks a subtitle field.
    pub async fn library_playlists(self) -> Result<Vec<Playlist>> {
        let Api::User(y) = &self else {
            return Ok(Vec::new());
        };
        let raw = y
            .raw_json_query(GetLibraryPlaylistsQuery)
            .await
            .map_err(err)?;
        let json: serde_json::Value = serde_json::from_str(&raw)?;
        let items = json
            .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/gridRenderer/items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                if raw.contains("Sign in to") {
                    anyhow!("YouTube says you're signed out, press Shift+O and sign in again")
                } else {
                    anyhow!("unexpected library response")
                }
            })?;
        Ok(items
            .iter()
            .filter_map(|item| {
                let r = item.get("musicTwoRowItemRenderer")?;
                let id = r
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")?
                    .as_str()?;
                let title = r.pointer("/title/runs/0/text")?.as_str()?;
                // Skips the "New playlist" tile and Liked Music (shown separately).
                (id.starts_with("VL") && id != "VLLM").then(|| Playlist {
                    id: id.to_string(),
                    title: title.to_string(),
                    thumb: json_thumb(r),
                })
            })
            .collect())
    }

    pub async fn playlist_tracks(self, id: String) -> Result<Vec<Track>> {
        // ytmapi-rs sends the id as the browseId, which needs the "VL" prefix.
        let id = if id.starts_with("VL") {
            id
        } else {
            format!("VL{id}")
        };
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
                    album_id: Some(s.album.id.get_raw().to_string()),
                    artist_id: s
                        .artists
                        .first()
                        .and_then(|a| a.id.as_ref())
                        .map(|i| i.get_raw().to_string()),
                    title: s.title,
                    artist: s
                        .artists
                        .into_iter()
                        .map(|a| a.name)
                        .collect::<Vec<_>>()
                        .join(", "),
                    album: s.album.name,
                    duration: s.duration,
                    link: None,
                    thumb: best_thumb(&s.thumbnails),
                }),
                PlaylistItem::Video(v) if v.is_available => Some(Track {
                    video_id: v.video_id.get_raw().to_string(),
                    album_id: None,
                    artist_id: None,
                    title: v.title,
                    artist: v.channel_name,
                    album: String::new(),
                    duration: v.duration,
                    link: None,
                    thumb: best_thumb(&v.thumbnails),
                }),
                _ => None,
            })
            .collect())
    }

    /// The home feed ("For you"): songs play, playlists/albums/mixes open. Reads up to three pages.
    pub async fn home(self) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        let mut continuation = None;
        for _ in 0..3 {
            let query = HomeQuery {
                continuation: continuation.take(),
            };
            let raw = match &self {
                Api::Guest(y) => y.raw_json_query(query).await,
                Api::User(y) => y.raw_json_query(query).await,
            }
            .map_err(err)?;
            let json: Value = serde_json::from_str(&raw)?;
            let list = json
                .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer")
                .or_else(|| json.pointer("/continuationContents/sectionListContinuation"));
            let Some(list) = list else { break };
            for shelf in list
                .get("contents")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(shelf) = shelf.get("musicCarouselShelfRenderer") else {
                    continue;
                };
                let section =
                    text(shelf.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"));
                for item in shelf
                    .get("contents")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = home_item(item, &section) {
                        out.push(t);
                    }
                }
            }
            continuation = list
                .pointer("/continuations/0/nextContinuationData/continuation")
                .and_then(Value::as_str)
                .map(String::from);
            if continuation.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn album_tracks(self, id: String) -> Result<Vec<Track>> {
        let album_id = id.clone();
        let aid = AlbumID::from_raw(id);
        let album = match &self {
            Api::Guest(y) => y.get_album(aid).await,
            Api::User(y) => y.get_album(aid).await,
        }
        .map_err(err)?;
        let artist = album
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let album_thumb = best_thumb(&album.thumbnails);
        let album_artist = album
            .artists
            .first()
            .and_then(|a| a.id.as_ref())
            .map(|i| i.get_raw().to_string());
        Ok(album
            .tracks
            .into_iter()
            .map(|t| Track {
                video_id: t.video_id.get_raw().to_string(),
                album_id: Some(album_id.clone()),
                artist_id: album_artist.clone(),
                title: t.title,
                artist: artist.clone(),
                album: album.title.clone(),
                duration: t.duration,
                link: None,
                thumb: album_thumb.clone(),
            })
            .collect())
    }

    /// YouTube Music's "up next" radio for a song (the song itself comes first).
    pub async fn radio(self, video_id: String) -> Result<Vec<Track>> {
        let vid = VideoID::from_raw(video_id);
        let tracks = match &self {
            Api::Guest(y) => y.get_watch_playlist_from_video_id(vid).await,
            Api::User(y) => y.get_watch_playlist_from_video_id(vid).await,
        }
        .map_err(err)?;
        Ok(tracks.into_iter().map(watch_track).collect())
    }

    /// Tracks of a mix ("RD..." playlist), which only exists as a watch queue.
    pub async fn mix_tracks(self, id: String) -> Result<Vec<Track>> {
        let query = GetWatchPlaylistQuery::new_from_playlist_id(PlaylistID::from_raw(id));
        let tracks = match &self {
            Api::Guest(y) => y.query(query).await,
            Api::User(y) => y.query(query).await,
        }
        .map_err(err)?;
        Ok(tracks.into_iter().map(watch_track).collect())
    }

    /// Listening history, newest first; `album` holds the period ("Today", "Yesterday"...).
    pub async fn history(self) -> Result<Vec<Track>> {
        let Api::User(y) = &self else {
            return Ok(Vec::new());
        };
        let periods = y.get_history().await.map_err(err)?;
        let mut out = Vec::new();
        for p in periods {
            for item in p.items {
                let t = match item {
                    HistoryItem::Song(s) => Track {
                        video_id: s.video_id.get_raw().to_string(),
                        album_id: Some(s.album.id.get_raw().to_string()),
                        artist_id: s
                            .artists
                            .first()
                            .and_then(|a| a.id.as_ref())
                            .map(|i| i.get_raw().to_string()),
                        title: s.title,
                        artist: s
                            .artists
                            .into_iter()
                            .map(|a| a.name)
                            .collect::<Vec<_>>()
                            .join(", "),
                        album: p.period_name.clone(),
                        duration: s.duration,
                        link: None,
                        thumb: best_thumb(&s.thumbnails),
                    },
                    HistoryItem::Video(v) => Track {
                        video_id: v.video_id.get_raw().to_string(),
                        album_id: None,
                        artist_id: None,
                        title: v.title,
                        artist: v.channel_name,
                        album: p.period_name.clone(),
                        duration: v.duration,
                        link: None,
                        thumb: best_thumb(&v.thumbnails),
                    },
                    _ => continue,
                };
                out.push(t);
            }
        }
        Ok(out)
    }

    /// New private playlist; returns it ready for the library list.
    pub async fn create_playlist(self, title: String) -> Result<Playlist> {
        let Api::User(y) = &self else {
            return Err(anyhow!("sign in to make playlists"));
        };
        let query = CreatePlaylistQuery::new(&title, None, PrivacyStatus::Private);
        let id = y.create_playlist(query).await.map_err(err)?;
        Ok(Playlist {
            id: format!("VL{}", id.get_raw()),
            title,
            thumb: None,
        })
    }

    pub async fn add_to_playlist(self, playlist_id: String, video_id: String) -> Result<()> {
        let Api::User(y) = &self else {
            return Err(anyhow!("sign in to edit playlists"));
        };
        // Library ids are browse ids ("VL..."); editing wants the bare playlist id.
        let id = playlist_id
            .strip_prefix("VL")
            .unwrap_or(&playlist_id)
            .to_string();
        y.add_video_items_to_playlist(PlaylistID::from_raw(id), [VideoID::from_raw(video_id)])
            .await
            .map_err(err)?;
        Ok(())
    }

    /// YouTube Music's own lyrics for a song; `None` when it has none.
    pub async fn lyrics(self, video_id: String) -> Result<Option<crate::lyrics::Lyrics>> {
        let vid = VideoID::from_raw(video_id);
        let id = match &self {
            Api::Guest(y) => y.get_lyrics_id(vid).await,
            Api::User(y) => y.get_lyrics_id(vid).await,
        };
        // No lyrics tab for this song.
        let Ok(id) = id else { return Ok(None) };
        let l = match &self {
            Api::Guest(y) => y.get_lyrics(id).await,
            Api::User(y) => y.get_lyrics(id).await,
        };
        Ok(l.ok()
            .and_then(|l| crate::lyrics::from_ytm(&l.lyrics, &l.source)))
    }

    /// An artist's top songs.
    pub async fn artist_songs(self, channel_id: String) -> Result<Vec<Track>> {
        let cid = ArtistChannelID::from_raw(channel_id.clone());
        let artist = match &self {
            Api::Guest(y) => y.get_artist(cid).await,
            Api::User(y) => y.get_artist(cid).await,
        }
        .map_err(err)?;
        let thumb = best_thumb(&artist.thumbnails);
        let songs = artist
            .top_releases
            .songs
            .map(|s| s.results)
            .unwrap_or_default();
        Ok(songs
            .into_iter()
            .map(|s| Track {
                video_id: s.video_id.get_raw().to_string(),
                album_id: Some(s.album.id.get_raw().to_string()),
                artist_id: Some(channel_id.clone()),
                title: s.title,
                artist: s
                    .artists
                    .into_iter()
                    .map(|a| a.name)
                    .collect::<Vec<_>>()
                    .join(", "),
                album: s.album.name,
                duration: String::new(),
                thumb: thumb.clone(),
                link: None,
            })
            .collect())
    }

    /// Thumbs down (or takes it back). YouTube Music plays disliked songs less.
    pub async fn dislike(self, video_id: String, on: bool) -> Result<()> {
        let Api::User(y) = &self else {
            return Err(anyhow!("sign in to rate songs"));
        };
        let status = if on {
            LikeStatus::Disliked
        } else {
            LikeStatus::Indifferent
        };
        y.rate_song(VideoID::from_raw(video_id), status)
            .await
            .map_err(err)
    }

    pub async fn rate(self, video_id: String, liked: bool) -> Result<()> {
        let Api::User(y) = &self else {
            return Err(anyhow!("sign in to like songs"));
        };
        let status = if liked {
            LikeStatus::Liked
        } else {
            LikeStatus::Indifferent
        };
        y.rate_song(VideoID::from_raw(video_id), status)
            .await
            .map_err(err)
    }

    /// Tells YouTube Music the song was played, so history and recommendations stay in sync.
    pub async fn report_play(self, video_id: String) -> Result<()> {
        let Api::User(y) = &self else { return Ok(()) };
        let url = y
            .get_song_tracking_url(VideoID::from_raw(video_id))
            .await
            .map_err(err)?;
        y.add_history_item(url).await.map_err(err)?;
        Ok(())
    }
}

/// ytmapi-rs currently derives SAPISIDHASH from a `SAPISID` cookie. Some Google web sessions
/// expose only the secure partitioned equivalent, so mirror its value under the expected name.
fn normalize_sapisid_cookie(cookie: &str) -> String {
    let pairs: Vec<_> = cookie
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(name, value)| (name.trim(), value.trim()))
        .collect();
    if pairs
        .iter()
        .any(|(name, value)| *name == "SAPISID" && !value.is_empty())
    {
        return cookie.to_owned();
    }
    let secure = pairs
        .iter()
        .find(|(name, value)| {
            !value.is_empty() && matches!(*name, "__Secure-3PAPISID" | "__Secure-1PAPISID")
        })
        .map(|(_, value)| *value);
    match secure {
        Some(value) => format!("{}; SAPISID={value}", cookie.trim_end_matches(';').trim()),
        None => cookie.to_owned(),
    }
}

/// Largest thumbnail of a list.
fn best_thumb(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs.iter().max_by_key(|t| t.width).map(|t| t.url.clone())
}

/// Largest thumbnail inside a raw renderer (`.../thumbnail/thumbnails[]`).
fn json_thumb(node: &Value) -> Option<String> {
    let list = node
        .pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
        .or_else(|| node.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"))?
        .as_array()?;
    list.last()?.get("url")?.as_str().map(String::from)
}

/// Google image URLs take a size suffix (`=w120-h120...`); ask for `px` square.
pub fn thumb_sized(url: &str, px: u32) -> String {
    match url.rfind("=w") {
        Some(i) if url.contains("googleusercontent") || url.contains("ggpht") => {
            format!("{}=w{px}-h{px}-l90-rj", &url[..i])
        }
        _ => url.to_string(),
    }
}

fn watch_track(t: WatchPlaylistTrack) -> Track {
    Track {
        video_id: t.video_id.get_raw().to_string(),
        album_id: None,
        artist_id: None,
        title: t.title,
        artist: t.author,
        album: String::new(),
        duration: t.duration,
        link: None,
        thumb: best_thumb(&t.thumbnails),
    }
}

/// Joins a `{runs: [{text}]}` node into plain text.
fn text(node: Option<&Value>) -> String {
    node.and_then(|n| n.get("runs"))
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text")?.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// One card of a home shelf. Artists and anything unknown are skipped.
fn home_item(item: &Value, section: &str) -> Option<Track> {
    if let Some(r) = item.get("musicTwoRowItemRenderer") {
        let title = text(r.get("title"));
        let subtitle = text(r.get("subtitle"));
        let nav = r.get("navigationEndpoint")?;
        let (video_id, link) = if let Some(v) = nav
            .pointer("/watchEndpoint/videoId")
            .and_then(Value::as_str)
        {
            (v.to_string(), None)
        } else if let Some(p) = nav
            .pointer("/watchPlaylistEndpoint/playlistId")
            .and_then(Value::as_str)
        {
            (String::new(), Some(Link::Mix(p.to_string())))
        } else {
            let id = nav
                .pointer("/browseEndpoint/browseId")
                .and_then(Value::as_str)?;
            let link = if id.starts_with("VL") {
                Link::Playlist(id.to_string())
            } else if id.starts_with("MPRE") {
                Link::Album(id.to_string())
            } else {
                return None;
            };
            (String::new(), Some(link))
        };
        let thumb = json_thumb(r);
        return Some(Track {
            video_id,
            title,
            artist: subtitle,
            album: section.to_string(),
            duration: String::new(),
            thumb,
            album_id: None,
            artist_id: None,
            link,
        });
    }
    // "Quick picks" style song rows.
    let r = item.get("musicResponsiveListItemRenderer")?;
    let video_id = r
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)?
        .to_string();
    let col = |i: usize| {
        text(r.pointer(&format!(
            "/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text"
        )))
    };
    let artist = col(1).split(" • ").next().unwrap_or_default().to_string();
    let thumb = json_thumb(r);
    Some(Track {
        video_id,
        title: col(0),
        artist,
        album: section.to_string(),
        duration: String::new(),
        thumb,
        album_id: None,
        artist_id: None,
        link: None,
    })
}

/// The YouTube Music home feed ("Quick picks", mixes...). Not in ytmapi-rs, so it's a custom
/// query read with `raw_json_query`; the parsed output is unused.
#[derive(Debug)]
struct HomeQuery {
    continuation: Option<String>,
}

#[derive(Debug)]
struct Unparsed;

impl ParseFrom<HomeQuery> for Unparsed {
    fn parse_from(_: ProcessedResult<HomeQuery>) -> ytmapi_rs::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for HomeQuery {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for HomeQuery {
    fn header(&self) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::from_iter([("browseId".to_string(), serde_json::json!("FEmusic_home"))])
    }
    fn params(&self) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        match &self.continuation {
            Some(c) => vec![
                ("ctoken", c.into()),
                ("continuation", c.into()),
                ("type", "next".into()),
            ],
            None => vec![],
        }
    }
    fn path(&self) -> &str {
        "browse"
    }
}

#[cfg(test)]
mod home_probe {
    use super::*;

    /// Prints section names and counts only.
    #[tokio::test]
    #[ignore]
    async fn home_feed() {
        let cookie = std::fs::read_to_string(crate::cookie_path()).unwrap();
        let api = Api::with_cookie(cookie).await.unwrap();
        let items = api.clone().home().await.unwrap();
        let mut sections: Vec<(String, usize, usize)> = Vec::new();
        for t in &items {
            let link = t.link.is_some() as usize;
            match sections.iter_mut().find(|s| s.0 == t.album) {
                Some(s) => {
                    s.1 += 1 - link;
                    s.2 += link;
                }
                None => sections.push((t.album.clone(), 1 - link, link)),
            }
        }
        for (name, songs, links) in &sections {
            println!("{name}: {songs} songs, {links} playlists/albums/mixes");
        }
        if let Some(t) = items.iter().find(|t| t.link.is_none()) {
            println!(
                "radio from a home song: {} tracks",
                api.clone().radio(t.video_id.clone()).await.unwrap().len()
            );
        }
        for t in &items {
            match &t.link {
                Some(Link::Mix(id)) => println!(
                    "mix: {} tracks",
                    api.clone().mix_tracks(id.clone()).await.unwrap().len()
                ),
                Some(Link::Album(id)) => println!(
                    "album: {} tracks",
                    api.clone().album_tracks(id.clone()).await.unwrap().len()
                ),
                _ => continue,
            }
        }
    }
}
