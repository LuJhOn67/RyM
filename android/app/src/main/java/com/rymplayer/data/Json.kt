package com.rymplayer.data

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** `json.at("a/0/b")`: walks objects by key and arrays by index; null when anything is missing. */
fun JsonElement?.at(path: String): JsonElement? {
    var cur: JsonElement? = this
    for (part in path.split('/')) {
        if (part.isEmpty()) continue
        cur = when (cur) {
            is JsonObject -> cur[part]
            is JsonArray -> part.toIntOrNull()?.let { cur.getOrNull(it) }
            else -> return null
        }
    }
    return cur
}

val JsonElement?.str: String? get() = (this as? JsonPrimitive)?.takeIf { it.isString }?.content
val JsonElement?.arr: List<JsonElement> get() = (this as? JsonArray) ?: emptyList()

/** Joins a `{runs: [{text}]}` node into plain text. */
fun JsonElement?.runsText(): String = at("runs").arr.joinToString("") { it.at("text").str.orEmpty() }

/** Largest thumbnail under a renderer, wherever YouTube Music put it this time. */
fun JsonElement?.thumb(): String? {
    val list = at("thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
        ?: at("thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        ?: at("thumbnail/thumbnails")
    return list.arr.lastOrNull().at("url").str
}
