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
import androidx.media3.common.Player as ExoListener
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.rymplayer.data.Track
import com.rymplayer.data.YtMusic
import com.rymplayer.data.parseDuration
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** The queue and UI state; actual audio lives in PlaybackService for background playback. */
class Player(context: Context, private val scope: CoroutineScope) {
    lateinit var api: YtMusic
    var onTrackChanged: (Track?) -> Unit = {}
    var onPlaybackRequested: () -> Unit = {}

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

    private val controllerReady = CompletableDeferred<MediaController>()
    private val controllerFuture = MediaController.Builder(
        context.applicationContext,
        SessionToken(context.applicationContext, ComponentName(context, PlaybackService::class.java)),
    ).buildAsync()
    @Volatile private var controller: MediaController? = null
    private var ticker: Job? = null
    private var loadJob: Job? = null

    init {
        controllerFuture.addListener({
            runCatching { controllerFuture.get() }
                .onSuccess { c ->
                    controller = c
                    c.addListener(object : ExoListener.Listener {
                        override fun onIsPlayingChanged(isPlaying: Boolean) {
                            playing = isPlaying
                            if (isPlaying) startTicker()
                        }

                        override fun onPlaybackStateChanged(state: Int) {
                            when (state) {
                                ExoListener.STATE_BUFFERING -> loading = true
                                ExoListener.STATE_READY, ExoListener.STATE_ENDED -> loading = false
                            }
                            if (state == ExoListener.STATE_ENDED) next()
                        }

                        override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                            loading = false
                            playing = false
                            message = "Playback stopped: ${error.message.orEmpty()}"
                        }
                    })
                    controllerReady.complete(c)
                }
                .onFailure { controllerReady.completeExceptionally(it) }
        }, ContextCompat.getMainExecutor(context))
    }

    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && controller?.isPlaying == true) {
                position = (controller?.currentPosition ?: 0L) / 1000f
                delay(120)
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
        prewarmNext(index)
    }

    fun playNext(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else {
            queue.add(index + 1, t)
            prewarmNext(index)
        }
    }

    private fun prewarmNext(after: Int) {
        queue.getOrNull(after + 1)?.takeIf { it.localUri == null }?.let { Streams.warm(it.videoId) }
    }

    fun play(i: Int) {
        val t = queue.getOrNull(i) ?: return
        loadJob?.cancel()
        index = i
        position = 0f
        playing = false
        loading = true
        message = ""
        onTrackChanged(t)
        onPlaybackRequested()
        controller?.stop()
        loadJob = scope.launch {
            try {
                val uri = t.localUri ?: Streams.resolve(t.videoId)
                if (uri.isNullOrBlank()) {
                    loading = false
                    message = "No playable audio for this song"
                    return@launch
                }
                val c = controllerReady.await()
                if (current?.videoId != t.videoId) return@launch
                val metadata = MediaMetadata.Builder()
                    .setTitle(t.title)
                    .setArtist(t.artist)
                    .apply { t.thumb?.let { setArtworkUri(Uri.parse(it)) } }
                    .build()
                val item = MediaItem.Builder()
                    .setMediaId(t.videoId)
                    .setUri(uri)
                    .setMediaMetadata(metadata)
                    .build()
                c.setMediaItem(item)
                c.prepare()
                c.play()
                prewarmNext(i)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                loading = false
                message = "Can't play: ${e.message.orEmpty()}"
            }
        }
    }

    fun toggle() {
        val c = controller
        when {
            c?.isPlaying == true -> c.pause()
            c?.playbackState == ExoListener.STATE_READY || c?.playbackState == ExoListener.STATE_BUFFERING -> c.play()
            loading -> Unit
            else -> play(index)
        }
    }

    fun next() {
        if (index + 1 < queue.size) play(index + 1)
    }

    fun prev() {
        if (position > 3f || index == 0) seekTo(0f) else if (index > 0) play(index - 1)
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
        loadJob?.cancel()
        controller?.stop()
        controller?.clearMediaItems()
        queue.clear()
        index = -1
        position = 0f
        playing = false
        loading = false
        onTrackChanged(null)
    }

    /** Unbind the UI controller while letting the media service keep playing in the background. */
    fun release() {
        loadJob?.cancel()
        ticker?.cancel()
        MediaController.releaseFuture(controllerFuture)
    }
}
