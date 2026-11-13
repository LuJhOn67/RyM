package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

/** The same song under another upload is known (backlog 58), as on the PC. */
class RepeatsTest {
    private fun t(id: String, title: String, artist: String) = Track(videoId = id, title = title, artist = artist)

    @Test
    fun anotherUploadIsTheSameSong() {
        val k = Radio.sameSongKey(t("a", "Bohemian Rhapsody", "Queen"))
        for (other in listOf("Queen - Bohemian Rhapsody (Official Video)", "Bohemian Rhapsody - Remastered 2011", "Bohemian Rhapsody | Lyrics", "Bohemian Rhapsody ft. Someone")) {
            assertEquals(other, k, Radio.sameSongKey(t("b", other, "Queen Official")))
        }
        assertNotEquals(k, Radio.sameSongKey(t("b", "Bohemian Rhapsody - Live Aid", "Queen")))
        val left = Radio.withoutRepeats(listOf(t("a", "Get Lucky", "Daft Punk")), listOf(t("b", "Get Lucky (Official Audio)", "Daft Punk - Topic"), t("c", "One More Time", "Daft Punk")))
        assertEquals(listOf("c"), left.map { it.videoId })
    }
}
