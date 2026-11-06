package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/**
 * English under Japanese lyrics: the lyrics site's own English translation (NetEase's, lined up
 * by time) when there is one, else a machine translation of every line at once. The same rules
 * as the PC apps' src/lyrics.rs.
 */
object Translate {
    private val http = OkHttpClient.Builder().callTimeout(10, TimeUnit.SECONDS).build()

    fun shownLines(l: Lyrics): List<String> = if (l.synced.isEmpty()) l.plain else l.synced.map { it.second }

    /** Kana (hiragana or katakana) in a fifth of the lines or more. */
    fun isJapanese(l: Lyrics): Boolean {
        val lines = shownLines(l)
        val kana = lines.count { s -> s.any { it in '぀'..'ヿ' } }
        return lines.isNotEmpty() && kana * 5 >= lines.size
    }

    /** Mostly Latin letters (English, not Chinese). */
    fun isEnglish(lines: List<String>): Boolean {
        var latin = 0
        var other = 0
        for (c in lines.joinToString("")) if (c.isLetter()) { if (c in 'a'..'z' || c in 'A'..'Z') latin++ else other++ }
        return latin > 20 && latin > other * 4
    }

    /** Each timed line gets the translation starting within half a second of it (none when most don't line up). */
    fun byTime(lines: List<Pair<Float, String>>, tr: List<Pair<Float, String>>): List<String> {
        val out = lines.map { (at, _) -> tr.firstOrNull { kotlin.math.abs(it.first - at) < 0.5f }?.second.orEmpty() }
        return if (out.count { it.isNotEmpty() } * 2 >= out.size) out else emptyList()
    }

    suspend fun translate(t: Track, l: Lyrics): List<String> {
        if (l.translation.isNotEmpty() || !isJapanese(l)) return l.translation
        if (l.synced.isNotEmpty()) {
            val n = runCatching { LyricSources.netease(t) }.getOrNull()
            if (n != null && n.translation.isNotEmpty()) {
                val lined = byTime(l.synced, n.synced.map { it.first }.zip(n.translation))
                if (lined.isNotEmpty()) return lined
            }
        }
        return machine(shownLines(l)).orEmpty()
    }

    /** Every line from Japanese to English in one request (Google Translate's free endpoint); null unless one for one. */
    suspend fun machine(lines: List<String>): List<String>? = withContext(Dispatchers.IO) {
        fun empty(s: String) = s.isBlank() || s.trim() == "♪"
        val text = lines.joinToString("\n") { if (empty(it)) "-" else it.replace('\n', ' ') }
        val req = Request.Builder()
            .url("https://translate.googleapis.com/translate_a/single?client=gtx&sl=ja&tl=en&dt=t")
            .post(FormBody.Builder().add("q", text).build())
            .build()
        runCatching {
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) return@use null
                val segs = (Json.parseToJsonElement(resp.body?.string().orEmpty()) as JsonArray)[0] as JsonArray
                val joined = segs.joinToString("") { ((it as JsonArray)[0] as? JsonPrimitive)?.content.orEmpty() }
                val out = joined.split('\n').map { it.trim() }
                if (out.size != lines.size) null
                else out.zip(lines).map { (tr, orig) -> if (empty(orig) || tr == "-") "" else tr }
            }
        }.getOrNull()
    }
}
