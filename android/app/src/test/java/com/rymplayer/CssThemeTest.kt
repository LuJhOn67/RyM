package com.rymplayer

import androidx.compose.ui.graphics.Color
import com.rymplayer.ui.AppColors
import com.rymplayer.ui.CssTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CssThemeTest {
    private val example = """
        /* comment */
        :root {
          --name: "Rose Night";
          --bg: #120d10;
          --accent: #ff7aa8;
          --hover: rgba(255, 255, 255, 0.06);
        }
        button { color: red; }
    """

    @Test
    fun readsVariablesAndColors() {
        val v = CssTheme.variables(example)
        assertEquals("Rose Night", v["--name"])
        assertEquals(Color(0xFFFF7AA8), CssTheme.color(v["--accent"]!!))
        assertEquals(0.06f, CssTheme.color(v["--hover"]!!)!!.alpha, 0.01f)
        assertEquals(Color(0xFFFFFFFF), CssTheme.color("#fff"))
        assertEquals(null, CssTheme.color("nope"))
    }

    @Test
    fun appliesAndResets() {
        assertTrue(AppColors.apply(example, "file"))
        assertEquals("Rose Night", AppColors.name)
        assertFalse(AppColors.light)
        assertTrue(AppColors.apply(":root { --bg: #fafafa; }", "light one"))
        assertTrue(AppColors.light)
        assertFalse(AppColors.apply("body { }", "x"))
        AppColors.reset()
        assertEquals(null, AppColors.name)
    }
}
