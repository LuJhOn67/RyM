package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RamPointsTest {
    private fun t(id: String, title: String, artist: String) = Track(id, title, artist)

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
    fun notesSurviveSaving() {
        val notes = RamPoints.Notes()
        repeat(2) { notes.learn(RamPoints.Kind.Husseini, t("s", "لطمية", "رادود"), emptyList()) }
        val back = RamPoints.Notes.decode(notes.encode())
        assertEquals(RamPoints.Kind.Husseini, back.artistKind("رادود"))
    }
}
