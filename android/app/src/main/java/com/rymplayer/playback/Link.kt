package com.rymplayer.playback

import android.os.Handler
import android.os.Looper
import com.rymplayer.data.Track
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.ServerSocket
import java.net.URLDecoder
import java.security.MessageDigest

/**
 * RyM Link: this phone tells the PCs on its Wi-Fi what it plays, and a PC signed in to the same
 * account can take it over (the song goes on there from the same second, and the phone pauses).
 * A small JSON note by UDP broadcast every 3 seconds (port [PORT]); `GET /state` and
 * `POST /pause` on the TCP port the note gives, only with the account's key. The same as the PC
 * apps' src/link.rs.
 */
object Link {
    /** A computer's note: what it plays, where in the song (when the note came) and its volume. */
    data class PcState(
        val ip: String,
        val port: Int,
        val device: String,
        val videoId: String,
        val title: String,
        val artist: String,
        val thumb: String?,
        val playing: Boolean,
        val position: Float,
        val duration: Float,
        val volume: Float,
        /** Its "Remote from the phone" setting is on. */
        val remote: Boolean,
        val at: Long,
    ) {
        /** Where its song is now: the note's second, plus the time since while it plays. */
        fun positionNow(now: Long = System.currentTimeMillis()): Float = (position + if (playing) (now - at) / 1000f else 0f).coerceAtMost(duration.takeIf { it > 0f } ?: Float.MAX_VALUE)
    }

    const val PORT = 47811

    /** What plays, as the app's player last said: the queue, the song, the second, and since when. */
    data class Snapshot(val queue: List<Track>, val index: Int, val positionMs: Long, val at: Long, val playing: Boolean, val speed: Float) {
        /** Seconds into the song now. */
        fun position(now: Long = System.currentTimeMillis()): Float =
            (positionMs + if (playing) ((now - at) * speed).toLong() else 0L) / 1000f
    }

    @Volatile var snapshot: Snapshot? = null
    /** The account's key (see [accountKey]); null while signed out (nothing is sent then). */
    @Volatile var account: String? = null
    /** Pauses the music (set by the app's player). */
    @Volatile var pause: (() -> Unit)? = null
    /** Music Cross Ears on (the setting): off, nothing is sent and nothing answered. */
    @Volatile var enabled = true

    /** The same key as the PC's for one account: a hash, so the notes don't carry its name. */
    fun accountKey(name: String, handle: String): String =
        MessageDigest.getInstance("SHA-256").digest("rym-link|$name|$handle".toByteArray()).take(8).joinToString("") { "%02x".format(it) }

    private fun trackJson(t: Track) = buildJsonObject {
        put("video_id", t.videoId)
        put("title", t.title)
        put("artist", t.artist)
        put("album", t.album)
        put("duration", t.duration)
        t.thumb?.let { put("thumb", it) }
    }

    fun note(device: String, port: Int): JsonObject? {
        if (!enabled) return null
        val key = account ?: return null
        val s = snapshot
        // A song only on this phone (its own files) isn't offered to the PCs: they can't play it.
        val t = s?.queue?.getOrNull(s.index)?.takeIf { com.rymplayer.data.isYouTube(it.videoId) || com.rymplayer.data.SoundCloud.isSoundCloud(it.videoId) }
        return buildJsonObject {
            put("rym", 1)
            put("kind", "phone")
            put("device", device)
            put("account", key)
            put("port", port)
            put("playing", s?.playing == true && t != null)
            if (t != null) {
                put("video_id", t.videoId)
                put("title", t.title)
                put("artist", t.artist)
                t.thumb?.let { put("thumb", it) }
                put("position", s.position())
            }
        }
    }

    fun state(): JsonObject? {
        val s = snapshot?.takeIf { snap -> snap.queue.getOrNull(snap.index)?.let { !com.rymplayer.data.LocalSongs.isLocal(it.videoId) } == true } ?: return null
        // The queue around the song (100 before, 400 after), like the session file.
        val from = (s.index - 100).coerceAtLeast(0)
        val to = (s.index + 401).coerceAtMost(s.queue.size)
        return buildJsonObject {
            put("songs", buildJsonArray { s.queue.subList(from, to).forEach { add(trackJson(it)) } })
            put("current", s.index - from)
            put("position", s.position())
            put("playing", s.playing)
        }
    }

    private var running = false

    /** Sends the notes and answers the PCs, on two threads, while the service lives. */
    @Synchronized fun start(device: String) {
        if (running) return
        running = true
        val server = runCatching { ServerSocket(0) }.getOrNull() ?: return
        val main = Handler(Looper.getMainLooper())
        Thread({
            while (running) {
                val client = runCatching { server.accept() }.getOrNull() ?: break
                runCatching {
                    client.soTimeout = 3000
                    client.use { c ->
                        val line = c.getInputStream().bufferedReader().readLine().orEmpty()
                        val (method, target) = line.split(' ').let { (it.getOrNull(0) ?: "") to (it.getOrNull(1) ?: "") }
                        val query = target.substringAfter('?', "").split('&').associate {
                            it.substringBefore('=') to URLDecoder.decode(it.substringAfter('=', ""), "UTF-8")
                        }
                        val ok = enabled && account != null && query["account"] == account
                        val (code, body) = when {
                            !ok -> 403 to "{}"
                            method == "GET" && target.startsWith("/state") -> (state()?.let { 200 to it.toString() } ?: (404 to "{}"))
                            method == "POST" && target.startsWith("/pause") -> {
                                main.post { pause?.invoke() }
                                200 to "{}"
                            }
                            else -> 404 to "{}"
                        }
                        val bytes = body.toByteArray()
                        c.getOutputStream().apply {
                            write("HTTP/1.1 $code OK\r\nContent-Type: application/json\r\nContent-Length: ${bytes.size}\r\nConnection: close\r\n\r\n".toByteArray())
                            write(bytes)
                            flush()
                        }
                    }
                }
            }
        }, "rym-link-server").apply { isDaemon = true }.start()
        Thread({
            val socket = runCatching { DatagramSocket().apply { broadcast = true } }.getOrNull() ?: return@Thread
            val everyone = InetAddress.getByName("255.255.255.255")
            while (running) {
                note(device, server.localPort)?.let { n ->
                    val bytes = n.toString().toByteArray()
                    runCatching { socket.send(DatagramPacket(bytes, bytes.size, everyone, PORT)) }
                }
                Thread.sleep(3000)
            }
            socket.close()
            server.close()
        }, "rym-link-notes").apply { isDaemon = true }.start()
    }

    fun stop() {
        running = false
    }

    private var listening = false

    /**
     * Seamless Music the other way, the remote and "Play along": a PC of the same account tells
     * what it plays (or that it's paused); [onPc] hears each note on the main thread.
     */
    @Synchronized fun listen(ctx: android.content.Context, onPc: (PcState) -> Unit) {
        if (listening) return
        listening = true
        val main = Handler(Looper.getMainLooper())
        Thread({
            // Some phones drop broadcasts on Wi-Fi without this.
            val wifi = ctx.applicationContext.getSystemService(android.content.Context.WIFI_SERVICE) as? android.net.wifi.WifiManager
            val lock = wifi?.createMulticastLock("rym-link")?.apply { setReferenceCounted(false); runCatching { acquire() } }
            val socket = runCatching {
                DatagramSocket(null).apply {
                    reuseAddress = true
                    broadcast = true
                    bind(java.net.InetSocketAddress(PORT))
                }
            }.getOrNull()
            if (socket == null) {
                listening = false
                lock?.release()
                return@Thread
            }
            val buf = ByteArray(4096)
            while (listening) {
                val packet = DatagramPacket(buf, buf.size)
                if (runCatching { socket.receive(packet) }.isFailure) break
                val note = runCatching { Json.parseToJsonElement(String(packet.data, 0, packet.length)).jsonObject }.getOrNull() ?: continue
                val key = account ?: continue
                if (!enabled || note["kind"]?.jsonPrimitive?.content != "pc" || note["account"]?.jsonPrimitive?.content != key) continue
                val port = note["port"]?.jsonPrimitive?.intOrNull ?: continue
                val ip = packet.address?.hostAddress ?: continue
                fun str(k: String) = note[k]?.jsonPrimitive?.takeIf { it.isString }?.content.orEmpty()
                fun num(k: String) = note[k]?.jsonPrimitive?.floatOrNull ?: 0f
                val state = PcState(
                    ip, port, str("device"), str("video_id"), str("title"), str("artist"), str("thumb").ifEmpty { null },
                    note["playing"]?.jsonPrimitive?.booleanOrNull == true, num("position"), num("duration"),
                    note["volume"]?.jsonPrimitive?.floatOrNull ?: 1f, note["remote"]?.jsonPrimitive?.booleanOrNull == true,
                    System.currentTimeMillis(),
                )
                main.post { onPc(state) }
            }
            socket.close()
            lock?.release()
        }, "rym-link-listen").apply { isDaemon = true }.start()
    }

    /** One request to a PC, with the account's key (and [query] after it). Blocking. */
    private fun call(ip: String, port: Int, method: String, path: String, query: String = ""): String? {
        val key = account ?: return null
        return runCatching {
            val c = java.net.URL("http://$ip:$port$path?account=$key$query").openConnection() as java.net.HttpURLConnection
            c.requestMethod = method
            c.connectTimeout = 4000
            c.readTimeout = 4000
            if (method == "POST") {
                c.doOutput = true
                c.outputStream.close()
            }
            c.inputStream.bufferedReader().use { it.readText() }.also { c.disconnect() }
        }.getOrNull()
    }

    /** The remote: play, pause, toggle, next, prev, seek (seconds) or volume (0 to 1). Blocking. */
    fun control(ip: String, port: Int, what: String, value: Float = 0f): Boolean =
        call(ip, port, "POST", "/control", "&do=$what&value=$value") != null

    /** Takes the PC's music: its queue, the song in it and the second; then the PC pauses. Blocking. */
    fun takeOver(ip: String, port: Int): Triple<List<Track>, Int, Float>? {
        val got = pcQueue(ip, port) ?: return null
        call(ip, port, "POST", "/pause")
        return got
    }

    /** The PC's queue, the song in it and the second, without pausing it ("Play along"). Blocking. */
    fun pcQueue(ip: String, port: Int): Triple<List<Track>, Int, Float>? {
        val state = call(ip, port, "GET", "/state")?.let { runCatching { Json.parseToJsonElement(it).jsonObject }.getOrNull() } ?: return null
        val songs = state["songs"]?.jsonArray?.mapNotNull { e ->
            val o = e.jsonObject
            val id = o["video_id"]?.jsonPrimitive?.content ?: return@mapNotNull null
            fun str(k: String) = o[k]?.jsonPrimitive?.takeIf { it.isString }?.content.orEmpty()
            Track(id, str("title"), str("artist"), str("album"), str("duration"), str("thumb").ifEmpty { null })
        }.orEmpty()
        val current = state["current"]?.jsonPrimitive?.intOrNull ?: 0
        if (songs.getOrNull(current) == null) return null
        val position = state["position"]?.jsonPrimitive?.floatOrNull ?: 0f
        return Triple(songs, current, position)
    }
}
