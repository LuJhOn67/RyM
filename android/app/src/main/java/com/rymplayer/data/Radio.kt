package com.rymplayer.data

/**
 * "Up next" from YouTube Music's radio for a song (`/next` with `RDAMVM<id>`), tidied: no repeats
 * of the same song (also under another upload), nothing already played or queued this session,
 * long videos kept (whole albums and mixes are fine), and songs by the same artist as the one
 * playing (or ones heard this session) moved up a little, so the mood holds.
 */
object Radio {
    private const val SAME_ARTIST_LIFT = 3
    private const val SESSION_ARTIST_LIFT = 1

    fun firstArtist(artist: String): String = artist.split(',', '&').first().trim().lowercase()

    /** "Song (Official Video) [HD]" by "A, B" -> "song|a": the same song under another upload. */
    private fun key(t: Track): String {
        val title = t.title.lowercase().replace(Regex("""\([^)]*\)|\[[^]]*]"""), "").split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
        return "$title|${firstArtist(t.artist)}"
    }

    fun tidy(radio: List<Track>, seed: Track, played: Set<String>, queued: Set<String>, sessionArtists: Set<String>): List<Track> {
        val keys = mutableSetOf(key(seed))
        val ids = mutableSetOf(seed.videoId)
        val seedArtist = firstArtist(seed.artist)
        val kept = mutableListOf<Pair<Int, Track>>()
        for (t in radio) {
            if (t.videoId.isEmpty() || t.videoId in played || t.videoId in queued) continue
            if (!ids.add(t.videoId) || !keys.add(key(t))) continue
            val artist = firstArtist(t.artist)
            val lift = when {
                seedArtist.isNotEmpty() && artist == seedArtist -> SAME_ARTIST_LIFT
                artist in sessionArtists -> SESSION_ARTIST_LIFT
                else -> 0
            }
            // Ranked by YouTube's order, lifted a few places; earlier stays first on a tie.
            val place = kept.size
            kept += ((place - lift).coerceAtLeast(0) * 2 + if (lift == 0) 1 else 0) to t
        }
        return kept.sortedBy { it.first }.map { it.second }
    }
}
