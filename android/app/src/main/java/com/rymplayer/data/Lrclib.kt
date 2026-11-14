package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * Synced lyrics from LRCLIB: the exact match and three looser searches, all asked at once and
 * taken in that order of preference (the first synced answer wins once the better ones are in).
 */
object Lrclib {
    private val http = OkHttpClient.Builder().callTimeout(20, TimeUnit.SECONDS).build()
    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Search by lyrics: the songs whose words hold [words] (LRCLIB's full-text search), as (title,
     * artist), each song once, the first [n].
     */
    suspend fun byWords(words: String, n: Int = 8): List<Pair<String, String>> {
        val url = "https://lrclib.net/api/search".toHttpUrl().newBuilder().addQueryParameter("q", words).build()
        val list = get(url.toString()) as? kotlinx.serialization.json.JsonArray ?: return emptyList()
        return list.mapNotNull { e ->
            val o = e as? kotlinx.serialization.json.JsonObject ?: return@mapNotNull null
            val title = (o["trackName"] as? kotlinx.serialization.json.JsonPrimitive)?.content.orEmpty()
            val artist = (o["artistName"] as? kotlinx.serialization.json.JsonPrimitive)?.content.orEmpty()
            (title to artist).takeIf { title.isNotBlank() }
        }.distinctBy { (t, a) -> t.lowercase().trim() + "|" + a.lowercase().trim() }.take(n)
    }

    /** GET with one retry: LRCLIB answers 503 when busy. Null when there's no usable answer. */
    private suspend fun get(url: String): JsonElement? = withContext(Dispatchers.IO) {
        repeat(2) { attempt ->
            val req = Request.Builder().url(url).header("User-Agent", "RyM-Android/0.1").build()
            http.newCall(req).execute().use { resp ->
                if (resp.isSuccessful) return@withContext json.parseToJsonElement(resp.body?.string().orEmpty())
                if (resp.code == 404) return@withContext null
            }
            if (attempt == 0) delay(400)
        }
        null
    }

    private fun toLyrics(e: JsonElement?): Lyrics? {
        // Enhanced LRC's word tags, when the entry has them (taken out of the lines).
        val (synced, words) = e.at("syncedLyrics").str?.let(WordTimes::parseEnhanced) ?: (emptyList<Pair<Float, String>>() to emptyList())
        val plain = e.at("plainLyrics").str?.lines().orEmpty()
        // User-submitted: skip stub or vandalized entries.
        return if (synced.size >= 4 || plain.size >= 4) Lyrics(synced, plain, "LRCLIB").also { it.words = words } else null
    }

    suspend fun find(title: String, artist: String, album: String, duration: Float?): Lyrics? = runCatching { coroutineScope {
        val exact = "https://lrclib.net/api/get".toHttpUrl().newBuilder()
            .addQueryParameter("track_name", title)
            .addQueryParameter("artist_name", artist)
            .apply {
                if (album.isNotEmpty()) addQueryParameter("album_name", album)
                if (duration != null) addQueryParameter("duration", duration.toInt().toString())
            }.build().toString()
        val clean = cleanTitle(title)
        val first = firstArtist(artist)
        val queries = listOf(
            listOf("track_name" to title, "artist_name" to artist),
            listOf("track_name" to clean, "artist_name" to first),
            listOf("q" to "$clean $first"),
        ).distinct()
        val jobs = listOf(async { toLyrics(get(exact)) }) + queries.map { q ->
            async {
                val url = "https://lrclib.net/api/search".toHttpUrl().newBuilder().apply { q.forEach { (k, v) -> addQueryParameter(k, v) } }.build()
                get(url.toString()).arr.mapNotNull { h ->
                    val off = duration?.let { d -> h.at("duration")?.jsonPrimitive?.floatOrNull?.let { abs(it - d) } } ?: 0f
                    if (off > 10f) null else toLyrics(h)?.let { off to it }
                }.sortedWith(compareBy({ it.second.synced.isEmpty() }, { it.first })).firstOrNull()?.second
            }
        }
        // In order of preference: waiting on a query only while the better ones had nothing synced.
        val answers = mutableListOf<Lyrics?>()
        for (job in jobs) {
            val l = runCatching { job.await() }.getOrNull()
            if (l != null && l.synced.isNotEmpty()) {
                jobs.forEach { it.cancel() }
                return@coroutineScope l
            }
            answers += l
        }
        answers.firstOrNull { it != null }
    } }.getOrNull()

    /**
     * Every usable LRCLIB match (the full search and the loose one), synced first, then the
     * closest length; up to four. For the lyrics source switcher.
     */
    suspend fun all(title: String, artist: String, duration: Float?): List<Lyrics> = runCatching { coroutineScope {
        val queries = listOf(
            listOf("track_name" to title, "artist_name" to artist),
            listOf("q" to "${cleanTitle(title)} ${firstArtist(artist)}"),
        )
        queries.map { q ->
            async {
                val url = "https://lrclib.net/api/search".toHttpUrl().newBuilder().apply { q.forEach { (k, v) -> addQueryParameter(k, v) } }.build()
                runCatching { get(url.toString()).arr }.getOrDefault(emptyList())
            }
        }.flatMap { it.await() }.mapNotNull { h ->
            val off = duration?.let { d -> h.at("duration")?.jsonPrimitive?.floatOrNull?.let { abs(it - d) } } ?: 0f
            if (off > 15f) null else toLyrics(h)?.let { Triple(it.synced.isEmpty(), off, it) }
        }.sortedWith(compareBy({ it.first }, { it.second })).map { it.third }.let(::distinct).take(4)
    } }.getOrDefault(emptyList())

    private val quick = http.newBuilder().callTimeout(10, TimeUnit.SECONDS).build()

    /** lyrics.ovh: plain lyrics by artist and title (no timing). It can be slow: 10 s at most. */
    suspend fun ovh(title: String, artist: String): Lyrics? = runCatching { withContext(Dispatchers.IO) {
        val url = "https://api.lyrics.ovh/v1/".toHttpUrl().newBuilder().addPathSegment(firstArtist(artist)).addPathSegment(cleanTitle(title)).build()
        quick.newCall(Request.Builder().url(url).header("User-Agent", "RyM-Android/0.1").build()).execute().use { resp ->
            if (!resp.isSuccessful) return@withContext null
            val plain = json.parseToJsonElement(resp.body?.string().orEmpty()).at("lyrics").str?.lines()?.map { it.trim() }.orEmpty()
            Lyrics(emptyList(), plain, "lyrics.ovh").takeIf { plain.count { it.isNotEmpty() } >= 4 }
        }
    } }.getOrNull()

    /**
     * The same words with the same timing only once (another timing stays: that's what switching
     * is for when the lines run early or late); LRCLIB's matches numbered "LRCLIB", "LRCLIB 2", ...
     */
    fun distinct(list: List<Lyrics>): List<Lyrics> {
        val seen = mutableSetOf<String>()
        var n = 0
        return list.filter { l ->
            val words = (l.plain.asSequence() + l.synced.asSequence().map { it.second }).flatMap { it.asSequence() }
                .filter { it.isLetterOrDigit() }.take(400).joinToString("").lowercase()
            val timing = l.synced.take(6).joinToString(",") { (it.first * 2).roundToInt().toString() }
            seen.add("$timing|$words")
        }.map { l ->
            if (!l.source.startsWith("LRCLIB")) l else {
                n++
                l.copy(source = if (n == 1) "LRCLIB" else "LRCLIB $n")
            }
        }
    }

    /** "Song (feat. X) [Live]" -> "Song" */
    fun cleanTitle(title: String): String =
        title.replace(Regex("""\([^)]*\)|\[[^]]*]"""), "").split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")

    /** "A, B & C featuring D" -> "A" */
    fun firstArtist(artist: String): String {
        val lower = artist.lowercase()
        val cut = listOf(",", " & ", " featuring ", " feat.", " ft.", " x ", " and ").mapNotNull { sep -> lower.indexOf(sep).takeIf { it >= 0 } }.minOrNull()
        return (if (cut != null) artist.substring(0, cut) else artist).trim()
    }

    /** `[mm:ss.xx] text` lines; a line may carry several timestamps. */
    fun parseLrc(src: String): List<Pair<Float, String>> {
        val out = mutableListOf<Pair<Float, String>>()
        val tag = Regex("""^\[(\d+):(\d+(?:\.\d+)?)]""")
        for (raw in src.lines()) {
            var rest = raw.trim()
            val times = mutableListOf<Float>()
            while (true) {
                val m = tag.find(rest) ?: break
                times += m.groupValues[1].toFloat() * 60 + m.groupValues[2].toFloat()
                rest = rest.substring(m.range.last + 1)
            }
            times.forEach { out += it to rest.trim() }
        }
        return out.sortedBy { it.first }
    }

    /** Index of the line being sung at `pos` seconds. */
    fun currentLine(synced: List<Pair<Float, String>>, pos: Float): Int? {
        val i = synced.indexOfLast { it.first <= pos }
        return i.takeIf { it >= 0 }
    }
}
