package com.rymplayer.data

import android.content.Context
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.io.File

/**
 * The last home feed and library playlists, saved as JSON so the app shows them the moment it
 * opens; fresh ones replace them when YouTube Music answers.
 */
object PageCache {
    private fun file(ctx: Context, name: String) = File(ctx.filesDir, "cache-$name.json")

    fun saveTracks(ctx: Context, name: String, tracks: List<Track>) {
        val json = buildJsonArray {
            for (t in tracks) add(buildJsonObject {
                put("v", t.videoId)
                put("t", t.title)
                put("a", t.artist)
                put("al", t.album)
                put("d", t.duration)
                t.thumb?.let { put("th", it) }
                t.albumId?.let { put("ai", it) }
                t.artistId?.let { put("ar", it) }
                when (val l = t.link) {
                    is Link.Playlist -> put("lp", l.id)
                    is Link.Album -> put("la", l.id)
                    is Link.Mix -> put("lm", l.id)
                    null -> {}
                }
            })
        }
        runCatching { file(ctx, name).writeText(json.toString()) }
    }

    fun loadTracks(ctx: Context, name: String): List<Track> = read(ctx, name).mapNotNull { e ->
        val o = e as? JsonObject ?: return@mapNotNull null
        fun s(k: String) = (o[k] as? JsonPrimitive)?.content
        val link = s("lp")?.let { Link.Playlist(it) } ?: s("la")?.let { Link.Album(it) } ?: s("lm")?.let { Link.Mix(it) }
        Track(s("v") ?: return@mapNotNull null, s("t").orEmpty(), s("a").orEmpty(), s("al").orEmpty(), s("d").orEmpty(), s("th"), s("ai"), s("ar"), link)
    }

    fun savePlaylists(ctx: Context, playlists: List<Playlist>) {
        val json = buildJsonArray {
            for (p in playlists) add(buildJsonObject {
                put("id", p.id)
                put("t", p.title)
                p.thumb?.let { put("th", it) }
            })
        }
        runCatching { file(ctx, "playlists").writeText(json.toString()) }
    }

    fun loadPlaylists(ctx: Context): List<Playlist> = read(ctx, "playlists").mapNotNull { e ->
        val o = e as? JsonObject ?: return@mapNotNull null
        fun s(k: String) = (o[k] as? JsonPrimitive)?.content
        Playlist(s("id") ?: return@mapNotNull null, s("t").orEmpty(), s("th"))
    }

    fun clear(ctx: Context) {
        listOf("home", "playlists").forEach { file(ctx, it).delete() }
    }

    private fun read(ctx: Context, name: String): List<JsonElement> =
        runCatching { Json.parseToJsonElement(file(ctx, name).readText()) as? JsonArray }.getOrNull() ?: emptyList()
}
