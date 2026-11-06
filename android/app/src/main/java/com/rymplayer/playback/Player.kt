package com.rymplayer.playback

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player as ExoListener
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.rymplayer.data.Downloads
import com.rymplayer.data.Track
import com.rymplayer.data.MusicApi
import com.rymplayer.data.parseDuration
import com.rymplayer.data.thumbSized
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * The queue, as Compose state. Audio plays in [PlaybackService] (so it keeps going in the
 * background and shows the media notification); this drives it through a [MediaController].
 */
class Player(private val context: Context, private val scope: CoroutineScope) {
    companion object {
        /** Played songs kept above a new start, at most. */
        const val HISTORY = 100
    }

    lateinit var api: MusicApi
    /** Hand songs to VLC instead of playing them here (see [Vlc]). */
    @Volatile var useVlc = false
    var onTrackChanged: (Track?) -> Unit = {}

    val queue = mutableStateListOf<Track>()
    var index by mutableIntStateOf(-1)
        private set
    var playing by mutableStateOf(false)
        private set
    var loading by mutableStateOf(false)
        private set
    var position by mutableFloatStateOf(0f)
        private set
    var message by mutableStateOf("")

    val current: Track? get() = queue.getOrNull(index)
    val duration: Float
        get() = controller?.duration?.takeIf { it != C.TIME_UNSET && it > 0 }?.let { it / 1000f }
            ?: current?.let { parseDuration(it.duration) } ?: 0f

    private val future = MediaController.Builder(
        context,
        SessionToken(context, ComponentName(context, PlaybackService::class.java)),
    ).buildAsync()
    private var controller: MediaController? = null
    private val ready = CompletableDeferred<MediaController>()
    private var ticker: Job? = null
    private var loadJob: Job? = null

    init {
        QueueBridge.next = { next() }
        QueueBridge.prev = { prev() }
        QueueBridge.hasNext = { index + 1 < queue.size }
        QueueBridge.mixNext = { done -> scope.launch { done(advanceForMix()) } }
        Link.pause = { pause() }
        future.addListener({
            val c = runCatching { future.get() }.getOrElse {
                ready.completeExceptionally(it)
                return@addListener
            }
            controller = c
            c.addListener(object : ExoListener.Listener {
                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    playing = isPlaying
                    countListening()
                    if (isPlaying) {
                        startTicker()
                        linkSnapshot()
                    } else saveSession()
                }

                override fun onPlaybackStateChanged(state: Int) {
                    if (state == ExoListener.STATE_READY && tapped > 0) {
                        android.util.Log.w("RyM", "timing: ${current?.title} started ${System.currentTimeMillis() - tapped} ms after the tap")
                        tapped = 0
                    }
                    loading = state == ExoListener.STATE_BUFFERING
                    if (state == ExoListener.STATE_ENDED) {
                        // Once per song, and only for the one playing now (a late "ended" from a
                        // song already skipped by hand must not skip the new one too).
                        val ended = controller?.currentMediaItem?.mediaId
                        if (ended != null && ended == current?.videoId && endedFor != playCount) {
                            endedFor = playCount
                            // Nothing after it yet: the radio is asked again and plays once it's in (see append).
                            if (index + 1 < queue.size) next() else current?.let(::keepGoing)
                        }
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    loading = false
                    // A cached file or link the player can't use is dropped, so the next try fetches it again.
                    current?.let {
                        Streams.forget(context, it.videoId)
                        Streams.forgetLink(it.videoId)
                    }
                    message = "${com.rymplayer.tr("Can't play")}: ${error.errorCodeName}"
                }
            })
            ready.complete(c)
        }, ContextCompat.getMainExecutor(context))
    }

    /**
     * While playing: the shown position once a second, only while the app is on screen (the
     * lyrics follow on their own, see the view model); where it is, kept every 15 seconds, and
     * listening time written once a minute, in the background too.
     */
    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && controller?.isPlaying == true) {
                if (visible) position = exactPosition()
                if (System.currentTimeMillis() - sessionSavedAt > 15_000) saveSession()
                if (System.currentTimeMillis() - statsWrittenAt > 60_000) {
                    countListening()
                    com.rymplayer.data.Recent.flushListening(context)
                    statsWrittenAt = System.currentTimeMillis()
                }
                delay(if (visible) 1000 else 15_000)
            }
            position = exactPosition()
        }
    }

    /** The app is on screen (the position is shown; otherwise it isn't kept up to date). */
    var visible by mutableStateOf(true)

    /** Changes when the user seeks (the lyrics jump with it). */
    var seeks by mutableIntStateOf(0)
        private set

    /** The player's own position, in seconds. */
    fun exactPosition(): Float = (controller?.currentPosition ?: 0L) / 1000f

    /** Playback speed (1 = normal). */
    val speed: Float get() = controller?.playbackParameters?.speed ?: 1f

    /** Listening time: counted from play/pause and song changes, added to the stats in batches. */
    private var playingSince = 0L
    private var statsWrittenAt = System.currentTimeMillis()

    private fun countListening() {
        val now = System.currentTimeMillis()
        val t = current
        if (playingSince > 0 && t != null) com.rymplayer.data.Recent.listened(t, now - playingSince)
        playingSince = if (controller?.isPlaying == true) now else 0L
    }

    /** Songs played this session (Up next leaves them out). */
    val played = mutableSetOf<String>()

    /** The song left off last time and the millisecond to go on from, until it plays. */
    private var resumeAt: Pair<String, Long>? = null
    private var sessionSavedAt = 0L
    /** The queue the session file holds (its songs are written again only when it changes). */
    private var savedQueue: List<String> = emptyList()

    /**
     * What plays, where in it, and the queue around it (100 songs before, 400 after), so the
     * next start picks up there even after a crash. Written on every new song and every 3 s.
     */
    fun saveSession(atMs: Long? = null) {
        sessionSavedAt = System.currentTimeMillis()
        val i = index
        val from = (i - 100).coerceAtLeast(0)
        val to = (if (i < 0) queue.size else i + 401).coerceAtMost(queue.size)
        val songs = queue.toList().subList(from, to)
        val ms = resumeAt?.takeIf { it.first == current?.videoId }?.second ?: atMs ?: (controller?.currentPosition ?: 0L)
        val ids = songs.map { it.videoId }
        if (ids != savedQueue) {
            com.rymplayer.data.PageCache.saveTracks(context, "session", songs)
            savedQueue = ids
        }
        context.getSharedPreferences("session", Context.MODE_PRIVATE).edit()
            .putInt("index", if (i < 0) -1 else i - from).putLong("ms", ms.coerceAtLeast(0)).apply()
        linkSnapshot(ms)
    }

    /** What plays, for RyM Link (a PC on the Wi-Fi can take it over from this second). */
    private fun linkSnapshot(ms: Long = controller?.currentPosition ?: 0L) {
        Link.snapshot = if (index < 0) null else Link.Snapshot(queue.toList(), index, ms.coerceAtLeast(0), System.currentTimeMillis(), controller?.isPlaying == true, speed)
    }

    /** The last session's queue, its song paused where it was left: play goes on from there. */
    fun restoreSession() {
        if (queue.isNotEmpty()) return
        val songs = com.rymplayer.data.PageCache.loadTracks(context, "session")
        val prefs = context.getSharedPreferences("session", Context.MODE_PRIVATE)
        val i = prefs.getInt("index", -1)
        if (i !in songs.indices) return
        queue += songs
        savedQueue = songs.map { it.videoId }
        index = i
        val ms = prefs.getLong("ms", 0)
        resumeAt = songs[i].videoId to ms
        position = ms / 1000f
    }

    /** How many songs added with "Add to queue" / "Play next" wait right after the playing one. */
    private var added = 0

    /** Counts play() calls, so a song's end is acted on once. */
    private var playCount = 0
    private var endedFor = -1

    /** Replaces the queue (and its history) with [list] from [start]. */
    fun setQueue(list: List<Track>, start: Int) {
        queue.clear()
        queue += list
        added = 0
        index = -1
        play(start)
    }

    /**
     * Plays [list] from [start], keeping the songs played so far (up to the playing one) above
     * it, so Previous goes back to them; the songs that were still to come are dropped.
     */
    fun start(list: List<Track>, start: Int) {
        if (list.isEmpty()) return
        val keep = (index + 1).coerceIn(0, queue.size)
        while (queue.size > keep) queue.removeAt(queue.lastIndex)
        while (queue.size > HISTORY) queue.removeAt(0)
        val at = queue.size + start.coerceIn(0, list.size - 1)
        queue += list
        added = 0
        play(at)
    }

    fun append(list: List<Track>) {
        queue += list.filter { t -> queue.none { it.videoId == t.videoId } }
        // The queue had run out waiting for these: go on.
        if (endedFor == playCount && index + 1 < queue.size) play(index + 1)
    }

    /** Right after the playing song (it joins the songs waiting there, before them). */
    fun playNext(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else {
            queue.add(index + 1, t)
            added++
        }
    }

    /** Right after the playing song and the songs added before it, not at the end of the queue. */
    fun addToQueue(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else {
            queue.add((index + 1 + added).coerceAtMost(queue.size), t)
            added++
        }
    }

    /** Moves row [from] to [to]; the playing song stays the playing one. */
    fun move(from: Int, to: Int) {
        if (from !in queue.indices || to !in queue.indices || from == to) return
        val t = queue.removeAt(from)
        queue.add(to, t)
        index = when {
            index == from -> to
            from < index && to >= index -> index - 1
            from > index && to <= index -> index + 1
            else -> index
        }
        added = 0
    }

    /** When a song was asked for, for the timing log. */
    private var tapped = 0L

    fun play(i: Int) {
        val t = queue.getOrNull(i) ?: return
        // A mix still loading its song gives way to the one picked here.
        QueueBridge.cancelMix?.invoke()
        // The song that was playing gets its listening time before the new one starts.
        countListening()
        tapped = System.currentTimeMillis()
        android.util.Log.w("RyM", "timing: play ${t.title} (${t.videoId})")
        // Moving on to the next song uses up one added song; jumping elsewhere forgets them.
        added = if (i == index + 1) (added - 1).coerceAtLeast(0) else 0
        // Another song than the one left off last time: that one's place no longer counts.
        if (resumeAt?.first != t.videoId) resumeAt = null
        index = i
        playCount++
        played += t.videoId
        com.rymplayer.data.Recent.played(context, t)
        position = 0f
        // The player still holds the last song's place: the new one starts at 0.
        saveSession(0)
        controller?.stop()
        onTrackChanged(t)
        loading = true
        loadJob?.cancel()
        loadJob = scope.launch {
            // Play songs in VLC (Library setting), when it's installed; the built-in player otherwise.
            if (useVlc && Vlc.installed(context) && com.rymplayer.data.isYouTube(t.videoId)) {
                val sent = runCatching { Vlc.open(context, t) }
                    .onFailure { if (it !is kotlinx.coroutines.CancellationException) message = "VLC couldn't open it: ${it.message}" }
                loading = false
                if (sent.isSuccess) return@launch
            }
            // A downloaded song plays from Music/RymPlayer; others stream (or play from the cache).
            val url = Downloads.uriFor(context, t.videoId)?.toString()
                ?: runCatching { Streams.playable(context, t.videoId) }
                    .onFailure { if (it !is kotlinx.coroutines.CancellationException) message = "${com.rymplayer.tr("Can't play")}: ${it.message}" }
                    .getOrNull()
            if (url == null) {
                loading = false
                return@launch
            }
            val c = runCatching { ready.await() }.getOrElse {
                loading = false
                message = "${com.rymplayer.tr("Can't start the player")}: ${it.message}"
                return@launch
            }
            if (current?.videoId != t.videoId) return@launch
            val item = mediaItem(t, url)
            // The song left off last time starts where it was (given with the song: a seek sent
            // right after loading it was lost).
            val from = resumeAt?.takeIf { it.first == t.videoId && it.second > 1000 }?.second
            resumeAt = null
            if (from != null) c.setMediaItem(item, from) else c.setMediaItem(item)
            c.prepare()
            c.play()
            // In the account's history, so the PC shows it too.
            reportPlay(t)
            // The next song's link is looked up while this one plays.
            Streams.prefetchLinks(queue.drop(i + 1).take(2).map { it.videoId })
            // Last song of the queue: a radio of similar songs follows it.
            keepGoing(t)
        }
    }

    /** The player's item for [t]: its link, and title, artist and cover for the notification. */
    private fun mediaItem(t: Track, url: String): MediaItem {
        val meta = MediaMetadata.Builder()
            .setTitle(t.title)
            .setArtist(t.artist)
            .apply { thumbSized(t.thumb, 544)?.let { setArtworkUri(Uri.parse(it)) } }
            .build()
        // SoundCloud's songs come in pieces (HLS).
        val hls = ".m3u8" in url
        return MediaItem.Builder().setMediaId(t.videoId).setUri(url).setMediaMetadata(meta).apply { if (hls) setMimeType(androidx.media3.common.MimeTypes.APPLICATION_M3U8) }.build()
    }

    /**
     * The automix moves on to the next song: its link first (nothing changes when there isn't
     * one), then the queue, the stats and the lyrics, like [play]; the service starts the song.
     */
    private suspend fun advanceForMix(): MediaItem? {
        val i = index + 1
        val t = queue.getOrNull(i) ?: return null
        if (useVlc && Vlc.installed(context)) return null
        val url = Downloads.uriFor(context, t.videoId)?.toString()
            ?: runCatching { Streams.playable(context, t.videoId) }.getOrNull()
            ?: return null
        if (index + 1 != i || queue.getOrNull(i)?.videoId != t.videoId) return null
        countListening()
        android.util.Log.w("RyM", "timing: mix into ${t.title} (${t.videoId})")
        added = (added - 1).coerceAtLeast(0)
        resumeAt = null
        index = i
        playCount++
        played += t.videoId
        com.rymplayer.data.Recent.played(context, t)
        position = 0f
        saveSession(0)
        onTrackChanged(t)
        Streams.prefetchLinks(queue.drop(i + 1).take(2).map { it.videoId })
        keepGoing(t)
        reportPlay(t)
        return mediaItem(t, url)
    }

    private fun reportPlay(t: Track) {
        if (!::api.isInitialized || !com.rymplayer.data.isYouTube(t.videoId)) return
        scope.launch {
            runCatching { api.reportPlay(t.videoId) }.onFailure { android.util.Log.w("RyM", "history: not reported (${it.message})") }
        }
    }

    fun pause() {
        controller?.pause()
    }

    fun toggle() {
        val c = controller ?: return
        when {
            c.isPlaying -> c.pause()
            c.playbackState != ExoListener.STATE_IDLE && c.mediaItemCount > 0 -> c.play()
            else -> play(index)
        }
    }

    fun next() {
        if (index + 1 < queue.size) play(index + 1)
    }

    /** When Previous was last pressed: twice in a row goes back even mid-song. */
    private var lastPrev = 0L

    /**
     * Previous: back to the start of the song after its first 3 seconds; before that (or pressed
     * twice in a row), the song before it. Reads the player's own position: the shown one only
     * moves while the screen is on.
     */
    fun prev() {
        val now = System.currentTimeMillis()
        val again = now - lastPrev < 1500
        lastPrev = now
        val pos = (controller?.currentPosition ?: 0L) / 1000f
        if (index > 0 && (pos < 3f || again)) play(index - 1) else seekTo(0f)
    }

    /** Songs like the one playing, added when the queue reaches its last song (the radio). */
    private fun keepGoing(t: Track) {
        if (index != queue.lastIndex || !::api.isInitialized) return
        scope.launch {
            runCatching { com.rymplayer.data.Radio.forTrack(api, t) }.getOrNull()?.let { radio -> append(tidy(radio, t)) }
        }
    }

    /** The radio for [seed] without repeats or songs played or queued already, same artist moved up. */
    fun tidy(radio: List<Track>, seed: Track): List<Track> {
        val heard = queue.take(index + 1).map { com.rymplayer.data.Radio.firstArtist(it.artist) }.filter { it.isNotEmpty() }.toSet()
        val queued = queue.map { it.videoId }.toSet()
        val similar = com.rymplayer.data.Radio.tidy(radio, seed, played, queued, heard)
        // Songs like it, with liked songs mixed in (same artist first).
        return com.rymplayer.data.Radio.withLiked(similar, seed, likedSongs(), played + queued)
    }

    /** The liked songs, mixed into Up next (set by the app). */
    var likedSongs: () -> List<Track> = { emptyList() }

    fun seekTo(secs: Float) {
        controller?.seekTo((secs * 1000).toLong())
        position = secs
        seeks++
        linkSnapshot((secs * 1000).toLong())
    }

    fun remove(i: Int) {
        if (i !in queue.indices || i == index) return
        queue.removeAt(i)
        if (i < index) index--
        // One of the added songs waiting after the playing one.
        if (i > index && i <= index + added) added--
    }

    fun stop() {
        controller?.stop()
        controller?.clearMediaItems()
        queue.clear()
        index = -1
        onTrackChanged(null)
    }

    /** Lets go of the service; music keeps playing if it was. */
    fun release() {
        saveSession()
        QueueBridge.next = null
        QueueBridge.prev = null
        QueueBridge.hasNext = null
        QueueBridge.mixNext = null
        Link.pause = null
        loadJob?.cancel()
        ticker?.cancel()
        MediaController.releaseFuture(future)
    }
}
