package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/**
 * Japanese lyrics in Latin letters (romaji), small under each line: Google Translate's reading of
 * the lines (it reads kanji too); when that fails, kana are turned by a table (kanji stay as they
 * are). The same as the PC apps' src/romaji.rs.
 */
object Romaji {
    private val http = OkHttpClient.Builder().callTimeout(10, TimeUnit.SECONDS).build()

    fun hasKana(s: String) = s.any { it in '぀'..'ヿ' }

    private fun isJapanese(c: Char) = c in '぀'..'ヿ' || c in '一'..'鿿'

    /** Each line in romaji (empty for a line without Japanese): one request for them all. */
    suspend fun romanize(lines: List<String>): List<String> = reading(lines) ?: lines.map { if (hasKana(it)) kana(it) else "" }

    /** Google Translate's reading (dt=rm) of every line at once; lines stay lines. */
    private suspend fun reading(lines: List<String>): List<String>? = withContext(Dispatchers.IO) {
        runCatching {
            val text = lines.joinToString(" | ") { l -> if (l.any(::isJapanese)) l.replace('\n', ' ').replace('|', ' ') else "-" }
            val req = Request.Builder()
                .url("https://translate.googleapis.com/translate_a/single?client=gtx&sl=ja&tl=en&dt=rm")
                .post(FormBody.Builder().add("q", text).build())
                .build()
            val body = http.newCall(req).execute().use { it.body?.string().orEmpty() }
            // [[[null, null, null, "the reading"], ...], ...]: the source's reading is the fourth item.
            val segs = (Json.parseToJsonElement(body) as JsonArray)[0] as JsonArray
            val joined = segs.joinToString("") { seg -> ((seg as? JsonArray)?.getOrNull(3) as? JsonPrimitive)?.contentOrNull.orEmpty() }
            val out = joined.split('|').map { it.trim() }
            if (out.size != lines.size) return@runCatching null
            out.zip(lines).map { (r, orig) -> if (orig.any(::isJapanese) && r != "-") r else "" }
        }.getOrNull()
    }

    /** Kana to romaji by table (Hepburn); anything else is kept. */
    fun kana(s: String): String {
        val chars = s.map(::toHiragana)
        val out = StringBuilder()
        var i = 0
        var double = false
        while (i < chars.size) {
            val c = chars[i]
            if (c == 'っ') {
                double = true
                i++
                continue
            }
            if (c == 'ー') {
                out.lastOrNull()?.takeIf { it in "aeiou" }?.let { out.append(it) }
                i++
                continue
            }
            // A syllable with a small ya / yu / yo after it (きゃ -> kya).
            val pair = chars.subList(i, minOf(chars.size, i + 2)).joinToString("")
            val combo = COMBO[pair]
            val (romaji, used) = if (combo != null) combo to 2 else (SINGLE[c] ?: c.toString()) to 1
            val first = romaji.firstOrNull()
            if (double && first != null && first.isLetter() && first.code < 128 && first !in "aeiou") out.append(if (romaji.startsWith("ch")) 't' else first)
            double = false
            out.append(romaji)
            i += used
        }
        return out.toString()
    }

    private fun toHiragana(c: Char): Char = if (c in 'ァ'..'ヶ') (c.code - 0x60).toChar() else c

    private val COMBO = mapOf(
        "きゃ" to "kya", "きゅ" to "kyu", "きょ" to "kyo", "しゃ" to "sha", "しゅ" to "shu", "しょ" to "sho",
        "ちゃ" to "cha", "ちゅ" to "chu", "ちょ" to "cho", "にゃ" to "nya", "にゅ" to "nyu", "にょ" to "nyo",
        "ひゃ" to "hya", "ひゅ" to "hyu", "ひょ" to "hyo", "みゃ" to "mya", "みゅ" to "myu", "みょ" to "myo",
        "りゃ" to "rya", "りゅ" to "ryu", "りょ" to "ryo", "ぎゃ" to "gya", "ぎゅ" to "gyu", "ぎょ" to "gyo",
        "じゃ" to "ja", "じゅ" to "ju", "じょ" to "jo", "びゃ" to "bya", "びゅ" to "byu", "びょ" to "byo",
        "ぴゃ" to "pya", "ぴゅ" to "pyu", "ぴょ" to "pyo", "てぃ" to "ti", "でぃ" to "di", "ふぁ" to "fa",
        "ふぃ" to "fi", "ふぇ" to "fe", "ふぉ" to "fo", "うぃ" to "wi", "うぇ" to "we", "しぇ" to "she",
        "ちぇ" to "che", "じぇ" to "je",
    )

    private val SINGLE = mapOf(
        'あ' to "a", 'い' to "i", 'う' to "u", 'え' to "e", 'お' to "o",
        'か' to "ka", 'き' to "ki", 'く' to "ku", 'け' to "ke", 'こ' to "ko",
        'さ' to "sa", 'し' to "shi", 'す' to "su", 'せ' to "se", 'そ' to "so",
        'た' to "ta", 'ち' to "chi", 'つ' to "tsu", 'て' to "te", 'と' to "to",
        'な' to "na", 'に' to "ni", 'ぬ' to "nu", 'ね' to "ne", 'の' to "no",
        'は' to "ha", 'ひ' to "hi", 'ふ' to "fu", 'へ' to "he", 'ほ' to "ho",
        'ま' to "ma", 'み' to "mi", 'む' to "mu", 'め' to "me", 'も' to "mo",
        'や' to "ya", 'ゆ' to "yu", 'よ' to "yo",
        'ら' to "ra", 'り' to "ri", 'る' to "ru", 'れ' to "re", 'ろ' to "ro",
        'わ' to "wa", 'を' to "wo", 'ん' to "n",
        'が' to "ga", 'ぎ' to "gi", 'ぐ' to "gu", 'げ' to "ge", 'ご' to "go",
        'ざ' to "za", 'じ' to "ji", 'ず' to "zu", 'ぜ' to "ze", 'ぞ' to "zo",
        'だ' to "da", 'ぢ' to "ji", 'づ' to "zu", 'で' to "de", 'ど' to "do",
        'ば' to "ba", 'び' to "bi", 'ぶ' to "bu", 'べ' to "be", 'ぼ' to "bo",
        'ぱ' to "pa", 'ぴ' to "pi", 'ぷ' to "pu", 'ぺ' to "pe", 'ぽ' to "po",
        'ぁ' to "a", 'ぃ' to "i", 'ぅ' to "u", 'ぇ' to "e", 'ぉ' to "o",
        'ゃ' to "ya", 'ゅ' to "yu", 'ょ' to "yo", 'ゔ' to "vu",
        '　' to " ", '、' to ", ", '。' to ". ", '「' to "\"", '」' to "\"",
    )
}
