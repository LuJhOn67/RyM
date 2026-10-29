package com.rymplayer.playback

import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
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

    @OptIn(UnstableApi::class)
    override fun onCreate() {
        super.onCreate()
        // Web links stream in 4 MB ranges (see RangedHttpDataSource); files play directly.
        val exo = ExoPlayer.Builder(this)
            .setMediaSourceFactory(DefaultMediaSourceFactory(DefaultDataSource.Factory(this, RangedHttpDataSource.Factory())))
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            .setHandleAudioBecomingNoisy(true)
            .build()
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

    /** Swiping the app away while paused stops the service; while playing, music continues. */
    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }
}
