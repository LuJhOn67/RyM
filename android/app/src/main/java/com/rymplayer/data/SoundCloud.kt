package com.rymplayer.data

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull

/**
 * SoundCloud plugin: SoundCloud's songs searched, streamed and downloaded through yt-dlp (a
 * search, or a SoundCloud link: a song, a set or an artist's page). A SoundCloud song is a
 * [Track] whose id is `sc.` and SoundCloud's number for it, the same as in the PC apps.
 */
object SoundCloud {
    const val PREFIX = "sc."

    fun isSoundCloud(id: String) = id.length > PREFIX.length && id.startsWith(PREFIX) && id.drop(PREFIX.length).all { it.isDigit() }

    /** The link yt-dlp opens for the song. */
    fun url(id: String) = "https://api.soundcloud.com/tracks/${id.removePrefix(PREFIX)}"

    /** A link anyone can open: SoundCloud's player with the song. */
    fun shareLink(id: String) = "https://w.soundcloud.com/player/?url=https%3A//api.soundcloud.com/tracks/${id.removePrefix(PREFIX)}"

    /** soundcloud.com/<artist>/<song>: one song (not a set, the artist's likes or their songs). */
    fun isSong(text: String): Boolean {
        if (!isLink(text)) return false
        val uri = runCatching { java.net.URI(text.trim()) }.getOrNull() ?: return false
        if (uri.host?.startsWith("on.") == true) return false
        val parts = uri.rawPath.orEmpty().split('/').filter { it.isNotEmpty() }
        return parts.size == 2 && parts[1] !in setOf("sets", "likes", "tracks", "albums", "reposts", "popular-tracks", "followers", "following")
    }

    /** A SoundCloud page: a song, a set, an artist. */
    fun isLink(text: String): Boolean {
        val t = text.trim().removePrefix("https://").removePrefix("http://")
        return listOf("soundcloud.com/", "m.soundcloud.com/", "on.soundcloud.com/", "www.soundcloud.com/").any { t.startsWith(it) }
    }

    suspend fun search(ctx: Context, query: String): List<Track> = list(ctx, "scsearch40:$query")

    /** The songs behind a SoundCloud link (the first 100 of a set or an artist's page). */
    suspend fun openLink(ctx: Context, link: String): List<Track> = list(ctx, link.trim())

    /** The song's stream (SoundCloud sends it in pieces, HLS), for the player. */
    suspend fun streamUrl(ctx: Context, id: String): String = withContext(Dispatchers.IO) { YtDlp.link(ctx, url(id)) }

    private suspend fun list(ctx: Context, what: String): List<Track> = withContext(Dispatchers.IO) {
        tracks(Json.parseToJsonElement(YtDlp.flatJson(ctx, what)))
    }

    /** yt-dlp's answer as songs: a list's entries, or the one song a song link gives. */
    fun tracks(info: JsonElement): List<Track> {
        val owner = info.at("uploader").str.orEmpty()
        val entries = info.at("entries")
        return if (entries != null) entries.arr.mapNotNull { track(it, owner) }.take(100) else listOfNotNull(track(info, ""))
    }

    /** One song. SoundCloud Go+ songs (only a 30-second preview here) and sets are left out. */
    private fun track(e: JsonElement, owner: String): Track? {
        val link = e.at("url").str ?: e.at("webpage_url").str.orEmpty()
        if ("/sets/" in link) return null
        val id = (e.at("id") as? JsonPrimitive)?.content ?: return null
        val secs = (e.at("duration") as? JsonPrimitive)?.doubleOrNull
        if (secs != null && secs <= 30.5) return null
        val thumb = e.at("thumbnails").arr.lastOrNull().at("url").str?.replace("-original.", "-t500x500.")
        val t = Track(
            videoId = "$PREFIX$id",
            title = e.at("title").str ?: return null,
            artist = e.at("artists/0").str ?: e.at("uploader").str ?: owner,
            album = "SoundCloud",
            duration = secs?.let { fmtTime(it.toFloat()) }.orEmpty(),
            thumb = thumb,
        )
        return t.takeIf { isSoundCloud(it.videoId) }
    }
}
