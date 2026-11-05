package com.rymplayer.data

import java.net.URI
import java.net.URLDecoder

/**
 * YouTube Music and YouTube links opened in RyM (from the browser, or shared to it): what they
 * point at.
 */
object YouTubeLinks {
    sealed interface Target {
        data class Song(val videoId: String) : Target
        data class Playlist(val id: String) : Target
        data class Mix(val id: String) : Target
        data class Album(val id: String) : Target
        data class Artist(val id: String) : Target
    }

    private val hosts = setOf("music.youtube.com", "youtube.com", "www.youtube.com", "m.youtube.com", "youtu.be", "www.youtu.be")
    private val videoId = Regex("^[A-Za-z0-9_-]{11}$")
    private val url = Regex("""https?://\S+""")

    /** The first link in shared text (apps share "Song name – https://…"). */
    fun firstLink(text: String): String? = url.find(text)?.value?.trimEnd('.', ',', ')', '"', '\'')

    fun isLink(text: String) = firstLink(text)?.let(::parse) != null

    fun parse(link: String): Target? {
        val uri = runCatching { URI(link.trim()) }.getOrNull() ?: return null
        val host = uri.host?.lowercase() ?: return null
        if (host !in hosts) return null
        val segments = uri.rawPath.orEmpty().split('/').filter { it.isNotEmpty() }
        val query = uri.rawQuery.orEmpty().split('&').mapNotNull {
            val (k, v) = it.split('=', limit = 2).takeIf { p -> p.size == 2 } ?: return@mapNotNull null
            k to URLDecoder.decode(v, "UTF-8")
        }.toMap()
        if (host.endsWith("youtu.be")) return segments.firstOrNull()?.takeIf { videoId.matches(it) }?.let { Target.Song(it) }
        val list = query["list"]
        return when (segments.firstOrNull()) {
            "watch" -> query["v"]?.takeIf { videoId.matches(it) }?.let { Target.Song(it) } ?: list?.let(::listTarget)
            "shorts", "live", "embed" -> segments.getOrNull(1)?.takeIf { videoId.matches(it) }?.let { Target.Song(it) }
            "playlist" -> list?.let(::listTarget)
            "browse" -> segments.getOrNull(1)?.let { id ->
                when {
                    id.startsWith("MPREb") -> Target.Album(id)
                    id.startsWith("VL") -> listTarget(id.removePrefix("VL"))
                    id.startsWith("UC") -> Target.Artist(id)
                    else -> null
                }
            }
            "channel" -> segments.getOrNull(1)?.takeIf { it.startsWith("UC") }?.let { Target.Artist(it) }
            else -> null
        }
    }

    /** A list id: a mix ("RD…", made for a song) or a playlist (YouTube Music's own "RDCLAK…" are playlists). */
    private fun listTarget(id: String): Target = if (id.startsWith("RD") && !id.startsWith("RDCLAK")) Target.Mix(id) else Target.Playlist(id)
}
