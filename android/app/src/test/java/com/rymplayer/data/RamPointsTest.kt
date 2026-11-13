package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RamPointsTest {
    private fun t(id: String, title: String, artist: String) = Track(id, title, artist)

    @Test
    fun songPointsGoToTheArtistAndBringTheirNewSongsUp() {
        val notes = RamPoints.Notes()
        val fav = t("f1", "Old favorite", "Fav")
        repeat(5) {
            notes.played(fav, false, 0L)
            notes.left(fav, 200f, 210f)
        }
        assertEquals(80f, notes.tracks.getValue("f1").points, 0.01f)
        assertEquals(24f, notes.artistPoints("fav"), 0.01f)
        val seed = t("s", "Seed", "Z")
        val out = RamPoints.upNext(notes, listOf(t("a", "A", "Other"), t("f2", "Not heard yet", "Fav")), seed, emptyList(), emptySet(), emptySet(), { 0L }, now = 10 * 24 * 3600_000L)
        assertEquals("f2", out.first().videoId)
    }

    @Test
    fun aSongJustPlayedCoolsDownAndPointsFadeByDay() {
        val notes = RamPoints.Notes()
        val hour = 3600_000L
        notes.played(t("x", "X", "A"), false, 1000L)
        assertEquals(3.5f, notes.trackPoints("x", 1000L + hour), 0.01f)
        assertEquals(9.5f, notes.trackPoints("x", 1000L + 24 * hour), 0.01f)
    }

    @Test
    fun aQuickSkipTakesPointsAndAPickByHandClearsThem() {
        val notes = RamPoints.Notes()
        val song = t("x", "X", "A")
        notes.played(song, false, 0L)
        notes.left(song, 5f, 200f)
        notes.left(song, 5f, 200f)
        assertEquals(-20f, notes.tracks.getValue("x").points, 0.01f)
        notes.played(song, false, 10L)
        assertEquals(-10f, notes.tracks.getValue("x").points, 0.01f)
        notes.played(song, true, 20L)
        assertEquals(10f, notes.tracks.getValue("x").points, 0.01f)
        assertEquals(3f, notes.artistPoints("a"), 0.01f)
    }

    @Test
    fun resetsWipeEverythingOrOnlyTheMinusPoints() {
        val notes = RamPoints.Notes()
        notes.played(t("g", "G", "Good"), false, 0L)
        notes.left(t("b", "B", "Bad"), 1f, 100f)
        notes.bias["bad"] = -20
        notes.bias["good"] = 25
        assertEquals(listOf("Good"), notes.summary().topArtists)
        notes.resetNegative()
        assertTrue("g" in notes.tracks && "b" !in notes.tracks)
        assertEquals(0f, notes.artistPoints("bad"), 0.01f)
        assertEquals(25, notes.bias["good"])
        assertFalse("bad" in notes.bias)
        notes.resetAll()
        assertEquals(0, notes.summary().tracked)
    }

    @Test
    fun resetsComeOnThePickedDayOrEveryWeekOnlyWhenAsked() {
        val day = 24L * 3600_000
        assertEquals(null, RamPoints.resetDue("", false, 0L, 365 * day, "2027-01-01"))
        assertEquals(null, RamPoints.resetDue("2026-12-31", false, 0L, 0L, "2026-12-30"))
        assertEquals("date", RamPoints.resetDue("2026-12-31", false, 0L, 0L, "2026-12-31"))
        assertEquals("date", RamPoints.resetDue("2026-12-31", false, 0L, 0L, "2027-01-02"))
        assertEquals(null, RamPoints.resetDue("", true, 10 * day, 16 * day, "2027-01-01"))
        assertEquals("week", RamPoints.resetDue("", true, 10 * day, 17 * day, "2027-01-01"))
    }

    @Test
    fun wordsTellTheKind() {
        assertEquals(RamPoints.Kind.Husseini, RamPoints.wordKind(t("a", "لطمية يا حسين", "باسم الكربلائي")))
        assertEquals(RamPoints.Kind.Quran, RamPoints.wordKind(t("a", "سورة الكهف", "مشاري العفاسي")))
        // A singer named Hussein, a child (الطفل): songs.
        assertEquals(null, RamPoints.wordKind(t("a", "بشرة خير", "حسين الجسمي")))
        assertEquals(null, RamPoints.wordKind(t("a", "الطفل", "Someone")))
        assertEquals(RamPoints.Script.Arabic, RamPoints.script(t("a", "يغادر كل ملك", "Basim Al-Karbalaei")))
    }

    @Test
    fun aHusseiniSongBringsOnlyHusseini() {
        val notes = RamPoints.Notes()
        repeat(3) { notes.learn(RamPoints.Kind.Songs, t("x", "x", "حسين الجسمي"), emptyList()) }
        val seed = t("s", "لطمية يا حسين", "باسم الكربلائي")
        val radio = listOf(
            t("a", "نعي الزهراء", "مسلم الوائلي"),
            t("pop", "بشرة خير", "حسين الجسمي"),
            t("b", "ونين", "باسم الكربلائي"),
            t("en", "Blinding Lights", "The Weeknd"),
        )
        val ids = RamPoints.upNext(notes, radio, seed, emptyList(), emptySet(), emptySet(), { 0L }).map { it.videoId }
        assertFalse(ids.toString(), "pop" in ids || "en" in ids)
        assertEquals("b", ids.first())
        assertTrue("a" in ids)
    }

    @Test
    fun aSongBringsOnlySongs() {
        val ids = RamPoints.upNext(RamPoints.Notes(), listOf(t("a", "Alpha", "Other"), t("lat", "لطمية يا حسين", "باسم الكربلائي"), t("b", "Borderline", "Tame Impala")),
            t("s", "Let It Happen", "Tame Impala"), emptyList(), emptySet(), emptySet(), { 0L }).map { it.videoId }
        assertEquals(listOf("b", "a"), ids)
    }

    @Test
    fun recommendMoreAndDontRecommend() {
        val notes = RamPoints.Notes(bias = hashMapOf("b" to 50), hidden = hashSetOf("gone"))
        val ids = RamPoints.upNext(notes, listOf(t("a", "A", "A"), t("gone", "Gone", "C"), t("bb", "B", "B")),
            t("s", "Seed", "Z"), emptyList(), emptySet(), emptySet(), { 0L }).map { it.videoId }
        assertEquals(listOf("bb", "a"), ids)
    }

    @Test
    fun notesSurviveSaving() {
        val notes = RamPoints.Notes()
        repeat(2) { notes.learn(RamPoints.Kind.Husseini, t("s", "لطمية", "رادود"), emptyList()) }
        val back = RamPoints.Notes.decode(notes.encode())
        assertEquals(RamPoints.Kind.Husseini, back.artistKind("رادود"))
    }

    @Test
    fun pointsFollowHowMuchWasHeard() {
        assertEquals(-15f, RamPoints.heardPoints(10f, 240f))
        val early = RamPoints.heardPoints(36f, 240f)
        assertTrue("left at 15%: a little minus", early < 0f && early > -6f)
        assertEquals(0f, RamPoints.heardPoints(72f, 240f), 1e-4f)
        assertEquals(3f, RamPoints.heardPoints(132f, 240f), 1e-3f)
        assertEquals(6f, RamPoints.heardPoints(200f, 240f))
        assertEquals(0f, RamPoints.heardPoints(45f, 0f))
        var last = -100f
        for (s in 30..240 step 5) {
            val now = RamPoints.heardPoints(s.toFloat(), 240f)
            assertTrue("at $s s", now >= last)
            last = now
        }
    }

    @Test
    fun loveReplaysFadingAndFatigue() {
        val day = 24L * 60 * 60 * 1000
        val n = RamPoints.Notes()
        n.played(t("a", "A", "Singer"), true, 1_000_000_000L)
        val first = n.trackPoints("a", 1_000_000_000L + 6 * day)
        n.played(t("a", "A", "Singer"), true, 1_000_000_000L + 10 * 60 * 1000)
        assertTrue("the replay gave extra", n.tracks.getValue("a").points > 20f)
        val before = n.tracks.getValue("a").points
        n.love(t("a", "A", "Singer"), RamPoints.Love.Download)
        assertTrue(n.tracks.getValue("a").points > before)
        assertTrue(first > 0f)
        // Minus points fade faster than plus points.
        val m = RamPoints.Notes()
        m.tracks["up"] = RamPoints.Score(20f, 0L)
        m.tracks["down"] = RamPoints.Score(-20f, 0L)
        assertTrue(kotlin.math.abs(m.trackPoints("down", 10 * day)) < m.trackPoints("up", 10 * day))
        // Five plays this week: it ranks lower until the week is over.
        val f = RamPoints.Notes()
        repeat(5) { k -> f.played(t("b", "B", "Singer"), false, 2_000_000_000L + k * day / 2) }
        val tired = f.trackPoints("b", 2_000_000_000L + 3 * day)
        f.tracks.getValue("b").plays = 1
        assertTrue(tired < f.trackPoints("b", 2_000_000_000L + 3 * day))
    }

    @Test
    fun hijriMonthsAndTheirWeights() {
        // The same days as the PC's test: 1 Muharram 1448, Ramadan 1448, Jumada al-Akhirah.
        assertEquals(1, RamPoints.hijriMonth((1_781_568_000L + 86_400L) * 1000))
        assertEquals(9, RamPoints.hijriMonth(1_802_390_400L * 1000))
        assertEquals(6, RamPoints.hijriMonth(1_794_528_000L * 1000))
        val muharram = RamPoints.contextWeights(RamPoints.Notes(), (1_781_568_000L + 86_400L) * 1000, 12)
        val heard = listOf(RamPoints.Kind.Songs to 1f, RamPoints.Kind.Husseini to 0.9f)
        assertEquals(RamPoints.Kind.Songs, RamPoints.pageKinds(heard)[0])
        assertEquals(RamPoints.Kind.Husseini, RamPoints.pageKinds(heard, muharram)[0])
    }
}
