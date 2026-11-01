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

    /**
     * YouTube Music's radio for a song. A SoundCloud song has none: the radio of the same song
     * found on YouTube Music by its name stands in.
     */
    suspend fun forTrack(api: MusicApi, t: Track): List<Track> {
        if (isYouTube(t.videoId)) return api.radio(t.videoId)
        val seed = api.search("${t.title} ${firstArtist(t.artist)}").firstOrNull() ?: return emptyList()
        return api.radio(seed.videoId)
    }

    /**
     * Up next for the playing song: its radio (songs like it) with the user's liked songs mixed
     * in, one in every three places. Liked songs by the same artist come first, then ones by
     * artists the radio has (the same mood), then others; none played or queued, no song twice.
     */
    fun withLiked(radio: List<Track>, seed: Track, liked: List<Track>, skip: Set<String>): List<Track> {
        val seedArtist = firstArtist(seed.artist)
        val radioArtists = radio.map { firstArtist(it.artist) }.toSet()
        val inRadio = radio.map { it.videoId }.toSet()
        val free = liked.filter { it.link == null && it.videoId != seed.videoId && it.videoId !in skip && it.videoId !in inRadio }
        val same = free.filter { seedArtist.isNotEmpty() && firstArtist(it.artist) == seedArtist }.shuffled()
        val near = free.filter { firstArtist(it.artist) != seedArtist && firstArtist(it.artist) in radioArtists }.shuffled()
        val rest = free.filter { firstArtist(it.artist) != seedArtist && firstArtist(it.artist) !in radioArtists }.shuffled()
        val picks = (same + near + rest).take(radio.size / 2).iterator()
        val out = mutableListOf<Track>()
        val keys = mutableSetOf<String>()
        radio.forEachIndexed { i, t ->
            if (keys.add(key(t))) out += t
            if (i % 2 == 1 && picks.hasNext()) {
                val l = picks.next()
                if (keys.add(key(l))) out += l
            }
        }
        return out
    }

    fun firstArtist(artist: String): String = artist.split(',', '&').first().trim().lowercase()

    /** "Song (Official Video) [HD]" by "A, B" -> "song|a": the same song under another upload. */
    fun sameSongKey(t: Track): String = key(t)

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
