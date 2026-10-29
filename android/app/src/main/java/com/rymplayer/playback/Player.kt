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
import androidx.media3.datasource.HttpDataSource
import androidx.media3.common.Player as ExoListener
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.rymplayer.data.Downloads
import com.rymplayer.data.Track
import com.rymplayer.data.YtMusic
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
    lateinit var api: YtMusic
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
                    loading = state == ExoListener.STATE_BUFFERING
                    if (state == ExoListener.STATE_ENDED) next()
                }

                override fun onPlayerError(error: PlaybackException) {
                    // The link didn't work after all: ask the next client before giving up.
                    val t = current
                    if (t != null && Streams.markBad(t.videoId)) {
                        play(index)
                        return
                    }
                    loading = false
                    val http = (error.cause as? HttpDataSource.InvalidResponseCodeException)?.responseCode
                    message = "Can't play: ${error.errorCodeName}" + (http?.let { " (HTTP $it)" } ?: "")
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

    fun play(i: Int) {
        val t = queue.getOrNull(i) ?: return
        index = i
        position = 0f
        controller?.stop()
        onTrackChanged(t)
        loading = true
        loadJob?.cancel()
        loadJob = scope.launch {
            // A downloaded song plays from its file.
            val url = Downloads.uriFor(context, t.videoId)?.toString()
                ?: runCatching { Streams.resolve(t.videoId) }.onFailure { message = "Can't play: ${it.message}" }.getOrNull()
            if (url == null) {
                loading = false
                if (message.isEmpty()) message = "No playable audio for this song"
                return@launch
            }
            val c = runCatching { ready.await() }.getOrElse {
                loading = false
                message = "Can't start the player: ${it.message}"
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

    fun prev() {
        if (position > 3f || index == 0) seekTo(0f) else play(index - 1)
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
