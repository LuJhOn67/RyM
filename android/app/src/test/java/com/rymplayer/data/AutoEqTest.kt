package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Test

/** The same cases as the PC's autoeq tests. */
class AutoEqTest {
    @Test
    fun indexLines() {
        val list = AutoEq.parseIndex("# Index\n- [Sony WH-1000XM4](./oratory1990/over-ear/Sony%20WH-1000XM4) by oratory1990\n- [Sony WH-1000XM4 (ANC on)](./HypetheSonics/over-ear/Sony%20WH-1000XM4%20(ANC%20on)) by HypetheSonics on GRAS\n")
        assertEquals(2, list.size)
        assertEquals("oratory1990", list[0].by)
        assertEquals("HypetheSonics/over-ear/Sony%20WH-1000XM4%20(ANC%20on)", list[1].path)
    }

    @Test
    fun aCurveFittedToFiveBands() {
        val tilt = listOf(50f to 4f, 60f to 4f, 70f to 4f, 200f to 2f, 230f to 2f, 260f to 2f, 800f to 0f, 910f to 0f, 1000f to 0f, 3500f to -2f, 3600f to -2f, 3700f to -2f, 13000f to -4f, 14000f to -4f, 15000f to -4f)
        assertEquals(listOf(4f, 2f, 0f, -2f, -4f), AutoEq.fit(tilt))
    }
}
