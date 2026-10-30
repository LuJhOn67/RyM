package com.rymplayer.data

import android.content.Context
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.float
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.io.File

/**
 * Lyrics found before, kept on the phone: a song heard before shows its lyrics at once.
 * "None found" is remembered for a day (lyrics get added to LRCLIB all the time).
 */
object LyricsCache {
    private const val NONE_FOR_MS = 24 * 60 * 60 * 1000L

    private fun file(ctx: Context, videoId: String) = File(File(ctx.filesDir, "lyrics").apply { mkdirs() }, "$videoId.json")

    /** Null: not known. `Found(null)`: looked up lately and there were none. */
    class Found(val lyrics: Lyrics?)

    fun get(ctx: Context, videoId: String): Found? = runCatching {
        val f = file(ctx, videoId)
        if (!f.exists()) return null
        val e = Json.parseToJsonElement(f.readText())
        if (e is JsonNull) return if (System.currentTimeMillis() - f.lastModified() < NONE_FOR_MS) Found(null) else null
        val o = e.jsonObject
        Found(
            Lyrics(
                o["synced"]!!.jsonArray.map { it.jsonArray[0].jsonPrimitive.float to it.jsonArray[1].jsonPrimitive.content },
                o["plain"]!!.jsonArray.map { it.jsonPrimitive.content },
                o["source"]!!.jsonPrimitive.content,
            )
        )
    }.getOrNull()

    fun put(ctx: Context, videoId: String, lyrics: Lyrics?) {
        val json = if (lyrics == null) JsonNull else buildJsonObject {
            put("synced", buildJsonArray { lyrics.synced.forEach { (t, l) -> add(buildJsonArray { add(t); add(l) }) } })
            put("plain", buildJsonArray { lyrics.plain.forEach { add(it) } })
            put("source", lyrics.source)
        }
        runCatching { file(ctx, videoId).writeText(json.toString()) }
    }
}
