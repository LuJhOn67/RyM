package com.rymplayer.data

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import com.rymplayer.playback.Streams
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.OutputStream
import java.util.concurrent.TimeUnit

/**
 * Songs saved to the phone's Music/RymPlayer folder. They show in Library > Downloads and play
 * from the file afterwards, also without internet.
 */
object Downloads {
    /** Bytes asked for per request: YouTube slows down single requests for whole songs. */
    private const val RANGE = 4L shl 20
    private val http = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(30, TimeUnit.SECONDS).build()

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("downloads", Context.MODE_PRIVATE)

    /** Saved songs, newest first. Files the user deleted are left out. */
    fun list(ctx: Context): List<Track> =
        prefs(ctx).all.mapNotNull { (id, v) ->
            val j = runCatching { Json.parseToJsonElement(v as String) }.getOrNull() ?: return@mapNotNull null
            val at = j.at("at")?.let { (it as? kotlinx.serialization.json.JsonPrimitive)?.content?.toLongOrNull() } ?: 0L
            at to Track(id, j.at("title").str.orEmpty(), j.at("artist").str.orEmpty(), j.at("album").str.orEmpty(), j.at("duration").str.orEmpty(), j.at("thumb").str)
        }.sortedByDescending { it.first }.map { it.second }.filter { uriFor(ctx, it.videoId) != null }

    /** The saved file for a song, if it's still there. */
    fun uriFor(ctx: Context, videoId: String): Uri? {
        val raw = prefs(ctx).getString(videoId, null) ?: return null
        val uri = runCatching { Json.parseToJsonElement(raw).at("uri").str }.getOrNull()?.let(Uri::parse) ?: return null
        val exists = runCatching { ctx.contentResolver.openFileDescriptor(uri, "r")?.close() != null }.getOrDefault(false)
        if (!exists) prefs(ctx).edit().remove(videoId).apply()
        return uri.takeIf { exists }
    }

    /** Downloads a song into Music/RymPlayer; returns the file name. */
    suspend fun save(ctx: Context, t: Track): String = withContext(Dispatchers.IO) {
        uriFor(ctx, t.videoId)?.let { return@withContext "already saved" }
        val url = Streams.resolve(t.videoId) ?: error("no audio for this song")
        val link = Uri.parse(url)
        val mime = link.getQueryParameter("mime") ?: "audio/webm"
        val name = safeName("${t.artist} - ${t.title}") + if (mime.contains("mp4")) ".m4a" else ".webm"
        val values = ContentValues().apply {
            put(MediaStore.Audio.Media.DISPLAY_NAME, name)
            put(MediaStore.Audio.Media.MIME_TYPE, mime)
            put(MediaStore.Audio.Media.RELATIVE_PATH, Environment.DIRECTORY_MUSIC + "/RymPlayer")
            put(MediaStore.Audio.Media.TITLE, t.title)
            put(MediaStore.Audio.Media.ARTIST, t.artist)
            put(MediaStore.Audio.Media.IS_PENDING, 1)
        }
        val resolver = ctx.contentResolver
        val item = resolver.insert(MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values)
            ?: error("can't create the file")
        try {
            resolver.openOutputStream(item)!!.use { copy(url, Streams.userAgentFor(link), it) }
            resolver.update(item, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
        } catch (e: Exception) {
            resolver.delete(item, null, null)
            throw e
        }
        val meta = buildJsonObject {
            put("uri", item.toString())
            put("title", t.title)
            put("artist", t.artist)
            put("album", t.album)
            put("duration", t.duration)
            t.thumb?.let { put("thumb", it) }
            put("at", System.currentTimeMillis())
        }
        prefs(ctx).edit().putString(t.videoId, meta.toString()).apply()
        name
    }

    /** Characters file systems refuse become `_`. */
    fun safeName(s: String): String = s.map { if (it in "<>:\"/\\|?*" || it.isISOControl()) '_' else it }.joinToString("").trim().trimEnd('.')

    private fun copy(url: String, userAgent: String?, out: OutputStream) {
        var pos = 0L
        var total = Long.MAX_VALUE
        while (pos < total) {
            val req = Request.Builder().url(url).header("Range", "bytes=$pos-${pos + RANGE - 1}")
                .apply { userAgent?.let { header("User-Agent", it) } }
                .build()
            http.newCall(req).execute().use { r ->
                check(r.isSuccessful) { "download: HTTP ${r.code}" }
                val body = r.body ?: error("download: empty answer")
                if (r.code == 200) {
                    // The server sent the whole song at once.
                    body.byteStream().copyTo(out)
                    return
                }
                total = r.header("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: total
                val n = body.byteStream().copyTo(out)
                check(n > 0) { "download stopped at $pos bytes" }
                pos += n
            }
        }
    }
}
