package com.rymplayer.data

import android.content.Context

/**
 * RaM points: how RyM picks the songs it suggests, in Up next and on the home page ("For you"),
 * the same rules as the PC apps' src/ram_points.rs.
 *
 * - **Kinds never mix.** Every song has a kind: songs, Husseini (latmiyat, nawa'i, qasa'id),
 *   Quran, nasheeds or talk (podcasts, lectures). The words in its title and artist tell; an
 *   artist's songs teach RyM the artist's kind (kept on the phone), and a song nothing tells about
 *   takes the kind of the radio it came in. A Husseini song brings only Husseini, a song only songs.
 * - **Points.** Each song that may come gets points: its place in YouTube's radio, the same artist
 *   as the playing song (a lot), artists heard lately or liked, the same writing (Arabic, Latin,
 *   Japanese...), and new songs over ones played or shown lately. The best go first; the same
 *   artist never twice in a row, liked songs only now and then.
 */
object RamPoints {
    enum class Kind(val shelf: String, val title: String) {
        Songs("Songs for you", "Songs"), Husseini("Husseini for you", "Husseini"), Quran("Quran for you", "Quran"),
        Nasheed("Nasheeds for you", "Nasheeds"), Talk("Talks for you", "Talks"),
    }

    val SHELVES = Kind.entries.map { it.shelf }.toSet()

    private val HUSSEINI = listOf(
        "لطمية", "لطميات", "لطم", "نعي", "نعاوي", "عزاء", "رادود", "الرادود", "محرم", "عاشوراء", "كربلاء", "الطف", "الحسين",
        "حسيني", "حسينية", "حسينيات", "ياحسين", "يا حسين", "الزهراء", "يا زهراء", "يازهراء", "يا زينب", "السيدة زينب", "ام البنين",
        "ابا الفضل", "ابو الفضل", "يا عباس", "العباس", "السجاد", "علي الاكبر", "الامام", "الائمة", "اهل البيت", "يا مهدي",
        "صاحب الزمان", "الاربعين", "مشاية", "موكب", "مواكب", "مواليد", "الكربلائي", "latmiya", "latmiyat", "latmya", "latmia",
        "nauha", "nohay", "nohe", "majlis", "muharram", "ashura", "karbala", "karbalaei", "karbalai", "matam", "radood",
        "ya hussain", "ya hussein", "ya husayn", "imam hussain", "imam hussein", "ya abbas", "ya zahra",
    )
    private val QURAN = listOf("قرآن", "القرآن", "سورة", "تلاوة", "ترتيل", "مرتل", "مجود", "quran", "surah", "surat", "tilawat", "tilawah")
    private val NASHEED = listOf("نشيد", "اناشيد", "انشودة", "nasheed", "nasheeds", "anasheed")
    private val TALK = listOf("بودكاست", "محاضرة", "خطبة", "podcast", "lecture", "sermon", "khutbah")
    /** Arabic word beginnings ("and", "the", "with the"...) a word may carry. */
    private val PREFIXES = listOf("وال", "بال", "فال", "لل", "ال", "و", "ب", "ف")

    /** Letters only, lower case; Arabic without its vowel marks and with one form of alif, taa marbuta and yaa. */
    fun normalize(s: String): String {
        val out = StringBuilder(s.length)
        for (c in s) {
            when {
                c in 'ً'..'ٟ' || c == 'ٰ' || c == 'ـ' -> {}
                c == 'أ' || c == 'إ' || c == 'آ' || c == 'ٱ' -> out.append('ا')
                c == 'ة' -> out.append('ه')
                c == 'ى' -> out.append('ي')
                c.isLetterOrDigit() -> out.append(c.lowercaseChar())
                else -> out.append(' ')
            }
        }
        return out.toString()
    }

    private class Words(val kind: Kind, val single: Set<String>, val phrases: List<String>)

    private val telling: List<Words> by lazy {
        // Quran first: a reciter's "imam" shouldn't make it Husseini.
        listOf(Kind.Quran to QURAN, Kind.Husseini to HUSSEINI, Kind.Nasheed to NASHEED, Kind.Talk to TALK).map { (kind, list) ->
            val single = mutableSetOf<String>()
            val phrases = mutableListOf<String>()
            for (w in list) {
                val n = normalize(w).split(' ').filter { it.isNotEmpty() }.joinToString(" ")
                if (' ' in n) phrases += " $n " else single += n
            }
            Words(kind, single, phrases)
        }
    }

    /** The kind the song's own words tell (title, artist, album), if any. */
    fun wordKind(t: Track): Kind? {
        val words = normalize("${t.title} ${t.artist} ${t.album}").split(' ').filter { it.isNotEmpty() }
        val spaced = " ${words.joinToString(" ")} "
        return telling.firstOrNull { w ->
            words.any { word -> word in w.single || PREFIXES.any { p -> word.startsWith(p) && word.removePrefix(p) in w.single } } ||
                w.phrases.any { it in spaced }
        }?.kind
    }

    enum class Script { Arabic, Latin, Japanese, Korean, Chinese, Cyrillic, Other }

    /** The writing of the title (the artist's name only when the title has no letters). */
    fun script(t: Track): Script = scriptOf(t.title).takeIf { it != Script.Other } ?: scriptOf(t.artist)

    private fun scriptOf(text: String): Script {
        val n = IntArray(6)
        for (c in text) {
            when (c.code) {
                in 0x0600..0x06FF, in 0x0750..0x077F, in 0x08A0..0x08FF, in 0xFB50..0xFDFF, in 0xFE70..0xFEFF -> n[0]++
                in 0x41..0x5A, in 0x61..0x7A, in 0xC0..0x24F -> n[1]++
                in 0x3040..0x30FF -> n[2]++
                in 0xAC00..0xD7AF, in 0x1100..0x11FF -> n[3]++
                in 0x4E00..0x9FFF -> n[4]++
                in 0x0400..0x04FF -> n[5]++
            }
        }
        if (n[2] > 0) return Script.Japanese
        val best = n.indices.maxBy { n[it] }
        if (n[best] == 0) return Script.Other
        return listOf(Script.Arabic, Script.Latin, Script.Japanese, Script.Korean, Script.Chinese, Script.Cyrillic)[best]
    }

    fun firstArtist(artist: String): String = Radio.firstArtist(artist)

    /** What RyM has learned about artists: how much each kind was seen with them. */
    /** [fixed]: artists the user put in a kind by hand (a song's menu); it wins. */
    class Notes(val artists: MutableMap<String, IntArray> = HashMap(), val fixed: MutableMap<String, Kind> = HashMap()) {
        private fun add(artist: String, kind: Kind, weight: Int) {
            if (artist.isEmpty()) return
            val seen = artists.getOrPut(artist) { IntArray(5) }
            seen[kind.ordinal] += weight
            // Halved now and then, so what's seen lately counts most.
            if (seen.sum() > 120) for (i in seen.indices) seen[i] /= 2
        }

        /** The artist's kind, once it's clear (seen enough, and mostly with one kind). */
        fun artistKind(artist: String): Kind? {
            val seen = artists[artist] ?: return null
            val total = seen.sum()
            val best = seen.indices.maxBy { seen[it] }
            return if (total >= 3 && seen[best] * 10 >= total * 7) Kind.entries[best] else null
        }

        /** The song's kind: its words, else its artist's; null when nothing tells. */
        fun kindOf(t: Track): Kind? = firstArtist(t.artist).let { a -> fixed[a] ?: wordKind(t) ?: artistKind(a) }

        /** A radio's song's kind: its own, else the kind known for most of its radio, else songs. */
        fun seedKind(seed: Track, radio: List<Track>): Kind {
            kindOf(seed)?.let { return it }
            val count = IntArray(5)
            for (t in radio) kindOf(t)?.let { count[it.ordinal]++ }
            val best = (1..4).maxBy { count[it] }
            return if (count[best] >= 2 && count[best] * 4 >= radio.size) Kind.entries[best] else Kind.Songs
        }

        /** What a radio teaches: telling words count three for their kind, the others one for the radio's kind. */
        fun learn(kind: Kind, seed: Track, radio: List<Track>) {
            val seedScript = script(seed)
            for (t in listOf(seed) + radio) {
                val artist = firstArtist(t.artist)
                if (artist in fixed) continue
                val k = wordKind(t)
                when {
                    k != null -> add(artist, k, 3)
                    (artistKind(artist) ?: kind) == kind && (kind == Kind.Songs || script(t) == seedScript) -> add(artist, kind, 1)
                }
            }
        }

        fun encode(): String = (artists.entries.sortedByDescending { it.value.sum() }.take(2000)
            .map { (a, c) -> a.replace('\t', ' ').replace('\n', ' ') + "\t" + c.joinToString(",") } +
            fixed.map { (a, k) -> "!" + a.replace('\t', ' ').replace('\n', ' ') + "\t" + k.name }).joinToString("\n")

        companion object {
            fun decode(s: String): Notes = Notes(s.lineSequence().filter { !it.startsWith("!") }.mapNotNull { line ->
                val (a, c) = line.split('\t').takeIf { it.size == 2 } ?: return@mapNotNull null
                val counts = c.split(',').mapNotNull { it.toIntOrNull() }.takeIf { it.size == 5 } ?: return@mapNotNull null
                a to counts.toIntArray()
            }.toMap(HashMap()), s.lineSequence().filter { it.startsWith("!") }.mapNotNull { line ->
                val (a, k) = line.drop(1).split('\t').takeIf { it.size == 2 } ?: return@mapNotNull null
                runCatching { a to Kind.valueOf(k) }.getOrNull()
            }.toMap(HashMap()))
        }
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ram_points", Context.MODE_PRIVATE)

    fun load(ctx: Context): Notes = Notes.decode(prefs(ctx).getString("artists", "").orEmpty())

    fun save(ctx: Context, notes: Notes) {
        prefs(ctx).edit().putString("artists", notes.encode()).apply()
    }

    /** The user says the song's artist is of [kind]: kept, and it wins over words and radios. */
    fun correct(ctx: Context, t: Track, kind: Kind) {
        val artist = firstArtist(t.artist).takeIf { it.isNotEmpty() } ?: return
        val notes = load(ctx)
        notes.fixed[artist] = kind
        notes.artists[artist] = IntArray(5).also { it[kind.ordinal] = 12 }
        save(ctx, notes)
    }

    /** "Only this kind" (Settings): For you shows only it; null: every kind heard. */
    fun onlyKind(ctx: Context): Kind? = prefs(ctx).getString("only_kind", null)?.let { n -> Kind.entries.firstOrNull { it.name == n } }

    fun setOnlyKind(ctx: Context, kind: Kind?) {
        prefs(ctx).edit().putString("only_kind", kind?.name).apply()
    }

    private class Scored(val t: Track, val points: Int, val artist: String, val liked: Boolean)

    /** The best first, [n] at most: the same artist as the last song loses 20 points (8 for the one before), a liked song only after [likedEvery] others, [perArtist] songs per artist. */
    private fun order(cands: MutableList<Scored>, n: Int, likedEvery: Int, perArtist: Int?): List<Track> {
        val out = mutableListOf<Track>()
        var last0 = ""
        var last1 = ""
        var sinceLiked = 0
        val per = HashMap<String, Int>()
        while (out.size < n) {
            var bestAt = -1
            var bestPoints = Int.MIN_VALUE
            for ((i, s) in cands.withIndex()) {
                if (s.liked && sinceLiked < likedEvery) continue
                if (s.artist.isNotEmpty() && perArtist != null && (per[s.artist] ?: 0) >= perArtist) continue
                val repeat = when {
                    s.artist.isEmpty() -> 0
                    s.artist == last0 -> 20
                    s.artist == last1 -> 8
                    else -> 0
                }
                if (s.points - repeat > bestPoints) {
                    bestPoints = s.points - repeat
                    bestAt = i
                }
            }
            if (bestAt < 0) break
            val s = cands.removeAt(bestAt)
            sinceLiked = if (s.liked) 0 else sinceLiked + 1
            per[s.artist] = (per[s.artist] ?: 0) + 1
            last1 = last0
            last0 = s.artist
            out += s.t
        }
        return out
    }

    private const val HOUR = 60L * 60 * 1000
    private const val DAY = 24 * HOUR

    /**
     * Up next for the playing song ([seed]) from its radio: only its kind, songs by its artist and
     * ones like it first, liked songs (same artist or an artist of the radio) one in four at most,
     * nothing in [skip] (played or queued), no song twice. [heard]: first artists heard this
     * session. [playedAt]: when a song was last played (ms, 0: never). Learns from the radio.
     */
    fun upNext(notes: Notes, radio: List<Track>, seed: Track, liked: List<Track>, skip: Set<String>, heard: Set<String>, playedAt: (String) -> Long, now: Long = System.currentTimeMillis()): List<Track> {
        val kind = notes.seedKind(seed, radio)
        notes.learn(kind, seed, radio)
        val seedArtist = firstArtist(seed.artist)
        val seedScript = script(seed)
        val likedArtists = liked.map { firstArtist(it.artist) }.toSet()
        val radioArtists = radio.map { firstArtist(it.artist) }.toSet()
        val ids = mutableSetOf(seed.videoId)
        val keys = mutableSetOf(Radio.sameSongKey(seed))
        val len = radio.size.coerceAtLeast(1)
        val likedOnes = liked.filter { val a = firstArtist(it.artist); a.isNotEmpty() && (a == seedArtist || a in radioArtists) }
        val cands = mutableListOf<Scored>()
        val all = radio.mapIndexed { i, t -> Triple(t, 30 - 30 * i / len, false) } + likedOnes.map { Triple(it, 0, true) }
        for ((t, base, isLiked) in all) {
            if (t.link != null || t.videoId.isEmpty() || t.videoId in skip) continue
            // Only the seed's kind. A song nothing tells about may come with a radio (a little lower
            // when the seed isn't a song, and only in the seed's writing); a liked one must be known to fit.
            val k = notes.kindOf(t)
            val fit = when {
                k == kind -> 0
                k != null -> continue
                kind == Kind.Songs -> 0
                isLiked || script(t) != seedScript -> continue
                else -> -12
            }
            if (!ids.add(t.videoId) || !keys.add(Radio.sameSongKey(t))) continue
            val artist = firstArtist(t.artist)
            var points = base + fit
            if (artist.isNotEmpty() && artist == seedArtist) points += 35 else if (artist in heard) points += 10
            if (artist in likedArtists) points += 6
            val s = script(t)
            if (s == seedScript) points += 12 else if (s != Script.Other && seedScript != Script.Other) points -= 25
            val at = playedAt(t.videoId)
            if (at > 0) {
                val ago = now - at
                if (ago < 12 * HOUR) points -= 50 else if (ago < 3 * DAY) points -= 30
            }
            if (isLiked) points -= 4
            points += (0..6).random()
            cands += Scored(t, points, artist, isLiked)
        }
        return order(cands, Int.MAX_VALUE, 3, null)
    }

    /** A song that may go on the home page: the points its source gives, the kind of the radio it came in (null: not from a radio), and whether it's a liked one. */
    class Candidate(val track: Track, val base: Int, val radioKind: Kind?, val liked: Boolean)

    /** The songs of a seed's radio, YouTube's first ones worth the most ([bonus] on top). */
    fun fromRadio(radio: List<Track>, kind: Kind, bonus: Int): List<Candidate> {
        val len = radio.size.coerceAtLeast(1)
        return radio.mapIndexed { i, t -> Candidate(t, bonus + 20 - 20 * i / len, kind, false) }
    }

    /** The kinds the home page shows, from how much each was heard lately: the main one, and a second when it's at least a quarter. */
    fun pageKinds(heard: List<Pair<Kind, Float>>): List<Kind> {
        val sum = FloatArray(5)
        for ((k, w) in heard) sum[k.ordinal] += w
        val total = sum.sum()
        if (total <= 0f) return listOf(Kind.Songs)
        val kinds = (0..4).filter { sum[it] > 0f }.sortedByDescending { sum[it] }
        return kinds.filterIndexed { place, i -> place == 0 || (place == 1 && sum[i] >= total * 0.25f) }.map { Kind.entries[it] }
    }

    /**
     * [n] songs of one [kind] for the home page, by points: new songs first, nothing played or
     * shown lately (shown in the last day: only if nothing else is left), at most three by one
     * artist, a liked song after five others at most; nothing in [skip].
     */
    fun forYou(notes: Notes, cands: List<Candidate>, kind: Kind, n: Int, skip: Set<String>, known: Set<String>, topArtists: Set<String>, shown: (String) -> Pair<Int, Long>?, playedAt: (String) -> Long, now: Long = System.currentTimeMillis()): List<Track> {
        val ids = mutableSetOf<String>()
        val keys = mutableSetOf<String>()
        val scored = mutableListOf<Scored>()
        for (c in cands) {
            val t = c.track
            if (t.link != null || t.videoId.isEmpty() || t.videoId in skip) continue
            val k = notes.kindOf(t) ?: c.radioKind ?: Kind.Songs
            if (k != kind || !ids.add(t.videoId) || !keys.add(Radio.sameSongKey(t))) continue
            val artist = firstArtist(t.artist)
            var points = c.base
            if (t.videoId !in known) points += 18
            val at = playedAt(t.videoId)
            if (at > 0) {
                val ago = now - at
                if (ago < 3 * DAY) points -= 40 else if (ago < 14 * DAY) points -= 10
            }
            shown(t.videoId)?.let { (times, last) ->
                val ago = now - last
                points -= if (ago < DAY) 60 else if (ago < 3 * DAY) 25 else 0
                if (times >= 3) points -= 10
            }
            if (artist in topArtists) points += 6
            points += (0..8).random()
            scored += Scored(t, points, artist, c.liked)
        }
        return order(scored, n, 5, 3)
    }
}
