package com.rymplayer.playback

import com.rymplayer.data.Track
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LinkTest {
    @Test
    fun theSameKeyAsThePc() {
        // src/link.rs's test gives this for the same account.
        assertEquals("b3827702b3d0f98e", Link.accountKey("Test", "@test"))
    }

    @Test
    fun notesAndState() {
        Link.account = null
        Link.snapshot = null
        assertNull("nothing is sent while signed out", Link.note("Pixel", 5000))
        Link.account = "b3827702b3d0f98e"
        val songs = listOf(Track("aaaaaaaaaaa", "A", "X"), Track("bbbbbbbbbbb", "B", "Y"))
        Link.snapshot = Link.Snapshot(songs, 1, 30_000, System.currentTimeMillis() - 2_000, playing = true, speed = 1f)
        val note = Link.note("Pixel", 5000).toString()
        assertTrue(note, "\"playing\":true" in note && "\"video_id\":\"bbbbbbbbbbb\"" in note && "\"port\":5000" in note)
        val state = Link.state().toString()
        assertTrue(state, "\"current\":1" in state && "\"video_id\":\"aaaaaaaaaaa\"" in state)
        // Two seconds playing since the snapshot: about 32 s in.
        assertEquals(32f, Link.snapshot!!.position(), 0.3f)
    }
}
