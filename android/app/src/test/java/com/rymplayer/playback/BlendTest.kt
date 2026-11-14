package com.rymplayer.playback

import com.rymplayer.data.Track
import org.junit.Assert.assertEquals
import org.junit.Test

/** Mix Tastes: the same case as the PC's link::blend test. */
class BlendTest {
    private fun t(id: String, title: String) = Track(id, title, "A")

    @Test
    fun takesTurnsAndSkipsRepeats() {
        val mixed = Link.blend(listOf(t("a1", "One"), t("a2", "Two"), t("a3", "Three")), listOf(t("b1", "Four"), t("a2", "Two"), t("b9", "One")))
        assertEquals(listOf("a1", "b1", "a2", "a3"), mixed.map { it.videoId })
    }
}
