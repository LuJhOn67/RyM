package com.rymplayer

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.rymplayer.data.LIKED_ID
import com.rymplayer.data.Link
import com.rymplayer.data.Lrclib
import com.rymplayer.data.Lyrics
import com.rymplayer.data.Playlist
import com.rymplayer.data.SignedOutException
import com.rymplayer.data.Track
import com.rymplayer.data.YtMusic
import com.rymplayer.data.parseDuration
import com.rymplayer.playback.Player
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

sealed interface Screen {
    data object Home : Screen
    data object Search : Screen
    data object Library : Screen
    data class Songs(val kind: Kind, val id: String, val title: String) : Screen

    enum class Kind { Playlist, Album, Mix, Radio, Artist, History, Results }
}

class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val prefs = app.getSharedPreferences("rym", Context.MODE_PRIVATE)

    var signedIn by mutableStateOf<Boolean?>(null)
        private set
    var api: YtMusic = YtMusic(null)
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
    private var lyricsJob: Job? = null

    init {
        val cookie = prefs.getString("cookie", null)
        if (cookie != null) start(cookie) else signedIn = false
        player.onTrackChanged = ::loadLyrics
    }

    // ---------------------------------------------------------------- session

    fun start(cookie: String?) {
        prefs.edit().apply { if (cookie != null) putString("cookie", cookie) else remove("cookie") }.apply()
        api = YtMusic(cookie)
        signedIn = cookie != null
        player.api = api
        loadHome()
        if (api.signedIn) {
            viewModelScope.launch {
                try {
                    playlists = api.libraryPlaylists()
                    liked.clear()
                    liked += api.playlist(LIKED_ID).map { it.videoId }
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
        player.stop()
        api = YtMusic(null)
        signedIn = false
        playlists = emptyList()
        liked.clear()
        stack.clear()
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
        if (pages[page] == null) load(page)
    }

    /** True when something was closed. */
    fun back(): Boolean = when {
        playerOpen -> { playerOpen = false; true }
        stack.isNotEmpty() -> { stack.removeAt(stack.lastIndex); true }
        tab != Screen.Home -> { tab = Screen.Home; true }
        else -> false
    }

    fun openLink(t: Track) {
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

    fun loadHome() {
        homeLoading = true
        viewModelScope.launch {
            home = runCatching { api.home() }.onFailure { status = it.message.orEmpty() }.getOrDefault(emptyList())
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
                }
            }
            r.onFailure { status = it.message.orEmpty() }
            pages[page] = r.getOrDefault(emptyList())
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
        player.setQueue(listOf(t), 0)
        viewModelScope.launch {
            runCatching { api.radio(t.videoId) }.getOrNull()?.let { radio -> player.append(radio.filter { it.videoId != t.videoId }) }
        }
    }

    fun toggleLike(t: Track) {
        val like = t.videoId !in liked
        if (like) liked += t.videoId else liked -= t.videoId
        viewModelScope.launch {
            runCatching { api.rate(t.videoId, if (like) YtMusic.Rating.Like else YtMusic.Rating.None) }.onFailure {
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

