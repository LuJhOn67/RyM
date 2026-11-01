package com.rymplayer.data

import android.content.Context

/**
 * Songs played on this phone, newest first, with how often each was played: the home page's
 * "Recently played" row, and the seeds and favourites its mix is made from.
 */
object Recent {
    private const val KEEP = 300

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("recent", Context.MODE_PRIVATE)

    /** The songs, newest first. */
    fun tracks(ctx: Context): List<Track> = PageCache.loadTracks(ctx, "recent")

    /** A song started playing: it goes to the front, its count goes up. */
    fun played(ctx: Context, t: Track) {
        if (t.link != null || t.videoId.isEmpty()) return
        val list = tracks(ctx).filter { it.videoId != t.videoId }.toMutableList()
        list.add(0, t.copy(album = ""))
        val kept = list.take(KEEP)
        PageCache.saveTracks(ctx, "recent", kept)
        val p = prefs(ctx)
        val edit = p.edit().putInt("n:${t.videoId}", p.getInt("n:${t.videoId}", 0) + 1).putLong("t:${t.videoId}", System.currentTimeMillis())
        // Counts of songs that fell off the list go too.
        for (gone in list.drop(KEEP)) edit.remove("n:${gone.videoId}").remove("t:${gone.videoId}")
        edit.apply()
    }

    /** The most played songs (played more than once; ties: the more recent first). */
    fun favourites(ctx: Context, n: Int): List<Track> {
        val p = prefs(ctx)
        return tracks(ctx).filter { p.getInt("n:${it.videoId}", 0) > 1 }
            .sortedWith(compareByDescending<Track> { p.getInt("n:${it.videoId}", 0) }.thenByDescending { p.getLong("t:${it.videoId}", 0L) })
            .take(n)
    }

    // ------------------------------------------------------------ listening stats

    /** Time listened since the last write: artist -> ms (written once a minute, not every second). */
    private val pending = HashMap<String, Long>()

    private fun stats(ctx: Context) = ctx.getSharedPreferences("stats", Context.MODE_PRIVATE)

    fun month(now: Long = System.currentTimeMillis()): String =
        java.text.SimpleDateFormat("yyyy-MM", java.util.Locale.US).apply { timeZone = java.util.TimeZone.getTimeZone("UTC") }.format(java.util.Date(now))

    @Synchronized
    fun listened(t: Track, ms: Long) {
        if (ms <= 0 || ms > 6 * 60 * 60 * 1000L) return
        val artist = t.artist.split(',', '&').first().trim()
        pending[artist] = (pending[artist] ?: 0L) + ms
    }

    @Synchronized
    fun flushListening(ctx: Context) {
        if (pending.isEmpty()) return
        val p = stats(ctx)
        val m = month()
        val edit = p.edit()
        var total = p.getLong("total", 0L)
        var monthMs = p.getLong("m:$m", 0L)
        for ((artist, ms) in pending) {
            total += ms
            monthMs += ms
            if (artist.isNotEmpty()) edit.putLong("a:$m:$artist", p.getLong("a:$m:$artist", 0L) + ms)
        }
        edit.putLong("total", total).putLong("m:$m", monthMs).apply()
        pending.clear()
    }

    /** "Your recap": hours this month and in all, the most played songs (with plays), the top artists this month (with hours). */
    data class Recap(val hoursMonth: Float, val hoursTotal: Float, val topSongs: List<Pair<Track, Int>>, val topArtists: List<Pair<String, Float>>)

    fun recap(ctx: Context): Recap {
        flushListening(ctx)
        val p = stats(ctx)
        val m = month()
        val counts = prefs(ctx)
        val songs = tracks(ctx).map { it to counts.getInt("n:${it.videoId}", 0) }.filter { it.second > 0 }.sortedByDescending { it.second }.take(10)
        val prefix = "a:$m:"
        val artists = p.all.filterKeys { it.startsWith(prefix) }.map { (k, v) -> k.removePrefix(prefix) to ((v as? Long ?: 0L) / 3_600_000f) }.sortedByDescending { it.second }.take(10)
        return Recap(p.getLong("m:$m", 0L) / 3_600_000f, p.getLong("total", 0L) / 3_600_000f, songs, artists)
    }
}
