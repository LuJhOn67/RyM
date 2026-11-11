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

    private val CLUTTER = setOf("official", "music", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial", "officiel", "clip", "full", "videoclip", "videoclipe", "with", "on", "screen")
    private val CLUTTER_MARK = setOf("video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial", "officiel")

    private fun isClutter(inner: String): Boolean {
        val words = inner.lowercase().split(' ', '-', '_', '\t').filter { it.isNotEmpty() }
        return words.isNotEmpty() && words.all { it in CLUTTER } && words.any { it in CLUTTER_MARK }
    }

    /**
     * A title as a song's, not an upload's: brackets that only say "(Official Video)", "[Official
     * Music Video]", "(Lyrics)", "[HD]" and the like go, so do "| Official Video", hashtags, and
     * the artist's name in front ("Artist - Song"). Brackets that mean something ("(Remix)",
     * "(Live)", "(feat. X)") stay. The same rules as the PC apps' api::clean_title.
     */
    fun cleanTitle(title: String, artist: String): String {
        val out = StringBuilder()
        var rest = title
        while (true) {
            val i = listOf(rest.indexOf('('), rest.indexOf('[')).filter { it >= 0 }.minOrNull() ?: break
            val close = if (rest[i] == '(') ')' else ']'
            val j = rest.indexOf(close, i + 1)
            if (j < 0) break
            out.append(rest, 0, i)
            if (!isClutter(rest.substring(i + 1, j))) out.append(rest, i, j + 1)
            rest = rest.substring(j + 1)
        }
        out.append(rest)
        var text = out.toString()
        for (sep in listOf(" | ", " - ", " – ")) {
            val i = text.lastIndexOf(sep)
            if (i >= 0 && isClutter(text.substring(i + sep.length))) text = text.substring(0, i)
        }
        val joined = text.split(Regex("""\s+""")).filter { it.isNotEmpty() && !it.startsWith("#") }.joinToString(" ")
        text = joined
        val first = firstArtist(artist)
        for (sep in listOf(" - ", " – ")) {
            val i = joined.indexOf(sep)
            if (i < 0) continue
            val song = joined.substring(i + sep.length).trim()
            if (first.isNotEmpty() && normalizeArtistKey(joined.substring(0, i)) == first && song.isNotEmpty()) {
                text = song
                break
            }
        }
        val cleaned = text.split(Regex("""\s+""")).filter { it.isNotEmpty() }.joinToString(" ").trim(' ', '-', '|', '–')
        return cleaned.ifEmpty { title.trim() }
    }

    /** The first of a song's artists, as a key (see [normalizeArtistKey]). */
    fun firstArtist(artist: String): String = normalizeArtistKey(artist.split(',', '&').first())

    /**
     * An artist's name as a key: lower case, trimmed, without YouTube's channel endings
     * (" - Topic", "VEVO", "Official"), so the same artist from the built-in engine and from
     * ytmusicapi is one key ("Adele", "AdeleVEVO" and "Adele - Topic" are all "adele").
     */
    fun normalizeArtistKey(name: String): String {
        var key = name.trim().lowercase()
        while (true) {
            val before = key.length
            for (ending in listOf(" - topic", "- topic", "vevo", " official", "official")) {
                if (key.endsWith(ending) && key.removeSuffix(ending).isNotBlank()) key = key.removeSuffix(ending).trimEnd()
            }
            if (key.length == before) break
        }
        return key.split(Regex("""\s+""")).filter { it.isNotEmpty() }.joinToString(" ")
    }

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
