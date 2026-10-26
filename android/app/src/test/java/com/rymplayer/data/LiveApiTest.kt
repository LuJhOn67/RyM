package com.rymplayer.data

import kotlinx.coroutines.runBlocking
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File

/** Hits the network with the desktop app's saved session. Prints counts only, never the cookie. */
class LiveApiTest {
    private val cookieFile = File(System.getenv("LOCALAPPDATA") ?: "", "RymPlayer/data/cookie.txt")

    @Test
    fun browseAndLyrics() = runBlocking {
        assumeTrue(System.getenv("RYM_LIVE") != null && cookieFile.exists())
        val api = YtMusic(cookieFile.readText().trim())
        val home = api.home()
        println("home: ${home.size} items, sections ${home.map { it.album }.distinct().size}, songs ${home.count { it.link == null }}")
        println("playlists: ${api.libraryPlaylists().size}")
        val liked = api.playlist(LIKED_ID)
        println("liked: ${liked.size}, with duration ${liked.count { it.duration.isNotEmpty() }}, with album id ${liked.count { it.albumId != null }}")
        val found = api.search("Rick Astley Never Gonna Give You Up")
        println("search: ${found.size}, first ${found.firstOrNull()?.title} / ${found.firstOrNull()?.artist}")
        val first = found.first()
        println("radio: ${api.radio(first.videoId).size}")
        println("history: ${api.history().size}")
        first.albumId?.let { println("album: ${api.album(it).size}") }
        first.artistId?.let { println("artist: ${api.artist(it).size}") }
        val synced = Lrclib.find(first.title, first.artist, first.album, parseDuration(first.duration))
        println("lrclib: ${synced?.synced?.size} synced lines")
        println("ytm lyrics: ${api.lyrics(first.videoId)?.plain?.size} lines")
    }
}
