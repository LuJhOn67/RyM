package com.rymplayer.data

import org.json.JSONObject

/**
 * Color themes: the same 11 built-in looks as RyM on the PC, and the user's own as JSON (the
 * same file as the PC's `themes.rs`: `name`, colors as `#rrggbb` or `#rrggbbaa`, `style`,
 * `round`, `glow`, `cover_border`, `glass`, `dot_clock`, `vu`). Missing fields take the
 * defaults; unknown ones are ignored.
 */
object Themes {
    data class Theme(
        val name: String = "Custom",
        val dark: Boolean = true,
        val bg: String = "#343345",
        val raised: String = "#3d3c50",
        val text: String = "#ffffff",
        val dim: String = "#c6c6c6",
        val icon: String = "#e4e3e3",
        val accent: String = "#ea814a",
        val accent2: String = "#f9d19e",
        val bar: String = "#d9d9d9",
        val border: String = "#66647780",
        /** plain, grid, dots, halftone, rain, scanlines, metal, hud, glass, aqua, stars */
        val style: String = "plain",
        val round: Float = 1f,
        val glow: Boolean = false,
        val coverBorder: Boolean = false,
        val glass: Float = 0f,
        val dotClock: Boolean = false,
        val vu: Boolean = false,
        /** The cover loses 70% of its color, the edges darken, and a slow glow beats with the bass (Remorse). */
        val muted: Boolean = false,
        val vignette: Boolean = false,
        val heartbeat: Boolean = false,
    ) {
        fun toJson(): String = JSONObject().apply {
            put("name", name); put("dark", dark)
            put("bg", bg); put("raised", raised); put("text", text); put("dim", dim); put("icon", icon)
            put("accent", accent); put("accent2", accent2); put("bar", bar); put("border", border)
            put("style", style); put("round", round.toDouble()); put("glow", glow); put("cover_border", coverBorder)
            put("glass", glass.toDouble()); put("dot_clock", dotClock); put("vu", vu)
            put("muted", muted); put("vignette", vignette); put("heartbeat", heartbeat)
        }.toString(2)
    }

    private fun t(name: String, dark: Boolean, c: List<String>, style: String, round: Float) =
        Theme(name, dark, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], style, round)

    /** In the order the settings show them (the same as the PC's). */
    val presets: List<Theme> = listOf(
        t("OLED Black", true, listOf("#000000", "#0b0b0b", "#ffffff", "#9a9a9a", "#e6e6e6", "#00e5ff", "#7cf9ff", "#262626", "#1f1f1f"), "plain", 1f).copy(glow = true, coverBorder = true),
        t("Frosted Glass", true, listOf("#141826", "#ffffff1c", "#ffffff", "#d3d9ea", "#f2f4fa", "#9ad0ff", "#d4ecff", "#ffffff40", "#ffffff33"), "glass", 1f).copy(glass = 0.55f),
        t("Tactical Beige", false, listOf("#dad4bb", "#cdc8b0", "#4e4b42", "#6b675b", "#4e4b42", "#4e4b42", "#8a8574", "#b5af98", "#4e4b4299"), "grid", 0f),
        t("Stellar Neon", true, listOf("#070b1a", "#0e1530", "#e8f6ff", "#8fa3c7", "#bfefff", "#00e1ff", "#ffc857", "#1c2a4d", "#00e1ff40"), "stars", 0.8f).copy(glow = true),
        t("Dot Matrix", true, listOf("#000000", "#111111", "#f2f2f2", "#8c8c8c", "#f2f2f2", "#d71921", "#ff5a60", "#2a2a2a", "#2a2a2a"), "dots", 1f).copy(dotClock = true),
        t("Hi-Fi Rack", true, listOf("#1a1b1d", "#2a2c2f", "#e9e4d8", "#a39d90", "#d8d2c4", "#ffb000", "#ffd27a", "#3a3c40", "#45484d"), "metal", 0.35f).copy(vu = true),
        t("Cyber HUD", true, listOf("#0a0a12", "#14141f", "#f5f5f5", "#9aa0b4", "#00f0ff", "#fcee09", "#00f0ff", "#23233a", "#00f0ff55"), "hud", 0f).copy(glow = true),
        t("Lo-Fi Rain", true, listOf("#1a1530", "#251e42", "#f3e9ff", "#b3a6d6", "#e8dcff", "#ffb347", "#ffd59a", "#3a2f63", "#4a3d7a80"), "rain", 1f),
        t("Aqua Glass", false, listOf("#dff6ff", "#ffffffb0", "#073b4c", "#2f6f80", "#0b6e8a", "#00b894", "#48dbfb", "#a8e6f0", "#ffffffcc"), "aqua", 1f).copy(glass = 0.35f),
        t("Manga Ink", false, listOf("#ffffff", "#f2f2f2", "#000000", "#333333", "#000000", "#000000", "#555555", "#cccccc", "#000000"), "halftone", 0f),
        t("Tactical Olive", true, listOf("#1d2117", "#2a2f22", "#d9dcc6", "#9aa08a", "#c8cdb0", "#8fbf3f", "#c3e86a", "#3b4230", "#5b6448"), "scanlines", 0.3f),
        // Sadness and guilt: an abyssal slate night, cold mist, dried crimson, ash, darkened edges, a slow heartbeat, colorless covers.
        t("Remorse", true, listOf("#0b0d12", "#14171f", "#c3c8cf", "#5c6b73", "#8e9aa3", "#8c2d38", "#b0606a", "#232833", "#2a303c"), "ash", 0.6f)
            .copy(muted = true, vignette = true, heartbeat = true),
    )

    fun preset(name: String): Theme? = presets.firstOrNull { it.name == name }

    /** `#rgb`, `#rrggbb` or `#rrggbbaa` as an ARGB int; null when it isn't a color. */
    fun argb(hex: String): Int? {
        val h = hex.trim().removePrefix("#").takeIf { hex.trim().startsWith("#") } ?: return null
        val full = when (h.length) {
            3 -> h.map { "$it$it" }.joinToString("") + "ff"
            6 -> h + "ff"
            8 -> h
            else -> return null
        }
        val v = full.toLongOrNull(16) ?: return null
        val rgb = (v shr 8) and 0xFFFFFF
        val a = v and 0xFF
        return ((a shl 24) or rgb).toInt()
    }

    /** A theme from JSON; a bad color or file is refused with what's wrong. */
    fun fromJson(text: String): Theme {
        val o = runCatching { JSONObject(text) }.getOrElse { throw IllegalArgumentException(it.message ?: "not JSON") }
        val d = Theme()
        val t = Theme(
            name = o.optString("name", d.name).ifBlank { "Custom" },
            dark = o.optBoolean("dark", d.dark),
            bg = o.optString("bg", d.bg), raised = o.optString("raised", d.raised), text = o.optString("text", d.text),
            dim = o.optString("dim", d.dim), icon = o.optString("icon", d.icon), accent = o.optString("accent", d.accent),
            accent2 = o.optString("accent2", d.accent2), bar = o.optString("bar", d.bar), border = o.optString("border", d.border),
            style = o.optString("style", d.style).lowercase(),
            round = o.optDouble("round", 1.0).toFloat().coerceIn(0f, 1f),
            glow = o.optBoolean("glow", false), coverBorder = o.optBoolean("cover_border", false),
            glass = o.optDouble("glass", 0.0).toFloat().coerceIn(0f, 1f),
            dotClock = o.optBoolean("dot_clock", false), vu = o.optBoolean("vu", false),
            muted = o.optBoolean("muted", false), vignette = o.optBoolean("vignette", false), heartbeat = o.optBoolean("heartbeat", false),
        )
        for ((field, value) in listOf("bg" to t.bg, "raised" to t.raised, "text" to t.text, "dim" to t.dim, "icon" to t.icon, "accent" to t.accent, "accent2" to t.accent2, "bar" to t.bar, "border" to t.border)) {
            if (argb(value) == null) throw IllegalArgumentException("$field: \"$value\" is not a color like #12ab34")
        }
        return t
    }
}
