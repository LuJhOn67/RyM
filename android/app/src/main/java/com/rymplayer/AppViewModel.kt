package com.rymplayer

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.rymplayer.data.Downloads
import com.rymplayer.data.FeedMemory
import com.rymplayer.data.PageCache
import com.rymplayer.data.LIKED_ID
import com.rymplayer.data.Link
import com.rymplayer.data.Lrclib
import com.rymplayer.data.Lyrics
import com.rymplayer.data.Playlist
import com.rymplayer.data.SignedOutException
import com.rymplayer.data.Track
import com.rymplayer.data.MusicApi
import com.rymplayer.data.parseDuration
import com.rymplayer.playback.Player
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

/** The song a menu is open for, and its place in the queue when opened from "Up next". */
data class MenuFor(val track: Track, val queueIndex: Int? = null)

sealed interface Screen {
    data object Home : Screen
    data object Search : Screen
    data object Library : Screen
    data class Songs(val kind: Kind, val id: String, val title: String) : Screen

    enum class Kind { Playlist, Album, Mix, Radio, Artist, History, Results, Downloads }
}

class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val prefs = app.getSharedPreferences("rym", Context.MODE_PRIVATE)

    var signedIn by mutableStateOf<Boolean?>(null)
        private set
    var api: MusicApi = MusicApi(null)
        private set
    var status by mutableStateOf("")

    /** Bottom tab; detail pages stack on top of it. */
    var tab by mutableStateOf<Screen>(Screen.Home)
        private set
    val stack = mutableStateListOf<Screen.Songs>()
    val screen: Screen get() = stack.lastOrNull() ?: tab

    var home by mutableStateOf<List<Track>>(emptyList())
        private set
    var homeLoading by mutableStateOf(false)
        private set
    var playlists by mutableStateOf<List<Playlist>>(emptyList())
        private set
    val pages = androidx.compose.runtime.mutableStateMapOf<Screen.Songs, List<Track>>()
    var query by mutableStateOf("")
    val liked = mutableStateListOf<String>()

    val player = Player(app, viewModelScope)
    var lyrics by mutableStateOf<Lyrics?>(null)
        private set
    var lyricsStatus by mutableStateOf("")
        private set
    var playerOpen by mutableStateOf(false)
    /** Song menu (bottom sheet), shown over everything; null when closed. */
    var menu by mutableStateOf<MenuFor?>(null)
    private var lyricsJob: Job? = null

    init {
        com.rymplayer.playback.Streams.connections = prefs.getInt("dl_connections", 8)
        com.rymplayer.playback.Streams.pieceMb = prefs.getInt("dl_piece_mb", 0)
        val cookie = prefs.getString("cookie", null)
        if (cookie != null) start(cookie) else signedIn = false
        player.onTrackChanged = ::loadLyrics
        player.useVlc = prefs.getBoolean("vlc", false)
    }

    // ---------------------------------------------------------------- session

    fun start(cookie: String?) {
        prefs.edit().apply { if (cookie != null) putString("cookie", cookie) else remove("cookie") }.apply()
        api = MusicApi(cookie)
        com.rymplayer.playback.Streams.cookie = cookie
        signedIn = cookie != null
        player.api = api
        // Last saved home and playlists show at once; fresh ones replace them when they arrive.
        if (home.isEmpty()) home = PageCache.loadTracks(getApplication(), "home")
        if (cookie != null && playlists.isEmpty()) playlists = PageCache.loadPlaylists(getApplication())
        loadHome()
        if (api.signedIn) {
            viewModelScope.launch {
                try {
                    // Both at once instead of one after the other.
                    val (lists, likedSongs) = coroutineScope {
                        val lists = async { api.libraryPlaylists() }
                        val likedSongs = async { api.playlist(LIKED_ID) }
                        lists.await() to likedSongs.await()
                    }
                    playlists = lists
                    PageCache.savePlaylists(getApplication(), lists)
                    liked.clear()
                    liked += likedSongs.map { it.videoId }
                } catch (e: SignedOutException) {
                    status = "Your session ended. Sign in again."
                    signOut()
                } catch (e: Exception) {
                    status = e.message.orEmpty()
                }
            }
        }
    }

    fun guest() = start(null).also { signedIn = true }

    fun signOut() {
        prefs.edit().remove("cookie").apply()
        // Forget the sign-in page's Google session too, so the next sign-in can pick another account.
        android.webkit.CookieManager.getInstance().removeAllCookies(null)
        player.stop()
        api = MusicApi(null)
        signedIn = false
        playlists = emptyList()
        liked.clear()
        stack.clear()
        // The saved home and playlists were this account's.
        PageCache.clear(getApplication())
        home = emptyList()
    }

    // ---------------------------------------------------------------- navigation

    fun selectTab(s: Screen) {
        stack.clear()
        tab = s
        playerOpen = false
    }

    fun open(page: Screen.Songs) {
        stack += page
        playerOpen = false
        // Downloads can change at any time; the rest is loaded once.
        if (pages[page] == null || page.kind == Screen.Kind.Downloads) load(page)
    }

    /** True when something was closed. */
    fun back(): Boolean = when {
        playerOpen -> { playerOpen = false; true }
        stack.isNotEmpty() -> { stack.removeAt(stack.lastIndex); true }
        tab != Screen.Home -> { tab = Screen.Home; true }
        else -> false
    }

    fun openLink(t: Track) {
        FeedMemory.opened(getApplication(), t)
        when (val l = t.link) {
            is Link.Playlist -> open(Screen.Songs(Screen.Kind.Playlist, l.id, t.title))
            is Link.Album -> open(Screen.Songs(Screen.Kind.Album, l.id, t.title))
            is Link.Mix -> open(Screen.Songs(Screen.Kind.Mix, l.id, t.title))
            null -> playRadio(t)
        }
    }

    fun search() {
        val q = query.trim()
        if (q.isNotEmpty()) open(Screen.Songs(Screen.Kind.Results, q, "Results for \"$q\""))
    }

    /** When the feed was last fetched; coming back after half an hour fetches a fresh one. */
    var homeLoadedAt = 0L
        private set

    fun refreshHomeIfOld() {
        if (!homeLoading && System.currentTimeMillis() - homeLoadedAt > 30 * 60 * 1000L) loadHome()
    }

    fun loadHome() {
        homeLoading = true
        viewModelScope.launch {
            // A failed refresh keeps what's on screen (the saved feed) instead of emptying it.
            runCatching { api.home() }
                .onFailure { if (home.isEmpty()) status = it.message.orEmpty() }
                .onSuccess { fresh ->
                    home = FeedMemory.arrange(getApplication(), fresh)
                    PageCache.saveTracks(getApplication(), "home", home)
                    homeLoadedAt = System.currentTimeMillis()
                }
            // Songs at the top start right away when tapped.
            com.rymplayer.playback.Streams.prefetchLinks(home.filter { it.link == null }.take(8).map { it.videoId })
            homeLoading = false
        }
    }

    private fun load(page: Screen.Songs) {
        viewModelScope.launch {
            val r = runCatching {
                when (page.kind) {
                    Screen.Kind.Playlist -> api.playlist(page.id)
                    Screen.Kind.Album -> api.album(page.id)
                    Screen.Kind.Mix -> api.mix(page.id)
                    Screen.Kind.Radio -> api.radio(page.id)
                    Screen.Kind.Artist -> api.artist(page.id)
                    Screen.Kind.History -> api.history()
                    Screen.Kind.Results -> api.search(page.id)
                    Screen.Kind.Downloads -> Downloads.list(getApplication())
                }
            }
            r.onFailure { status = it.message.orEmpty() }
            pages[page] = r.getOrDefault(emptyList())
            com.rymplayer.playback.Streams.prefetchLinks(pages[page].orEmpty().filter { it.link == null }.take(8).map { it.videoId })
        }
    }

    // ---------------------------------------------------------------- playback

    /** Plays row `index` of a list; the queue becomes the list's songs. */
    fun playFrom(list: List<Track>, index: Int) {
        val songs = list.filter { it.link == null }
        val start = songs.indexOfFirst { it.videoId == list[index].videoId }.coerceAtLeast(0)
        player.setQueue(songs, start)
    }

    /** One song, and YouTube Music's radio after it. */
    fun playRadio(t: Track) {
        FeedMemory.opened(getApplication(), t)
        player.setQueue(listOf(t), 0)
        viewModelScope.launch {
            runCatching { api.radio(t.videoId) }.getOrNull()?.let { radio -> player.append(radio.filter { it.videoId != t.videoId }) }
        }
    }

    /** Download connections at once (1-16) and piece size in MB (0 = open: split evenly). */
    var connections by mutableStateOf(prefs.getInt("dl_connections", 8))
        private set
    var pieceMb by mutableStateOf(prefs.getInt("dl_piece_mb", 0))
        private set

    fun nextConnections() {
        val steps = listOf(1, 2, 4, 6, 8, 10, 12, 16)
        connections = steps[(steps.indexOf(connections).coerceAtLeast(0) + 1) % steps.size]
        prefs.edit().putInt("dl_connections", connections).apply()
        com.rymplayer.playback.Streams.connections = connections
    }

    fun nextPieceSize() {
        val steps = listOf(0, 1, 2, 4, 8)
        pieceMb = steps[(steps.indexOf(pieceMb).coerceAtLeast(0) + 1) % steps.size]
        prefs.edit().putInt("dl_piece_mb", pieceMb).apply()
        com.rymplayer.playback.Streams.pieceMb = pieceMb
    }

    /** Play songs in VLC instead of the built-in player (only offered when VLC is installed). */
    var playInVlc by mutableStateOf(prefs.getBoolean("vlc", false))
        private set

    fun togglePlayInVlc() {
        playInVlc = !playInVlc
        prefs.edit().putBoolean("vlc", playInVlc).apply()
        player.useVlc = playInVlc
    }

    fun openInVlc(t: Track) = viewModelScope.launch {
        runCatching { com.rymplayer.playback.Vlc.open(getApplication(), t) }.onFailure { status = "VLC couldn't open it: ${it.message}" }
    }

    /** Downloads in the best quality (yt-dlp, slow on a phone) instead of the playback file. */
    var downloadBest by mutableStateOf(prefs.getBoolean("download_best", false))
        private set

    fun toggleDownloadQuality() {
        downloadBest = !downloadBest
        prefs.edit().putBoolean("download_best", downloadBest).apply()
    }

    /** Saves a song to Music/RymPlayer; it plays from there afterwards. */
    fun download(t: Track) = viewModelScope.launch {
        status = "Downloading \"${t.title}\"..."
        status = runCatching { Downloads.save(getApplication(), t, prefs.getString("cookie", null), downloadBest) }.fold({ "Saved to Music/RymPlayer" }, {
            android.util.Log.w("RyM", "download of ${t.videoId} failed", it)
            "Download failed: ${it.message ?: it.javaClass.simpleName}"
        })
    }

    fun toggleLike(t: Track) {
        val like = t.videoId !in liked
        if (like) liked += t.videoId else liked -= t.videoId
        viewModelScope.launch {
            runCatching { api.rate(t.videoId, if (like) MusicApi.Rating.Like else MusicApi.Rating.None) }.onFailure {
                if (like) liked -= t.videoId else liked += t.videoId
                status = "Couldn't save the like"
            }
        }
    }

    fun addToPlaylist(p: Playlist, t: Track) = viewModelScope.launch {
        status = runCatching { api.addToPlaylist(p.id, t.videoId) }.fold({ "Added to \"${p.title}\"" }, { "Couldn't add: ${it.message}" })
    }

    fun createPlaylist(title: String, with: Track?) = viewModelScope.launch {
        runCatching { api.createPlaylist(title) }.onSuccess { p ->
            playlists = listOf(p) + playlists
            status = "Created \"$title\""
            if (with != null) addToPlaylist(p, with)
        }.onFailure { status = "Couldn't create the playlist" }
    }

    private fun loadLyrics(t: Track?) {
        lyricsJob?.cancel()
        lyrics = null
        if (t == null) return
        lyricsStatus = "Looking for lyrics..."
        lyricsJob = viewModelScope.launch {
            val found = Lrclib.find(t.title, t.artist, t.album, parseDuration(t.duration))
                ?: runCatching { api.lyrics(t.videoId) }.getOrNull()
            lyrics = found
            lyricsStatus = if (found == null) "No lyrics for this song" else ""
        }
    }

    override fun onCleared() {
        player.release()
    }
}

