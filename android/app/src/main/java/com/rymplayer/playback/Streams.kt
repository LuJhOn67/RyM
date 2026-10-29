package com.rymplayer.playback

import android.content.Context
import com.rymplayer.data.arr
import com.rymplayer.data.at
import com.rymplayer.data.str
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * Song audio, fetched the same way as the PC version (which plays everything): the link comes
 * from YouTube's player API (InnerTube), the audio is downloaded in 4 MB ranges with the app
 * client's user agent into the app's cache, and the player plays that file. A client whose link
 * is refused is skipped and the next one asked.
 */
object Streams {
    private const val PLAYER = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false"
    /** Bytes asked for per request: YouTube refuses or slows down open-ended requests. */
    private const val RANGE = 4L shl 20
    /** Songs kept in the cache (oldest are removed). */
    private const val KEEP = 40

    private class Client(val name: String, val id: String, val version: String, val userAgent: String, val extra: Map<String, Any>) {
        val key get() = "$name $version"
    }

    private fun vr(version: String) = Client(
        "ANDROID_VR", "28", version,
        "com.google.android.apps.youtube.vr.oculus/$version (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
        mapOf("deviceMake" to "Oculus", "deviceModel" to "Quest 3", "androidSdkVersion" to 32, "osName" to "Android", "osVersion" to "12L"),
    )

    private val clients = listOf(
        vr("1.62.27"),
        vr("1.65.10"),
        Client(
            "IOS", "5", "20.10.4",
            "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
            mapOf("deviceMake" to "Apple", "deviceModel" to "iPhone16,2", "osName" to "iPhone", "osVersion" to "18.3.2.22D82"),
        ),
    )

    private val api = OkHttpClient.Builder().callTimeout(15, TimeUnit.SECONDS).build()
    private val media = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(30, TimeUnit.SECONDS).build()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    /** A play and a prefetch of the same song share one download. */
    private val inFlight = ConcurrentHashMap<String, Deferred<File>>()

    private fun dir(ctx: Context) = File(ctx.cacheDir, "audio").apply { mkdirs() }

    private fun cachedFile(ctx: Context, videoId: String): File? =
        listOf("webm", "m4a").map { File(dir(ctx), "$videoId.$it") }.firstOrNull { it.exists() && it.length() > 0 }

    /** The song's audio file, downloading it first if needed. Throws the reasons when no client works. */
    suspend fun audioFile(ctx: Context, videoId: String): File {
        cachedFile(ctx, videoId)?.let { return it.apply { setLastModified(System.currentTimeMillis()) } }
        val job = inFlight.computeIfAbsent(videoId) { scope.async { download(ctx.applicationContext, videoId) } }
        try {
            return job.await()
        } finally {
            if (job.isCompleted) inFlight.remove(videoId, job)
        }
    }

    /** Starts downloading a song that will play soon, so it starts right away. */
    fun prefetch(ctx: Context, videoId: String) {
        if (cachedFile(ctx, videoId) != null) return
        scope.async { runCatching { audioFile(ctx, videoId) } }
    }

    /** Throws away a cached file the player couldn't read. */
    fun forget(ctx: Context, videoId: String) {
        cachedFile(ctx, videoId)?.delete()
    }

    private fun download(ctx: Context, videoId: String): File {
        val why = mutableListOf<String>()
        for (c in clients) {
            val asked = runCatching { player(c, videoId) }
            if (asked.isFailure) {
                why += "${c.name}: ${asked.exceptionOrNull()?.message}"
                continue
            }
            val (url, ext) = asked.getOrThrow()
            val part = File(dir(ctx), "$videoId.part")
            val done = File(dir(ctx), "$videoId.$ext")
            val ok = runCatching { save(url, c.userAgent, part) }.onFailure { why += "${c.name}: ${it.message}" }.isSuccess
            if (ok && part.renameTo(done)) {
                trim(ctx)
                return done
            }
            part.delete()
        }
        error(why.joinToString("; ").ifEmpty { "YouTube gave no audio" })
    }

    /** Writes the link to `file` in ranges, like the PC version. */
    internal fun save(url: String, userAgent: String, file: File) {
        file.outputStream().use { out ->
            var pos = 0L
            var total = Long.MAX_VALUE
            while (pos < total) {
                val req = Request.Builder().url(url)
                    .header("User-Agent", userAgent)
                    .header("Range", "bytes=$pos-${pos + RANGE - 1}")
                    .build()
                media.newCall(req).execute().use { r ->
                    check(r.code == 200 || r.code == 206) { "link refused (HTTP ${r.code})" }
                    val body = checkNotNull(r.body) { "empty answer" }
                    if (r.code == 200) {
                        // The server sent the whole song at once.
                        body.byteStream().copyTo(out)
                        return
                    }
                    total = r.header("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: total
                    val n = body.byteStream().copyTo(out)
                    check(n > 0) { "download stopped at $pos bytes" }
                    pos += n
                }
            }
        }
    }

    private fun trim(ctx: Context) {
        val files = dir(ctx).listFiles { f -> !f.name.endsWith(".part") }?.sortedByDescending { it.lastModified() } ?: return
        files.drop(KEEP).forEach { it.delete() }
    }

    /** Asks InnerTube for the song; returns the audio link and its file type. */
    private fun player(c: Client, videoId: String): Pair<String, String> {
        val body = buildJsonObject {
            putJsonObject("context") {
                putJsonObject("client") {
                    put("clientName", c.name)
                    put("clientVersion", c.version)
                    put("userAgent", c.userAgent)
                    put("hl", "en")
                    put("gl", "US")
                    for ((k, v) in c.extra) {
                        when (v) {
                            is Number -> put(k, v)
                            else -> put(k, v.toString())
                        }
                    }
                }
            }
            put("videoId", videoId)
            put("contentCheckOk", true)
            put("racyCheckOk", true)
        }
        val request = Request.Builder().url(PLAYER)
            .header("User-Agent", c.userAgent)
            .header("X-YouTube-Client-Name", c.id)
            .header("X-YouTube-Client-Version", c.version)
            .post(body.toString().toRequestBody("application/json".toMediaType()))
            .build()
        val answer = api.newCall(request).execute().use { r ->
            check(r.isSuccessful) { "HTTP ${r.code}" }
            Json.parseToJsonElement(r.body!!.string())
        }
        val url = pick(answer)
        return url to (if (url.contains("mime=audio%2Fmp4")) "m4a" else "webm")
    }

    /** Chooses the stream from a player answer; throws YouTube's reason when it won't play. */
    fun pick(answer: JsonElement): String {
        val status = answer.at("playabilityStatus/status").str
        check(status == "OK") { answer.at("playabilityStatus/reason").str ?: status ?: "not playable" }
        fun JsonElement.num(k: String) = (at(k) as? JsonPrimitive)?.content?.toLongOrNull() ?: 0L
        val rank = mapOf(251L to 4, 140L to 3, 250L to 2, 249L to 1)
        val best = answer.at("streamingData/adaptiveFormats").arr
            .filter { it.at("mimeType").str?.startsWith("audio/") == true && it.at("url").str != null }
            .maxWithOrNull(compareBy({ rank[it.num("itag")] ?: 0 }, { it.num("bitrate") }))
        return checkNotNull(best?.at("url").str) { "no direct audio stream" }
    }
}
