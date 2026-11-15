package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Test

/** The same cases as the PC's likes::the_first_page_merges_into_the_list. */
class LikedListTest {
    private fun merged(top: List<String>, before: List<String>) = LikedList.mergeTop(top, before) { it }

    @Test
    fun theFirstPageMergesIntoTheList() {
        val before = listOf("e", "d", "c", "b", "a")
        // Two new likes on top, "d" unliked; the page ends at "c".
        assertEquals(listOf("g", "f", "e", "c", "b", "a"), merged(listOf("g", "f", "e", "c"), before))
        // Only new songs on the page: the old list follows whole.
        assertEquals(listOf("x", "y", "e", "d", "c", "b", "a"), merged(listOf("x", "y"), before))
        assertEquals(listOf("x"), merged(listOf("x"), emptyList()))
    }
}
