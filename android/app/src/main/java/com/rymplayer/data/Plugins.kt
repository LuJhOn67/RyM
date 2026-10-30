package com.rymplayer.data

import android.content.Context
import com.rymplayer.I18n
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/**
 * Plugins: listed in the RyM-rustYoutubeMusic-plugins repository on GitHub (its plugins.json,
 * with a copy in the app for when GitHub can't be reached) and switched on per user. A plugin's
 * engine ships inside the app; installing it from the store turns it on.
 *
 * The first plugin, "video", is the YouTube video player and universal downloader: without it
 * RyM stays an audio-only music player.
 */
object Plugins {
    const val INDEX = "https://raw.githubusercontent.com/LuJhOn67/RyM-rustYoutubeMusic-plugins/main/plugins.json"
    const val PAGE = "https://github.com/LuJhOn67/RyM-rustYoutubeMusic-plugins"
    const val VIDEO = "video"

    data class Plugin(
        val id: String,
        val nameEn: String,
        val nameAr: String,
        val version: String,
        val descriptionEn: String,
        val descriptionAr: String,
    ) {
        val name get() = if (I18n.arabic && nameAr.isNotEmpty()) nameAr else nameEn
        val description get() = if (I18n.arabic && descriptionAr.isNotEmpty()) descriptionAr else descriptionEn
    }

    private val http = OkHttpClient.Builder().callTimeout(20, TimeUnit.SECONDS).build()

    fun parse(text: String): List<Plugin> = Json.parseToJsonElement(text).jsonObject["plugins"]?.jsonArray.orEmpty().mapNotNull { e ->
        val o = e.jsonObject
        fun s(k: String) = o[k]?.jsonPrimitive?.contentOrNull.orEmpty()
        val platforms = o["platforms"]?.jsonArray?.map { it.jsonPrimitive.content }.orEmpty()
        if (platforms.isNotEmpty() && "android" !in platforms) return@mapNotNull null
        Plugin(s("id"), s("name"), s("name_ar"), s("version"), s("description"), s("description_ar")).takeIf { it.id.isNotEmpty() }
    }

    /** The store's list, from GitHub (the copy in the app when it can't be reached). */
    suspend fun store(ctx: Context): List<Plugin> = withContext(Dispatchers.IO) {
        val online = runCatching {
            http.newCall(Request.Builder().url(INDEX).build()).execute().use { r ->
                check(r.isSuccessful) { "plugins: ${r.code}" }
                r.body?.string().orEmpty()
            }
        }.getOrNull()
        parse(online ?: ctx.assets.open("plugins.json").bufferedReader().readText())
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("plugins", Context.MODE_PRIVATE)

    /** Installed plugins: id -> version. */
    fun installed(ctx: Context): Map<String, String> =
        prefs(ctx).all.mapNotNull { (k, v) -> (v as? String)?.let { k to it } }.toMap()

    fun set(ctx: Context, id: String, version: String?) {
        prefs(ctx).edit().apply { if (version == null) remove(id) else putString(id, version) }.apply()
    }
}
