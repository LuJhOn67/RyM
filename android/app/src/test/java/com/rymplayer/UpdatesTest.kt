package com.rymplayer

import com.rymplayer.data.Plugins
import com.rymplayer.data.Updates
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdatesTest {
    @Test
    fun versionsCompareByNumber() {
        assertEquals(listOf(6, 7), Updates.parse("v6.7.0"))
        assertTrue(Updates.newer("6.10", "6.9"))
        assertFalse(Updates.newer("6.7.0", "6.7"))
        assertFalse(Updates.newer("6.5", "6.7"))
    }

    @Test
    fun betasOnlyOnTheBetaChannel() {
        val list = Json.parseToJsonElement(
            """[
              {"tag_name":"v6.9","prerelease":true,"draft":false,"name":"RyM 6.9","body":"beta","html_url":"p9","assets":[]},
              {"tag_name":"v6.8","prerelease":false,"draft":false,"name":"RyM 6.8","body":"## RyM 6.8\n- **New**","html_url":"p8",
               "assets":[{"name":"RyM-Android-6.8.apk","browser_download_url":"a8"}]},
              {"tag_name":"android-v9.0","prerelease":false,"draft":false,"name":"old","body":"","html_url":"p","assets":[]}
            ]""",
        ).jsonArray
        val stable = Updates.pick(list, beta = false, current = "6.7")!!
        assertEquals("6.8", stable.version)
        assertEquals("a8", stable.apk)
        assertEquals("RyM 6.8\n- New", stable.plainNotes)
        assertEquals("6.9", Updates.pick(list, beta = true, current = "6.7")!!.version)
        assertNull(Updates.pick(list, beta = true, current = "6.9"))
    }

    @Test
    fun pluginListForThePhone() {
        val list = Plugins.parse("""{"plugins":[{"id":"video","name":"Video","version":"1.0.0","platforms":["android"]},{"id":"pc","name":"PC only","version":"1","platforms":["windows"]}]}""")
        assertEquals(listOf("video"), list.map { it.id })
    }
}
