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

    /**
     * Downloads a song into the app's files and returns the file (webm or m4a). Blocking. yt-dlp
     * finds the link; the song comes over the app's parallel connections, or through yt-dlp
     * itself when that fails.
     */
    fun download(ctx: Context, videoId: String, cookie: String?): File {
        prepare(ctx)
        val dir = File(ctx.cacheDir, "ytdlp").apply { mkdirs() }
        dir.listFiles()?.filter { it.name.startsWith("$videoId.") }?.forEach { it.delete() }
        runCatching {
            val info = YoutubeDL.getInstance().getInfo(YoutubeDLRequest("https://music.youtube.com/watch?v=$videoId").apply {
                addOption("-f", "774/251/141/140/bestaudio")
                addOption("--no-playlist")
                if (!cookie.isNullOrBlank()) addOption("--cookies", cookieFile(ctx, cookie).absolutePath)
            })
            val url = checkNotNull(info.url) { "yt-dlp gave no link" }
            val file = File(dir, "$videoId.${info.ext ?: "webm"}")
            com.rymplayer.playback.Streams.save(url, info.httpHeaders ?: emptyMap(), file)
            return file
        }.onFailure { android.util.Log.w("RyM", "parallel download through yt-dlp's link failed, letting yt-dlp download", it) }
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

    /**
     * The video plugin's universal downloader: a link from YouTube or any site yt-dlp knows, as a
     * video (picture and sound in one file, best up to 1080p) or just the sound. Blocking; returns
     * the file in the app's cache, ready to be published.
     */
    fun downloadAny(ctx: Context, url: String, video: Boolean, cookie: String?): File {
        prepare(ctx)
        val dir = File(ctx.cacheDir, "ytdlp-any").apply { deleteRecursively(); mkdirs() }
        val request = YoutubeDLRequest(url.trim()).apply {
            // One file without joining (the app has no ffmpeg): the best already-joined video, or the sound.
            addOption("-f", if (video) "b[height<=1080][ext=mp4]/b[height<=1080]/b" else "ba[ext=m4a]/ba/b")
            addOption("--no-playlist")
            addOption("--no-part")
            addOption("--restrict-filenames")
            addOption("-o", File(dir, "%(title).100B [%(id)s].%(ext)s").absolutePath)
            if (!cookie.isNullOrBlank() && ("youtube.com" in url || "youtu.be" in url)) addOption("--cookies", cookieFile(ctx, cookie).absolutePath)
        }
        YoutubeDL.getInstance().execute(request, "rym-link")
        return dir.listFiles()?.filter { it.length() > 0 }?.maxByOrNull { it.length() } ?: error("yt-dlp saved nothing")
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
