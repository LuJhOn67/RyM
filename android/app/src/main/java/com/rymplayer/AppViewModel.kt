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
import com.rymplayer.data.LyricsCache
import com.rymplayer.data.Playlist
import com.rymplayer.data.SignedOutException
import com.rymplayer.data.Track
import com.rymplayer.data.MusicApi
import com.rymplayer.data.parseDuration
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import com.rymplayer.playback.Player
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
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
        I18n.arabic = prefs.getBoolean("arabic", false)
        com.rymplayer.playback.Streams.warmUp()
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
                    syncLiked(likedSongs)
                } catch (e: SignedOutException) {
                    status = tr("Your session ended. Sign in again.")
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
        localLikes.clear()
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
        if (q.isNotEmpty()) open(Screen.Songs(Screen.Kind.Results, q, "${tr("Results for")} \"$q\""))
    }

    /** When the feed was last fetched; coming back after half an hour fetches a fresh one. */
    var homeLoadedAt = 0L
        private set

    fun refreshHomeIfOld() {
        if (!homeLoading && System.currentTimeMillis() - homeLoadedAt > 30 * 60 * 1000L) loadHome()
    }

    /**
     * A fresh home feed: YouTube's page (rested items left out, see [FeedMemory]) under a
     * "New for you" shelf made from the radio of a few liked or recently played songs, different
     * ones each time. The liked songs are read on the way, so hearts are up to date too.
     */
    fun loadHome() {
        if (homeLoading && homeJob?.isActive == true) return
        homeLoading = true
        homeJob = viewModelScope.launch {
            val ctx = getApplication<Application>()
            // A failed refresh keeps what's on screen (the saved feed) instead of emptying it.
            runCatching {
                coroutineScope {
                    val user = api.signedIn
                    val page = async { api.home() }
                    val likedSongs = async { if (user) runCatching { api.playlist(LIKED_ID) }.getOrNull() else null }
                    val played = async { if (user) runCatching { api.history() }.getOrDefault(emptyList()) else emptyList() }
                    val arranged = FeedMemory.arrange(ctx, page.await())
                    val likedList = likedSongs.await()
                    likedList?.let(::syncLiked)
                    val history = played.await()
                    val pool = likedList.orEmpty().take(60) + history.take(40) + arranged.filter { it.link == null }
                    val seeds = FeedMemory.pickSeeds(ctx, pool, 3)
                    val radios = seeds.map { s -> async { runCatching { api.radio(s) }.getOrDefault(emptyList()) } }.awaitAll()
                    val known = (likedList.orEmpty() + history + arranged).map { it.videoId }.toSet() + seeds
                    FeedMemory.pickNew(ctx, radios, known) + arranged
                }
            }
                .onFailure { if (home.isEmpty()) status = it.message.orEmpty() }
                .onSuccess { fresh ->
                    home = fresh
                    PageCache.saveTracks(ctx, "home", home)
                    homeLoadedAt = System.currentTimeMillis()
                }
            // Songs at the top start right away when tapped.
            com.rymplayer.playback.Streams.prefetchLinks(home.filter { it.link == null }.take(8).map { it.videoId })
            homeLoading = false
        }
    }

    private var homeJob: Job? = null

    // ---------------------------------------------------------------- liked songs, kept in step

    /** Likes made here lately, with when: they win over a synced list YouTube may not have updated yet. */
    private val localLikes = HashMap<String, Pair<Boolean, Long>>()
    private var syncJob: Job? = null

    /** The account's liked songs as YouTube Music has them now (likes from the PC show up here). */
    private fun syncLiked(songs: List<Track>) {
        val now = System.currentTimeMillis()
        localLikes.entries.removeAll { now - it.value.second > 60_000 }
        val ids = LinkedHashSet(songs.map { it.videoId })
        var shown = songs
        for ((id, change) in localLikes) {
            if (change.first) ids += id else { ids -= id; shown = shown.filter { it.videoId != id } }
        }
        if (liked.toSet() != ids) {
            liked.clear()
            liked += ids
        }
        likedPages().forEach { pages[it] = shown }
    }

    private fun likedPages() = pages.keys.filter { it.kind == Screen.Kind.Playlist && it.id == LIKED_ID }

    fun syncLikesNow() {
        if (!api.signedIn || syncJob?.isActive == true) return
        syncJob = viewModelScope.launch { runCatching { api.playlist(LIKED_ID) }.onSuccess(::syncLiked) }
    }

    private var foregroundJob: Job? = null

    /** App on screen: liked songs are re-read every two minutes (and right away). */
    fun onForeground() {
        if (foregroundJob?.isActive == true) return
        foregroundJob = viewModelScope.launch {
            while (isActive) {
                syncLikesNow()
                delay(120_000)
            }
        }
    }

    fun onBackground() {
        foregroundJob?.cancel()
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

    /** The app's words in Arabic instead of English (Library > Language). */
    fun toggleArabic() {
        I18n.arabic = !I18n.arabic
        prefs.edit().putBoolean("arabic", I18n.arabic).apply()
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
        runCatching { com.rymplayer.playback.Vlc.open(getApplication(), t) }.onFailure { status = "${tr("VLC couldn't open it")}: ${it.message}" }
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
        status = "${tr("Downloading")} \"${t.title}\"..."
        status = runCatching { Downloads.save(getApplication(), t, prefs.getString("cookie", null), downloadBest) }.fold({ tr("Saved to Music/RymPlayer") }, {
            android.util.Log.w("RyM", "download of ${t.videoId} failed", it)
            "${tr("Download failed")}: ${it.message ?: it.javaClass.simpleName}"
        })
    }

    /** The artist's page (every song, then albums and singles); found by name when the song lacks the id. */
    fun goArtist(t: Track) {
        if (t.artist.isBlank()) return
        val name = t.artist.substringBefore(", ")
        playerOpen = false
        t.artistId?.let { open(Screen.Songs(Screen.Kind.Artist, it, name)); return }
        status = tr("Finding the artist...")
        viewModelScope.launch {
            val id = runCatching { api.artistIdFor(name) }.getOrNull()
            if (id == null) status = tr("Artist not found") else open(Screen.Songs(Screen.Kind.Artist, id, name))
        }
    }

    /** How the liked songs batch download is going (null: not started). */
    data class Batch(val done: Int, val total: Int, val saved: Int = 0, val skipped: Int = 0, val failed: Int = 0) {
        val finished get() = done >= total
    }

    var batch by mutableStateOf<Batch?>(null)
        private set

    /** Downloads the first [limit] liked songs, one after another, skipping ones already saved. */
    fun downloadAll(limit: Int) {
        if (batch?.finished == false || !api.signedIn) return
        batch = Batch(0, 0)
        viewModelScope.launch {
            val songs = runCatching { api.playlistAll(LIKED_ID, limit) }.getOrElse {
                status = "${tr("Download failed")}: ${it.message}"
                batch = null
                return@launch
            }
            var b = Batch(0, songs.size)
            batch = b
            for (t in songs) {
                val r = runCatching { Downloads.save(getApplication(), t, prefs.getString("cookie", null), downloadBest) }
                b = when {
                    r.getOrNull() == "already saved" -> b.copy(skipped = b.skipped + 1)
                    r.isSuccess -> b.copy(saved = b.saved + 1)
                    else -> b.copy(failed = b.failed + 1)
                }
                b = b.copy(done = b.done + 1)
                batch = b
            }
        }
    }

    fun toggleLike(t: Track) {
        val like = t.videoId !in liked
        setLiked(t, like)
        viewModelScope.launch {
            runCatching { api.rate(t.videoId, if (like) MusicApi.Rating.Like else MusicApi.Rating.None) }.onFailure {
                setLiked(t, !like)
                status = tr("Couldn't save the like")
            }
        }
    }

    /** Hearts and the Liked Music page follow at once. */
    private fun setLiked(t: Track, like: Boolean) {
        localLikes[t.videoId] = like to System.currentTimeMillis()
        if (like) { if (t.videoId !in liked) liked += t.videoId } else liked -= t.videoId
        for (key in likedPages()) {
            val list = pages[key].orEmpty()
            pages[key] = if (like) (if (list.none { it.videoId == t.videoId }) listOf(t) + list else list) else list.filter { it.videoId != t.videoId }
        }
    }

    fun addToPlaylist(p: Playlist, t: Track) = viewModelScope.launch {
        status = runCatching { api.addToPlaylist(p.id, t.videoId) }.fold({ "${tr("Added to")} \"${p.title}\"" }, { "${tr("Couldn't add")}: ${it.message}" })
    }

    fun createPlaylist(title: String, with: Track?) = viewModelScope.launch {
        runCatching { api.createPlaylist(title) }.onSuccess { p ->
            playlists = listOf(p) + playlists
            status = "Created \"$title\""
            if (with != null) addToPlaylist(p, with)
        }.onFailure { status = tr("Couldn't create the playlist") }
    }

    private fun loadLyrics(t: Track?) {
        lyricsJob?.cancel()
        lyrics = null
        if (t == null) return
        lyricsStatus = tr("Looking for lyrics...")
        lyricsJob = viewModelScope.launch {
            val found = findLyrics(t)
            lyrics = found
            lyricsStatus = if (found == null) tr("No lyrics for this song") else ""
            // The next song's lyrics are ready before it starts.
            player.queue.getOrNull(player.index + 1)?.let { next ->
                if (LyricsCache.get(getApplication(), next.videoId) == null) findLyrics(next)
            }
        }
    }

    /** Saved lyrics, or LRCLIB and YouTube Music asked at once (LRCLIB's preferred: it has timing). */
    private suspend fun findLyrics(t: Track): Lyrics? {
        val ctx = getApplication<Application>()
        LyricsCache.get(ctx, t.videoId)?.let { return it.lyrics }
        return coroutineScope {
            val ytm = async { runCatching { api.lyrics(t.videoId) }.getOrNull() }
            val found = Lrclib.find(t.title, t.artist, t.album, parseDuration(t.duration)) ?: ytm.await()
            ytm.cancel()
            LyricsCache.put(ctx, t.videoId, found)
            found
        }
    }

    override fun onCleared() {
        player.release()
    }
}

