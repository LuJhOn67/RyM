package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Test

/** The same cases as the PC apps' clean_title test and the bridge's. */
class CleanTitleTest {
    @Test
    fun titlesLoseTheUploadClutter() {
        for ((title, artist, want) in listOf(
            Triple("Blinding Lights (Official Video)", "The Weeknd", "Blinding Lights"),
            Triple("Tame Impala - The Less I Know The Better [Official Music Video]", "Tame Impala", "The Less I Know The Better"),
            Triple("Song (Lyrics) [HD]", "X", "Song"),
            Triple("Song | Official Audio", "X", "Song"),
            Triple("Song (Official Visualizer) #shorts #music", "X", "Song"),
            Triple("Song (Remix) (Official Video)", "X", "Song (Remix)"),
            Triple("Song (Live at Wembley)", "X", "Song (Live at Wembley)"),
            Triple("Song (feat. Y) [4K]", "X", "Song (feat. Y)"),
            Triple("Song (Official)", "X", "Song (Official)"),
            Triple("Other Artist - Song", "X", "Other Artist - Song"),
            Triple("خطار إجاني (Official Video)", "باسم الكربلائي", "خطار إجاني"),
            Triple("(Official Video)", "X", "(Official Video)"),
        )) assertEquals(title, want, Radio.cleanTitle(title, artist))
    }
}
