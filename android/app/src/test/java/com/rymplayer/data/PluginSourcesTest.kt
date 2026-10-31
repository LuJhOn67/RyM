package com.rymplayer.data

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PluginSourcesTest {
    @Test
    fun spotifyLinksAndPages() {
        assertEquals("playlist" to "37i9dQZF1DXcBWIGoYBM5M", Spotify.parseLink("https://open.spotify.com/intl-ar/playlist/37i9dQZF1DXcBWIGoYBM5M?si=ab"))
        assertEquals("album" to "4aawyAB9vmqN3uQ7FjRGTy", Spotify.parseLink("spotify:album:4aawyAB9vmqN3uQ7FjRGTy"))
        assertNull(Spotify.parseLink("https://open.spotify.com/artist/abc"))
        val page = """<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Hits","trackList":[{"title":"Loser","subtitle":"Tame Impala","duration":223069}]}}}}}}</script>"""
        assertEquals(Spotify.SongList("Hits", listOf(Spotify.Wanted("Loser", "Tame Impala", 223.069f))), Spotify.parseEmbed(page))
        val w = Spotify.Wanted("Loser (feat. Someone)", "Tame Impala, Other", 223f)
        val pick = Spotify.best(w, listOf(Track("1", "Loser (Live)", "Cover Band", duration = "4:10"), Track("2", "Loser", "Tame Impala", duration = "3:43")))
        assertEquals("2", pick?.videoId)
        assertNull(Spotify.best(w, listOf(Track("3", "Unrelated", "Nobody", duration = "9:00"))))
    }

    @Test
    fun soundCloudAnswers() {
        assertTrue(SoundCloud.isSoundCloud("sc.42") && !SoundCloud.isSoundCloud("sc.") && !isYouTube("sc.42") && isYouTube("dQw4w9WgXcQ"))
        assertTrue(SoundCloud.isLink("https://soundcloud.com/levity_music/tameflip") && !SoundCloud.isLink("https://youtube.com/x"))
        val answer = Json.parseToJsonElement(
            """{"uploader":"Page","entries":[
              {"id":"1","title":"Full song","artists":["Band"],"duration":200.5,"url":"https://api.soundcloud.com/tracks/1","thumbnails":[{"url":"https://i1.sndcdn.com/a-original.jpg"}]},
              {"id":"2","title":"Go+ preview","duration":30.0},
              {"id":"3","title":"A set","url":"https://soundcloud.com/x/sets/y"},
              {"id":"4","title":"From a page","url":"https://soundcloud.com/x/z"}]}""",
        )
        val got = SoundCloud.tracks(answer)
        assertEquals(listOf("sc.1", "sc.4"), got.map { it.videoId })
        assertEquals("Band", got[0].artist)
        assertEquals("https://i1.sndcdn.com/a-t500x500.jpg", got[0].thumb)
        assertEquals("Page", got[1].artist)
    }
}
