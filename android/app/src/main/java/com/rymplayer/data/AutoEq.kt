package com.rymplayer.data

import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * AutoEq (the same as the PC's autoeq.rs): measured corrections for thousands of headphones,
 * fitted to RyM's 5-band equalizer (each band: the curve's average within half an octave,
 * centred, within ±12 dB).
 */
object AutoEq {
    private const val BASE = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results"
    private val http by lazy { OkHttpClient.Builder().callTimeout(20, java.util.concurrent.TimeUnit.SECONDS).build() }

    data class Headphone(val name: String, val by: String, val path: String)

    @Volatile private var index: List<Headphone>? = null

    private fun get(url: String): String? = runCatching {
        http.newCall(Request.Builder().url(url).build()).execute().use { if (it.isSuccessful) it.body?.string() else null }
    }.getOrNull()

    fun parseIndex(text: String): List<Headphone> = text.lineSequence().mapNotNull { line ->
        val l = line.trim().removePrefix("- [").takeIf { it != line.trim() } ?: return@mapNotNull null
        val name = l.substringBefore("](./", "").takeIf { it.isNotEmpty() } ?: return@mapNotNull null
        val rest = l.substringAfter("](./")
        val at = rest.lastIndexOf(") by ")
        val path = if (at >= 0) rest.substring(0, at) else rest.trimEnd().removeSuffix(")")
        val by = if (at >= 0) rest.substring(at + 5).substringBefore(" on ").trim() else ""
        Headphone(name, by, path)
    }.toList()

    /** The headphones whose name has every word typed, oratory1990's first, each name once. Blocking. */
    fun search(query: String, n: Int = 6): List<Headphone> {
        val list = index ?: get("$BASE/INDEX.md")?.let(::parseIndex)?.also { index = it } ?: return emptyList()
        val words = query.lowercase().split(' ').filter { it.isNotBlank() }
        if (words.isEmpty()) return emptyList()
        return list.filter { h -> words.all { h.name.lowercase().contains(it) } }
            .sortedWith(compareBy({ it.by != "oratory1990" }, { it.name.length }))
            .distinctBy { it.name }.take(n)
    }

    /** The headphones' correction as the 5 band gains, or null. Blocking. */
    fun bands(h: Headphone): List<Float>? {
        val file = h.path.substringAfterLast('/')
        val text = get("$BASE/${h.path}/$file%20GraphicEQ.txt") ?: return null
        val points = text.trim().removePrefix("GraphicEQ:").split(';').mapNotNull { p ->
            val parts = p.trim().split(' ').filter { it.isNotEmpty() }
            val f = parts.getOrNull(0)?.toFloatOrNull() ?: return@mapNotNull null
            val g = parts.getOrNull(1)?.toFloatOrNull() ?: return@mapNotNull null
            f to g
        }
        if (points.isEmpty()) return null
        return fit(points)
    }

    fun fit(points: List<Pair<Float, Float>>): List<Float> {
        val raw = com.rymplayer.playback.Sound.BANDS.map { centre ->
            val near = points.filter { (f, _) -> kotlin.math.abs(kotlin.math.log2(f / centre)) <= 0.5f }.map { it.second }
            if (near.isEmpty()) 0f else near.average().toFloat()
        }
        val mean = raw.average().toFloat()
        return raw.map { kotlin.math.round((it - mean).coerceIn(-12f, 12f) * 2f) / 2f }
    }
}
