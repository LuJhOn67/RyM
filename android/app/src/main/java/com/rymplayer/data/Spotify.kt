package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit
import kotlin.math.abs

/**
 * Spotify plugin: the songs of a Spotify link (a song, an album or a public playlist) found on
 * YouTube Music, to play or download. Spotify's own sound is locked (DRM), so only the list of
 * songs is read, from the page Spotify gives for embedding (no account or key needed; it holds
 * the first 100 songs of a playlist). The same as the PC apps' src/spotify.rs.
 */
object Spotify {
    data class Wanted(val title: String, val artist: String, val secs: Float?)
    data class SongList(val name: String, val songs: List<Wanted>)

    private val http = OkHttpClient.Builder().callTimeout(20, TimeUnit.SECONDS).build()

    /** "https://open.spotify.com/intl-ar/playlist/ID?si=..." or "spotify:album:ID" -> ("playlist", ID). */
    fun parseLink(text: String): Pair<String, String>? {
        val t = text.trim()
        val rest = if (t.startsWith("spotify:")) t.removePrefix("spotify:").replace(':', '/')
        else t.removePrefix("https://").removePrefix("http://").takeIf { it.startsWith("open.spotify.com/") }?.removePrefix("open.spotify.com/") ?: return null
        val parts = rest.split('/').filter { it.isNotEmpty() && !it.startsWith("intl-") }
        val kind = parts.getOrNull(0)?.takeIf { it in listOf("track", "album", "playlist") } ?: return null
        val id = parts.getOrNull(1)?.substringBefore('?')?.substringBefore('#') ?: return null
        return (kind to id).takeIf { id.isNotEmpty() && id.all { it.isLetterOrDigit() && it.code < 128 } }
    }

    fun isLink(text: String) = parseLink(text) != null

    suspend fun read(link: String): SongList = withContext(Dispatchers.IO) {
        val (kind, id) = parseLink(link) ?: error("Not a Spotify song, album or playlist link")
        val req = Request.Builder().url("https://open.spotify.com/embed/$kind/$id")
            .header("User-Agent", "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36").build()
        http.newCall(req).execute().use { r ->
            check(r.isSuccessful) { "Spotify: ${r.code}" }
            parseEmbed(r.body?.string().orEmpty())
        }
    }

    /** The song list in the embed page's data (`__NEXT_DATA__`). */
    fun parseEmbed(html: String): SongList {
        val tag = """<script id="__NEXT_DATA__" type="application/json">"""
        val start = html.indexOf(tag).takeIf { it >= 0 } ?: error("Spotify's page changed: no song list")
        val json = html.substring(start + tag.length).substringBefore("</script>")
        val e = Json.parseToJsonElement(json).at("props/pageProps/state/data/entity")
        fun secs(v: kotlinx.serialization.json.JsonElement?) = (v as? JsonPrimitive)?.doubleOrNull?.takeIf { it > 0 }?.let { (it / 1000).toFloat() }
        val list = e.at("trackList").arr
        val songs = if (list.isNotEmpty()) {
            list.mapNotNull { t -> Wanted(t.at("title").str ?: return@mapNotNull null, t.at("subtitle").str.orEmpty(), secs(t.at("duration"))) }
        } else {
            val title = e.at("name").str ?: e.at("title").str ?: error("No songs on this Spotify page")
            listOf(Wanted(title, e.at("artists").arr.mapNotNull { it.at("name").str }.joinToString(", "), secs(e.at("duration"))))
        }
        return SongList(e.at("name").str ?: e.at("title").str ?: "Spotify", songs)
    }

    /** Lower case letters and digits only, without the "(feat. ...)" and "[...]" parts. */
    private fun norm(s: String): String {
        val out = StringBuilder()
        var depth = 0
        for (c in s) {
            when {
                c == '(' || c == '[' -> depth++
                c == ')' || c == ']' -> depth = (depth - 1).coerceAtLeast(0)
                depth > 0 -> {}
                c.isLetterOrDigit() -> out.append(c.lowercaseChar())
                out.isNotEmpty() && out.last() != ' ' -> out.append(' ')
            }
        }
        return out.toString().trim()
    }

    private fun firstArtist(a: String) = a.split(',', '&').first().trim()

    /** How well a YouTube Music song matches: title, artist and length. */
    private fun score(w: Wanted, t: Track): Int {
        val want = norm(w.title)
        val got = norm(t.title)
        var s = when {
            want == got -> 3
            want.isNotEmpty() && (want in got || got in want) -> 2
            else -> 0
        }
        val artist = norm(firstArtist(w.artist))
        if (artist.isNotEmpty() && artist in norm(t.artist)) s += 2
        val b = parseDuration(t.duration)
        if (w.secs != null && b != null) {
            val d = abs(w.secs - b)
            s += when {
                d <= 3 -> 2
                d <= 10 -> 1
                d > 30 -> -2
                else -> 0
            }
        }
        return s
    }

    /** The song on YouTube Music: the best of the first results, when it's close enough (the earlier wins a tie). */
    fun best(w: Wanted, results: List<Track>): Track? {
        var pick: Pair<Int, Track>? = null
        for (t in results.take(6)) {
            val s = score(w, t)
            if (s >= 3 && (pick == null || s > pick.first)) pick = s to t
        }
        return pick?.second
    }

    /** Every song of the list found on YouTube Music (six searches at a time), in order; songs not found are left out. */
    suspend fun findAll(api: MusicApi, list: SongList): List<Track> = coroutineScope {
        val limit = Semaphore(6)
        list.songs.map { w ->
            async { limit.withPermit { runCatching { best(w, api.search("${w.title} ${firstArtist(w.artist)}")) }.getOrNull() } }
        }.awaitAll().filterNotNull().distinctBy { it.videoId }
    }

    /** A Spotify link's songs, found on YouTube Music. */
    suspend fun import(api: MusicApi, link: String): List<Track> {
        val list = read(link)
        return findAll(api, list).ifEmpty { error("None of the songs of \"${list.name}\" were found on YouTube Music") }
    }
}
