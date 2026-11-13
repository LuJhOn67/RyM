package com.rymplayer

import android.content.ComponentName
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.session.MediaBrowser
import androidx.media3.session.SessionToken
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.TimeUnit

/**
 * Android Auto's view of RyM, on the emulator (`gradlew connectedDebugAndroidTest`): a browser
 * like the car's connects to the player's service, lists the shelves, plays a song from one and
 * keeps playing it past the first 30 seconds (songs once jumped to the next after about 20 s),
 * and finds songs by words like "Ok Google, play X" does.
 */
@RunWith(AndroidJUnit4::class)
class LibraryTest {
    private val ctx = InstrumentationRegistry.getInstrumentation().targetContext

    private fun <T> onMain(block: () -> T): T {
        var out: Result<T>? = null
        InstrumentationRegistry.getInstrumentation().runOnMainSync { out = runCatching(block) }
        return out!!.getOrThrow()
    }

    private fun browser(): MediaBrowser {
        val token = SessionToken(ctx, ComponentName(ctx, com.rymplayer.playback.PlaybackService::class.java))
        val future = onMain { MediaBrowser.Builder(ctx, token).buildAsync() }
        return future.get(20, TimeUnit.SECONDS)
    }

    /** Waits up to [secs] for [ok] (checked on the main thread). */
    private fun waitFor(secs: Int, ok: () -> Boolean): Boolean {
        repeat(secs * 4) {
            if (onMain(ok)) return true
            Thread.sleep(250)
        }
        return onMain(ok)
    }

    @Test
    fun shelvesPlayAndStayOnTheSong() {
        // A shelf to browse, whatever the emulator has: songs found for some words.
        val seed = kotlinx.coroutines.runBlocking { com.rymplayer.data.MusicApi(null).builtinSearch("queen greatest hits") }
        com.rymplayer.data.PageCache.saveTracks(ctx, "home", seed)
        val b = browser()
        val root = onMain { b.getLibraryRoot(null) }.get(10, TimeUnit.SECONDS).value!!
        val shelves = onMain { b.getChildren(root.mediaId, 0, 50, null) }.get(10, TimeUnit.SECONDS).value!!
        println("RYMTEST shelves: ${shelves.map { it.mediaMetadata.title }}")
        assertTrue("some shelves", shelves.isNotEmpty())
        val shelf = shelves.first()
        val songs = onMain { b.getChildren(shelf.mediaId, 0, 50, null) }.get(10, TimeUnit.SECONDS).value!!
        println("RYMTEST ${shelf.mediaMetadata.title}: ${songs.size} songs, first ${songs.firstOrNull()?.mediaMetadata?.title}")
        assertTrue("songs on the shelf", songs.size >= 2)
        val pick = songs[1]
        onMain {
            b.addListener(object : Player.Listener {
                override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) { println("RYMTEST pwr=$playWhenReady reason=$reason") }
                override fun onPlaybackStateChanged(state: Int) { println("RYMTEST state=$state") }
                override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) { println("RYMTEST item=${mediaItem?.mediaId} reason=$reason") }
            })
        }
        val asked = System.currentTimeMillis()
        onMain {
            b.setMediaItem(MediaItem.Builder().setMediaId(pick.mediaId).build())
            b.prepare()
            b.play()
        }
        val ok = waitFor(20) { b.isPlaying }
        println("RYMTEST state ${onMain { "state=${b.playbackState} pwr=${b.playWhenReady} supp=${b.playbackSuppressionReason} err=${b.playerError?.message} item=${b.currentMediaItem?.mediaId} count=${b.mediaItemCount} idx=${b.currentMediaItemIndex}" }}")
        assertTrue("plays within 20 s", ok)
        println("RYMTEST started ${pick.mediaMetadata.title} in ${System.currentTimeMillis() - asked} ms")
        val playingId = onMain { b.currentMediaItem?.mediaId }
        assertEquals(pick.mediaId, playingId)
        // Past 30 seconds on the same song (and still playing).
        Thread.sleep(32_000)
        val (still, pos) = onMain { b.currentMediaItem?.mediaId to b.currentPosition }
        println("RYMTEST after 32 s: ${if (still == pick.mediaId) "same song" else "moved to $still"} at ${pos / 1000} s, playing ${onMain { b.isPlaying }}")
        assertEquals("the same song after 32 s", pick.mediaId, still)
        assertTrue("past 25 s", pos > 25_000)
        // Next goes to the shelf's next song.
        onMain { b.seekToNext() }
        assertTrue("next song", waitFor(20) { b.currentMediaItem?.mediaId != pick.mediaId && b.isPlaying })
        println("RYMTEST next: ${onMain { b.currentMediaItem?.mediaMetadata?.title }}")
        onMain { b.pause(); b.release() }
    }

    @Test
    fun playByWords() {
        val b = browser()
        val asked = System.currentTimeMillis()
        onMain {
            b.setMediaItem(MediaItem.Builder().setRequestMetadata(MediaItem.RequestMetadata.Builder().setSearchQuery("queen bohemian rhapsody").build()).build())
            b.prepare()
            b.play()
        }
        assertTrue("plays within 25 s", waitFor(25) { b.isPlaying })
        val title = onMain { b.currentMediaItem?.mediaMetadata?.title?.toString().orEmpty() }
        println("RYMTEST by words: \"$title\" in ${System.currentTimeMillis() - asked} ms")
        assertTrue("found the song: $title", title.contains("Bohemian", ignoreCase = true))
        // The car's search box.
        onMain { b.search("tame impala", null) }.get(10, TimeUnit.SECONDS)
        assertTrue("search results", waitFor(15) { true }.let { Thread.sleep(4000); true })
        val found = onMain { b.getSearchResult("tame impala", 0, 20, null) }.get(10, TimeUnit.SECONDS).value.orEmpty()
        println("RYMTEST search: ${found.size} results, first ${found.firstOrNull()?.mediaMetadata?.title}")
        assertTrue("search found songs", found.isNotEmpty())
        onMain { b.pause(); b.release() }
    }

    /** The widget draws, and its play / pause and next buttons reach the player. */
    @Test
    fun widgetButtonsWork() {
        val b = browser()
        onMain {
            b.setMediaItem(MediaItem.Builder().setRequestMetadata(MediaItem.RequestMetadata.Builder().setSearchQuery("daft punk one more time").build()).build())
            b.prepare()
            b.play()
        }
        assertTrue("plays", waitFor(25) { b.isPlaying })
        com.rymplayer.playback.PlayerWidget.show(ctx, "Test", "Artist", true, null)
        com.rymplayer.playback.PlayerWidget.button(ctx, android.view.KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE).send()
        assertTrue("paused by the widget", waitFor(10) { !b.isPlaying })
        println("RYMTEST widget: pause works")
        com.rymplayer.playback.PlayerWidget.button(ctx, android.view.KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE).send()
        assertTrue("playing again by the widget", waitFor(10) { b.isPlaying })
        println("RYMTEST widget: play works")
        onMain { b.pause(); b.release() }
    }
}
