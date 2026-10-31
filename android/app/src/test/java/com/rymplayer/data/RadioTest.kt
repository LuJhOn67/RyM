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
}
