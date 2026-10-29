package com.rymplayer.data

/** A song, or (with [link]) a card that opens a playlist, album or mix. */
data class Track(
    val videoId: String,
    val title: String,
    val artist: String,
    /** Album name, or the section/period name on the home and history pages. */
    val album: String = "",
    val duration: String = "",
    val thumb: String? = null,
    val albumId: String? = null,
    val artistId: String? = null,
    val link: Link? = null,
    /** A content URI for on-device audio. Null means this is an online track. */
    val localUri: String? = null,
)

sealed interface Link {
    val id: String

    data class Playlist(override val id: String) : Link
    data class Album(override val id: String) : Link
    data class Mix(override val id: String) : Link
}

data class Playlist(val id: String, val title: String, val thumb: String? = null)

data class Lyrics(
    /** (start seconds, line). Empty when only plain lyrics exist. */
    val synced: List<Pair<Float, String>>,
    val plain: List<String>,
    val source: String,
)

const val LIKED_ID = "LM"

/** Google image URLs take a size suffix (`=w120-h120...`); ask for `px` square. */
fun thumbSized(url: String?, px: Int): String? {
    if (url == null) return null
    val i = url.lastIndexOf("=w")
    return if (i > 0 && (url.contains("googleusercontent") || url.contains("ggpht"))) {
        url.substring(0, i) + "=w$px-h$px-l90-rj"
    } else {
        url
    }
}

/** "3:45" or "1:02:03" to seconds. */
fun parseDuration(d: String): Float? {
    if (d.isBlank()) return null
    var secs = 0f
    for (part in d.split(":")) secs = secs * 60 + (part.trim().toFloatOrNull() ?: return null)
    return secs
}

fun fmtTime(secs: Float): String {
    val t = secs.coerceAtLeast(0f).toInt()
    return "%d:%02d".format(t / 60, t % 60)
}
