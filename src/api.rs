//! Thin wrapper over the YouTube Music API library (music_api) that hides the auth type and flattens results.

use std::fmt;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use music_api::YtMusic as Client;
use music_api::auth::BrowserToken;
use music_api::auth::noauth::NoAuthToken;
use serde_json::Value;
use music_api::auth::AuthToken;
use music_api::common::{AlbumID, ArtistChannelID, LikeStatus, PlaylistID, Thumbnail, VideoID, YoutubeID};
use music_api::parse::{HistoryItem, ParseFrom, PlaylistItem, ProcessedResult, WatchPlaylistTrack};
use music_api::query::playlist::{CreatePlaylistQuery, PrivacyStatus};
use music_api::query::{GetLibraryPlaylistsQuery, GetWatchPlaylistQuery, PostMethod, PostQuery, Query};

/// Liked Music is a regular playlist with a fixed id.
pub const LIKED_ID: &str = "LM";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    Guest(Arc<Client<NoAuthToken>>),
    User(Arc<Client<BrowserToken>>),
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.logged_in() { "Api(User)" } else { "Api(Guest)" })
    }
}

/// YouTube Music's connection, over HTTP/1.1: signed in, Google's HTTP/2 answers started
/// breaking the connection ("stream error: unspecific protocol error", 30 Oct 2026).
fn http1_client() -> Result<music_api::Client> {
    let client = reqwest::Client::builder().http1_only().build()?;
    Ok(music_api::Client::new_from_reqwest_client(client))
}

fn err(e: impl fmt::Debug) -> anyhow::Error {
    anyhow!("{e:?}")
}

impl Api {
    pub async fn guest() -> Result<Api> {
        let yt = music_api::YtMusicBuilder::new_with_client(http1_client()?).build().await.map_err(err)?;
        Ok(Api::Guest(Arc::new(yt)))
    }

    pub async fn with_cookie(cookie: String) -> Result<Api> {
        let yt = music_api::YtMusicBuilder::new_with_client(http1_client()?)
            .with_browser_token_cookie(cookie.trim().to_string())
            .build()
            .await
            .map_err(err)?;
        Ok(Api::User(Arc::new(yt)))
    }

    pub fn logged_in(&self) -> bool {
        matches!(self, Api::User(_))
    }

    /// The signed-in Google account: its name, handle and picture (from the account menu).
    pub async fn account(self) -> Result<Account> {
        let query = RawBrowse { path: "account/account_menu", body: serde_json::Map::new(), ctoken: None };
        let raw = match &self {
            Api::User(y) => y.raw_json_query(query).await.map_err(err)?,
            Api::Guest(_) => return Err(anyhow!("not signed in")),
        };
        let page: Value = serde_json::from_str(&raw)?;
        account_from(&page).ok_or_else(|| anyhow!("no account in YouTube Music's answer"))
    }

    /// The signed-in cookie, for the ytmusicapi engine (None as a guest).
    fn cookie(&self) -> Option<String> {
        if matches!(self, Api::User(_)) { crate::secret::read_cookie() } else { None }
    }

    /// Songs for a search: ytmusicapi, with the built-in engine as the fallback (see
    /// [`crate::ytmusicapi`]).
    pub async fn search(self, query: String) -> Result<Vec<Track>> {
        let cookie = self.cookie();
        crate::ytmusicapi::hybrid("search", crate::ytmusicapi::search(query.clone(), "songs", cookie), || self.builtin_search(query)).await
    }

    /// The built-in engine's search (ytmapi-rs over InnerTube).
    pub async fn builtin_search(self, query: String) -> Result<Vec<Track>> {
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

    /// Parsed by hand: the library fails the whole list when one playlist lacks a subtitle field.
    pub async fn library_playlists(self) -> Result<Vec<Playlist>> {
        let Api::User(y) = &self else { return Ok(Vec::new()) };
        let raw = y.raw_json_query(GetLibraryPlaylistsQuery).await.map_err(err)?;
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
                let id = r.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?;
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
        // The library sends the id as the browseId, which needs the "VL" prefix.
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
                    album_id: Some(s.album.id.get_raw().to_string()),
                    artist_id: s.artists.first().and_then(|a| a.id.as_ref()).map(|i| i.get_raw().to_string()),
                    title: s.title,
                    artist: s.artists.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
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

    /// The home feed ("For you"): songs play, playlists/albums/mixes open. Reads up to six pages
    /// (YouTube gives the next page's token in the older `nextContinuationData` or the newer
    /// `continuationCommand`; only following the older one stopped after the first page).
    pub async fn home(self) -> Result<Vec<Track>> {
        Ok(self.home_pages(6).await?.tracks)
    }

    /// The first `pages` pages of the home feed, and where the next one starts.
    pub async fn home_pages(self, pages: usize) -> Result<Paged> {
        let mut out = Vec::new();
        let mut page = self.raw_browse(body(&[("browseId", "FEmusic_home")]), None).await?;
        let mut more = None;
        for n in 0..pages {
            out.extend(home_shelves(&page));
            more = next_token(&page);
            if n + 1 == pages {
                break;
            }
            match more.take() {
                Some(token) => page = self.next_page("browse", &[("browseId", "FEmusic_home")], token).await?,
                None => break,
            }
        }
        Ok(Paged { tracks: out, more: more.map(More::Home) })
    }

    /// The page after `token` on `path` ("browse" or "search").
    async fn next_page(&self, path: &'static str, base: &[(&str, &str)], token: Token) -> Result<Value> {
        let (body, ctoken) = match token {
            Token::Body(t) => {
                let mut b = body(base);
                b.insert("continuation".into(), Value::from(t));
                (b, None)
            }
            Token::Link(t) => (body(base), Some(t)),
        };
        let query = RawBrowse { path, body, ctoken };
        let raw = match self {
            Api::Guest(y) => y.raw_json_query(query).await,
            Api::User(y) => y.raw_json_query(query).await,
        }
        .map_err(err)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Songs found for `query` (the first page), and where the next page starts.
    /// Songs first, then YouTube videos (songs that are only on YouTube, not on YouTube Music),
    /// each video once; asked at the same time. One of the two failing still gives the other.
    /// Search results (songs, then videos): ytmusicapi, with the built-in engine as the fallback.
    pub async fn search_paged(self, query: String) -> Result<Paged> {
        let cookie = self.cookie();
        let q = query.clone();
        let new = async move {
            let (songs, videos) = tokio::join!(crate::ytmusicapi::search(q.clone(), "songs", cookie.clone()), crate::ytmusicapi::search(q, "videos", cookie));
            let mut tracks = songs?;
            let seen: std::collections::HashSet<String> = tracks.iter().map(|t| t.video_id.clone()).collect();
            let found: Vec<Track> = videos.unwrap_or_default().into_iter().filter(|t| !seen.contains(&t.video_id)).map(video_row).collect();
            if let Ok(mut v) = video_ids().lock() {
                if v.len() > 5000 {
                    v.clear();
                }
                v.extend(found.iter().map(|t| t.video_id.clone()));
            }
            tracks.extend(found);
            Ok(Paged { tracks, more: None })
        };
        crate::ytmusicapi::hybrid("search", new, || self.builtin_search_paged(query)).await
    }

    /// The built-in engine's search results, with their next pages.
    pub async fn builtin_search_paged(self, query: String) -> Result<Paged> {
        let (songs, videos) = tokio::join!(self.raw_search(&query, SONGS_FILTER), self.raw_search(&query, VIDEOS_FILTER));
        let mut tracks = Vec::new();
        let mut more = None;
        if let Ok(page) = &songs {
            tracks = rows(page);
            more = next_token(page).map(|token| More::Search { query: query.clone(), token });
        }
        if let Ok(page) = &videos {
            let seen: std::collections::HashSet<String> = tracks.iter().map(|t| t.video_id.clone()).collect();
            let found: Vec<Track> = rows(page).into_iter().filter(|t| !seen.contains(&t.video_id)).map(video_row).collect();
            if let Ok(mut v) = video_ids().lock() {
                if v.len() > 5000 {
                    v.clear();
                }
                v.extend(found.iter().map(|t| t.video_id.clone()));
            }
            tracks.extend(found);
        }
        if tracks.is_empty() {
            songs?;
            videos?;
        }
        Ok(Paged { tracks, more })
    }

    async fn raw_search(&self, query: &str, params: &str) -> Result<Value> {
        let ask = RawBrowse { path: "search", body: body(&[("query", query), ("params", params)]), ctoken: None };
        let raw = match self {
            Api::Guest(y) => y.raw_json_query(ask).await,
            Api::User(y) => y.raw_json_query(ask).await,
        }
        .map_err(err)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// A playlist's first page of songs, and where the next page starts.
    pub async fn playlist_paged(self, id: String) -> Result<Paged> {
        let id = if id.starts_with("VL") { id } else { format!("VL{id}") };
        let page = self.raw_browse(body(&[("browseId", &id)]), None).await?;
        Ok(Paged { tracks: rows(&page), more: next_token(&page).map(More::Browse) })
    }

    /// YouTube Music's home page further down from `token`.
    pub async fn home_after(self, token: Token) -> Result<Paged> {
        let page = self.next_page("browse", &[("browseId", "FEmusic_home")], token).await?;
        Ok(Paged { tracks: home_shelves(&page), more: next_token(&page).map(More::Home) })
    }

    /// The next part of a list that goes on (search results, a playlist, the home feed).
    pub async fn more(self, more: More) -> Result<Paged> {
        match more {
            More::Search { query, token } => {
                let page = self.next_page("search", &[("query", &query), ("params", SONGS_FILTER)], token).await?;
                Ok(Paged { tracks: rows(&page), more: next_token(&page).map(|token| More::Search { query, token }) })
            }
            More::Browse(token) => {
                let page = self.next_page("browse", &[], token).await?;
                Ok(Paged { tracks: rows(&page), more: next_token(&page).map(More::Browse) })
            }
            More::Home(token) => self.home_after(token).await,
            More::Feed(cursor) => crate::feed::more(self, *cursor).await,
        }
    }

    pub async fn album_tracks(self, id: String) -> Result<Vec<Track>> {
        let album_id = id.clone();
        let aid = AlbumID::from_raw(id);
        let album = match &self {
            Api::Guest(y) => y.get_album(aid).await,
            Api::User(y) => y.get_album(aid).await,
        }
        .map_err(err)?;
        let artist = album.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
        let album_thumb = best_thumb(&album.thumbnails);
        let album_artist = album.artists.first().and_then(|a| a.id.as_ref()).map(|i| i.get_raw().to_string());
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
    /// A song's radio: ytmusicapi, with the built-in engine as the fallback.
    pub async fn radio(self, video_id: String) -> Result<Vec<Track>> {
        let cookie = self.cookie();
        crate::ytmusicapi::hybrid("radio", crate::ytmusicapi::radio(video_id.clone(), cookie), || self.builtin_radio(video_id)).await
    }

    /// The built-in engine's radio.
    pub async fn builtin_radio(self, video_id: String) -> Result<Vec<Track>> {
        let vid = VideoID::from_raw(video_id);
        let tracks = match &self {
            Api::Guest(y) => y.get_watch_playlist_from_video_id(vid).await,
            Api::User(y) => y.get_watch_playlist_from_video_id(vid).await,
        }
        .map_err(err)?;
        Ok(tracks.into_iter().map(watch_track).collect())
    }

    /// Tracks of a mix ("RD..." playlist), which only exists as a watch queue: ytmusicapi, with
    /// the built-in engine as the fallback.
    pub async fn mix_tracks(self, id: String) -> Result<Vec<Track>> {
        let cookie = self.cookie();
        crate::ytmusicapi::hybrid("mix", crate::ytmusicapi::mix(id.clone(), cookie), || self.builtin_mix_tracks(id)).await
    }

    /// The built-in engine's mix.
    pub async fn builtin_mix_tracks(self, id: String) -> Result<Vec<Track>> {
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
        let Api::User(y) = &self else { return Ok(Vec::new()) };
        let periods = y.get_history().await.map_err(err)?;
        let mut out = Vec::new();
        for p in periods {
            for item in p.items {
                let t = match item {
                    HistoryItem::Song(s) => Track {
                        video_id: s.video_id.get_raw().to_string(),
                        album_id: Some(s.album.id.get_raw().to_string()),
                        artist_id: s.artists.first().and_then(|a| a.id.as_ref()).map(|i| i.get_raw().to_string()),
                        title: s.title,
                        artist: s.artists.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
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
        let Api::User(y) = &self else { return Err(anyhow!("sign in to make playlists")) };
        let query = CreatePlaylistQuery::new(&title, None, PrivacyStatus::Private);
        let id = y.create_playlist(query).await.map_err(err)?;
        Ok(Playlist { id: format!("VL{}", id.get_raw()), title, thumb: None })
    }

    pub async fn add_to_playlist(self, playlist_id: String, video_id: String) -> Result<()> {
        let Api::User(y) = &self else { return Err(anyhow!("sign in to edit playlists")) };
        // Library ids are browse ids ("VL..."); editing wants the bare playlist id.
        let id = playlist_id.strip_prefix("VL").unwrap_or(&playlist_id).to_string();
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
        Ok(l.ok().and_then(|l| crate::lyrics::from_youtube_music(&l.lyrics, &l.source)))
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
        let songs = artist.top_releases.songs.map(|s| s.results).unwrap_or_default();
        Ok(songs
            .into_iter()
            .map(|s| Track {
                video_id: s.video_id.get_raw().to_string(),
                album_id: Some(s.album.id.get_raw().to_string()),
                artist_id: Some(channel_id.clone()),
                title: s.title,
                artist: s.artists.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
                album: s.album.name,
                duration: String::new(),
                thumb: thumb.clone(),
                link: None,
            })
            .collect())
    }

    /// YouTube Music's search suggestions for what's typed so far; signed in, its own search
    /// history comes first (the same on every device of the account), with an empty `input` too.
    pub async fn search_suggestions(&self, input: String) -> Result<Vec<Suggestion>> {
        let query = RawBrowse { path: "music/get_search_suggestions", body: body(&[("input", &input)]), ctoken: None };
        let raw = match self {
            Api::Guest(y) => y.raw_json_query(query).await,
            Api::User(y) => y.raw_json_query(query).await,
        }
        .map_err(err)?;
        Ok(suggestions(&serde_json::from_str(&raw)?))
    }

    /// Takes a search off the account's history (the token comes with the suggestion).
    pub async fn forget_search(&self, token: String) -> Result<()> {
        let mut b = serde_json::Map::new();
        b.insert("feedbackTokens".into(), Value::from(vec![token]));
        let query = RawBrowse { path: "feedback", body: b, ctoken: None };
        match self {
            Api::Guest(y) => y.raw_json_query(query).await,
            Api::User(y) => y.raw_json_query(query).await,
        }
        .map_err(err)?;
        Ok(())
    }

    async fn raw_browse(&self, body: serde_json::Map<String, Value>, ctoken: Option<String>) -> Result<Value> {
        let query = RawBrowse { path: "browse", body, ctoken };
        let raw = match self {
            Api::Guest(y) => y.raw_json_query(query).await,
            Api::User(y) => y.raw_json_query(query).await,
        }
        .map_err(err)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// A whole playlist, page after page, up to `limit` songs (the library's own call stops at
    /// the first hundred).
    pub async fn playlist_all(self, id: String, limit: usize) -> Result<Vec<Track>> {
        let id = if id.starts_with("VL") { id } else { format!("VL{id}") };
        let mut page = self.raw_browse(body(&[("browseId", &id)]), None).await?;
        let mut out: Vec<Track> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..60 {
            for t in all(&page, "musicResponsiveListItemRenderer").into_iter().filter_map(song_row) {
                if seen.insert(t.video_id.clone()) {
                    out.push(t);
                }
            }
            if out.len() >= limit {
                break;
            }
            let newer = all(&page, "continuationCommand").into_iter().find_map(|c| c.get("token").and_then(Value::as_str)).map(String::from);
            let older = all(&page, "nextContinuationData").into_iter().find_map(|c| c.get("continuation").and_then(Value::as_str)).map(String::from);
            page = match (newer, older) {
                (Some(token), _) => self.raw_browse(body(&[("continuation", &token)]), None).await?,
                (None, Some(token)) => self.raw_browse(serde_json::Map::new(), Some(token)).await?,
                (None, None) => break,
            };
        }
        out.truncate(limit);
        Ok(out)
    }

    /// An artist's whole catalogue: every song (from the page's "See all" list, not just the top
    /// five), then their albums and singles (from each shelf's "See all" grid) to open.
    pub async fn artist_page(self, channel_id: String) -> Result<Vec<Track>> {
        let page = self.raw_browse(body(&[("browseId", &channel_id)]), None).await?;
        let shelf = all(&page, "musicShelfRenderer").into_iter().next();
        let all_songs = shelf.and_then(|s| {
            s.pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .or_else(|| s.pointer("/bottomEndpoint/browseEndpoint/browseId"))
                .and_then(Value::as_str)
                .map(String::from)
        });
        let mut out = match all_songs {
            Some(id) => self.clone().playlist_all(id, 3000).await.unwrap_or_default(),
            None => Vec::new(),
        };
        if out.is_empty()
            && let Some(s) = shelf
        {
            out = all(s, "musicResponsiveListItemRenderer").into_iter().filter_map(song_row).collect();
        }
        for t in &mut out {
            t.artist_id = t.artist_id.take().or_else(|| Some(channel_id.clone()));
        }
        for carousel in all(&page, "musicCarouselShelfRenderer") {
            let header = carousel.pointer("/header/musicCarouselShelfBasicHeaderRenderer");
            let name = text(header.and_then(|h| h.get("title")));
            let more = header.and_then(|h| {
                h.pointer("/moreContentButton/buttonRenderer/navigationEndpoint/browseEndpoint")
                    .or_else(|| h.pointer("/title/runs/0/navigationEndpoint/browseEndpoint"))
            });
            let mut cards: Vec<Track> = Vec::new();
            if let Some(m) = more
                && let Some(id) = m.get("browseId").and_then(Value::as_str)
            {
                let mut b = body(&[("browseId", id)]);
                if let Some(p) = m.get("params").and_then(Value::as_str) {
                    b.insert("params".into(), Value::from(p));
                }
                if let Ok(grid) = self.raw_browse(b, None).await {
                    cards = all(&grid, "musicTwoRowItemRenderer")
                        .into_iter()
                        .filter_map(|r| home_item(&serde_json::json!({ "musicTwoRowItemRenderer": r }), &name))
                        .collect();
                }
            }
            if cards.is_empty() {
                cards = carousel.get("contents").and_then(Value::as_array).into_iter().flatten().filter_map(|i| home_item(i, &name)).collect();
            }
            // Albums, singles and EPs; not videos, playlists it's featured on, or other artists.
            out.extend(cards.into_iter().filter(|t| matches!(t.link, Some(Link::Album(_)))));
        }
        Ok(out)
    }

    /// The channel of an artist known only by name (songs from radios and some lists).
    pub async fn artist_id_for(self, name: String) -> Result<Option<String>> {
        let first = name.split(", ").next().unwrap_or(&name).to_string();
        let found = match &self {
            Api::Guest(y) => y.search_artists(first.as_str()).await,
            Api::User(y) => y.search_artists(first.as_str()).await,
        }
        .map_err(err)?;
        Ok(found.into_iter().next().map(|a| a.browse_id.get_raw().to_string()))
    }

    /// Thumbs down (or takes it back). YouTube Music plays disliked songs less.
    pub async fn dislike(self, video_id: String, on: bool) -> Result<()> {
        let Api::User(y) = &self else { return Err(anyhow!("sign in to rate songs")) };
        let status = if on { LikeStatus::Disliked } else { LikeStatus::Indifferent };
        y.rate_song(VideoID::from_raw(video_id), status).await.map_err(err)
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
        title: clean_title(&t.title, &t.author),
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
        .map(|runs| runs.iter().filter_map(|r| r.get("text")?.as_str()).collect())
        .unwrap_or_default()
}

/// One card of a home shelf. Artists and anything unknown are skipped.
fn home_item(item: &Value, section: &str) -> Option<Track> {
    if let Some(r) = item.get("musicTwoRowItemRenderer") {
        let title = text(r.get("title"));
        let subtitle = text(r.get("subtitle"));
        let nav = r.get("navigationEndpoint")?;
        let (video_id, link) = if let Some(v) = nav.pointer("/watchEndpoint/videoId").and_then(Value::as_str) {
            (v.to_string(), None)
        } else if let Some(p) = nav.pointer("/watchPlaylistEndpoint/playlistId").and_then(Value::as_str) {
            (String::new(), Some(Link::Mix(p.to_string())))
        } else {
            let id = nav.pointer("/browseEndpoint/browseId").and_then(Value::as_str)?;
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
    let video_id = r.pointer("/playlistItemData/videoId").and_then(Value::as_str)?.to_string();
    let col = |i: usize| text(r.pointer(&format!("/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text")));
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

/// Any `browse` request the library has no query for (the home feed, whole playlists, artist
/// pages), read with `raw_json_query`; the parsed output is unused. `ctoken` is the older style
/// of asking for the next page; the newer one puts `continuation` in the body.
#[derive(Debug)]
struct RawBrowse {
    /// The endpoint: "browse", or another one ("account/account_menu").
    path: &'static str,
    body: serde_json::Map<String, Value>,
    ctoken: Option<String>,
}

/// A search suggestion: the words, and (from the account's search history) the token that
/// takes it off.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub text: String,
    pub forget: Option<String>,
}

impl Suggestion {
    pub fn from_history(&self) -> bool {
        self.forget.is_some()
    }
}

/// History first, then suggestions, each once.
fn suggestions(v: &Value) -> Vec<Suggestion> {
    let words = |r: &Value| -> Option<String> {
        let runs = r.get("suggestion")?.get("runs")?.as_array()?;
        Some(runs.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect::<String>()).filter(|t| !t.trim().is_empty())
    };
    let mut out: Vec<Suggestion> = Vec::new();
    for r in all(v, "historySuggestionRenderer") {
        let forget = all(r, "feedbackToken").into_iter().find_map(Value::as_str).map(String::from);
        if let Some(text) = words(r) {
            out.push(Suggestion { text, forget: forget.or_else(|| Some(String::new())) });
        }
    }
    for r in all(v, "searchSuggestionRenderer") {
        if let Some(text) = words(r)
            && !out.iter().any(|s| s.text.eq_ignore_ascii_case(&text))
        {
            out.push(Suggestion { text, forget: None });
        }
    }
    out
}

fn body(pairs: &[(&str, &str)]) -> serde_json::Map<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::from(*v))).collect()
}

#[derive(Debug)]
struct Unparsed;

impl ParseFrom<RawBrowse> for Unparsed {
    fn parse_from(_: ProcessedResult<RawBrowse>) -> music_api::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for RawBrowse {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for RawBrowse {
    fn header(&self) -> serde_json::Map<String, serde_json::Value> {
        self.body.clone()
    }
    fn params(&self) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        match &self.ctoken {
            Some(c) => vec![("ctoken", c.into()), ("continuation", c.into()), ("type", "next".into())],
            None => vec![],
        }
    }
    fn path(&self) -> &str {
        self.path
    }
}

/// The "Songs" filter of the search page.
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
/// Search for videos only.
const VIDEOS_FILTER: &str = "EgWKAQIQAWoKEAkQChAFEAMQBA==";

fn video_ids() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static V: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    V.get_or_init(Default::default)
}

/// The search found this as a YouTube video, not a YouTube Music song (its badge, the filter).
pub fn found_as_video(video_id: &str) -> bool {
    video_ids().lock().is_ok_and(|v| v.contains(video_id))
}

/// A video found by search: its line has the views where a song has its album.
fn video_row(mut t: Track) -> Track {
    if t.album.ends_with("views") || t.album.ends_with("view") {
        t.album.clear();
    }
    t.title = clean_title(&t.title, &t.artist);
    t
}

/// Words that only ever describe the upload, never the song.
const CLUTTER: [&str; 22] = [
    "official", "music", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial",
    "officiel", "clip", "full", "videoclip", "videoclipe", "with", "on", "screen",
];
/// One of these must be there for brackets to count as clutter ("(Official)" alone stays).
const CLUTTER_MARK: [&str; 13] = ["video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial", "officiel"];

fn is_clutter(inner: &str) -> bool {
    let words: Vec<String> = inner.to_lowercase().split(|c: char| c.is_whitespace() || c == '-' || c == '_').filter(|w| !w.is_empty()).map(str::to_string).collect();
    !words.is_empty() && words.iter().all(|w| CLUTTER.contains(&w.as_str())) && words.iter().any(|w| CLUTTER_MARK.contains(&w.as_str()))
}

/// A title as a song's, not an upload's: brackets that only say "(Official Video)", "[Official
/// Music Video]", "(Lyrics)", "(Visualizer)", "[HD]" and the like go, so do "| Official Video",
/// hashtags, and the artist's name in front ("Artist - Song"). Brackets that mean something
/// ("(Remix)", "(Live)", "(feat. X)") stay. The same rules as ytm_bridge.py and the phone's.
pub fn clean_title(title: &str, artist: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(i) = rest.find(['(', '[']) {
        let close = if rest[i..].starts_with('(') { ')' } else { ']' };
        let Some(j) = rest[i + 1..].find(close).map(|j| i + 1 + j) else { break };
        out.push_str(&rest[..i]);
        if !is_clutter(&rest[i + 1..j]) {
            out.push_str(&rest[i..=j]);
        }
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    // "Song | Official Video", "Song - Official Audio".
    for sep in [" | ", " - ", " – "] {
        if let Some(i) = out.rfind(sep)
            && is_clutter(&out[i + sep.len()..])
        {
            out.truncate(i);
        }
    }
    let joined = out.split_whitespace().filter(|w| !w.starts_with('#')).collect::<Vec<_>>().join(" ");
    // "Artist - Song" when the artist is the song's own.
    let first = crate::radio::first_artist(artist);
    let mut text = joined.clone();
    for sep in [" - ", " – "] {
        if let Some((front, song)) = joined.split_once(sep)
            && !first.is_empty()
            && crate::radio::normalize_artist_key(front) == first
            && !song.trim().is_empty()
        {
            text = song.trim().to_string();
            break;
        }
    }
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches(|c: char| c == '-' || c == '|' || c == '–' || c.is_whitespace()).to_string();
    if cleaned.is_empty() { title.trim().to_string() } else { cleaned }
}

/// Where the next page of a list starts: the newer token goes in the request's body, the older
/// one in its link.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Body(String),
    Link(String),
}

/// A list that goes on, and how to get its next part.
#[derive(Debug, Clone)]
pub enum More {
    Search { query: String, token: Token },
    /// A playlist (or another browse page) further down.
    Browse(Token),
    /// YouTube Music's own home page further down.
    Home(Token),
    /// The mixed "For you" feed: more songs from new seeds, and the home page further down.
    Feed(Box<crate::feed::Cursor>),
}

/// Part of a list, and where it goes on (none: that was all).
#[derive(Debug, Clone, Default)]
pub struct Paged {
    pub tracks: Vec<Track>,
    pub more: Option<More>,
}

fn next_token(page: &Value) -> Option<Token> {
    let newer = all(page, "continuationCommand").into_iter().find_map(|c| c.get("token").and_then(Value::as_str));
    let older = all(page, "nextContinuationData").into_iter().find_map(|c| c.get("continuation").and_then(Value::as_str));
    newer.map(|t| Token::Body(t.into())).or_else(|| older.map(|t| Token::Link(t.into())))
}

/// The song rows on a page (search results, a playlist's songs).
fn rows(page: &Value) -> Vec<Track> {
    all(page, "musicResponsiveListItemRenderer").into_iter().filter_map(song_row).collect()
}

/// The home page's shelves: songs to play, playlists, albums and mixes to open.
fn home_shelves(page: &Value) -> Vec<Track> {
    let mut out = Vec::new();
    for shelf in all(page, "musicCarouselShelfRenderer") {
        let section = text(shelf.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"));
        for item in shelf.get("contents").and_then(Value::as_array).into_iter().flatten() {
            if let Some(t) = home_item(item, &section) {
                out.push(t);
            }
        }
    }
    out
}

/// A Google account as YouTube Music shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub name: String,
    /// "@handle", when there is one.
    pub handle: String,
    /// The account picture (largest given).
    pub photo: Option<String>,
}

fn account_from(page: &Value) -> Option<Account> {
    let h = all(page, "activeAccountHeaderRenderer").into_iter().next()?;
    let text = |v: &Value| v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect::<String>()).or_else(|| v["simpleText"].as_str().map(String::from)).unwrap_or_default();
    let photo = h["accountPhoto"]["thumbnails"].as_array().and_then(|t| t.last()).and_then(|t| t["url"].as_str()).map(String::from);
    Some(Account { name: text(&h["accountName"]), handle: text(&h["channelHandle"]), photo })
}

/// Every object stored under `key`, anywhere in the tree (depth-first, in order).
fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                }
                find_all(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| find_all(x, key, out)),
        _ => {}
    }
}

fn all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    find_all(v, key, &mut out);
    out
}

fn is_duration(s: &str) -> bool {
    !s.is_empty() && s.split(':').count() >= 2 && s.split(':').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// One song row of a raw page (playlists, artist songs): title, artists and album with their
/// ids, length.
fn song_row(r: &Value) -> Option<Track> {
    let video_id = r
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .or_else(|| all(r, "watchEndpoint").iter().find_map(|w| w.get("videoId").and_then(Value::as_str)))?
        .to_string();
    let col = |i: usize| r.pointer(&format!("/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text"));
    let (mut artists, mut artist_id, mut album, mut album_id, mut duration) = (Vec::new(), None, String::new(), None, String::new());
    for i in 1..=3 {
        for run in col(i).and_then(|c| c.get("runs")).and_then(Value::as_array).into_iter().flatten() {
            let Some(t) = run.get("text").and_then(Value::as_str) else { continue };
            let browse = run.pointer("/navigationEndpoint/browseEndpoint");
            let kind = browse
                .and_then(|b| b.pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let id = browse.and_then(|b| b.get("browseId")).and_then(Value::as_str).map(String::from);
            match kind {
                "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
                    artists.push(t.to_string());
                    artist_id = artist_id.or(id);
                }
                "MUSIC_PAGE_TYPE_ALBUM" => {
                    album = t.to_string();
                    album_id = id;
                }
                _ if is_duration(t) => duration = t.to_string(),
                _ => {}
            }
        }
    }
    if artists.is_empty() {
        // Rows without links: "Artist • Album • 3:12".
        let line = text(col(1));
        let mut parts = line.split(" • ");
        artists.extend(parts.next().map(String::from));
        if album.is_empty() {
            album = parts.next().filter(|p| !is_duration(p)).unwrap_or_default().to_string();
        }
    }
    if duration.is_empty() {
        let fixed = text(r.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text"));
        if is_duration(&fixed) {
            duration = fixed;
        }
    }
    Some(Track {
        video_id,
        title: text(col(0)),
        artist: artists.join(", "),
        album,
        duration,
        thumb: json_thumb(r),
        album_id,
        artist_id,
        link: None,
    })
}

#[cfg(test)]
mod home_probe {
    use super::*;

    /// Prints section names and counts only.
    #[tokio::test]
    #[ignore]
    async fn home_feed() {
        let cookie = crate::secret::read_cookie_io().unwrap();
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
            println!("radio from a home song: {} tracks", api.clone().radio(t.video_id.clone()).await.unwrap().len());
        }
        for t in &items {
            match &t.link {
                Some(Link::Mix(id)) => println!("mix: {} tracks", api.clone().mix_tracks(id.clone()).await.unwrap().len()),
                Some(Link::Album(id)) => println!("album: {} tracks", api.clone().album_tracks(id.clone()).await.unwrap().len()),
                _ => continue,
            }
        }
    }
}

#[cfg(test)]
mod raw_tests {
    #[test]
    fn account_menu_header() {
        let page = serde_json::json!({"actions": [{"openPopupAction": {"popup": {"multiPageMenuRenderer": {"header": {"activeAccountHeaderRenderer": {
            "accountName": {"runs": [{"text": "Some One"}]},
            "channelHandle": {"runs": [{"text": "@someone"}]},
            "accountPhoto": {"thumbnails": [{"url": "https://yt3.ggpht.com/a=s48"}, {"url": "https://yt3.ggpht.com/a=s88"}]}
        }}}}}}]});
        assert_eq!(super::account_from(&page), Some(super::Account { name: "Some One".into(), handle: "@someone".into(), photo: Some("https://yt3.ggpht.com/a=s88".into()) }));
    }

    /// The signed-in home feed: how many items and sections come back.
    #[tokio::test]
    #[ignore]
    async fn live_home_pages() {
        let cookie = crate::secret::read_cookie_io().unwrap();
        let home = super::Api::with_cookie(cookie).await.unwrap().home().await.unwrap();
        let mut sections: Vec<&str> = Vec::new();
        for t in &home {
            if !sections.contains(&t.album.as_str()) {
                sections.push(&t.album);
            }
        }
        println!("{} items in {} sections", home.len(), sections.len());
    }

    /// The saved sign-in's account (prints only whether each part came back).
    #[tokio::test]
    #[ignore]
    async fn live_account() {
        let cookie = crate::secret::read_cookie_io().unwrap();
        let a = super::Api::with_cookie(cookie).await.unwrap().account().await.unwrap();
        println!("name: {}, handle: {}, photo: {}", !a.name.is_empty(), !a.handle.is_empty(), a.photo.is_some());
    }

    use super::*;

    #[test]
    fn song_rows_keep_artist_album_ids_and_length() {
        let row = serde_json::json!({
            "playlistItemData": { "videoId": "abc" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Singer", "navigationEndpoint": { "browseEndpoint": { "browseId": "UC1",
                        "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" } } } } },
                    { "text": " • " },
                    { "text": "Record", "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREb_1",
                        "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ALBUM" } } } } }
                ] } } }
            ],
            "fixedColumns": [{ "musicResponsiveListItemFixedColumnRenderer": { "text": { "runs": [{ "text": "3:41" }] } } }]
        });
        let t = song_row(&row).unwrap();
        assert_eq!((t.video_id.as_str(), t.title.as_str(), t.artist.as_str(), t.album.as_str(), t.duration.as_str()), ("abc", "Song", "Singer", "Record", "3:41"));
        assert_eq!((t.artist_id.as_deref(), t.album_id.as_deref()), (Some("UC1"), Some("MPREb_1")));
        // Both ways of saying "next page" are found.
        let page = serde_json::json!({ "x": [{ "continuationItemRenderer": { "continuationEndpoint": { "continuationCommand": { "token": "T1" } } } }] });
        assert_eq!(all(&page, "continuationCommand")[0]["token"], "T1");
    }
}

#[cfg(test)]
mod live_probe {
    use super::*;

    /// Signed in: the whole liked list and one artist's page. Prints counts only.
    #[tokio::test]
    #[ignore]
    async fn liked_and_artist() {
        let cookie = crate::secret::read_cookie_io().unwrap();
        let api = Api::with_cookie(cookie).await.unwrap();
        let first = api.clone().playlist_tracks(LIKED_ID.into()).await.unwrap();
        let all = api.clone().playlist_all(LIKED_ID.into(), 5000).await.unwrap();
        println!("liked: first page {}, whole list {}", first.len(), all.len());
        let id = match all.iter().find_map(|t| t.artist_id.clone()) {
            Some(id) => id,
            None => api.clone().artist_id_for(all[0].artist.clone()).await.unwrap().unwrap(),
        };
        let page = api.clone().artist_page(id).await.unwrap();
        let songs = page.iter().filter(|t| t.link.is_none()).count();
        println!("artist page: {songs} songs, {} albums/singles", page.len() - songs);
        let found = api.artist_id_for("Daft Punk".into()).await.unwrap();
        println!("artist by name found: {}", found.is_some());
    }
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;

    /// Read only: what YouTube Music suggests, and whether the account's search history comes
    /// back: `cargo test --lib live_search_suggestions -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_search_suggestions() {
        let cookie = crate::secret::read_cookie_io().expect("signed in");
        let api = Api::with_cookie(cookie).await.unwrap();
        for input in ["", "mac de"] {
            let list = api.search_suggestions(input.to_string()).await.unwrap();
            let history = list.iter().filter(|s| s.from_history()).count();
            let tokens = list.iter().filter(|s| s.forget.as_deref().is_some_and(|t| !t.is_empty())).count();
            println!("input {input:?}: {} suggestions, {history} from the account's history ({tokens} with a forget token); first: {:?}", list.len(), list.iter().take(4).map(|s| &s.text).collect::<Vec<_>>());
        }
    }
}

#[cfg(test)]
mod search_tests {
    /// Songs then videos for a search: `cargo test --lib live_search_videos -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_search_videos() {
        let cookie = crate::secret::read_cookie_io().unwrap_or_default();
        let api = if cookie.trim().is_empty() { super::Api::guest().await.unwrap() } else { super::Api::with_cookie(cookie).await.unwrap() };
        for q in ["Philosophie du Soi", "باسم الكربلائي محاضرة"] {
            let p = api.clone().search_paged(q.into()).await.unwrap();
            println!("{q}: {} results, more {}", p.tracks.len(), p.more.is_some());
            for t in p.tracks.iter().take(3).chain(p.tracks.iter().rev().take(2)) {
                println!("  [{}] {} / {} / {} {}", t.video_id, t.title, t.artist, t.album, t.duration);
            }
        }
    }

    #[test]
    fn titles_lose_the_upload_clutter() {
        let cases = [
            ("Blinding Lights (Official Video)", "The Weeknd", "Blinding Lights"),
            ("Tame Impala - The Less I Know The Better [Official Music Video]", "Tame Impala", "The Less I Know The Better"),
            ("Song (Lyrics) [HD]", "X", "Song"),
            ("Song | Official Audio", "X", "Song"),
            ("Song (Official Visualizer) #shorts #music", "X", "Song"),
            ("Song (Remix) (Official Video)", "X", "Song (Remix)"),
            ("Song (Live at Wembley)", "X", "Song (Live at Wembley)"),
            ("Song (feat. Y) [4K]", "X", "Song (feat. Y)"),
            ("Song (Official)", "X", "Song (Official)"),
            ("Other Artist - Song", "X", "Other Artist - Song"),
            ("خطار إجاني (Official Video)", "باسم الكربلائي", "خطار إجاني"),
            ("(Official Video)", "X", "(Official Video)"),
        ];
        for (title, artist, want) in cases {
            assert_eq!(super::clean_title(title, artist), want, "{title}");
        }
    }

}
