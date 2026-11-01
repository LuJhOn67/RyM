package com.rymplayer.playback

import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * The queue lives in the app's [com.rymplayer.playback.Player], which hands the media player one
 * song at a time. The notification's and lock screen's next/previous buttons ask it through here.
 */
object QueueBridge {
    @Volatile var next: (() -> Unit)? = null
    @Volatile var prev: (() -> Unit)? = null
}

/** Plays in the background and shows Android's media notification (with lock-screen controls). */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private var exo: ExoPlayer? = null
    private var effects: SoundEffects? = null
    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
    /** Sound settings changed in the app: applied at once. */
    private val soundChanged = android.content.SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> applySound() }

    @OptIn(UnstableApi::class)
    override fun onCreate() {
        super.onCreate()
        // Web links stream in 4 MB ranges (see RangedHttpDataSource); files play directly.
        // Start after half a second of audio instead of the default 2.5 s; songs stream fast enough for it.
        val load = DefaultLoadControl.Builder().setBufferDurationsMs(15_000, 50_000, 500, 2_000).build()
        val exo = ExoPlayer.Builder(this)
            .setLoadControl(load)
            .setMediaSourceFactory(DefaultMediaSourceFactory(DefaultDataSource.Factory(this, WebDataSource.Factory())))
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            .setHandleAudioBecomingNoisy(true)
            // Keeps Wi-Fi awake only while streaming, and the CPU only while playing.
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        this.exo = exo
        effects = SoundEffects(exo)
        exo.addListener(object : Player.Listener {
            override fun onAudioSessionIdChanged(audioSessionId: Int) = applySound()
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                handler.removeCallbacks(fades)
                if (isPlaying) handler.post(fades)
            }
        })
        Sound.listen(this, soundChanged)
        applySound()
        val skips = listOf(
            Player.COMMAND_SEEK_TO_NEXT,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        )
        val withQueue = object : ForwardingPlayer(exo) {
            override fun getAvailableCommands(): Player.Commands = super.getAvailableCommands().buildUpon().addAll(*skips.toIntArray()).build()
            override fun isCommandAvailable(command: Int) = command in skips || super.isCommandAvailable(command)
            override fun seekToNext() { QueueBridge.next?.invoke() }
            override fun seekToNextMediaItem() { QueueBridge.next?.invoke() }
            override fun seekToPrevious() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun seekToPreviousMediaItem() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
        }
        session = MediaSession.Builder(this, withQueue).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    private fun applySound() {
        val e = effects ?: return
        e.apply(Sound.load(this))
        if (exo?.isPlaying == true) {
            handler.removeCallbacks(fades)
            handler.post(fades)
        }
    }

    /**
     * Crossfade (the end of a song fades out, the next fades in) and the sleep timer (the last 10
     * seconds fade, then the music pauses). Runs only while playing, a few times a second when a
     * fade is near, otherwise it sleeps until one could start.
     */
    private val fades: Runnable = object : Runnable {
        override fun run() {
            val p = exo ?: return
            if (!p.isPlaying) return
            val s = Sound.load(this@PlaybackService)
            val pos = p.currentPosition
            val dur = p.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: 0L
            val speed = s.speed.coerceAtLeast(0.1f)
            val fadeMs = s.crossfade * 1000L
            var volume = 1f
            var wait = 5_000L
            if (fadeMs > 0 && dur > fadeMs * 3) {
                val fromEnd = ((dur - pos) / speed).toLong()
                val fromStart = (pos / speed).toLong()
                if (fromStart < fadeMs) volume = minOf(volume, fromStart.toFloat() / fadeMs)
                if (fromEnd < fadeMs) volume = minOf(volume, fromEnd.toFloat() / fadeMs)
                wait = if (fromStart < fadeMs || fromEnd < fadeMs) 100L else minOf(wait, fromEnd - fadeMs)
            }
            if (s.sleepAt != 0L) {
                val left = if (s.sleepAt > 0) s.sleepAt - System.currentTimeMillis() else if (dur > 0) ((dur - pos) / speed).toLong() else Long.MAX_VALUE
                if (left <= 250) {
                    p.pause()
                    p.volume = 1f
                    Sound.save(this@PlaybackService, s.copy(sleepAt = 0))
                    return
                }
                if (left < 10_000) {
                    volume = minOf(volume, left / 10_000f)
                    wait = 100L
                } else wait = minOf(wait, left - 10_000)
            }
            p.volume = volume.coerceIn(0f, 1f)
            handler.postDelayed(this, wait.coerceIn(50L, 5_000L))
        }
    }

    /** Swiping the app away while paused stops the service; while playing, music continues. */
    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        handler.removeCallbacks(fades)
        Sound.unlisten(this, soundChanged)
        effects?.release()
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }
}
