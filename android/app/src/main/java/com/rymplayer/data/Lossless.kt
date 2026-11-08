package com.rymplayer.data

import org.json.JSONArray
import org.json.JSONObject

/**
 * "Prefer lossless (FLAC)", the same as the PC's lossless.rs: the song as a FLAC file from the
 * Internet Archive (archive.org, free and legal), found by title, artist and length.
 */
object Lossless {
    private fun norm(s: String) = s.lowercase().filter { it.isLetterOrDigit() }

    /** "3:45" or "1:02:03" as seconds. */
    fun secsOf(duration: String): Float? {
        var total = 0f
        for (part in duration.trim().split(':')) total = total * 60 + (part.trim().toFloatOrNull() ?: return null)
        return total.takeIf { it > 0f }
    }

    /** The item's file that is this song: FLAC, its title (or name) the song's, as long as it (within 4 s) when both are known. */
    fun pickFile(files: JSONArray, title: String, secs: Float?): String? {
        val want = norm(title)
        if (want.isEmpty()) return null
        for (i in 0 until files.length()) {
            val f = files.optJSONObject(i) ?: continue
            val name = f.optString("name")
            if (!name.lowercase().endsWith(".flac")) continue
            val named = norm(f.optString("title")).ifEmpty { norm(name.substringAfterLast('/').removeSuffix(".flac")) }
            if (!(named == want || named.endsWith(want) || (want.length >= 6 && want in named))) continue
            val length = f.optString("length").let { it.toFloatOrNull() ?: secsOf(it) }
            if (length != null && secs != null && kotlin.math.abs(length - secs) > 4f) continue
            return name
        }
        return null
    }

    private fun getJson(url: String): JSONObject {
        val c = java.net.URL(url).openConnection() as java.net.HttpURLConnection
        c.connectTimeout = 6000
        c.readTimeout = 6000
        c.setRequestProperty("User-Agent", "RyM")
        return c.inputStream.bufferedReader().use { JSONObject(it.readText()) }
    }

    /** The song's FLAC link on the Internet Archive, or null. Call off the main thread. */
    fun find(t: Track): String? = runCatching {
        val artist = t.artist.split(',', '&').first().trim()
        if (t.title.isBlank() || artist.isEmpty()) return null
        fun q(s: String) = "\"" + s.replace("\"", " ") + "\""
        val query = "(title:(${q(t.title)}) OR ${q(t.title)}) AND creator:(${q(artist)}) AND format:(FLAC)"
        val search = "https://archive.org/advancedsearch.php?q=" + java.net.URLEncoder.encode(query, "UTF-8") + "&fl%5B%5D=identifier&rows=5&output=json"
        val docs = getJson(search).optJSONObject("response")?.optJSONArray("docs") ?: return null
        val secs = secsOf(t.duration)
        for (i in 0 until docs.length()) {
            val id = docs.optJSONObject(i)?.optString("identifier")?.takeIf { it.isNotEmpty() } ?: continue
            val files = runCatching { getJson("https://archive.org/metadata/$id").optJSONArray("files") }.getOrNull() ?: continue
            val name = pickFile(files, t.title, secs) ?: continue
            val path = name.split('/').joinToString("/") { java.net.URLEncoder.encode(it, "UTF-8").replace("+", "%20") }
            return "https://archive.org/download/$id/$path"
        }
        null
    }.getOrNull()
}
