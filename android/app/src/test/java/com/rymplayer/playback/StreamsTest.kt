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

/** Ranged download against a local server that answers Range requests (cutting some short). */
class RangedSaveTest {
    @Test
    fun savesWholeFileInRanges() {
        val data = ByteArray((4 shl 20) * 2 + 12345) { (it * 7 % 251).toByte() }
        val server = java.net.ServerSocket(0)
        val agents = java.util.Collections.synchronizedList(mutableListOf<String>())
        Thread {
            var n = 0
            while (!server.isClosed) {
                val s = runCatching { server.accept() }.getOrNull() ?: break
                s.use { sock ->
                    val reader = sock.getInputStream().bufferedReader()
                    var range: Pair<Int, Int>? = null
                    while (true) {
                        val line = reader.readLine() ?: break
                        if (line.isEmpty()) break
                        if (line.lowercase().startsWith("range: bytes=")) {
                            val (a, b) = line.substringAfter('=').split('-')
                            range = a.trim().toInt() to b.trim().toInt()
                        }
                        if (line.lowercase().startsWith("user-agent:")) agents += line.substringAfter(':').trim()
                    }
                    val (a, b0) = range ?: (0 to data.size - 1)
                    val b = minOf(b0, data.size - 1).let { if (n++ % 3 == 2 && it - a > 2000) it - 1000 else it }
                    val out = sock.getOutputStream()
                    out.write("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes $a-$b/${data.size}\r\nContent-Length: ${b - a + 1}\r\nConnection: close\r\n\r\n".toByteArray())
                    out.write(data, a, b - a + 1)
                    out.flush()
                }
            }
        }.start()
        val file = java.io.File.createTempFile("rym", ".webm")
        try {
            Streams.save("http://127.0.0.1:${server.localPort}/audio", "test-agent", file)
            org.junit.Assert.assertArrayEquals(data, file.readBytes())
            org.junit.Assert.assertTrue(agents.isNotEmpty() && agents.all { it == "test-agent" })
        } finally {
            server.close()
            file.delete()
        }
    }
}
