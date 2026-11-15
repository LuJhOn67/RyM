package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same case as the PC's reset_sync_tests: a reset reaches the other device. */
class ResetSyncTest {
    private fun score(points: Float, lastSecs: Long) = RamPoints.Score(points, lastSecs * 1000, 1, 0L, IntArray(4))

    @Test
    fun aResetReachesTheOtherDevice() {
        val phone = RamPoints.Notes()
        phone.tracks["old"] = score(30f, 500)
        phone.tracks["later"] = score(5f, 1200)
        phone.affinity["someone"] = 9f
        val pc = RamPoints.Notes()
        pc.resetAt = 1000_000
        pc.tracks["new"] = score(4f, 1100)
        pc.mergeScores(phone.export())
        assertFalse("old" in pc.tracks)
        assertTrue("later" in pc.tracks && "new" in pc.tracks)
        phone.mergeScores(pc.export())
        assertEquals(1000_000L, phone.resetAt)
        assertFalse("old" in phone.tracks)
        assertTrue(phone.affinity.isEmpty())
    }
}
