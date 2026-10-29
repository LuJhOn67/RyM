package com.rymplayer.playback

import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
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

/** Keeps playback alive and publishes Android's standard media notification. */
class PlaybackService : MediaSessionService() {
    private var mediaSession: MediaSession? = null

    @UnstableApi
    override fun onCreate() {
        super.onCreate()
        val loadControl = DefaultLoadControl.Builder()
            // Start audio sooner on normal connections while retaining a healthy stall buffer.
            .setBufferDurationsMs(10_000, 50_000, 500, 1_000)
            .setPrioritizeTimeOverSizeThresholds(true)
            .build()
        val player = ExoPlayer.Builder(this)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                    .build(),
                true,
            )
            .setHandleAudioBecomingNoisy(true)
            .setLoadControl(loadControl)
            .build()
        val skips = listOf(
            Player.COMMAND_SEEK_TO_NEXT,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        )
        val withQueue = object : ForwardingPlayer(player) {
            override fun getAvailableCommands(): Player.Commands =
                super.getAvailableCommands().buildUpon().addAll(*skips.toIntArray()).build()

            override fun isCommandAvailable(command: Int) = command in skips || super.isCommandAvailable(command)
            override fun seekToNext() { QueueBridge.next?.invoke() }
            override fun seekToNextMediaItem() { QueueBridge.next?.invoke() }
            override fun seekToPrevious() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun seekToPreviousMediaItem() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
        }
        mediaSession = MediaSession.Builder(this, withQueue).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = mediaSession

    override fun onDestroy() {
        mediaSession?.run {
            player.release()
            release()
        }
        mediaSession = null
        super.onDestroy()
    }
}
