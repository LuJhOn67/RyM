package com.rymplayer.data

import android.content.Context
import org.json.JSONObject

/**
 * Long talks (backlog 48): a recording over 20 minutes (a talk, a lecture, a long recitation)
 * keeps where it was stopped and starts there next time; it also plays at its own speed
 * ([com.rymplayer.playback.Sound.Settings.talkSpeed]), which doesn't touch the songs'. The same
 * as the PC's `talk_place`. The last 200 are kept.
 */
object TalkPlace {
    const val LONG_SECS = 20 * 60f
    private const val EDGE_SECS = 30f
    private const val KEPT = 200

    /** The speeds to pick from. */
    val SPEEDS = listOf(1f, 1.1f, 1.25f, 1.5f, 1.75f, 2f)

    fun isLong(seconds: Float) = seconds > LONG_SECS

    private var written: Pair<String, Float>? = null

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("talk_places", Context.MODE_PRIVATE)

    /** Where [id] ([seconds] long) was stopped, to start there; null for a song or from the top. */
    fun place(ctx: Context, id: String, seconds: Float): Float? {
        if (!isLong(seconds)) return null
        val at = runCatching { JSONObject(prefs(ctx).getString(id, "") ?: "").getDouble("at").toFloat() }.getOrNull() ?: return null
        return at.takeIf { it > EDGE_SECS && it < seconds - EDGE_SECS }
    }

    /** The player is at [at] of [id]: kept every 10 s; near the end it's forgotten. */
    @Synchronized
    fun note(ctx: Context, id: String, at: Float, seconds: Float) {
        if (!isLong(seconds) || id.isEmpty()) return
        val p = prefs(ctx)
        if (at >= seconds - EDGE_SECS) {
            if (p.contains(id)) p.edit().remove(id).apply()
            written = id to at
            return
        }
        val was = written
        if (at <= EDGE_SECS || (was != null && was.first == id && kotlin.math.abs(at - was.second) < 10f)) return
        written = id to at
        val e = p.edit().putString(id, JSONObject().put("at", at.toDouble()).put("when", System.currentTimeMillis()).toString())
        // The oldest go past 200.
        val all = p.all
        if (all.size > KEPT) {
            all.entries.sortedBy { runCatching { JSONObject(it.value as String).getLong("when") }.getOrDefault(0L) }
                .take(all.size - KEPT).forEach { e.remove(it.key) }
        }
        e.apply()
    }
}
