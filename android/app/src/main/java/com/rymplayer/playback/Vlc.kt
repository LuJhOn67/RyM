package com.rymplayer.playback

import android.content.Context
import android.content.Intent
import android.net.Uri
import com.rymplayer.data.Downloads
import com.rymplayer.data.Track
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * VLC for Android as an outside player: a song's stream link (or its downloaded file) is handed
 * to it. Used from the song menu, or for every song when Library > Play songs in is set to VLC.
 */
object Vlc {
    const val PACKAGE = "org.videolan.vlc"

    fun installed(ctx: Context): Boolean =
        runCatching { ctx.packageManager.getPackageInfo(PACKAGE, 0) }.isSuccess

    /** Opens the song in VLC. Throws when there's no link or VLC can't take it. */
    suspend fun open(ctx: Context, t: Track) {
        val uri = Downloads.uriFor(ctx, t.videoId) ?: Uri.parse(Streams.streamLink(t.videoId))
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "audio/*")
            .setPackage(PACKAGE)
            .putExtra("title", t.title)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        withContext(Dispatchers.Main) { ctx.startActivity(intent) }
    }
}
