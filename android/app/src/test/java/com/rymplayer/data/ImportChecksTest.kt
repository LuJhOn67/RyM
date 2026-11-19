package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Only real numbers, known song ids, sane times and clean names come in (as on the PC). */
class ImportChecksTest {
    @Test
    fun badLinesArePassedOver() {
        val n = RamPoints.Notes()
        val now = System.currentTimeMillis() / 1000
        val ahead = now + 10 * 86_400
        val bell = '\u0007'
        val text = "#artist\tqueen\tNaN\tQueen\n#artist\tdaft punk\t12\tDaft${bell}Punk\n" +
            "fJ9rUzIMcZQ\tNaN\t$now\t3\t0\nkJQP7kiw5Fk\tInfinity\t$now\nlYBUbBu4W08\t40\t$ahead\t1\t0\n" +
            "../../etc\t50\t$now\ndQw4w9WgXcQ\t25\t$now\t99999999\t0\nnot a line\n"
        n.mergeScores(text, restore = true)
        assertTrue("no NaN", n.tracks.values.all { it.points.isFinite() })
        assertFalse(n.tracks.containsKey("fJ9rUzIMcZQ"))
        assertFalse(n.tracks.containsKey("kJQP7kiw5Fk"))
        assertFalse(n.tracks.containsKey("lYBUbBu4W08"))
        assertFalse(n.tracks.containsKey("../../etc"))
        assertEquals(25f, n.tracks["dQw4w9WgXcQ"]!!.points)
        assertEquals(100_000, n.tracks["dQw4w9WgXcQ"]!!.plays)
        assertFalse(n.affinity.containsKey("queen"))
        assertEquals("DaftPunk", n.names["daft punk"])
    }
}
