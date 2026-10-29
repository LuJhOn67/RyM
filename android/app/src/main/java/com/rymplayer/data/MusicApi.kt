package com.rymplayer.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

/**
 * YouTube Music's web API, signed in with the user's own browser session (the same requests the
 * music.youtube.com website makes). Without a cookie it works as a guest.
 */
class MusicApi(private val cookie: String?) {
    private val http = OkHttpClient.Builder().callTimeout(30, TimeUnit.SECONDS).build()
    private val json = Json { ignoreUnknownKeys = true }
    @Volatile private var clientVersion: String? = null

    private val sapisid: String? = cookie?.split(';')?.map { it.trim() }
        ?.firstOrNull { it.startsWith("SAPISID=") || it.startsWith("__Secure-3PAPISID=") }
        ?.substringAfter('=')

    val signedIn: Boolean get() = sapisid != null

    /** The website's current client version, read from its page once. */
    private fun version(): String {
        clientVersion?.let { return it }
        val req = Request.Builder().url(ORIGIN).header("User-Agent", UA).apply { cookie?.let { header("Cookie", it) } }.build()
        val page = http.newCall(req).execute().use { it.body?.string().orEmpty() }
        val v = page.substringAfter("INNERTUBE_CLIENT_VERSION\":\"", "").substringBefore('"').ifEmpty { FALLBACK_VERSION }
        clientVersion = v
        return v
    }

    private fun authHeader(): String? {
        val s = sapisid ?: return null
        val ts = System.currentTimeMillis() / 1000
        val hash = MessageDigest.getInstance("SHA-1").digest("$ts $s $ORIGIN".toByteArray()).joinToString("") { "%02x".format(it) }
        return "SAPISIDHASH ${ts}_$hash"
    }

    private suspend fun post(endpoint: String, extra: JsonObject, query: String = ""): JsonElement = withContext(Dispatchers.IO) {
        val body = buildJsonObject {
            putJsonObject("context") {
                putJsonObject("client") {
                    put("clientName", "WEB_REMIX")
                    put("clientVersion", version())
                    put("hl", "en")
                    put("gl", "US")
                }
                putJsonObject("user") {}
            }
            extra.forEach { (k, v) -> put(k, v) }
        }
        val req = Request.Builder()
            .url("$ORIGIN/youtubei/v1/$endpoint?alt=json&prettyPrint=false$query")
            .post(body.toString().toRequestBody("application/json".toMediaType()))
            .header("User-Agent", UA)
            .header("Origin", ORIGIN)
            .header("X-Origin", ORIGIN)
            .header("X-Goog-AuthUser", "0")
            .apply {
                cookie?.let { header("Cookie", it) }
                authHeader()?.let { header("Authorization", it) }
            }
            .build()
        http.newCall(req).execute().use { resp ->
            val text = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) throw ApiException("YouTube Music answered ${resp.code}")
            json.parseToJsonElement(text)
        }
    }

    private suspend fun browse(id: String, query: String = ""): JsonElement = post("browse", buildJsonObject { put("browseId", id) }, query)

    // ------------------------------------------------------------------ pages

    /** The home feed: songs and playlist/album/mix cards, `album` = section name. Up to 3 pages. */
    suspend fun home(): List<Track> {
        val out = mutableListOf<Track>()
        var page = browse("FEmusic_home")
        // More pages than one screen needs, so the feed can rotate (see FeedMemory).
        repeat(6) {
            val list = page.at("contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer")
                ?: page.at("continuationContents/sectionListContinuation") ?: return out
            for (shelf in list.at("contents").arr) {
                val s = shelf.at("musicCarouselShelfRenderer") ?: continue
                val section = s.at("header/musicCarouselShelfBasicHeaderRenderer/title").runsText()
                s.at("contents").arr.mapNotNullTo(out) { card(it, section) }
            }
            val token = list.at("continuations/0/nextContinuationData/continuation").str ?: return out
            page = post("browse", JsonObject(emptyMap()), "&ctoken=$token&continuation=$token&type=next")
        }
        return out
    }

    suspend fun libraryPlaylists(): List<Playlist> {
        val page = browse("FEmusic_liked_playlists")
        if (page.at("contents").toString().contains("Sign in to")) throw SignedOutException()
        return page.findAll("musicTwoRowItemRenderer").mapNotNull { r ->
            val id = r.at("navigationEndpoint/browseEndpoint/browseId").str ?: return@mapNotNull null
            if (!id.startsWith("VL") || id == "VLLM") return@mapNotNull null
            Playlist(id, r.at("title").runsText(), r.thumb())
        }
    }

    suspend fun playlist(id: String): List<Track> = songRows(browse(if (id.startsWith("VL")) id else "VL$id"))

    suspend fun album(id: String): List<Track> {
        val page = browse(id)
        val cover = page.findAll("musicThumbnailRenderer").firstOrNull()?.at("thumbnail/thumbnails").arr.lastOrNull().at("url").str
        val name = page.findAll("musicResponsiveHeaderRenderer").firstOrNull()?.at("title").runsText().orEmpty()
        return songRows(page).map { it.copy(thumb = it.thumb ?: cover, album = it.album.ifEmpty { name }, albumId = it.albumId ?: id) }
    }

    suspend fun artist(id: String): List<Track> = songRows(browse(id)).map { it.copy(artistId = it.artistId ?: id) }

    suspend fun search(q: String): List<Track> =
        songRows(post("search", buildJsonObject { put("query", q); put("params", SONGS_FILTER) }))

    /** Listening history; `album` holds the period ("Today", "Yesterday"...). */
    suspend fun history(): List<Track> = browse("FEmusic_history").findAll("musicShelfRenderer").flatMap { shelf ->
        val period = shelf.at("title").runsText()
        shelf.at("contents").arr.mapNotNull { it.at("musicResponsiveListItemRenderer")?.let(::songRow)?.copy(album = period) }
    }

    /** YouTube Music's radio for a song (the song first). */
    suspend fun radio(videoId: String): List<Track> =
        queue(post("next", buildJsonObject { put("videoId", videoId); put("playlistId", "RDAMVM$videoId"); put("isAudioOnly", true) }))

    suspend fun mix(playlistId: String): List<Track> =
        queue(post("next", buildJsonObject { put("playlistId", playlistId); put("isAudioOnly", true) }))

    /** YouTube Music's own (unsynced) lyrics. */
    suspend fun lyrics(videoId: String): Lyrics? {
        val next = post("next", buildJsonObject { put("videoId", videoId) })
        val id = next.findAll("browseEndpoint").mapNotNull { it.at("browseId").str }.firstOrNull { it.startsWith("MPLY") } ?: return null
        val page = browse(id)
        val shelf = (page.findAll("musicDescriptionShelfRenderer") + page.findAll("descriptionShelfRenderer")).firstOrNull() ?: return null
        val text = shelf.at("description").runsText()
        if (text.isBlank()) return null
        val source = shelf.at("footer").runsText().removePrefix("Source: ").ifEmpty { "YouTube Music" }
        return Lyrics(emptyList(), text.lines(), source)
    }

    // ------------------------------------------------------------------ account actions

    suspend fun rate(videoId: String, rating: Rating) {
        val endpoint = when (rating) {
            Rating.Like -> "like/like"
            Rating.Dislike -> "like/dislike"
            Rating.None -> "like/removelike"
        }
        post(endpoint, buildJsonObject { putJsonObject("target") { put("videoId", videoId) } })
    }

    suspend fun createPlaylist(title: String): Playlist {
        val r = post("playlist/create", buildJsonObject { put("title", title); put("privacyStatus", "PRIVATE") })
        val id = r.at("playlistId").str ?: throw ApiException("No playlist id in the answer")
        return Playlist("VL$id", title)
    }

    suspend fun addToPlaylist(playlistId: String, videoId: String) {
        post("browse/edit_playlist", buildJsonObject {
            put("playlistId", playlistId.removePrefix("VL"))
            put("actions", buildJsonArray {
                add(buildJsonObject { put("action", "ACTION_ADD_VIDEO"); put("addedVideoId", videoId) })
            })
        })
    }

    // ------------------------------------------------------------------ parsing

    private fun songRows(page: JsonElement): List<Track> =
        page.findAll("musicResponsiveListItemRenderer").mapNotNull(::songRow)

    /** One song row (playlists, albums, search, history). */
    private fun songRow(r: JsonElement): Track? {
        val videoId = r.at("playlistItemData/videoId").str
            ?: r.findAll("watchEndpoint").firstNotNullOfOrNull { it.at("videoId").str }
            ?: return null
        val col = { i: Int -> r.at("flexColumns/$i/musicResponsiveListItemFlexColumnRenderer/text") }
        var artist: String? = null
        var artistId: String? = null
        var album: String? = null
        var albumId: String? = null
        var duration: String? = null
        for (i in 1..3) {
            for (run in col(i).at("runs").arr) {
                val text = run.at("text").str ?: continue
                val browse = run.at("navigationEndpoint/browseEndpoint")
                when (browse.at("browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType").str) {
                    "MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL" -> {
                        artist = if (artist == null) text else "$artist, $text"
                        artistId = artistId ?: browse.at("browseId").str
                    }
                    "MUSIC_PAGE_TYPE_ALBUM" -> { album = text; albumId = browse.at("browseId").str }
                    else -> if (DURATION.matches(text)) duration = text
                }
            }
        }
        if (artist == null) {
            // Rows without links: "Artist • Album • 3:12".
            val parts = col(1).runsText().split(" • ")
            artist = parts.firstOrNull()
            if (album == null && parts.size > 2) album = parts[1]
        }
        duration = duration
            ?: r.at("fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text").runsText().takeIf { DURATION.matches(it) }
        return Track(
            videoId = videoId,
            title = col(0).runsText(),
            artist = artist.orEmpty(),
            album = album.orEmpty(),
            duration = duration.orEmpty(),
            thumb = r.thumb(),
            albumId = albumId,
            artistId = artistId,
        )
    }

    /** A home card: a song (plays) or a playlist/album/mix (opens). Artists are skipped. */
    private fun card(item: JsonElement, section: String): Track? {
        item.at("musicTwoRowItemRenderer")?.let { r ->
            val nav = r.at("navigationEndpoint") ?: return null
            val title = r.at("title").runsText()
            val sub = r.at("subtitle").runsText()
            val videoId = nav.at("watchEndpoint/videoId").str
            val link: Link? = when {
                videoId != null -> null
                nav.at("watchPlaylistEndpoint/playlistId").str != null -> Link.Mix(nav.at("watchPlaylistEndpoint/playlistId").str!!)
                else -> {
                    val id = nav.at("browseEndpoint/browseId").str ?: return null
                    when {
                        id.startsWith("VL") -> Link.Playlist(id)
                        id.startsWith("MPRE") -> Link.Album(id)
                        else -> return null
                    }
                }
            }
            return Track(videoId.orEmpty(), title, sub, section, thumb = r.thumb(), link = link)
        }
        val r = item.at("musicResponsiveListItemRenderer") ?: return null
        return songRow(r)?.copy(album = section)
    }

    private fun queue(next: JsonElement): List<Track> = next.findAll("playlistPanelVideoRenderer").mapNotNull { r ->
        val videoId = r.at("videoId").str ?: return@mapNotNull null
        val byline = r.at("longBylineText").runsText().split(" • ")
        Track(
            videoId = videoId,
            title = r.at("title").runsText(),
            artist = byline.firstOrNull().orEmpty(),
            album = byline.getOrNull(1).orEmpty(),
            duration = r.at("lengthText").runsText(),
            thumb = r.at("thumbnail/thumbnails").arr.lastOrNull().at("url").str,
        )
    }

    enum class Rating { Like, Dislike, None }

    companion object {
        const val ORIGIN = "https://music.youtube.com"
        private const val UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0"
        private const val FALLBACK_VERSION = "1.20250101.01.00"
        /** The "Songs" filter of the search page. */
        private const val SONGS_FILTER = "EgWKAQIIAWoKEAkQBRAKEAMQBA=="
        private val DURATION = Regex("""^\d+(:\d\d)+$""")
    }
}

class ApiException(message: String) : Exception(message)
class SignedOutException : Exception("YouTube Music says you're signed out")

/** Every object stored under `key`, anywhere in the tree (depth-first). */
fun JsonElement.findAll(key: String): List<JsonElement> {
    val out = mutableListOf<JsonElement>()
    fun walk(e: JsonElement) {
        when (e) {
            is JsonObject -> for ((k, v) in e) {
                if (k == key) out += v
                walk(v)
            }
            is JsonArray -> e.forEach(::walk)
            else -> {}
        }
    }
    walk(this)
    return out
}
