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
    private val url = Regex("""(?:https?://|rym://|spotify:)\S+""", RegexOption.IGNORE_CASE)

    /** The first link in shared text (apps share "Song name – https://…"). */
    fun firstLink(text: String): String? = url.find(text)?.value?.trimEnd('.', ',', ')', '"', '\'')

    fun isLink(text: String) = firstLink(text)?.let(::parse) != null

    /** rym://play?url=<a link>: that link (not another rym:// one); null for anything else. */
    fun unwrapRym(link: String): String? {
        val uri = runCatching { URI(link.trim()) }.getOrNull() ?: return null
        if (uri.scheme?.lowercase() != "rym") return null
        return query(uri)["url"]?.trim()?.takeIf { it.isNotEmpty() && !it.lowercase().startsWith("rym:") }
    }

    private fun query(uri: URI): Map<String, String> = uri.rawQuery.orEmpty().split('&').mapNotNull {
        val (k, v) = it.split('=', limit = 2).takeIf { p -> p.size == 2 } ?: return@mapNotNull null
        k to URLDecoder.decode(v, "UTF-8")
    }.toMap()

    fun parse(link: String): Target? {
        val uri = runCatching { URI(link.trim()) }.getOrNull() ?: return null
        if (uri.scheme?.lowercase() == "rym") {
            // rym://play?url=… (one level), rym://play?v=…, rym://play?list=…, rym://play/<id>.
            unwrapRym(link)?.let { return parse(it) }
            val q = query(uri)
            q["v"]?.takeIf { videoId.matches(it) }?.let { return Target.Song(it) }
            q["list"]?.takeIf { it.isNotEmpty() }?.let { return listTarget(it) }
            return uri.rawPath.orEmpty().split('/').firstOrNull { it.isNotEmpty() }?.takeIf { videoId.matches(it) }?.let { Target.Song(it) }
        }
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
