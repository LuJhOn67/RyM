package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemesTest {
    @Test
    fun elevenPresetsWithGoodColors() {
        assertEquals(11, Themes.presets.size)
        for (t in Themes.presets) for (c in listOf(t.bg, t.raised, t.text, t.dim, t.icon, t.accent, t.accent2, t.bar, t.border)) assertNotNull("${t.name}: $c", Themes.argb(c))
    }

    @Test
    fun colorsReadInEveryForm() {
        assertEquals(0xFFFFFFFF.toInt(), Themes.argb("#fff"))
        assertEquals(0xFF12AB34.toInt(), Themes.argb("#12ab34"))
        assertEquals(0x8012AB34.toInt(), Themes.argb("#12ab3480"))
        assertEquals(null, Themes.argb("12ab34"))
    }

    @Test
    fun jsonFromThePcOpensHere() {
        // The PC's export (snake_case fields, lowercase style).
        val t = Themes.fromJson("""{"name":"Mine","dark":false,"accent":"#ff0000","style":"halftone","round":3,"cover_border":true,"dot_clock":true}""")
        assertEquals("Mine", t.name)
        assertEquals("#ff0000", t.accent)
        assertEquals("#343345", t.bg)
        assertEquals(1f, t.round, 0f)
        assertTrue(t.coverBorder && t.dotClock && !t.dark)
        val back = Themes.fromJson(Themes.preset("Cyber HUD")!!.toJson())
        assertEquals(Themes.preset("Cyber HUD"), back)
        val bad = runCatching { Themes.fromJson("""{"bg":"red"}""") }.exceptionOrNull()
        assertTrue(bad?.message.orEmpty().contains("bg"))
    }
}
