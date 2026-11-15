package com.rymplayer.data

/**
 * Reading the liked songs: the whole list (every page, however many) at the start and once a day;
 * between, only its first page (new likes come at the top), merged into the list read before.
 * The same as the PC's likes::merge_top.
 */
object LikedList {
    /** The whole list is read again this often (and at the start). */
    const val FULL_EVERY_MS = 24 * 60 * 60 * 1000L

    /**
     * The first page (newest first) over the list read before: the songs above the page's last
     * known song that aren't on it any more were unliked; the rest of the old list follows.
     * Unlikes further down show at the next whole read.
     */
    fun <T> mergeTop(top: List<T>, before: List<T>, id: (T) -> String): List<T> {
        val onTop = top.map(id).toHashSet()
        val anchor = top.asReversed().firstNotNullOfOrNull { t -> before.indexOfFirst { id(it) == id(t) }.takeIf { it >= 0 } }
        val rest = if (anchor != null) before.drop(anchor + 1) else before
        return top + rest.filter { id(it) !in onTop }
    }
}
