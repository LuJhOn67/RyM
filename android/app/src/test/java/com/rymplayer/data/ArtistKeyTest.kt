package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Test

/** The same cases as the PC apps' artist key test. */
class ArtistKeyTest {
    @Test
    fun keysMergeWithoutLosingPoints() {
        assertEquals("adele", Radio.normalizeArtistKey("Adele"))
        assertEquals("adele", Radio.normalizeArtistKey("AdeleVEVO"))
        assertEquals("adele", Radio.normalizeArtistKey("Adele - Topic"))
        assertEquals("tame impala", Radio.normalizeArtistKey("  Tame   Impala Official "))
        assertEquals("a name that is only the ending stays", "vevo", Radio.normalizeArtistKey("VEVO"))
        assertEquals("bruno mars", Radio.firstArtist("Bruno Mars, Lady Gaga"))
        val merged = RamPoints.rekey(hashMapOf("adelevevo" to 4f, "adele" to 2f)) { a, b -> a + b }
        assertEquals(mapOf("adele" to 6f), merged)
    }
}
