package com.rymplayer.playback

import android.content.Context
import android.net.Uri
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService.LibraryParams
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import com.rymplayer.data.Track
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * RyM's songs for Android Auto and the voice assistant (backlog 28 and 54): shelves to browse
 * (Recently played, For you, the last queue), search, and "Ok Google, play X on RyM". A song's
 * item carries a `rym://song/<id>` link that the player's data source looks up when it loads
 * (see [Streams.resolve]), so a whole shelf starts at once and each link is found when needed.
 * When the app is open, its queue takes over the list (so Next and the radio work as usual).
 */
@UnstableApi
class Library(private val ctx: Context, private val scope: CoroutineScope) : MediaLibrarySession.Callback {
    private val shelves = linkedMapOf("recent" to "Recently played", "home" to "For you", "session" to "Up next")

    /** A song's id -> the list it was shown in (to play the rest of the list after it). */
    private val shownIn = java.util.concurrent.ConcurrentHashMap<String, List<Track>>()
    @Volatile private var found: List<Track> = emptyList()

    private fun tracksOf(id: String): List<Track> = when (id) {
        "recent" -> com.rymplayer.data.Recent.tracks(ctx)
        "home" -> com.rymplayer.data.PageCache.loadTracks(ctx, "home")
        "session" -> com.rymplayer.data.PageCache.loadTracks(ctx, "session")
        "search" -> found
        else -> emptyList()
    }.filter { it.link == null && com.rymplayer.data.isYouTube(it.videoId) }.distinctBy { it.videoId }.take(100)

    private fun folder(id: String, title: String) = MediaItem.Builder().setMediaId(id).setMediaMetadata(
        MediaMetadata.Builder().setTitle(com.rymplayer.tr(title)).setIsBrowsable(true).setIsPlayable(false)
            .setMediaType(MediaMetadata.MEDIA_TYPE_FOLDER_MIXED).build(),
    ).build()

    private fun song(t: Track) = MediaItem.Builder().setMediaId(t.videoId).setUri("rym://song/${t.videoId}").setMediaMetadata(
        MediaMetadata.Builder().setTitle(t.title).setArtist(t.artist)
            .apply { com.rymplayer.data.thumbSized(t.thumb, 544)?.let { setArtworkUri(Uri.parse(it)) } }
            .setIsBrowsable(false).setIsPlayable(true).setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC).build(),
    ).build()

    private fun shown(list: List<Track>): List<Track> = list.also { l -> l.forEach { shownIn[it.videoId] = l } }

    /** Songs for the words [q]: YouTube Music's own search (quick, nothing to start first). */
    private suspend fun search(q: String): List<Track> = withContext(Dispatchers.IO) {
        val api = com.rymplayer.data.MusicApi(Streams.cookie)
        runCatching { api.builtinSearch(q) }.getOrNull().orEmpty().ifEmpty { runCatching { api.search(q) }.getOrNull().orEmpty() }
            .filter { it.link == null && com.rymplayer.data.isYouTube(it.videoId) }.take(50)
    }

    override fun onGetLibraryRoot(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> =
        Futures.immediateFuture(LibraryResult.ofItem(folder("root", "RyM"), params))

    override fun onGetChildren(
        session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
        val all = if (parentId == "root") shelves.filter { tracksOf(it.key).isNotEmpty() }.map { folder(it.key, it.value) }
        else shown(tracksOf(parentId)).map(::song)
        val size = if (pageSize > 0) pageSize else all.size.coerceAtLeast(1)
        return Futures.immediateFuture(LibraryResult.ofItemList(ImmutableList.copyOf(all.drop(page * size).take(size)), params))
    }

    override fun onGetItem(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> {
        val t = shownIn[mediaId]?.firstOrNull { it.videoId == mediaId }
        return Futures.immediateFuture(if (t != null) LibraryResult.ofItem(song(t), null) else LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE))
    }

    override fun onSearch(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: LibraryParams?): ListenableFuture<LibraryResult<Void>> {
        scope.launch {
            found = shown(search(query))
            session.notifySearchResultChanged(browser, query, found.size, params)
        }
        return Futures.immediateFuture(LibraryResult.ofVoid())
    }

    override fun onGetSearchResult(
        session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, page: Int, pageSize: Int, params: LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
        val size = if (pageSize > 0) pageSize else found.size.coerceAtLeast(1)
        return Futures.immediateFuture(LibraryResult.ofItemList(ImmutableList.copyOf(found.drop(page * size).take(size).map(::song)), params))
    }

    /** Items asked for by id (the controller's carry no link): RyM's own, with the link to look up. */
    override fun onAddMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>): ListenableFuture<MutableList<MediaItem>> {
        val q = mediaItems.singleOrNull()?.requestMetadata?.searchQuery
        if (q != null) {
            val out = SettableFuture.create<MutableList<MediaItem>>()
            scope.launch { out.set(shown(search(q)).take(1).map(::song).toMutableList()) }
            return out
        }
        return Futures.immediateFuture(mediaItems.map { m ->
            shownIn[m.mediaId]?.firstOrNull { it.videoId == m.mediaId }?.let(::song)
                ?: m.buildUpon().setUri("rym://song/${m.mediaId}").build()
        }.toMutableList())
    }

    /**
     * A tap on a song (or "play X"): the rest of its list follows it. With the app open, its queue
     * takes the list and the player gets the one song (the app moves on as usual); otherwise the
     * player gets the whole list.
     */
    /** [list] from [at]: with the app open its queue takes the list and the player the one song. */
    private fun withList(list: List<Track>, at: Int, from: Long): MediaSession.MediaItemsWithStartPosition {
        val adopt = QueueBridge.adopt
        return if (adopt != null) {
            adopt(list, at)
            MediaSession.MediaItemsWithStartPosition(listOf(song(list[at])), 0, from)
        } else MediaSession.MediaItemsWithStartPosition(list.map(::song), at, from)
    }

    /** Play pressed with nothing loaded (the car, a Bluetooth button): the last queue goes on. */
    override fun onPlaybackResumption(session: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val last = shown(tracksOf("session")).ifEmpty { shown(tracksOf("recent")) }
        return if (last.isEmpty()) Futures.immediateFailedFuture(UnsupportedOperationException("nothing played yet"))
        else Futures.immediateFuture(withList(last, 0, C.TIME_UNSET))
    }

    override fun onSetMediaItems(
        session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val one = mediaItems.singleOrNull()
        val q = one?.requestMetadata?.searchQuery
        // A song from a shelf: answered at once (a Play sent right after it finds it there).
        val shelf = one?.let { shownIn[it.mediaId] }
        if (q == null && one != null && shelf != null) {
            val at = shelf.indexOfFirst { it.videoId == one.mediaId }.coerceAtLeast(0)
            return Futures.immediateFuture(withList(shelf, at, startPositionMs))
        }
        if (one != null && q != null) {
            val out = SettableFuture.create<MediaSession.MediaItemsWithStartPosition>()
            scope.launch {
                val list = shown(search(q))
                val at = 0
                if (list.isEmpty()) {
                    out.set(MediaSession.MediaItemsWithStartPosition(emptyList(), 0, C.TIME_UNSET))
                    return@launch
                }
                out.set(withList(list, at, C.TIME_UNSET))
            }
            return out
        }
        return Futures.transform(onAddMediaItems(session, controller, mediaItems), { MediaSession.MediaItemsWithStartPosition(it, startIndex, startPositionMs) }, com.google.common.util.concurrent.MoreExecutors.directExecutor())
    }
}
