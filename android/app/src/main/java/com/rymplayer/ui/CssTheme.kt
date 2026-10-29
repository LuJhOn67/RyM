package com.rymplayer.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color

/**
 * The app's colors, changeable at run time by a theme written as CSS: the same `.css` files the
 * PC version reads (`:root { --accent: #ff7aa8; ... }`). Only the variables are used here.
 */
object AppColors {
    private val defaults = listOf(Color(0xFFFF2D55), Color(0xFFAAAAAA), Color(0xFF0B0B0D), Color(0xFF111114), Color(0xFF1C1C21), Color.White)

    var accent by mutableStateOf(defaults[0])
    var dim by mutableStateOf(defaults[1])
    var bg by mutableStateOf(defaults[2])
    var bar by mutableStateOf(defaults[3])
    var raised by mutableStateOf(defaults[4])
    var text by mutableStateOf(defaults[5])
    var light by mutableStateOf(false)
    /** Name of the CSS theme in use, null for the built-in look. */
    var name by mutableStateOf<String?>(null)

    fun reset() {
        accent = defaults[0]; dim = defaults[1]; bg = defaults[2]; bar = defaults[3]; raised = defaults[4]; text = defaults[5]
        light = false
        name = null
    }

    /** Applies a CSS theme; false when the file has no usable colors. */
    fun apply(css: String, fallbackName: String): Boolean {
        val vars = CssTheme.variables(css)
        val color = { k: String -> vars[k]?.let(CssTheme::color) }
        if (vars.keys.none { color(it) != null }) return false
        reset()
        color("--bg")?.let { bg = it }
        light = vars["--light"]?.toFloatOrNull()?.let { it != 0f } ?: (bg.luminance() > 0.5f)
        if (light) { text = Color(0xFF282C34); dim = Color(0xFF808691); raised = Color(0xFFF0F2F5); bar = Color.White }
        color("--accent")?.let { accent = it }
        color("--dim")?.let { dim = it }
        color("--text")?.let { text = it }
        (color("--bar") ?: color("--surface"))?.let { bar = it }
        (color("--menu") ?: color("--input-bg")?.takeIf { it.alpha > 0.9f })?.let { raised = it }
        name = vars["--name"] ?: fallbackName
        return true
    }

    private fun Color.luminance() = 0.2126f * red + 0.7152f * green + 0.0722f * blue
}

object CssTheme {
    /** `--name: value` pairs from the `:root { }` block, comments removed. */
    fun variables(src: String): Map<String, String> {
        val clean = Regex("/\\*.*?\\*/", RegexOption.DOT_MATCHES_ALL).replace(src, "")
        val start = clean.indexOf(":root").takeIf { it >= 0 } ?: return emptyMap()
        val open = clean.indexOf('{', start).takeIf { it >= 0 } ?: return emptyMap()
        val close = clean.indexOf('}', open).takeIf { it >= 0 } ?: return emptyMap()
        return clean.substring(open + 1, close).split(';').mapNotNull { decl ->
            val i = decl.indexOf(':').takeIf { it > 0 } ?: return@mapNotNull null
            val k = decl.substring(0, i).trim()
            val v = decl.substring(i + 1).trim().trim('"', '\'')
            if (k.startsWith("--") && v.isNotEmpty()) k to v else null
        }.toMap()
    }

    /** `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)` or `rgba(r, g, b, a)`. */
    fun color(v: String): Color? {
        val s = v.trim()
        if (s.startsWith("#")) {
            val h = s.drop(1)
            if (h.any { Character.digit(it, 16) < 0 }) return null
            val full = when (h.length) {
                3, 4 -> h.map { "$it$it" }.joinToString("")
                6, 8 -> h
                else -> return null
            }
            val n = full.chunked(2).map { it.toInt(16) / 255f }
            return Color(n[0], n[1], n[2], n.getOrElse(3) { 1f })
        }
        val inner = Regex("^rgba?\\((.*)\\)$").find(s)?.groupValues?.get(1) ?: return null
        val p = inner.split(',').map { it.trim().toFloatOrNull() ?: return null }
        return when (p.size) {
            3 -> Color(p[0] / 255f, p[1] / 255f, p[2] / 255f)
            4 -> Color(p[0] / 255f, p[1] / 255f, p[2] / 255f, p[3].coerceIn(0f, 1f))
            else -> null
        }
    }
}
