package com.rymplayer.data

import android.content.Context

/**
 * Keeps the home feed fresh. YouTube Music sends much the same home page every time, so items
 * shown twice without being opened rest for a few days and new ones come first; anything opened
 * or played is never rested. A "New for you" shelf on top comes from the radio of a few songs the
 * user liked or played lately (different ones each refresh), leaving out songs they already know.
 */
object FeedMemory {
    const val NEW_FOR_YOU = "New for you"
    private const val TIRED_AFTER = 2
    private const val REST_MS = 3L * 24 * 60 * 60 * 1000
    private const val PICKS = 20

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

    /** Up to [n] seed songs at random, skipping ones used for the last few refreshes. */
    fun pickSeeds(ctx: Context, pool: List<Track>, n: Int): List<String> {
        val p = prefs(ctx)
        val used = p.getString("seeds", "").orEmpty().split(',').filter { it.isNotEmpty() }.toMutableList()
        val ids = pool.map { it.videoId }.filter { it.isNotEmpty() }.distinct()
        var candidates = ids.filter { it !in used }
        if (candidates.size < n) {
            // Everything was used lately: start over.
            used.clear()
            candidates = ids
        }
        val picked = candidates.shuffled().take(n)
        used += picked
        p.edit().putString("seeds", used.takeLast(60).joinToString(",")).apply()
        return picked
    }

    /** Songs from the radios, taken in turns, that the user doesn't know and wasn't shown lately. */
    fun pickNew(ctx: Context, radios: List<List<Track>>, known: Set<String>): List<Track> {
        val p = prefs(ctx)
        val now = System.currentTimeMillis()
        val out = mutableListOf<Track>()
        val iters = radios.map { it.iterator() }.toMutableList()
        while (out.size < PICKS && iters.isNotEmpty()) {
            val each = iters.iterator()
            while (each.hasNext() && out.size < PICKS) {
                val it = each.next()
                if (!it.hasNext()) { each.remove(); continue }
                val t = it.next()
                val k = t.videoId
                val recent = p.getInt("n:$k", 0) > 0 && now - p.getLong("t:$k", 0L) < REST_MS
                if (k !in known && !recent && out.none { o -> o.videoId == k }) out += t.copy(album = NEW_FOR_YOU)
            }
        }
        val edit = p.edit()
        for (t in out) edit.putInt("n:${t.videoId}", p.getInt("n:${t.videoId}", 0) + 1).putLong("t:${t.videoId}", now)
        edit.apply()
        return out
    }
}
