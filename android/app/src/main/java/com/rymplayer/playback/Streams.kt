package com.rymplayer.playback

import android.net.Uri
import com.rymplayer.data.arr
import com.rymplayer.data.at
import com.rymplayer.data.str
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
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
import java.util.concurrent.TimeUnit

/**
 * Audio links straight from YouTube's own player API (InnerTube), the same way the PC version
 * gets them. App clients answer with ready-to-use stream URLs, so no page scripts have to run.
 */
object Streams {
    private const val PLAYER = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false"

    private class Client(val name: String, val id: String, val version: String, val userAgent: String, val extra: Map<String, Any>)

    private val clients = listOf(
        Client(
            "ANDROID_VR", "28", "1.65.10",
            "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
            mapOf("deviceMake" to "Oculus", "deviceModel" to "Quest 3", "androidSdkVersion" to 32, "osName" to "Android", "osVersion" to "12L"),
        ),
        Client(
            "IOS", "5", "20.10.4",
            "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
            mapOf("deviceMake" to "Apple", "deviceModel" to "iPhone16,2", "osName" to "iPhone", "osVersion" to "18.3.2.22D82"),
        ),
    )

    private val http = OkHttpClient.Builder().callTimeout(15, TimeUnit.SECONDS).build()

    /** Links stay valid for hours; keep each for one. */
    private class Entry(val url: String, val until: Long)
    private val cache = object : LinkedHashMap<String, Entry>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Entry>) = size > 64
    }

    /** The best audio link for a song: Opus (itag 251) first, then AAC. Null when there is none. */
    suspend fun resolve(videoId: String): String? = withContext(Dispatchers.IO) {
        synchronized(cache) { cache[videoId]?.takeIf { it.until > System.currentTimeMillis() }?.let { return@withContext it.url } }
        var why = ""
        for (c in clients) {
            val url = runCatching { player(c, videoId) }.onFailure { why = it.message.orEmpty() }.getOrNull() ?: continue
            synchronized(cache) { cache[videoId] = Entry(url, System.currentTimeMillis() + 60 * 60 * 1000L) }
            return@withContext url
        }
        throw IllegalStateException(why.ifEmpty { "YouTube gave no audio" })
    }

    /** User agent of the client a link was made for (links say it in `c=`); the stream wants it. */
    fun userAgentFor(uri: Uri): String? {
        val name = uri.getQueryParameter("c") ?: return null
        return clients.firstOrNull { it.name.equals(name, ignoreCase = true) }?.userAgent
    }

    private fun player(c: Client, videoId: String): String {
        val body = buildJsonObject {
            putJsonObject("context") {
                putJsonObject("client") {
                    put("clientName", c.name)
                    put("clientVersion", c.version)
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
        val answer = http.newCall(request).execute().use { r ->
            check(r.isSuccessful) { "HTTP ${r.code}" }
            Json.parseToJsonElement(r.body!!.string())
        }
        return pick(answer)
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
