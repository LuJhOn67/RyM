package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RomajiTest {
    @Test
    fun kanaByTable() {
        // The same cases as the PC's romaji.rs.
        assertEquals("kiminonamae", Romaji.kana("きみのなまえ"))
        assertEquals("tookyoo", Romaji.kana("トーキョー"))
        assertEquals("kitte", Romaji.kana("きって"))
        assertEquals("matcha", Romaji.kana("まっちゃ"))
        assertEquals("夜nikakeru", Romaji.kana("夜にかける"))
        assertTrue(Romaji.hasKana("夜に駆ける") && !Romaji.hasKana("夜駆"))
    }
}
