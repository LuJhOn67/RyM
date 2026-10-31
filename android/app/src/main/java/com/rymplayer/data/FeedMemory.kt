package com.rymplayer.data

import android.content.Context

/**
 * Keeps the home feed fresh (the same rules as the PC apps' src/feed.rs). YouTube Music sends much
 * the same home page every time, so items seen on three different days (or so) without being
 * opened rest for two days and new ones come first; opening the app again and again counts once
 * per six hours; when too much rests, the items seen longest ago come back so the page stays
 * full; anything opened or played is never rested. A "New for you" shelf on top comes from the
 * radio of a few songs the user played lately or liked (different ones each refresh), leaving out
 * songs they already know, the same song under another upload, and more than two by one artist.
 */
object FeedMemory {
    const val NEW_FOR_YOU = "New for you"
    private const val TIRED_AFTER = 3
    private const val REST_MS = 2L * 24 * 60 * 60 * 1000
    /** Showings closer together than this count once. */
    private const val SHOW_GAP_MS = 6L * 60 * 60 * 1000
    /** The page keeps at least this many items. */
    private const val AT_LEAST = 40
    const val PICKS = 30
    const val SEEDS = 5
    private const val PER_ARTIST = 2

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
        // Too much rests: the items seen longest ago come back (in YouTube's order).
        val result = if (arranged.size >= AT_LEAST) arranged else {
            val have = arranged.map(::key).toSet()
            val back = items.filter { key(it) !in have }.sortedBy { last(key(it)) }.take(AT_LEAST - arranged.size).map(::key).toSet()
            items.filter { key(it) in have || key(it) in back }
        }
        for (t in result) {
            val k = key(t)
            // Opening the app again and again soon after counts as one showing.
            val n = if (shown(k) == 0 || now - last(k) >= SHOW_GAP_MS) shown(k) + 1 else shown(k)
            edit.putInt("n:$k", n).putLong("t:$k", now)
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
        val songs = mutableSetOf<String>()
        val perArtist = mutableMapOf<String, Int>()
        val iters = radios.map { it.iterator() }.toMutableList()
        while (out.size < PICKS && iters.isNotEmpty()) {
            val each = iters.iterator()
            while (each.hasNext() && out.size < PICKS) {
                val it = each.next()
                if (!it.hasNext()) { each.remove(); continue }
                val t = it.next()
                val k = t.videoId
                val recent = p.getInt("n:$k", 0) > 0 && now - p.getLong("t:$k", 0L) < REST_MS
                val artist = Radio.firstArtist(t.artist)
                val crowded = artist.isNotEmpty() && (perArtist[artist] ?: 0) >= PER_ARTIST
                if (k !in known && !recent && !crowded && out.none { o -> o.videoId == k } && songs.add(Radio.sameSongKey(t))) {
                    perArtist[artist] = (perArtist[artist] ?: 0) + 1
                    out += t.copy(album = NEW_FOR_YOU)
                }
            }
        }
        val edit = p.edit()
        for (t in out) edit.putInt("n:${t.videoId}", p.getInt("n:${t.videoId}", 0) + 1).putLong("t:${t.videoId}", now)
        edit.apply()
        return out
    }
}
