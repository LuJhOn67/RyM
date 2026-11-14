package com.rymplayer.data

import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONArray
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * SponsorBlock (sponsor.ajay.app): the parts of a music video that aren't music (an intro, a
 * talk, an outro), marked by its users as `music_offtopic`, so the player skips them. Asked by the
 * first 4 letters of the id's SHA-256, so the server isn't told which song plays. The same as the
 * PC's `sponsorblock.rs`.
 */
object SponsorBlock {
    private val http = OkHttpClient.Builder().callTimeout(8, TimeUnit.SECONDS).build()
    private val known = ConcurrentHashMap<String, List<Pair<Float, Float>>>()

    private fun prefix(id: String): String =
        MessageDigest.getInstance("SHA-256").digest(id.toByteArray()).take(2).joinToString("") { "%02x".format(it) }

    /** The video's non-music parts as (start, end) seconds; none when it has none or there's no answer. Blocking. */
    fun segments(videoId: String): List<Pair<Float, Float>> {
        if (videoId.isEmpty() || !isYouTube(videoId)) return emptyList()
        known[videoId]?.let { return it }
        val found = runCatching {
            val url = "https://sponsor.ajay.app/api/skipSegments/${prefix(videoId)}?categories=%5B%22music_offtopic%22%5D&actionTypes=%5B%22skip%22%5D"
            http.newCall(Request.Builder().url(url).build()).execute().use { r ->
                if (r.code == 404) emptyList() else if (!r.isSuccessful) error("SponsorBlock ${r.code}") else parse(r.body?.string().orEmpty(), videoId)
            }
        }.getOrDefault(emptyList())
        if (known.size > 500) known.clear()
        known[videoId] = found
        return found
    }

    /** The video's highlight (SponsorBlock's "poi_highlight": where the best part starts, in seconds), or null. Blocking. */
    fun highlight(videoId: String): Float? {
        if (videoId.isEmpty() || !isYouTube(videoId)) return null
        return runCatching {
            val url = "https://sponsor.ajay.app/api/skipSegments/${prefix(videoId)}?categories=%5B%22poi_highlight%22%5D&actionTypes=%5B%22poi%22%5D"
            http.newCall(Request.Builder().url(url).build()).execute().use { r ->
                if (!r.isSuccessful) return@use null
                val list = JSONArray(r.body?.string().orEmpty())
                (0 until list.length()).mapNotNull { list.optJSONObject(it) }.firstOrNull { it.optString("videoID") == videoId }
                    ?.optJSONArray("segments")?.optJSONObject(0)?.optJSONArray("segment")?.optDouble(0)?.toFloat()
            }
        }.getOrNull()
    }

    fun parse(json: String, videoId: String): List<Pair<Float, Float>> {
        val list = runCatching { JSONArray(json) }.getOrNull() ?: return emptyList()
        val out = mutableListOf<Pair<Float, Float>>()
        for (i in 0 until list.length()) {
            val v = list.optJSONObject(i) ?: continue
            if (v.optString("videoID") != videoId) continue
            val segs = v.optJSONArray("segments") ?: continue
            for (j in 0 until segs.length()) {
                val s = segs.optJSONObject(j) ?: continue
                if (s.optString("category") != "music_offtopic") continue
                val at = s.optJSONArray("segment") ?: continue
                val a = at.optDouble(0).toFloat()
                val b = at.optDouble(1).toFloat()
                if (b > a + 1f) out += a to b
            }
        }
        return out.sortedBy { it.first }
    }

    /** The end of the part [pos] is in (where to jump), unless it's within its last half second. */
    fun skipTo(segments: List<Pair<Float, Float>>, pos: Float): Float? =
        segments.firstOrNull { pos >= it.first && pos < it.second - 0.5f }?.second
}
