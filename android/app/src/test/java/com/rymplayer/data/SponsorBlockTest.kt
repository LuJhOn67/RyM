package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SponsorBlockTest {
    @Test
    fun picksThisVideosMusicOfftopicParts() {
        val json = """[
            {"videoID":"other","segments":[{"category":"music_offtopic","segment":[0.0,30.0]}]},
            {"videoID":"abc","segments":[
                {"category":"music_offtopic","segment":[200.0,240.5]},
                {"category":"sponsor","segment":[50.0,60.0]},
                {"category":"music_offtopic","segment":[0.0,12.0]}
            ]}
        ]"""
        val s = SponsorBlock.parse(json, "abc")
        assertEquals(listOf(0f to 12f, 200f to 240.5f), s)
        assertEquals(12f, SponsorBlock.skipTo(s, 3f))
        assertNull(SponsorBlock.skipTo(s, 11.8f))
        assertNull(SponsorBlock.skipTo(s, 100f))
    }
}
