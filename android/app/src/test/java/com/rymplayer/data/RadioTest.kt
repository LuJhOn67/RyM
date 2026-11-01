package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Test

class RadioTest {
    private fun t(id: String, title: String, artist: String) = Track(id, title, artist)

    @Test
    fun upNextHasNoRepeatsNorPlayedSongsAndTheSameArtistMovesUp() {
        val seed = t("s", "Seed", "Band")
        val radio = listOf(
            t("s", "Seed", "Band"),
            t("a", "Song A", "Other"),
            t("b", "Song B", "Band"),
            t("c", "Heard", "Other"),
            t("a2", "Song A (Official Video)", "Other"),
            t("e", "Song E", "Third"),
        )
        val out = Radio.tidy(radio, seed, played = setOf("c"), queued = emptySet(), sessionArtists = emptySet())
        assertEquals(listOf("b", "a", "e"), out.map { it.videoId })
    }

    @Test
    fun lyricsSourcesKeepOtherTimingsAndNumberLrclib() {
        val words = listOf("one", "two", "three", "four")
        val early = Lyrics(words.mapIndexed { i, w -> i * 2f to w }, words, "LRCLIB")
        val late = Lyrics(words.mapIndexed { i, w -> i * 2f + 1.5f to w }, words, "LRCLIB")
        val plain = Lyrics(emptyList(), words, "lyrics.ovh")
        val out = Lrclib.distinct(listOf(early, early.copy(), late, plain, plain.copy(source = "YouTube Music")))
        assertEquals(listOf("LRCLIB", "LRCLIB 2", "lyrics.ovh"), out.map { it.source })
    }

    @Test
    fun likedSongsJoinTheRadioSameArtistFirst() {
        val seed = t("blue", "Blue Hair", "TV Girl")
        val radio = (0 until 6).map { t("r$it", "R$it", if (it % 2 == 0) "Mac DeMarco" else "Other") }
        val liked = listOf(t("x", "X", "Far Away"), t("tv1", "Tv1", "TV Girl"), t("blue", "Blue Hair", "TV Girl"), t("played", "P", "TV Girl"))
        val out = Radio.withLiked(radio, seed, liked, setOf("played")).map { it.videoId }
        assertEquals(listOf("r0", "r1", "tv1", "r2", "r3"), out.take(5))
        assertEquals(8, out.size)
        assertEquals(setOf("x"), out.toSet() - radio.map { it.videoId }.toSet() - "tv1")
    }
}
