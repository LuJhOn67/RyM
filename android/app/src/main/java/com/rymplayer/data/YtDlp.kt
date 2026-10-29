package com.rymplayer.data

import android.content.Context
import com.yausername.youtubedl_android.YoutubeDL
import com.yausername.youtubedl_android.YoutubeDLRequest
import java.io.File

/**
 * Downloads through yt-dlp (bundled with Python and QuickJS by youtubedl-android), with the
 * signed-in account: Premium gets YouTube Music's 256 kbps Opus (format 774), others the best
 * they can (251, then AAC 141/140). yt-dlp updates itself once a day, since YouTube changes often.
 */
object YtDlp {
    @Volatile private var ready = false
    @Volatile private var updatedAt = 0L

    private fun prepare(ctx: Context) {
        if (!ready) synchronized(this) {
            if (!ready) {
                YoutubeDL.getInstance().init(ctx)
                ready = true
            }
        }
        if (System.currentTimeMillis() - updatedAt > 24 * 60 * 60 * 1000L) {
            runCatching { YoutubeDL.getInstance().updateYoutubeDL(ctx, YoutubeDL.UpdateChannel.STABLE) }
                .onFailure { android.util.Log.w("RyM", "yt-dlp update failed", it) }
            updatedAt = System.currentTimeMillis()
        }
    }

    /** Downloads a song into the app's files and returns the file (webm or m4a). Blocking. */
    fun download(ctx: Context, videoId: String, cookie: String?): File {
        prepare(ctx)
        val dir = File(ctx.cacheDir, "ytdlp").apply { mkdirs() }
        dir.listFiles()?.filter { it.name.startsWith("$videoId.") }?.forEach { it.delete() }
        val request = YoutubeDLRequest("https://music.youtube.com/watch?v=$videoId").apply {
            addOption("-f", "774/251/141/140/bestaudio")
            addOption("--no-playlist")
            addOption("--no-part")
            addOption("-o", File(dir, "%(id)s.%(ext)s").absolutePath)
            if (!cookie.isNullOrBlank()) addOption("--cookies", cookieFile(ctx, cookie).absolutePath)
        }
        YoutubeDL.getInstance().execute(request, "rym-$videoId")
        return dir.listFiles()?.firstOrNull { it.name.startsWith("$videoId.") && it.length() > 0 }
            ?: error("yt-dlp finished but the file is missing")
    }

    /** yt-dlp wants a Netscape cookie file; this converts the `Cookie:` header the app signs in with. */
    private fun cookieFile(ctx: Context, header: String): File {
        val text = buildString {
            append("# Netscape HTTP Cookie File\n")
            for (pair in header.split(';')) {
                val i = pair.indexOf('=').takeIf { it > 0 } ?: continue
                append(".youtube.com\tTRUE\t/\tTRUE\t2147483647\t${pair.substring(0, i).trim()}\t${pair.substring(i + 1).trim()}\n")
            }
        }
        return File(ctx.filesDir, "cookies-ytdlp.txt").apply { writeText(text) }
    }
}
