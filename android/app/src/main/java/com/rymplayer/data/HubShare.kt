package com.rymplayer.data

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * RyM huB's "now playing" (the same as the PC's discord::share_now_playing): the song's YouTube id
 * (and the listener's name, if set) to the relay, which posts it in the Discord room. The relay's
 * address is read from RyM's site, so it can move without an update.
 */
object HubShare {
    private const val HUB_INFO = "https://lujhon67.github.io/RyM/hub.json"
    private val http by lazy { OkHttpClient.Builder().callTimeout(8, java.util.concurrent.TimeUnit.SECONDS).build() }
    @Volatile private var address: String? = null
    @Volatile private var looked = false

    private fun address(): String? {
        if (!looked) {
            address = runCatching {
                http.newCall(Request.Builder().url(HUB_INFO).build()).execute().use { r ->
                    Json.parseToJsonElement(r.body?.string().orEmpty()).jsonObject["now_playing"]?.jsonPrimitive?.content
                }
            }.getOrNull()?.takeIf { it.startsWith("https://") }
            looked = true
        }
        return address
    }

    /** Blocking; nothing when RyM huB has no relay yet. */
    fun share(videoId: String, name: String) {
        val to = address() ?: return
        val body = buildJsonObject {
            put("video_id", videoId)
            put("name", name.trim())
        }.toString()
        runCatching { http.newCall(Request.Builder().url(to).post(body.toRequestBody("application/json".toMediaType())).build()).execute().close() }
    }
}
