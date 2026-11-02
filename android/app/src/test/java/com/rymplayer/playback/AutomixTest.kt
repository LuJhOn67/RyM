package com.rymplayer.playback

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs
import kotlin.math.roundToInt

/** The same cases as the PC apps' automix tests. */
class AutomixTest {
    /** Clicks at [bpm] over a low hum, [secs] long, the first one [offset] blocks in. */
    private fun clicks(bpm: Float, secs: Int, offset: Int): FloatArray {
        val period = 60f * Automix.BLOCKS / bpm
        val env = FloatArray(secs * Automix.BLOCKS) { 0.02f }
        var at = offset.toFloat()
        while (at.roundToInt() < env.size) {
            val i = at.roundToInt()
            floatArrayOf(0.5f, 0.3f, 0.15f).forEachIndexed { k, v -> if (i + k < env.size) env[i + k] = v }
            at += period
        }
        return env
    }

    @Test
    fun tempoPhaseAndNudge() {
        for (bpm in listOf(90f, 120f, 128f, 150f)) {
            val b = Automix.beat(clicks(bpm, 8, 13))
            assertNotNull("$bpm", b)
            assertTrue("$bpm: found ${b!!.bpm}", abs(b.bpm - bpm) < bpm * 0.02f)
            val lastAt = (8 * Automix.BLOCKS - 2) - b.last
            val beats = (lastAt - 13f) / b.period
            assertTrue("$bpm: phase", abs(beats - beats.roundToInt()) < 0.25f)
        }
        assertNull(Automix.beat(FloatArray(800) { 0.1f }))
        assertEquals(1f, Automix.nudge(124f, 124f)!!, 1e-6f)
        assertNotNull("half time", Automix.nudge(124f, 62.5f))
        assertNull(Automix.nudge(124f, 110f))
    }

    @Test
    fun cueOutAndBassCut() {
        val loud = FloatArray(1200) { 0.2f }
        assertFalse(Automix.fadedOut(loud))
        assertFalse("300 ms", Automix.fadedOut(loud + FloatArray(30) { 0.02f }))
        assertTrue(Automix.fadedOut(loud + FloatArray(45) { 0.02f }))
        assertFalse("quiet all along", Automix.fadedOut(FloatArray(1500) { 0.03f }))
        assertEquals(0f, Automix.bassCut(0f), 0f)
        assertEquals(Automix.BASS_HZ, Automix.bassCut(1f), 0.01f)
        val (out, inc) = Automix.gains(0.5f)
        assertEquals(1f, out * out + inc * inc, 1e-5f)
    }

    @Test
    fun meterFindsTheNextBeat() {
        val m = Automix.Meter()
        clicks(120f, 10, 7).forEach(m::push)
        m.anchorMs = 30_000
        // Beats every 500 ms from 30 070 ms; the next after 34 000 + 150 is 34 570.
        val (at, b) = m.nextBeatMs(34_000, 150)!!
        assertEquals(120f, b.bpm, 2f)
        assertEquals(34_570f, at.toFloat(), 30f)
    }
}
