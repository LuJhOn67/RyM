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

/**
 * Songs saved to the phone's Music/RymPlayer folder. They show in Library > Downloads and play
 * from the file afterwards, also without internet.
 */
object Downloads {
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
    /**
     * Puts a downloaded file where the phone's apps see it: videos in Movies/RymPlayer, sound in
     * Music/RymPlayer. Returns where it went.
     */
    fun publish(ctx: Context, file: java.io.File, video: Boolean): String {
        val resolver = ctx.contentResolver
        val ext = file.extension.lowercase()
        val mime = when {
            video && ext == "webm" -> "video/webm"
            video -> "video/mp4"
            ext == "webm" -> "audio/webm"
            ext == "m4a" || ext == "mp4" -> "audio/mp4"
            ext == "mp3" -> "audio/mpeg"
            ext == "opus" || ext == "ogg" -> "audio/ogg"
            else -> "application/octet-stream"
        }
        val folder = if (video) Environment.DIRECTORY_MOVIES + "/RymPlayer" else Environment.DIRECTORY_MUSIC + "/RymPlayer"
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, folder)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val collection = if (video) MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        else MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val item = resolver.insert(collection, values) ?: error("can't create the file in $folder")
        resolver.openOutputStream(item)!!.use { out -> file.inputStream().use { it.copyTo(out) } }
        resolver.update(item, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        file.delete()
        return folder
    }

    suspend fun save(ctx: Context, t: Track, cookie: String?, best: Boolean): String = withContext(Dispatchers.IO) {
        uriFor(ctx, t.videoId)?.let { return@withContext "already saved" }
        // Best: yt-dlp with the account (256 kbps for Premium, slow on a phone); if it fails, or
        // in fast mode, the file playback uses (about 160 kbps, in seconds).
        val audio = if (best) {
            runCatching { YtDlp.download(ctx, t.videoId, cookie) }
                .onFailure { android.util.Log.w("RyM", "yt-dlp download of ${t.videoId} failed, using the player's file", it) }
                .getOrElse { Streams.audioFile(ctx, t.videoId) }
        } else {
            Streams.audioFile(ctx, t.videoId)
        }
        val mp4 = audio.extension == "m4a"
        val base = safeName("${t.artist} - ${t.title}")
        // Some phones refuse ".webm" in Music (they take it for video); Matroska is the same container.
        val kinds = if (mp4) listOf("m4a" to "audio/mp4") else listOf("webm" to "audio/webm", "mka" to "audio/x-matroska")
        val resolver = ctx.contentResolver
        var name = ""
        var refused: Exception? = null
        val item = kinds.firstNotNullOfOrNull { (ext, mime) ->
            name = "$base.$ext"
            val values = ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                put(MediaStore.Audio.Media.MIME_TYPE, mime)
                put(MediaStore.Audio.Media.RELATIVE_PATH, Environment.DIRECTORY_MUSIC + "/RymPlayer")
                put(MediaStore.Audio.Media.TITLE, t.title)
                put(MediaStore.Audio.Media.ARTIST, t.artist)
                put(MediaStore.Audio.Media.IS_PENDING, 1)
            }
            runCatching { resolver.insert(MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values) }
                .onFailure { refused = it as? Exception; android.util.Log.w("RyM", "Music folder refused $name", it) }
                .getOrNull()
        } ?: error("can't create the file in Music${refused?.message?.let { ": $it" } ?: ""}")
        try {
            resolver.openOutputStream(item)!!.use { out -> audio.inputStream().use { it.copyTo(out) } }
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
}
