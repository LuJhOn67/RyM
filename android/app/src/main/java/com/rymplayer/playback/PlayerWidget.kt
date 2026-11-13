package com.rymplayer.playback

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.view.KeyEvent
import android.widget.RemoteViews
import com.rymplayer.R

/**
 * RyM's home-screen widget (backlog 30): the playing song's cover, name and artist, with
 * previous, play / pause and next. The buttons go to the player's service as media buttons (it
 * starts if it isn't running); a tap on the song opens RyM's player. The service tells the
 * widget when the song or play / pause changes ([show]).
 */
class PlayerWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        draw(context, manager, ids)
    }

    companion object {
        private var title = "RyM"
        private var artist = ""
        private var playing = false
        private var cover: Bitmap? = null

        /** The song or play / pause changed: every RyM widget shows it. */
        fun show(ctx: Context, title: String?, artist: String?, playing: Boolean, cover: Bitmap?) {
            this.title = title?.takeIf { it.isNotBlank() } ?: "RyM"
            this.artist = artist.orEmpty()
            this.playing = playing
            this.cover = cover
            val manager = AppWidgetManager.getInstance(ctx)
            val ids = manager.getAppWidgetIds(ComponentName(ctx, PlayerWidget::class.java))
            if (ids.isNotEmpty()) draw(ctx, manager, ids)
        }

        internal fun button(ctx: Context, key: Int): PendingIntent {
            val intent = Intent(Intent.ACTION_MEDIA_BUTTON)
                .setComponent(ComponentName(ctx, PlaybackService::class.java))
                .putExtra(Intent.EXTRA_KEY_EVENT, KeyEvent(KeyEvent.ACTION_DOWN, key))
            return PendingIntent.getForegroundService(ctx, key, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }

        private fun draw(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
            val v = RemoteViews(ctx.packageName, R.layout.widget_player)
            v.setTextViewText(R.id.widget_title, title)
            v.setTextViewText(R.id.widget_artist, artist)
            cover?.let { v.setImageViewBitmap(R.id.widget_cover, it) } ?: v.setImageViewResource(R.id.widget_cover, R.mipmap.ic_launcher)
            v.setImageViewResource(R.id.widget_play, if (playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play)
            v.setOnClickPendingIntent(R.id.widget_prev, button(ctx, KeyEvent.KEYCODE_MEDIA_PREVIOUS))
            v.setOnClickPendingIntent(R.id.widget_play, button(ctx, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE))
            v.setOnClickPendingIntent(R.id.widget_next, button(ctx, KeyEvent.KEYCODE_MEDIA_NEXT))
            val open = PendingIntent.getActivity(
                ctx, 1,
                Intent(ctx, com.rymplayer.MainActivity::class.java).putExtra(com.rymplayer.MainActivity.OPEN_PLAYER, true).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            v.setOnClickPendingIntent(R.id.widget_cover, open)
            v.setOnClickPendingIntent(R.id.widget_text, open)
            manager.updateAppWidget(ids, v)
        }
    }
}
