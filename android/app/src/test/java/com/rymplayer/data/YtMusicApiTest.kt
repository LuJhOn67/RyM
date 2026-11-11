package com.rymplayer.data

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The same rules as the PC apps' ytmusicapi tests. */
class YtMusicApiTest {
    private suspend fun run(new: suspend () -> List<Int>, builtin: List<Int>): List<Int> =
        YtMusicApi.hybrid("test", { it.isNotEmpty() }, new) { builtin }

    @Test
    fun autoFallsBack() = runBlocking {
        YtMusicApi.useEngine(YtMusicApi.Engine.Auto)
        assertEquals(listOf(1), run({ error("parser broke") }, listOf(1)))
        assertEquals("an empty answer counts as a failure", listOf(2), run({ emptyList() }, listOf(2)))
        assertEquals(listOf(3), run({ listOf(3) }, listOf(4)))
        YtMusicApi.useEngine(YtMusicApi.Engine.Builtin)
        assertEquals("rollback skips ytmusicapi", listOf(4), run({ listOf(3) }, listOf(4)))
        YtMusicApi.useEngine(YtMusicApi.Engine.YtMusicApi)
        assertTrue("forced: no fallback", runCatching { run({ error("down") }, listOf(4)) }.isFailure)
        YtMusicApi.useEngine(YtMusicApi.Engine.Auto)
    }

    /** The phone's copy of the bridge is the PC's (assets/ytm_bridge.py at the repo's root). */
    @Test
    fun bridgeMatchesThePc() {
        val phone = File("src/main/assets/ytm_bridge.py").readText().replace("\r\n", "\n")
        val pc = File("../../assets/ytm_bridge.py").readText().replace("\r\n", "\n")
        assertEquals(pc, phone)
    }
}
