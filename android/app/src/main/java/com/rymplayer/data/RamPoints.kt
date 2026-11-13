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

    /**
     * Points for how much of a song was heard (the same as the PC's heard_points): skipped in
     * its first 30 seconds, SKIP; left before 30%, up to EARLY_LEAVE (the earlier the more); from
     * 30% to 80%, from 0 up to FINISH; 80% or more, FINISH.
     */
    /**
     * The favorites by RaM points, for the recap: the songs with the most points now (named from
     * [known], songs played here) and the artists. The same as the PC's ram_points::favorites.
     */
    fun favorites(ctx: Context, known: List<Track>, n: Int): Pair<List<Pair<Track, Float>>, List<Pair<String, Float>>> {
        val notes = load(ctx)
        val now = System.currentTimeMillis()
        val byId = known.associateBy { it.videoId }
        val songs = notes.tracks.keys.mapNotNull { id -> byId[id]?.let { it to notes.trackPoints(id, now) } }
            .filter { it.second > 0f }.sortedByDescending { it.second }.take(n)
        val artists = notes.affinity.map { (a, p) -> (notes.names[a] ?: a) to p + (notes.bias[a] ?: 0) }
            .filter { it.second > 0f }.sortedByDescending { it.second }.take(n)
        return songs to artists
    }

    /** Up next from familiar songs (0) to new ones (4), 2 balanced (the setting, set by the app). */
    @Volatile var discovery = 2

    val DISCOVERY_NAMES = listOf("Familiar", "Mostly familiar", "Balanced", "Mostly new", "New")

    /** Points for familiar or new: a song never heard gains toward "new", a liked one toward "familiar". */
    fun discoveryPoints(heardBefore: Boolean, likedPoints: Float): Int {
        val lean = discovery - 2
        return when {
            !heardBefore -> lean * 8
            likedPoints > 5f -> -lean * 6
            else -> 0
        }
    }

    /** A quick skip (in the first 30 seconds, before the song's half). */
    fun isSkip(listened: Float, length: Float) = heardPoints(listened, length) == SKIP

    fun heardPoints(listened: Float, length: Float): Float {
        if (listened < 30f && (length <= 0f || listened < length * 0.5f)) return SKIP
        if (length <= 0f) return 0f
        val f = (listened / length).coerceIn(0f, 1f)
        return when {
            f < 0.3f -> EARLY_LEAVE * (0.3f - f) / 0.3f
            f < 0.8f -> FINISH * (f - 0.3f) / 0.5f
            else -> FINISH
        }
    }

    /** What RyM has learned about artists: how much each kind was seen with them. */
    /** [fixed]: artists the user put in a kind by hand (a song's menu); it wins. */
    /** [bias]: points added to (or taken from) an artist ("Recommend more" / "Don't recommend"); [hidden]: songs never suggested again. */
    /** [tracks]: each song's points and when it last played; [affinity]: each artist's share of their songs' points; [names]: artists as written. */
    class Notes(
        val artists: MutableMap<String, IntArray> = HashMap(),
        val fixed: MutableMap<String, Kind> = HashMap(),
        val bias: MutableMap<String, Int> = HashMap(),
        val hidden: MutableSet<String> = HashSet(),
        val tracks: MutableMap<String, Score> = HashMap(),
        val affinity: MutableMap<String, Float> = HashMap(),
        val names: MutableMap<String, String> = HashMap(),
        /** Songs of each kind played at each hour of the day (local time): what fits this hour. */
        val hours: Array<IntArray> = Array(5) { IntArray(24) },
        /** Artists heard one after the other in the same session ("a|b", in order), and how often. */
        val together: MutableMap<String, Int> = HashMap(),
    ) {
        /** The artist last heard (not skipped) and when (ms), for [together]. */
        var lastHeard: Pair<String, Long>? = null

        /** Two artists' place together: how often they were heard one after the other. */
        fun together(a: String, b: String): Int {
            if (a.isEmpty() || b.isEmpty() || a == b) return 0
            return together[if (a < b) "$a|$b" else "$b|$a"] ?: 0
        }

        private fun heardWith(t: Track, now: Long) {
            val b = firstArtist(t.artist)
            val (a, at) = lastHeard ?: return
            if (b.isEmpty() || a.isEmpty() || a == b || now - at > SESSION_GAP_MS) return
            val key = if (a < b) "$a|$b" else "$b|$a"
            together[key] = (together[key] ?: 0) + 1
        }

        /** [delta] points to the song, and 30% of it to its artist. */
        private fun give(t: Track, delta: Float) {
            if (t.videoId.isEmpty() || delta == 0f) return
            val s = tracks.getOrPut(t.videoId) { Score() }
            s.points = (s.points + delta).coerceIn(-100f, 100f)
            val artist = firstArtist(t.artist)
            if (artist.isNotEmpty()) {
                names.getOrPut(artist) { t.artist.split(',', '&').first().trim() }
                affinity[artist] = ((affinity[artist] ?: 0f) + delta * TO_ARTIST).coerceIn(-60f, 60f)
            }
        }

        /** The song starts. Picked by hand, its minus points go first (back to 0). */
        fun played(t: Track, manual: Boolean, now: Long) {
            if (t.videoId.isEmpty()) return
            // Played again by hand soon after (and not skipped before): a replay.
            val replay = manual && tracks[t.videoId]?.let { it.points >= 0f && it.last > 0 && now - it.last < REPLAY_WITHIN_MS } == true
            val below = tracks[t.videoId]?.points?.takeIf { it < 0f }
            if (manual && below != null) give(t, -below)
            give(t, PLAY + if (replay) REPLAY else 0f)
            val s = tracks.getValue(t.videoId)
            s.last = now
            if (now - s.week >= WEEK_MS) {
                s.week = now
                s.plays = 0
            }
            s.plays++
            heardWith(t, now)
            // What kind plays at this hour (local time).
            kindOf(t)?.let { k -> hours[k.ordinal][java.util.Calendar.getInstance().apply { timeInMillis = now }.get(java.util.Calendar.HOUR_OF_DAY)]++ }
        }

        /** A sign the song is loved: its points go up (and 30% to its artist). */
        fun love(t: Track, how: Love) = give(t, how.points)

        /** The song stopped after [listened] of its [length] seconds: a quick skip loses points, hearing it to the end gives some. */
        fun left(t: Track, listened: Float, length: Float) {
            val points = heardPoints(listened, length)
            if (points != 0f) give(t, points)
            if (!isSkip(listened, length)) lastHeard = firstArtist(t.artist) to System.currentTimeMillis()
        }

        /** The song's points now: 5% less each day since it played, a third for 6 hours after. */
        fun trackPoints(id: String, now: Long): Float {
            val s = tracks[id] ?: return 0f
            val ago = (now - s.last).coerceAtLeast(0L)
            // Minus points fade faster; heard a lot this week, it ranks lower (fatigue).
            val decay = if (s.points < 0f) DECAY_NEGATIVE else DECAY
            var p = s.points * Math.pow(decay.toDouble(), ago.toDouble() / DAY).toFloat()
            if (p > 0f && ago < COOLDOWN_MS) p *= COOLDOWN
            if (p > 0f && s.plays >= FATIGUE_PLAYS && now - s.week < WEEK_MS) p *= FATIGUE
            return p
        }

        fun artistPoints(artist: String): Float = affinity[artist] ?: 0f

        /** What songs and artists add to a song's place in a list. */
        fun liking(t: Track, artist: String, now: Long): Int =
            Math.round(trackPoints(t.videoId, now)).coerceIn(-40, 40) + Math.round(artistPoints(artist)).coerceIn(-30, 30)

        fun summary(): Summary {
            val top = affinity.map { (a, n) -> a to n + (bias[a] ?: 0) }.filter { it.second > 0f }.sortedByDescending { it.second }.take(5).map { names[it.first] ?: it.first }
            return Summary(tracks.size, top)
        }

        fun resetAll() {
            tracks.clear(); affinity.clear(); bias.clear(); names.clear()
        }

        fun resetNegative() {
            tracks.values.removeAll { it.points < 0f }
            affinity.values.removeAll { it < 0f }
            bias.values.removeAll { it < 0 }
        }

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

    /** A song's points, when it last played (ms), and its plays this week (since [week] began). */
    class Score(var points: Float = 0f, var last: Long = 0L, var plays: Int = 0, var week: Long = 0L)

    /** What shows a song is loved, beyond playing it (the same as the PC's ram_points::Love). */
    enum class Love(val points: Float) { BackToStart(4f), Download(12f), AddToPlaylist(12f) }

    /**
     * Why each song (picked for Up next after [seed]) is there, in a few words ("" when it's
     * plain): in your likes, heard with the seed's artist, a song you like, an artist you play a
     * lot, or new for you. The same as the PC's ram_points::reasons.
     */
    fun reasons(ctx: Context, songs: List<Track>, seed: Track, liked: Set<String>): List<String> {
        val notes = load(ctx)
        val now = System.currentTimeMillis()
        val seedArtist = firstArtist(seed.artist)
        val seedName = seed.artist.split(',', '&').first().trim()
        return songs.map { t ->
            val artist = firstArtist(t.artist)
            when {
                artist.isEmpty() || artist == seedArtist -> ""
                t.videoId in liked -> com.rymplayer.tr("In your likes")
                notes.together(seedArtist, artist) >= 2 -> "${com.rymplayer.tr("Heard with")} $seedName"
                notes.trackPoints(t.videoId, now) >= 15f -> com.rymplayer.tr("A song you like")
                notes.artistPoints(artist) >= 10f -> com.rymplayer.tr("An artist you play a lot")
                t.videoId !in notes.tracks && notes.artistPoints(artist) == 0f -> com.rymplayer.tr("New for you")
                else -> ""
            }
        }
    }

    /** A song was loved ([how]): its points go up, and its artist's. */
    @Synchronized
    fun loved(ctx: Context, t: Track, how: Love) {
        if (t.videoId.isEmpty()) return
        val notes = load(ctx)
        notes.love(t, how)
        save(ctx, notes)
    }

    /** What the settings show: songs with points, and the artists with the most. */
    data class Summary(val tracked: Int, val topArtists: List<String>)

    private const val PLAY = 10f
    private const val FINISH = 6f
    /** Left early (past the first 30 s, before 30% of the song), at most. */
    private const val EARLY_LEAVE = -6f
    private const val SKIP = -15f
    private const val TO_ARTIST = 0.3f
    private const val COOLDOWN = 0.35f
    private const val COOLDOWN_MS = 6L * 60 * 60 * 1000
    private const val DECAY = 0.95f
    private const val DECAY_NEGATIVE = 0.85f
    private const val SESSION_GAP_MS = 45L * 60 * 1000
    private const val REPLAY_WITHIN_MS = 15L * 60 * 1000
    private const val REPLAY = 8f
    private const val FATIGUE_PLAYS = 5
    private const val FATIGUE = 0.6f
    private const val TRACKS_KEPT = 5000

    private fun clean(s: String) = s.replace('\t', ' ').replace('\n', ' ')

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ram_points", Context.MODE_PRIVATE)

    /** One song gave way to another: the old one's listening time counts, then the new one is played ([manual]: picked by hand). */
    @Synchronized
    fun movedOn(ctx: Context, left: Triple<Track, Float, Float>?, next: Track, manual: Boolean, now: Long = System.currentTimeMillis()) {
        val notes = load(ctx)
        left?.let { (t, listened, length) -> notes.left(t, listened, length) }
        notes.played(next, manual, now)
        save(ctx, notes)
    }

    @Synchronized
    fun summary(ctx: Context): Summary = load(ctx).summary()

    /** "Reset all points": every song's and artist's points back to 0 (kinds and hidden songs stay). */
    @Synchronized
    fun resetAll(ctx: Context) {
        val notes = load(ctx)
        notes.resetAll()
        save(ctx, notes)
    }

    private const val WEEK_MS = 7L * 24 * 60 * 60 * 1000

    /** Why the points reset by themselves now, if they do: the picked day ("date") or a week ("week", only when turned on). */
    fun resetDue(date: String, weekly: Boolean, lastReset: Long, now: Long, today: String): String? = when {
        date.isNotEmpty() && today >= date -> "date"
        weekly && now - lastReset >= WEEK_MS -> "week"
        else -> null
    }

    fun today(): String = java.time.LocalDate.now().toString()

    /** The day the points reset by themselves ("2026-12-31"), or "" for none. */
    fun resetDate(ctx: Context): String = prefs(ctx).getString("reset_date", "").orEmpty()

    fun setResetDate(ctx: Context, date: String) {
        prefs(ctx).edit().putString("reset_date", date).apply()
    }

    /** The weekly reset: off unless the user turns it on (after a warning). */
    fun weeklyReset(ctx: Context): Boolean = prefs(ctx).getBoolean("weekly_reset", false)

    fun setWeeklyReset(ctx: Context, on: Boolean) {
        // The first reset comes a week after turning it on.
        prefs(ctx).edit().putBoolean("weekly_reset", on).putLong("last_reset", System.currentTimeMillis()).apply()
    }

    /** The reset the user scheduled, when it's due: resets and says why ("date" / "week"), else null. */
    @Synchronized
    fun scheduledReset(ctx: Context): String? {
        val p = prefs(ctx)
        val due = resetDue(resetDate(ctx), weeklyReset(ctx), p.getLong("last_reset", 0L), System.currentTimeMillis(), today()) ?: return null
        val notes = load(ctx)
        notes.resetAll()
        save(ctx, notes)
        p.edit().putLong("last_reset", System.currentTimeMillis()).apply()
        if (due == "date") setResetDate(ctx, "")
        return due
    }

    /** "Reset negative points only": skipped songs and artists back to 0; favorites stay. */
    @Synchronized
    fun resetNegative(ctx: Context) {
        val notes = load(ctx)
        notes.resetNegative()
        save(ctx, notes)
    }

    fun load(ctx: Context): Notes {
        val p = prefs(ctx)
        val base = Notes.decode(p.getString("artists", "").orEmpty())
        val bias = p.getString("bias", "").orEmpty().lineSequence().mapNotNull { line ->
            val (a, n) = line.split('\t').takeIf { it.size == 2 } ?: return@mapNotNull null
            n.toIntOrNull()?.let { a to it }
        }.toMap(HashMap())
        val hidden = p.getStringSet("hidden", emptySet()).orEmpty().toHashSet()
        val tracks = p.getString("tracks", "").orEmpty().lineSequence().mapNotNull { line ->
            // id, points, last (and plays, week since 7.4.4.2).
            val parts = line.split('\t').takeIf { it.size >= 3 } ?: return@mapNotNull null
            val points = parts[1].toFloatOrNull() ?: return@mapNotNull null
            parts[0] to Score(points, parts[2].toLongOrNull() ?: 0L, parts.getOrNull(3)?.toIntOrNull() ?: 0, parts.getOrNull(4)?.toLongOrNull() ?: 0L)
        }.toMap(HashMap())
        val affinity = HashMap<String, Float>()
        val names = HashMap<String, String>()
        p.getString("affinity", "").orEmpty().lineSequence().forEach { line ->
            val parts = line.split('\t').takeIf { it.size >= 2 } ?: return@forEach
            val n = parts[1].toFloatOrNull() ?: return@forEach
            affinity[parts[0]] = n
            parts.getOrNull(2)?.takeIf { it.isNotEmpty() }?.let { names[parts[0]] = it }
        }
        // Kind, hour, count lines.
        val hours = Array(5) { IntArray(24) }
        p.getString("hours", "").orEmpty().lineSequence().forEach { line ->
            val (k, h, n) = line.split('\t').takeIf { it.size == 3 }?.map { it.toIntOrNull() ?: -1 } ?: return@forEach
            if (k in 0..4 && h in 0..23 && n > 0) hours[k][h] = n
        }
        val together = HashMap<String, Int>()
        p.getString("together", "").orEmpty().lineSequence().forEach { line ->
            val (k, n) = line.split('\t').takeIf { it.size == 2 } ?: return@forEach
            n.toIntOrNull()?.let { together[k] = it }
        }
        return Notes(rekey(base.artists) { a, b -> IntArray(a.size) { a[it] + b.getOrElse(it) { 0 } } }, rekey(base.fixed) { a, _ -> a }, rekey(bias) { a, b -> a + b }, hidden, tracks, rekey(affinity) { a, b -> a + b }, rekey(names) { a, _ -> a }, hours, together).also { n ->
            p.getString("last_heard", "").orEmpty().split('\t').takeIf { it.size == 2 }?.let { (a, at) -> at.toLongOrNull()?.let { n.lastHeard = a to it } }
        }
    }

    /**
     * Artist keys saved before [Radio.normalizeArtistKey] ("adelevevo", "adele - topic") moved
     * onto the normalized key, their counts and points added together, so no earlier points are
     * lost. Songs stay keyed by their id.
     */
    internal fun <V> rekey(map: MutableMap<String, V>, add: (V, V) -> V): MutableMap<String, V> {
        if (map.keys.all { Radio.normalizeArtistKey(it) == it }) return map
        val out = HashMap<String, V>()
        for ((k, v) in map) {
            val key = Radio.normalizeArtistKey(k)
            out[key] = out[key]?.let { add(it, v) } ?: v
        }
        return out
    }

    fun save(ctx: Context, notes: Notes) {
        prefs(ctx).edit()
            .putString("artists", notes.encode())
            .putString("bias", notes.bias.entries.joinToString("\n") { (a, n) -> a.replace('\t', ' ').replace('\n', ' ') + "\t" + n })
            .putStringSet("hidden", notes.hidden.toSet())
            .putString("tracks", notes.tracks.entries.sortedByDescending { kotlin.math.abs(it.value.points) }.take(TRACKS_KEPT)
                .joinToString("\n") { (id, s) -> clean(id) + "\t" + s.points + "\t" + s.last + "\t" + s.plays + "\t" + s.week })
            .putString("affinity", notes.affinity.entries.joinToString("\n") { (a, n) -> clean(a) + "\t" + n + "\t" + clean(notes.names[a].orEmpty()) })
            .putString("together", notes.together.entries.sortedByDescending { it.value }.take(3000).joinToString("\n") { (k, n) -> clean(k) + "\t" + n })
            .putString("last_heard", notes.lastHeard?.let { clean(it.first) + "\t" + it.second }.orEmpty())
            .putString("hours", (0..4).flatMap { k -> (0..23).filter { notes.hours[k][it] > 0 }.map { h -> "$k\t$h\t${notes.hours[k][h]}" } }.joinToString("\n"))
            .apply()
    }

    /** "Recommend more": the song's artist comes up more in Up next and For you. */
    fun recommendMore(ctx: Context, t: Track) {
        val notes = load(ctx)
        notes.hidden -= t.videoId
        firstArtist(t.artist).takeIf { it.isNotEmpty() }?.let { a -> notes.bias[a] = ((notes.bias[a] ?: 0) + 25).coerceAtMost(50) }
        save(ctx, notes)
    }

    /** "Don't recommend": the song is never suggested again, and its artist a little less. */
    fun dontRecommend(ctx: Context, t: Track) {
        val notes = load(ctx)
        notes.hidden += t.videoId
        firstArtist(t.artist).takeIf { it.isNotEmpty() }?.let { a -> notes.bias[a] = ((notes.bias[a] ?: 0) - 20).coerceAtLeast(-60) }
        save(ctx, notes)
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
            if (t.link != null || t.videoId.isEmpty() || t.videoId in skip || t.videoId in notes.hidden) continue
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
            var points = base + fit + (notes.bias[artist] ?: 0) + notes.liking(t, artist, now)
            if (artist.isNotEmpty() && artist == seedArtist) points += 35 else if (artist in heard) points += 10
            // Heard together with the seed's artist in earlier sessions.
            points += notes.together(seedArtist, artist).coerceAtMost(6) * 2
            // Familiar or new (the setting).
            points += discoveryPoints(t.videoId in notes.tracks, notes.trackPoints(t.videoId, now))
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
    /**
     * The Hijri month (1 Muharram ... 9 Ramadan ... 12 Dhu al-Hijjah) of a day, by the tabular
     * Islamic calendar (within a day or so of the moon's sighting). The same as the PC's.
     */
    fun hijriMonth(ms: Long): Int {
        val jd = Math.floorDiv(ms, 86_400_000L) + 2_440_588L
        var l = jd - 1_948_440L + 10_632L
        val n = (l - 1) / 10_631L
        l = l - 10_631L * n + 354L
        val j = ((10_985L - l) / 5_316L) * ((50L * l) / 17_719L) + (l / 5_670L) * ((43L * l) / 15_238L)
        l = l - ((30L - j) / 15L) * ((17_719L * j) / 50L) - (j / 16L) * ((15_238L * j) / 43L) + 29L
        return ((24L * l) / 709L).toInt()
    }

    /**
     * Each kind's weight now for the home page: the season (Muharram and Safar favor Husseini,
     * Ramadan Quran and nasheeds) and what's played at this [hour], as learned.
     */
    fun contextWeights(notes: Notes, now: Long, hour: Int): FloatArray {
        val w = FloatArray(5) { 1f }
        when (hijriMonth(now)) {
            1, 2 -> w[Kind.Husseini.ordinal] *= 1.6f
            9 -> { w[Kind.Quran.ordinal] *= 1.6f; w[Kind.Nasheed.ordinal] *= 1.4f }
        }
        val at = IntArray(5) { notes.hours[it][hour % 24] }
        val total = at.sum()
        if (total >= 5) for (k in 0..4) w[k] *= 1f + 0.8f * at[k] / total
        return w
    }

    fun pageKinds(heard: List<Pair<Kind, Float>>, context: FloatArray = FloatArray(5) { 1f }): List<Kind> {
        val sum = FloatArray(5)
        for ((k, w) in heard) sum[k.ordinal] += w * context[k.ordinal]
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
            if (t.link != null || t.videoId.isEmpty() || t.videoId in skip || t.videoId in notes.hidden) continue
            val k = notes.kindOf(t) ?: c.radioKind ?: Kind.Songs
            if (k != kind || !ids.add(t.videoId) || !keys.add(Radio.sameSongKey(t))) continue
            val artist = firstArtist(t.artist)
            var points = c.base + (notes.bias[artist] ?: 0) + notes.liking(t, artist, now)
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
