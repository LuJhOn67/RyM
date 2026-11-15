package com.rymplayer.data

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the PC's artist_search_tests. */
class ArtistSearchTest {
    @Test
    fun anArtistByName() {
        assertTrue(namesArtist("linkin park", "Linkin Park"))
        assertTrue(namesArtist("LINKIN", "Linkin Park"))
        assertTrue(namesArtist("محمد عبده", "مُحمد عبدة"))
        assertFalse(namesArtist("adele hello", "Adele"))
        assertFalse(namesArtist("the", "The Weeknd"))
        assertFalse(namesArtist("numb", "Linkin Park"))
    }
}
