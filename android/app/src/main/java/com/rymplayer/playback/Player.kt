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
        future.addListener({
            val c = runCatching { future.get() }.getOrElse {
                ready.completeExceptionally(it)
                return@addListener
            }
            controller = c
            c.addListener(object : ExoListener.Listener {
                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    playing = isPlaying
                    if (isPlaying) startTicker()
                }

                override fun onPlaybackStateChanged(state: Int) {
                    if (state == ExoListener.STATE_READY && tapped > 0) {
                        android.util.Log.w("RyM", "timing: ${current?.title} started ${System.currentTimeMillis() - tapped} ms after the tap")
                        tapped = 0
                    }
                    loading = state == ExoListener.STATE_BUFFERING
                    if (state == ExoListener.STATE_ENDED) next()
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

    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && controller?.isPlaying == true) {
                position = (controller?.currentPosition ?: 0L) / 1000f
                delay(100)
            }
            position = (controller?.currentPosition ?: 0L) / 1000f
        }
    }

    fun setQueue(list: List<Track>, start: Int) {
        queue.clear()
        queue += list
        play(start)
    }

    fun append(list: List<Track>) {
        queue += list.filter { t -> queue.none { it.videoId == t.videoId } }
    }

    fun playNext(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else queue.add(index + 1, t)
    }

    /** When a song was asked for, for the timing log. */
    private var tapped = 0L

    fun play(i: Int) {
        val t = queue.getOrNull(i) ?: return
        tapped = System.currentTimeMillis()
        android.util.Log.w("RyM", "timing: play ${t.title} (${t.videoId})")
        index = i
        position = 0f
        controller?.stop()
        onTrackChanged(t)
        loading = true
        loadJob?.cancel()
        loadJob = scope.launch {
            // Play songs in VLC (Library setting), when it's installed; the built-in player otherwise.
            if (useVlc && Vlc.installed(context)) {
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
            // Title, artist and cover for the notification and the lock screen.
            val meta = MediaMetadata.Builder()
                .setTitle(t.title)
                .setArtist(t.artist)
                .apply { thumbSized(t.thumb, 544)?.let { setArtworkUri(Uri.parse(it)) } }
                .build()
            c.setMediaItem(MediaItem.Builder().setMediaId(t.videoId).setUri(url).setMediaMetadata(meta).build())
            c.prepare()
            c.play()
            // The next song's link is looked up while this one plays.
            Streams.prefetchLinks(queue.drop(i + 1).take(2).map { it.videoId })
            // Last song of the queue: a radio of similar songs follows it.
            keepGoing(t)
        }
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
            runCatching { api.radio(t.videoId) }.getOrNull()?.let { radio -> append(radio.filter { it.videoId != t.videoId }) }
        }
    }

    fun seekTo(secs: Float) {
        controller?.seekTo((secs * 1000).toLong())
        position = secs
    }

    fun remove(i: Int) {
        if (i !in queue.indices || i == index) return
        queue.removeAt(i)
        if (i < index) index--
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
        QueueBridge.next = null
        QueueBridge.prev = null
        loadJob?.cancel()
        ticker?.cancel()
        MediaController.releaseFuture(future)
    }
}
