package com.rymplayer.playback

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class StreamsTest {
    @Test
    fun picksOpusOverAac() {
        val answer = Json.parseToJsonElement(
            """{"playabilityStatus":{"status":"OK"},"streamingData":{"adaptiveFormats":[
              {"itag":137,"mimeType":"video/mp4","url":"v","bitrate":4000000},
              {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"a","bitrate":130000},
              {"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","url":"o","bitrate":140000}]}}""",
        )
        assertEquals("o", Streams.pick(answer))
    }

    @Test
    fun saysWhyASongWontPlay() {
        val answer = Json.parseToJsonElement("""{"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":"Sign in to confirm your age"}}""")
        val e = assertThrows(IllegalStateException::class.java) { Streams.pick(answer) }
        assertEquals("Sign in to confirm your age", e.message)
    }
}

class DownloadNameTest {
    @Test
    fun safeNames() {
        assertEquals("AC_DC - What_ _Live_", com.rymplayer.data.Downloads.safeName("AC/DC - What? <Live>"))
        assertEquals("A - Song", com.rymplayer.data.Downloads.safeName("A - Song..."))
    }
}
