package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/**
 * Update notices: the newest release of RyM on GitHub, compared with this build. Pre-releases
 * count only on the beta channel (Settings > Beta updates).
 */
object Updates {
    const val REPO = "LuJhOn67/RyM"
    /** How often the app looks on its own. */
    const val EVERY_MS = 12 * 60 * 60 * 1000L

    data class Release(
        val version: String,
        val title: String,
        val notes: String,
        val beta: Boolean,
        val page: String,
        /** The phone's file (the APK), or null when the release has none. */
        val apk: String?,
    ) {
        /** The notes without Markdown marks. */
        val plainNotes: String
            get() = notes.lines()
                .filterNot { it.trim().startsWith("|---") }
                .joinToString("\n") { it.trimEnd().trimStart('#', '>').trimStart().replace("**", "").replace("`", "") }
    }

    private val http = OkHttpClient.Builder().callTimeout(20, TimeUnit.SECONDS).build()

    /** "v6.7", "6.7.0" -> [6, 7]; not a version: empty. */
    fun parse(v: String): List<Int> {
        val core = v.trim().trimStart('v', 'V').takeWhile { it.isDigit() || it == '.' }
        val parts = core.split('.').map { it.toIntOrNull() ?: return emptyList() }.toMutableList()
        while (parts.size > 1 && parts.last() == 0) parts.removeAt(parts.lastIndex)
        return parts
    }

    /** [a] is a later version than [b]. */
    fun newer(a: String, b: String): Boolean {
        val x = parse(a)
        val y = parse(b)
        if (x.isEmpty()) return false
        for (i in 0 until maxOf(x.size, y.size)) {
            val p = x.getOrElse(i) { 0 }
            val q = y.getOrElse(i) { 0 }
            if (p != q) return p > q
        }
        return false
    }

    /** The newest release in GitHub's list newer than [current] (pre-releases only when [beta]). */
    fun pick(list: JsonArray, beta: Boolean, current: String): Release? = list
        .map { it.jsonObject }
        .filter { it["draft"]?.jsonPrimitive?.booleanOrNull != true }
        .filter { beta || it["prerelease"]?.jsonPrimitive?.booleanOrNull != true }
        .mapNotNull { r ->
            val tag = r["tag_name"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            if (!tag.startsWith("v") || parse(tag).isEmpty()) return@mapNotNull null
            tag.removePrefix("v") to r
        }
        .maxWithOrNull { a, b -> if (newer(a.first, b.first)) 1 else if (newer(b.first, a.first)) -1 else 0 }
        ?.takeIf { newer(it.first, current) }
        ?.let { (version, r) ->
            val assets = r["assets"]?.jsonArray.orEmpty().map { it.jsonObject }
            fun str(e: JsonElement?) = e?.jsonPrimitive?.contentOrNull.orEmpty()
            Release(
                version = version,
                title = str(r["name"]),
                notes = str(r["body"]),
                beta = r["prerelease"]?.jsonPrimitive?.booleanOrNull == true,
                page = str(r["html_url"]),
                apk = assets.firstOrNull { str(it["name"]).endsWith(".apk") }?.let { str(it["browser_download_url"]) },
            )
        }

    /** A release newer than [current], if there is one. */
    suspend fun check(beta: Boolean, current: String): Release? = withContext(Dispatchers.IO) {
        val req = Request.Builder()
            .url("https://api.github.com/repos/$REPO/releases?per_page=30")
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "RyM-Android")
            .build()
        http.newCall(req).execute().use { resp ->
            if (!resp.isSuccessful) throw ApiException("GitHub answered ${resp.code}")
            pick(Json.parseToJsonElement(resp.body?.string().orEmpty()).jsonArray, beta, current)
        }
    }
}
