package com.rymplayer.data

import android.Manifest
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import androidx.core.content.ContextCompat

/**
 * Songs on this phone (any app's or copied ones, not only RyM's downloads), from Android's music
 * library. Each is a [Track] whose id is `local:<its number in the library>`; it plays from its
 * file, and the YouTube-only things (radio, likes, SponsorBlock) leave it alone.
 */
object LocalSongs {
    const val PREFIX = "local:"

    fun isLocal(id: String) = id.startsWith(PREFIX)

    /** The song's file in Android's music library. */
    fun uri(id: String): Uri = ContentUris.withAppendedId(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, id.removePrefix(PREFIX).toLongOrNull() ?: -1L)

    /** "Music and audio" (Android 13+) or storage before it: asked the first time the page opens. */
    val permission: String = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE

    fun allowed(ctx: Context) = ContextCompat.checkSelfPermission(ctx, permission) == PackageManager.PERMISSION_GRANTED

    /** Every song on the phone (music, 30 seconds or longer), by title. */
    fun list(ctx: Context): List<Track> {
        if (!allowed(ctx)) return emptyList()
        val cols = arrayOf(
            MediaStore.Audio.Media._ID, MediaStore.Audio.Media.TITLE, MediaStore.Audio.Media.ARTIST,
            MediaStore.Audio.Media.ALBUM, MediaStore.Audio.Media.DURATION, MediaStore.Audio.Media.ALBUM_ID,
        )
        val where = "${MediaStore.Audio.Media.IS_MUSIC} != 0 AND ${MediaStore.Audio.Media.DURATION} >= 30000"
        val out = mutableListOf<Track>()
        ctx.contentResolver.query(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, cols, where, null, "${MediaStore.Audio.Media.TITLE} COLLATE NOCASE")?.use { c ->
            while (c.moveToNext()) {
                val id = c.getLong(0)
                fun text(i: Int) = c.getString(i)?.takeIf { it.isNotBlank() && it != MediaStore.UNKNOWN_STRING }.orEmpty()
                val secs = c.getLong(4) / 1000
                out += Track(
                    "$PREFIX$id", text(1), text(2), text(3), "%d:%02d".format(secs / 60, secs % 60),
                    "content://media/external/audio/albumart/${c.getLong(5)}",
                )
            }
        }
        return out
    }
}
