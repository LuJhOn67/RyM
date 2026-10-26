package com.rymplayer.playback

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player as ExoListener
import androidx.media3.exoplayer.ExoPlayer
import com.rymplayer.data.Track
import com.rymplayer.data.YtMusic
import com.rymplayer.data.parseDuration
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** Finds a playable audio URL for a song. Not available yet in this build. */
object Streams {
    suspend fun resolve(@Suppress("UNUSED_PARAMETER") videoId: String): String? = null
}

/** The queue and the audio player, as Compose state. */
class Player(context: Context, private val scope: CoroutineScope) {
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
        get() = exo.duration.takeIf { it != C.TIME_UNSET && it > 0 }?.let { it / 1000f }
            ?: current?.let { parseDuration(it.duration) } ?: 0f

    private val exo = ExoPlayer.Builder(context)
        .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
        .setHandleAudioBecomingNoisy(true)
        .build()
    private var ticker: Job? = null
    private var loadJob: Job? = null

    init {
        exo.addListener(object : ExoListener.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                playing = isPlaying
                if (isPlaying) startTicker()
            }

            override fun onPlaybackStateChanged(state: Int) {
                if (state == ExoListener.STATE_ENDED) next()
            }
        })
    }

    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && exo.isPlaying) {
                position = exo.currentPosition / 1000f
                delay(100)
            }
            position = exo.currentPosition / 1000f
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
        exo.stop()
        onTrackChanged(t)
        loading = true
        loadJob?.cancel()
        loadJob = scope.launch {
            val url = runCatching { Streams.resolve(t.videoId) }.getOrNull()
            loading = false
            if (url == null) {
                message = "Playback isn't in this test build yet"
                return@launch
            }
            exo.setMediaItem(MediaItem.fromUri(url))
            exo.prepare()
            exo.play()
        }
    }

    fun toggle() = if (exo.isPlaying) exo.pause() else if (exo.playbackState != ExoListener.STATE_IDLE) exo.play() else play(index)

    fun next() {
        if (index + 1 < queue.size) play(index + 1)
    }

    fun prev() {
        if (position > 3f || index == 0) seekTo(0f) else play(index - 1)
    }

    fun seekTo(secs: Float) {
        exo.seekTo((secs * 1000).toLong())
        position = secs
    }

    fun remove(i: Int) {
        if (i !in queue.indices || i == index) return
        queue.removeAt(i)
        if (i < index) index--
    }

    fun stop() {
        exo.stop()
        queue.clear()
        index = -1
        onTrackChanged(null)
    }

    fun release() = exo.release()
}
