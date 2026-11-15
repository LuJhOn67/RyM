package com.rymplayer.playback

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.view.KeyEvent

/**
 * RyM in Quick Settings (pull the status bar down twice, then edit to add it): the playing song,
 * and a tap plays or pauses without opening the app.
 */
class RymTile : TileService() {
    override fun onStartListening() = draw()

    override fun onClick() {
        val intent = Intent(Intent.ACTION_MEDIA_BUTTON)
            .setComponent(ComponentName(this, PlaybackService::class.java))
            .putExtra(Intent.EXTRA_KEY_EVENT, KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE))
        val sent = runCatching { androidx.core.content.ContextCompat.startForegroundService(this, intent) }.isSuccess
        if (!sent || title.isEmpty()) {
            // Nothing to play yet (or Android said no from the background): RyM opens.
            openApp()
            return
        }
        playing = !playing
        draw()
    }

    private fun openApp() {
        val open = Intent(this, com.rymplayer.MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE))
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(open)
        }
    }

    private fun draw() {
        val tile = qsTile ?: return
        tile.state = if (playing) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = title.ifEmpty { "RyM" }
        if (Build.VERSION.SDK_INT >= 29) tile.subtitle = if (title.isEmpty()) "RyM" else artist
        tile.updateTile()
    }

    companion object {
        @Volatile private var title = ""
        @Volatile private var artist = ""
        @Volatile private var playing = false

        /** The song or play / pause changed (from the playback service, as the widget's). */
        fun show(ctx: Context, title: String?, artist: String?, playing: Boolean) {
            this.title = title.orEmpty()
            this.artist = artist.orEmpty()
            this.playing = playing
            runCatching { requestListeningState(ctx, ComponentName(ctx, RymTile::class.java)) }
        }
    }
}
