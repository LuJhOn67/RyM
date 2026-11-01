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
}
