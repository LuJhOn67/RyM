package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FeedAndLyricsTest {
    private fun t(id: String, artist: String = id, title: String = id) = Track(id, title, artist)

    @Test
    fun mixTakesThePoolsInTurnsWithoutRepeats() {
        val pool = { p: String, n: Int -> (0 until n).map { t("$p$it") } }
        val got = FeedMemory.mix(pool("f", 10), pool("n", 10), pool("a", 10), setOf("f0"), 20, FeedMemory.FOR_YOU)
        assertEquals(20, got.size)
        assertTrue(got.all { it.album == FeedMemory.FOR_YOU && it.videoId != "f0" })
        val count = { p: Char -> got.count { it.videoId.first() == p } }
        assertTrue(got.toString(), count('f') in 6..8 && count('n') in 5..7 && count('a') in 6..8)
        // A pool that runs out leaves its turns to the others; one artist at most three times.
        val same = (0 until 10).map { t("b$it", artist = "Band") }
        val other = FeedMemory.mix(same, emptyList(), pool("a", 10), emptySet(), 10, FeedMemory.FOR_YOU)
        assertEquals(3, other.count { it.artist == "Band" })
        assertEquals(10, other.size)
    }

    @Test
    fun creditsHeadersAndGeniusText() {
        val song = Track("x", "The Less I Know The Better", "Tame Impala", duration = "3:38")
        val lrc = "[00:00.00] 作词 : Kevin Parker\n[00:01.00]作曲：Kevin\n[00:02.00]The Less I Know The Better - Tame Impala\n[00:12.47]Written by：Kevin\n[00:25.05]Someone said they left together\n[00:29.18]I ran out the door to get her"
        val synced = Lrclib.parseLrc(lrc)
        val clean = LyricSources.clean(Lyrics(synced, synced.map { it.second }, LyricSources.NETEASE), song)
        assertEquals(listOf("Someone said they left together", "I ran out the door to get her"), clean.synced.map { it.second })
        assertEquals(LyricSources.LRCLIB, LyricSources.provider(Lyrics(emptyList(), emptyList(), "LRCLIB 2")))
        assertEquals(LyricSources.YOUTUBE, LyricSources.provider(Lyrics(emptyList(), emptyList(), "LyricFind")))
        val html = """<div data-lyrics-container="true" class="a"><div data-exclude-from-selection="true">185 Contributors<div>x</div></div>[Verse 1]<br/>Someone said &amp; <i>left</i><br/>I ran</div><p>no</p><div data-lyrics-container="true">Last line</div>"""
        assertEquals(listOf("[Verse 1]", "Someone said & left", "I ran", "Last line"), LyricSources.geniusLines(html))
    }
}
