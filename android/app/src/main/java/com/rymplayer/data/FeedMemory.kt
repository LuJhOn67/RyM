package com.rymplayer.data

import android.content.Context

/**
 * Keeps the home feed fresh. YouTube Music sends much the same home page every time, so items
 * shown twice without being opened rest for a few days and new ones come first; anything opened
 * or played is never rested.
 */
object FeedMemory {
    private const val TIRED_AFTER = 2
    private const val REST_MS = 3L * 24 * 60 * 60 * 1000

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("feed", Context.MODE_PRIVATE)

    fun key(t: Track): String = t.link?.id ?: t.videoId

    /** The feed to show: rested items left out, unseen ones first in each section. Remembers what it showed. */
    fun arrange(ctx: Context, items: List<Track>): List<Track> {
        val p = prefs(ctx)
        val now = System.currentTimeMillis()
        fun shown(k: String) = p.getInt("n:$k", 0)
        fun last(k: String) = p.getLong("t:$k", 0L)
        val edit = p.edit()
        val fresh = items.filter { t ->
            val k = key(t)
            when {
                p.getBoolean("o:$k", false) -> true
                shown(k) < TIRED_AFTER -> true
                // Rested long enough: comes back as if new.
                now - last(k) > REST_MS -> { edit.putInt("n:$k", 0); true }
                else -> false
            }
        }
        // Keep the sections in YouTube's order; inside each, never-shown items first.
        val order = fresh.map { it.album }.distinct()
        val arranged = order.flatMap { section ->
            fresh.filter { it.album == section }.sortedBy { if (shown(key(it)) == 0) 0 else 1 }
        }
        // A section that lost everything keeps what YouTube sent, so the page never empties.
        val result = if (arranged.size < 8) items else arranged
        for (t in result) {
            val k = key(t)
            edit.putInt("n:$k", shown(k) + 1).putLong("t:$k", now)
        }
        edit.apply()
        return result
    }

    /** Opened or played: never rested. */
    fun opened(ctx: Context, t: Track) {
        prefs(ctx).edit().putBoolean("o:${key(t)}", true).apply()
    }
}
