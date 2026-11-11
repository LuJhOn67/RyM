package com.rymplayer.data

import com.rymplayer.data.YouTubeLinks.Target
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class YouTubeLinksTest {
    @Test
    fun songs() {
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc"))
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123"))
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("https://youtu.be/dQw4w9WgXcQ?si=xyz"))
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("https://m.youtube.com/shorts/dQw4w9WgXcQ"))
    }

    @Test
    fun listsAlbumsArtists() {
        assertEquals(Target.Playlist("PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG"), YouTubeLinks.parse("https://music.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG&si=1"))
        assertEquals(Target.Playlist("RDCLAK5uy_kmPRjHDECIcuVwnKsx2Ng7fyNgFKWNJFs"), YouTubeLinks.parse("https://music.youtube.com/playlist?list=RDCLAK5uy_kmPRjHDECIcuVwnKsx2Ng7fyNgFKWNJFs"))
        assertEquals(Target.Mix("RDAMVMdQw4w9WgXcQ"), YouTubeLinks.parse("https://music.youtube.com/watch?list=RDAMVMdQw4w9WgXcQ"))
        assertEquals(Target.Album("MPREb_abcdef"), YouTubeLinks.parse("https://music.youtube.com/browse/MPREb_abcdef"))
        assertEquals(Target.Playlist("PL123"), YouTubeLinks.parse("https://music.youtube.com/browse/VLPL123"))
        assertEquals(Target.Artist("UCabc"), YouTubeLinks.parse("https://music.youtube.com/channel/UCabc"))
    }

    @Test
    fun sharedTextAndOthers() {
        assertEquals("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=x", YouTubeLinks.firstLink("Listen to this – https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=x."))
        assertNull(YouTubeLinks.parse("https://open.spotify.com/track/abc"))
        assertNull(YouTubeLinks.parse("https://www.youtube.com/feed/library"))
        assertNull(YouTubeLinks.parse("not a link"))
    }

    @Test
    fun rymLinks() {
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("rym://play?url=https%3A%2F%2Fmusic.youtube.com%2Fwatch%3Fv%3DdQw4w9WgXcQ"))
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("rym://play?v=dQw4w9WgXcQ"))
        assertEquals(Target.Song("dQw4w9WgXcQ"), YouTubeLinks.parse("rym://play/dQw4w9WgXcQ"))
        assertEquals(Target.Playlist("PLxyz"), YouTubeLinks.parse("rym://play?list=PLxyz"))
        assertEquals("https://open.spotify.com/track/abc", YouTubeLinks.unwrapRym("rym://play?url=https%3A%2F%2Fopen.spotify.com%2Ftrack%2Fabc"))
        // Not twice: rym:// inside rym:// is refused.
        assertNull(YouTubeLinks.parse("rym://play?url=rym%3A%2F%2Fplay%3Fv%3DdQw4w9WgXcQ"))
        assertEquals("rym://play?v=dQw4w9WgXcQ", YouTubeLinks.firstLink("open rym://play?v=dQw4w9WgXcQ"))
        assertEquals(true, SoundCloud.isSong("https://soundcloud.com/artist/a-song"))
        assertEquals(false, SoundCloud.isSong("https://soundcloud.com/artist/sets/a-set"))
    }
}
