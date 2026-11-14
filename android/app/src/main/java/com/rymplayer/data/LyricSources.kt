package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit
import kotlin.math.abs

/**
 * More lyrics providers besides LRCLIB and YouTube Music (the same as the PC apps'
 * src/lyric_sources.rs): NetEase Cloud Music and KuGou (synced LRC), Genius (plain, the last
 * resort). A match must have the same title and, when both are known, a length within 10 s.
 * Credit lines and "Title - Artist" header lines are left out.
 */
object LyricSources {
    const val LRCLIB = "LRCLIB"
    const val NETEASE = "NetEase"
    const val KUGOU = "KuGou"
    const val GENIUS = "Genius"
    const val YOUTUBE = "YouTube Music"

    /** The providers in the order they are tried (and shown in the lyrics view's switcher). */
    val PROVIDERS = listOf(LRCLIB, NETEASE, KUGOU, GENIUS, YOUTUBE)

    /** Which provider some lyrics came from ("LRCLIB 2" is LRCLIB; YouTube Music names its own sources). */
    fun provider(l: Lyrics): String = PROVIDERS.firstOrNull { it != YOUTUBE && l.source.startsWith(it) } ?: YOUTUBE

    private const val BROWSER = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36"
    private val http = OkHttpClient.Builder().callTimeout(10, TimeUnit.SECONDS).build()
    private val json = Json { ignoreUnknownKeys = true }

    private fun text(url: String, referer: String? = null): String {
        val req = Request.Builder().url(url).header("User-Agent", BROWSER).apply { referer?.let { header("Referer", it) } }.build()
        return http.newCall(req).execute().use { r ->
            check(r.isSuccessful) { "HTTP ${r.code}" }
            r.body?.string().orEmpty()
        }
    }

    private fun get(url: String, referer: String? = null): JsonElement = json.parseToJsonElement(text(url, referer))

    /** Lower case letters and digits, without the "(feat. ...)" and "[...]" parts. */
    fun norm(s: String): String {
        val out = StringBuilder()
        var depth = 0
        for (c in s) {
            when {
                c in "([（【" -> depth++
                c in ")]）】" -> depth = (depth - 1).coerceAtLeast(0)
                depth > 0 -> {}
                c.isLetterOrDigit() -> out.append(c.lowercaseChar())
            }
        }
        return out.toString()
    }

    private fun firstArtist(a: String) = a.split(',', '&', '、', '/').first().trim()

    private fun sameSong(t: Track, title: String, secs: Float?): Boolean {
        val a = norm(t.title)
        val b = norm(title)
        val titles = a.isNotEmpty() && b.isNotEmpty() && (b in a || a in b)
        val mine = parseDuration(t.duration)
        val lengths = mine == null || secs == null || abs(mine - secs) <= 10f
        return titles && lengths
    }

    private fun query(t: Track) = "${Lrclib.cleanTitle(t.title)} ${firstArtist(t.artist)}"

    private val CREDITS = listOf(
        "作词", "作曲", "编曲", "制作", "混音", "母带", "和声", "吉他", "贝斯", "鼓", "录音", "监制", "出品", "发行", "词", "曲",
        "written by", "composed by", "lyrics by", "lyricist", "composer", "producer",
    )

    /** Credit lines and the "Title - Artist" header line are not lyrics. */
    fun clean(l: Lyrics, t: Track): Lyrics {
        val title = norm(t.title)
        val artist = norm(firstArtist(t.artist))
        fun keep(line: String): Boolean {
            val lower = line.trim().lowercase()
            val credit = CREDITS.any { c -> lower.startsWith(c) && lower.removePrefix(c).trimStart().let { it.startsWith(":") || it.startsWith("：") } }
            val n = norm(line)
            val header = title.isNotEmpty() && artist.isNotEmpty() && title in n && artist in n
            return !credit && !header
        }
        // The word times go with their lines.
        var words = if (l.words.size == l.synced.size) l.words.filterIndexed { i, _ -> keep(l.synced[i].second) } else emptyList()
        var synced = l.synced.filter { keep(it.second) }
        var plain = l.plain.filter(::keep)
        // A first line naming the artist with a dash ("Artist -<Title>"): a header, title misspelled.
        fun header(s: String) = artist.isNotEmpty() && artist in norm(s) && '-' in s
        if (synced.firstOrNull()?.let { header(it.second) } == true) {
            synced = synced.drop(1)
            words = words.drop(1)
            if (plain.firstOrNull()?.let(::header) == true) plain = plain.drop(1)
        }
        return l.copy(synced = synced, plain = plain).also { it.words = words; it.translation = l.translation }
    }

    private fun usable(l: Lyrics) = l.takeIf { it.synced.size >= 4 || it.plain.count { s -> s.isNotBlank() } >= 4 }

    private fun num(e: JsonElement?) = (e as? JsonPrimitive)?.doubleOrNull

    /** NetEase Cloud Music's synced lyrics. */
    suspend fun netease(t: Track): Lyrics? = withContext(Dispatchers.IO) {
        val search = "https://music.163.com/api/search/get".toHttpUrl().newBuilder()
            .addQueryParameter("s", query(t)).addQueryParameter("type", "1").addQueryParameter("limit", "10").addQueryParameter("offset", "0").build()
        val songs = get(search.toString(), "https://music.163.com/").at("result/songs").arr
        val id = songs.firstOrNull { s -> sameSong(t, s.at("name").str.orEmpty(), num(s.at("duration"))?.let { (it / 1000).toFloat() }) }
            ?.at("id")?.let { (it as? JsonPrimitive)?.content } ?: return@withContext null
        // v1 also sends the word-timed lyrics (YRC) when NetEase has them.
        val answer = get("https://music.163.com/api/song/lyric/v1?id=$id&lv=1&kv=1&tv=-1&yv=1", "https://music.163.com/")
        var synced = Lrclib.parseLrc(answer.at("lrc/lyric").str.orEmpty())
        val (timed, timedWords) = WordTimes.parseTimed(answer.at("yrc/lyric").str.orEmpty())
        // The word-timed lines when they're the whole song.
        val words = if (timed.isNotEmpty() && timed.size * 10 >= synced.size * 6) timedWords.also { synced = timed } else emptyList()
        val l = clean(Lyrics(synced, synced.map { it.second }, NETEASE).also { it.words = words }, t)
        // Its own translation (often Chinese): kept when it's English, line by line by time.
        val tr = Lrclib.parseLrc(answer.at("tlyric/lyric").str.orEmpty())
        if (Translate.isEnglish(tr.map { it.second })) l.translation = Translate.byTime(l.synced, tr)
        usable(l)
    }

    /** KuGou's synced lyrics. */
    suspend fun kugou(t: Track): Lyrics? = withContext(Dispatchers.IO) {
        val search = "http://mobilecdn.kugou.com/api/v3/search/song".toHttpUrl().newBuilder()
            .addQueryParameter("format", "json").addQueryParameter("keyword", query(t))
            .addQueryParameter("page", "1").addQueryParameter("pagesize", "10").addQueryParameter("showtype", "1").build()
        val hit = get(search.toString()).at("data/info").arr.firstOrNull { s -> sameSong(t, s.at("songname").str.orEmpty(), num(s.at("duration"))?.toFloat()) }
            ?: return@withContext null
        val hash = hit.at("hash").str ?: return@withContext null
        val ms = ((num(hit.at("duration")) ?: 0.0) * 1000).toLong()
        val candidate = get("https://krcs.kugou.com/search?ver=1&man=yes&client=mobi&keyword=&duration=$ms&hash=$hash").at("candidates").arr.firstOrNull()
            ?: return@withContext null
        val id = (candidate.at("id") as? JsonPrimitive)?.content ?: return@withContext null
        val key = candidate.at("accesskey").str ?: return@withContext null
        // KRC: word by word (scrambled and packed); plain LRC when it isn't there or can't be read.
        runCatching {
            val krc = get("https://lyrics.kugou.com/download?ver=1&client=pc&id=$id&accesskey=$key&fmt=krc&charset=utf8").at("content").str
            val text = krc?.let { WordTimes.decodeKrc(java.util.Base64.getMimeDecoder().decode(it)) }
            val (lines, words) = WordTimes.parseTimed(text.orEmpty())
            if (lines.isNotEmpty()) usable(clean(Lyrics(lines, lines.map { it.second }, KUGOU).also { it.words = words }, t)) else null
        }.getOrNull()?.let { return@withContext it }
        val content = get("https://lyrics.kugou.com/download?ver=1&client=pc&id=$id&accesskey=$key&fmt=lrc&charset=utf8").at("content").str ?: return@withContext null
        val synced = Lrclib.parseLrc(String(java.util.Base64.getMimeDecoder().decode(content), Charsets.UTF_8))
        usable(clean(Lyrics(synced, synced.map { it.second }, KUGOU), t))
    }

    /** Search by lyrics: Genius's lyric search (it looks in the songs' words), as (title, artist). */
    suspend fun geniusByWords(words: String, n: Int = 8): List<Pair<String, String>> = withContext(Dispatchers.IO) {
        val search = "https://genius.com/api/search/lyric".toHttpUrl().newBuilder().addQueryParameter("q", words).addQueryParameter("per_page", n.toString()).build()
        runCatching { get(search.toString()) }.getOrNull()?.at("response/sections")?.arr.orEmpty().flatMap { it.at("hits").arr }.mapNotNull { h ->
            val r = h.at("result")
            r.at("title").str?.let { it to r.at("primary_artist/name").str.orEmpty() }
        }
    }

    /** Genius's lyrics (plain: Genius has no timing), from the song's page. */
    suspend fun genius(t: Track): Lyrics? = withContext(Dispatchers.IO) {
        val search = "https://genius.com/api/search/song".toHttpUrl().newBuilder().addQueryParameter("q", query(t)).addQueryParameter("per_page", "5").build()
        val artist = norm(firstArtist(t.artist))
        val page = get(search.toString()).at("response/sections").arr.flatMap { it.at("hits").arr }.firstNotNullOfOrNull { h ->
            val r = h.at("result")
            val by = norm(r.at("primary_artist/name").str.orEmpty())
            // Translations come from "Genius Traducciones" and such: the artist must match.
            val artistOk = artist.isEmpty() || artist in by || by in artist
            r.at("url").str.takeIf { artistOk && sameSong(t, r.at("title").str.orEmpty(), null) }
        } ?: return@withContext null
        usable(clean(Lyrics(emptyList(), geniusLines(text(page)), GENIUS), t))
    }

    /** The text of the page's lyrics containers, line by line (their nested headers left out). */
    fun geniusLines(html: String): List<String> {
        val parts = mutableListOf<String>()
        var from = 0
        while (true) {
            val at = html.indexOf("data-lyrics-container=\"true\"", from).takeIf { it >= 0 } ?: break
            val tag = html.lastIndexOf("<div", at).takeIf { it >= 0 } ?: at
            var inner = divInner(html, tag)
            from = at + 1
            while (true) {
                val x = inner.indexOf("data-exclude-from-selection=\"true\"").takeIf { it >= 0 } ?: break
                val t = inner.lastIndexOf("<div", x).takeIf { it >= 0 } ?: x
                val len = divInner(inner, t).length
                val openEnd = inner.indexOf('>', t).let { if (it < 0) inner.length else it + 1 }
                inner = inner.removeRange(t, (openEnd + len + "</div>".length).coerceAtMost(inner.length))
            }
            parts += inner
        }
        val text = parts.joinToString("\n").replace(Regex("<br\\s*/?>"), "\n").replace(Regex("<[^>]+>"), "")
        val lines = unescape(text).lines().map { it.trim() }
        return lines.dropWhile { it.isEmpty() }
    }

    private fun divInner(html: String, tag: Int): String {
        val openEnd = html.indexOf('>', tag).takeIf { it >= 0 }?.plus(1) ?: return ""
        var depth = 1
        var at = openEnd
        while (true) {
            val close = html.indexOf("</div>", at).takeIf { it >= 0 } ?: return html.substring(openEnd)
            val open = html.indexOf("<div", at)
            if (open in 0 until close) {
                depth++
                at = open + 4
            } else {
                depth--
                if (depth == 0) return html.substring(openEnd, close)
                at = close + 6
            }
        }
    }

    private fun unescape(s: String): String = Regex("&(#x[0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos|nbsp);").replace(s) { m ->
        when (val n = m.groupValues[1]) {
            "amp" -> "&"
            "lt" -> "<"
            "gt" -> ">"
            "quot" -> "\""
            "apos" -> "'"
            "nbsp" -> " "
            else -> (if (n.startsWith("#x")) n.drop(2).toIntOrNull(16) else n.drop(1).toIntOrNull())?.let { String(Character.toChars(it)) } ?: m.value
        }
    }
}
