package com.rymplayer.playback

import android.content.Context
import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.MediaType.Companion.toMediaType
import org.schabi.newpipe.extractor.MediaFormat
import org.schabi.newpipe.extractor.NewPipe
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.downloader.Downloader
import org.schabi.newpipe.extractor.downloader.Response
import org.schabi.newpipe.extractor.localization.Localization
import org.schabi.newpipe.extractor.services.youtube.YoutubeParsingHelper
import org.schabi.newpipe.extractor.services.youtube.YoutubeJavaScriptPlayerManager
import org.schabi.newpipe.extractor.services.youtube.YoutubeStreamHelper
import org.schabi.newpipe.extractor.localization.ContentCountry
import com.grack.nanojson.JsonObject
import org.schabi.newpipe.extractor.stream.DeliveryMethod
import kotlinx.coroutines.sync.withPermit
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import org.schabi.newpipe.extractor.downloader.Request as PipeRequest

/** OkHttp behind the extractor library. */
private object OkDownloader : Downloader() {
    const val UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0"
    private val http = OkHttpClient.Builder().callTimeout(30, TimeUnit.SECONDS).build()

    override fun execute(request: PipeRequest): Response {
        val body = request.dataToSend()?.toRequestBody()
        val builder = okhttp3.Request.Builder().url(request.url()).method(request.httpMethod(), body).header("User-Agent", UA)
        request.headers().forEach { (k, values) ->
            builder.removeHeader(k)
            values.forEach { builder.addHeader(k, it) }
        }
        http.newCall(builder.build()).execute().use { r ->
            return Response(r.code, r.message, r.headers.toMultimap(), r.body?.string(), r.request.url.toString())
        }
    }
}

/**
 * Song audio. The link comes from NewPipe's extractor, which keeps up with YouTube's protection;
 * the audio is downloaded in 4 MB ranges into the app's cache (like the PC version) and the
 * player plays that file.
 */
object Streams {
    /** Bytes asked for per request: YouTube refuses or slows down open-ended requests. */
    const val RANGE = 4L shl 20
    /** Songs kept in the cache (oldest are removed). */
    private const val KEEP = 40

    private val media = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(30, TimeUnit.SECONDS).build()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    /** A play and a prefetch of the same song share one download. */
    private val inFlight = ConcurrentHashMap<String, Deferred<File>>()

    @Volatile private var ready = false

    /** Links stay valid for hours; each is kept for one so replays and the next song start at once. */
    private class Link(val url: String, val ext: String, val until: Long)
    private val links = ConcurrentHashMap<String, Link>()

    /** What the player should open: the cached file if there is one, else the stream link. */
    /** The signed-in cookie (set by the app), for yt-dlp when YouTube refuses the quick ways. */
    @Volatile var cookie: String? = null

    /** Where song links come from (Settings, like the PC): 0 the quick ways, yt-dlp when YouTube
     *  refuses them; 1 yt-dlp only; 2 the quick ways only. */
    @Volatile var source = 0
        private set
    @Volatile private var appContext: Context? = null

    fun useSource(ctx: Context, which: Int) {
        appContext = ctx.applicationContext
        if (source != which) links.clear()
        source = which
    }

    /** The source saved in Settings (the player's service starts it too, without the app's screen). */
    fun loadSource(ctx: Context) = useSource(ctx, ctx.getSharedPreferences("rym", Context.MODE_PRIVATE).getInt("audio_source", 0))

    suspend fun playable(ctx: Context, videoId: String): String {
        // A song on this phone plays from its own file.
        if (com.rymplayer.data.LocalSongs.isLocal(videoId)) return com.rymplayer.data.LocalSongs.uri(videoId).toString()
        cachedFile(ctx, videoId)?.let { return Uri.fromFile(it).toString() }
        if (com.rymplayer.data.SoundCloud.isSoundCloud(videoId)) return com.rymplayer.data.SoundCloud.streamUrl(ctx, videoId)
        return kotlinx.coroutines.withContext(Dispatchers.IO) {
            runCatching { cachedLink(videoId).url }.getOrElse { refused ->
                // The quick ways only: no yt-dlp.
                if (source == 2) throw IllegalStateException(refused.message ?: "no audio link")
                // Some songs YouTube won't give the visionOS app ("This video is not available"):
                // yt-dlp with your account gets them, slower, into the cache.
                android.util.Log.w("RyM", "quick ways refused $videoId (${refused.message}), trying yt-dlp")
                val got = runCatching { com.rymplayer.data.YtDlp.download(ctx, videoId, cookie) }.getOrElse {
                    throw IllegalStateException("${refused.message}; yt-dlp: ${it.message}")
                }
                val dest = File(dir(ctx), "$videoId.${got.extension}")
                if (!got.renameTo(dest)) got.copyTo(dest, overwrite = true).also { got.delete() }
                trim(ctx)
                Uri.fromFile(dest).toString()
            }
        }
    }

    /**
     * Gets NewPipe and YouTube's player code (needed to unlock every link) ready when the app
     * opens, so the first song tapped doesn't wait for them.
     */
    fun warmUp() {
        scope.async {
            runCatching {
                initNewPipe()
                YoutubeJavaScriptPlayerManager.getUrlWithThrottlingParameterDeobfuscated(
                    "dQw4w9WgXcQ",
                    "https://rr1---sn-warm.googlevideo.com/videoplayback?n=AAAAAAAAAAAAAAAA",
                )
            }.onFailure { android.util.Log.w("RyM", "warm-up: ${it.message}") }
        }
    }

    /** The song's stream link (not a cached file), for an outside player. */
    suspend fun streamLink(videoId: String): String = kotlinx.coroutines.withContext(Dispatchers.IO) { cachedLink(videoId).url }

    /** Looks up the next song's link while this one plays. */
    fun prefetchLink(videoId: String) {
        scope.async { lookups.withPermit { runCatching { RangedHttpDataSource.warm(cachedLink(videoId).url) } } }
    }

    /** Links for songs about to be tapped (the first ones of a page), a few at a time. */
    fun prefetchLinks(videoIds: List<String>) {
        videoIds.filter { it.isNotEmpty() && com.rymplayer.data.isYouTube(it) && links[it] == null }.forEach(::prefetchLink)
    }

    /** At most two lookups run ahead at once, so they don't slow down the song being played. */
    private val lookups = kotlinx.coroutines.sync.Semaphore(2)

    /** One lookup per song at a time: a play waits for a running prefetch of it instead of starting another. */
    private val pending = ConcurrentHashMap<String, Any>()

    /** A song's link to play (blocking; the player's loading thread calls it for rym:// items). */
    fun resolve(videoId: String): String = cachedLink(videoId).url

    private fun cachedLink(videoId: String): Link {
        fun fresh() = links[videoId]?.takeIf { it.until > System.currentTimeMillis() }
        fresh()?.let { return it }
        synchronized(pending.computeIfAbsent(videoId) { Any() }) {
            fresh()?.let { return it }
            val started = System.currentTimeMillis()
            val viaYtDlp = source == 1
            // Signed in with "Best download quality": YouTube Music's own player first (256 kbps
            // with Premium); signed in otherwise: it's the second try when the quick way fails.
            val signedIn = !cookie.isNullOrBlank()
            val wantBest = signedIn && appContext?.getSharedPreferences("rym", Context.MODE_PRIVATE)?.getBoolean("download_best", false) == true
            fun premium() = if (System.currentTimeMillis() < premiumBrokenUntil) null else runCatching { premiumLink(videoId) }
                .onFailure {
                    android.util.Log.w("RyM", "YouTube Music's player failed for $videoId: ${it.message}")
                    // YouTube changed its player's puzzles and they can't be solved yet: not asked
                    // again for half an hour, so each song doesn't wait for a try that fails.
                    if (it.message.orEmpty().contains("deobfuscation", ignoreCase = true)) premiumBrokenUntil = System.currentTimeMillis() + 30 * 60 * 1000L
                }
                .getOrNull()
            val best = if (!viaYtDlp && wantBest) premium() else null
            val fast = if (viaYtDlp || best != null) null else runCatching { fastLink(videoId) }
                .onFailure { android.util.Log.w("RyM", "quick lookup failed for $videoId: ${it.message}") }
                .getOrNull() ?: if (!viaYtDlp && signedIn && !wantBest) premium() else null
            val firstTry = best ?: fast
            val (url, ext) = firstTry ?: runCatching {
                if (viaYtDlp) com.rymplayer.data.YtDlp.youtubeLink(checkNotNull(appContext) { "not ready" }, videoId, cookie) else link(videoId)
            }.getOrElse {
                android.util.Log.w("RyM", "no audio link for $videoId", it)
                throw IllegalStateException(it.message ?: it.javaClass.simpleName)
            }
            val how = if (viaYtDlp) "yt-dlp" else if (best != null) "YouTube Music" else if (fast != null) "quick" else "full NewPipe"
            android.util.Log.w("RyM", "timing: link $videoId took ${System.currentTimeMillis() - started} ms ($how, $ext)")
            return Link(url, ext, System.currentTimeMillis() + 60 * 60 * 1000L).also { links[videoId] = it }
        }
    }

    /** Until when YouTube Music's own player isn't asked (its puzzles couldn't be solved). */
    @Volatile private var premiumBrokenUntil = 0L

    /** "Update parsers": every saved link is looked up again (and YouTube Music's player is tried again). */
    fun forgetAllLinks() {
        links.clear()
        premiumBrokenUntil = 0L
    }

    /** The link stopped working: look it up again next time. */
    fun forgetLink(videoId: String) {
        links.remove(videoId)
    }

    private fun dir(ctx: Context) = File(ctx.cacheDir, "audio").apply { mkdirs() }

    private fun cachedFile(ctx: Context, videoId: String): File? =
        listOf("webm", "m4a").map { File(dir(ctx), "$videoId.$it") }.firstOrNull { it.exists() && it.length() > 0 }

    /** The song's audio file, downloading it first if needed. */
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
        if (com.rymplayer.data.SoundCloud.isSoundCloud(videoId)) {
            return com.rymplayer.data.YtDlp.downloadAudio(ctx, com.rymplayer.data.SoundCloud.url(videoId), dir(ctx), videoId).also { trim(ctx) }
        }
        val found = cachedLink(videoId)
        val (url, ext) = found.url to found.ext
        val part = File(dir(ctx), "$videoId.part")
        val done = File(dir(ctx), "$videoId.$ext")
        try {
            // The stream wants the user agent of the client that made the link; a browser's otherwise.
            runCatching { save(url, userAgentFor(url), part) }.getOrElse { save(url, OkDownloader.UA, part) }
        } catch (e: Exception) {
            part.delete()
            android.util.Log.w("RyM", "download failed for $videoId", e)
            throw e
        }
        check(part.renameTo(done)) { "can't keep the downloaded song" }
        trim(ctx)
        return done
    }

    /** The best audio-only link (Opus or AAC) and its file type. */
    internal fun initNewPipe() {
        if (!ready) synchronized(this) {
            if (!ready) {
                NewPipe.init(OkDownloader, Localization("en", "US"))
                ready = true
            }
        }
    }

    private val english = Localization("en", "US")
    private val country = ContentCountry("US")

    /**
     * The quick way: InnerTube's player with the visionOS app client (the one NewPipe itself
     * uses and YouTube accepts), through NewPipe's request for it, and NewPipe only to work out
     * the link's "n" parameter, whose code it keeps after the first song. Skips the rest of a
     * full NewPipe lookup, which also fetches the song page's details (about 0.85 s against 2.3 s).
     */
    private fun fastLink(videoId: String): Pair<String, String> {
        initNewPipe()
        val cpn = YoutubeParsingHelper.generateContentPlaybackNonce()
        // NewPipe's own visionOS request asks for a fresh visitor id each time. Reusing one for
        // several songs got "Sign in to confirm you're not a bot" from the second song on.
        val answer: JsonObject = YoutubeStreamHelper.getVisionOsPlayerResponse(country, english, videoId, cpn)
        val status = answer.getObject("playabilityStatus")
        check(status.getString("status") == "OK") { status.getString("reason") ?: status.getString("status") ?: "not playable" }
        val formats = answer.getObject("streamingData").getArray("adaptiveFormats")
        val rank = mapOf(251 to 4, 140 to 3, 250 to 2, 249 to 1)
        val best = (0 until formats.size).map { formats.getObject(it) }
            .filter { it.getString("mimeType", "").startsWith("audio/") && it.getString("url") != null }
            .maxWithOrNull(compareBy({ rank[it.getInt("itag")] ?: 0 }, { it.getInt("bitrate") }))
            ?: error("no direct audio link")
        var url = best.getString("url") + "&cpn=" + cpn
        if (Uri.parse(url).getQueryParameter("n") != null) {
            url = YoutubeJavaScriptPlayerManager.getUrlWithThrottlingParameterDeobfuscated(videoId, url)
        }
        return url to (if (best.getString("mimeType", "").contains("mp4")) "m4a" else "webm")
    }

    private const val MUSIC_VERSION = "1.20260707.12.00"

    /**
     * YouTube Music's own web player with your account: Premium gets the 256 kbps Opus stream
     * (itag 774). Its links come with puzzles, solved by NewPipe from YouTube's player script.
     */
    private fun premiumLink(videoId: String): Pair<String, String> {
        val cookie = cookie?.takeIf { it.isNotBlank() } ?: error("not signed in")
        initNewPipe()
        val sapisid = cookie.split(";").map { it.trim() }.firstNotNullOfOrNull { kv ->
            when {
                kv.startsWith("SAPISID=") -> kv.removePrefix("SAPISID=")
                kv.startsWith("__Secure-3PAPISID=") -> kv.removePrefix("__Secure-3PAPISID=")
                else -> null
            }
        } ?: error("no SAPISID in the sign-in")
        val origin = "https://music.youtube.com"
        val ts = System.currentTimeMillis() / 1000
        val hash = java.security.MessageDigest.getInstance("SHA-1").digest("$ts $sapisid $origin".toByteArray()).joinToString("") { "%02x".format(it) }
        val sts = YoutubeJavaScriptPlayerManager.getSignatureTimestamp(videoId)
        val body = org.json.JSONObject()
            .put("context", org.json.JSONObject().put("client", org.json.JSONObject().put("clientName", "WEB_REMIX").put("clientVersion", MUSIC_VERSION).put("hl", "en").put("gl", "US")))
            .put("videoId", videoId)
            .put("playbackContext", org.json.JSONObject().put("contentPlaybackContext", org.json.JSONObject().put("signatureTimestamp", sts)))
            .put("contentCheckOk", true)
            .put("racyCheckOk", true)
        val req = Request.Builder().url("$origin/youtubei/v1/player?prettyPrint=false")
            .header("User-Agent", OkDownloader.UA)
            .header("X-YouTube-Client-Name", "67")
            .header("X-YouTube-Client-Version", MUSIC_VERSION)
            .header("Cookie", cookie)
            .header("Authorization", "SAPISIDHASH ${ts}_$hash")
            .header("Origin", origin)
            .header("X-Origin", origin)
            .header("X-Goog-AuthUser", "0")
            .post(body.toString().toRequestBody("application/json".toMediaType()))
            .build()
        val answer = media.newCall(req).execute().use { r ->
            check(r.isSuccessful) { "YouTube Music answered ${r.code}" }
            org.json.JSONObject(r.body?.string().orEmpty())
        }
        val status = answer.optJSONObject("playabilityStatus")
        check(status?.optString("status") == "OK") { status?.optString("reason")?.ifEmpty { null } ?: status?.optString("status") ?: "not playable" }
        val formats = answer.optJSONObject("streamingData")?.optJSONArray("adaptiveFormats") ?: error("no streams")
        val rank = mapOf(774 to 5, 251 to 4, 140 to 3, 250 to 2, 249 to 1)
        val best = (0 until formats.length()).map { formats.getJSONObject(it) }
            .filter { it.optString("mimeType").startsWith("audio/") && (it.has("url") || it.has("signatureCipher")) }
            .maxWithOrNull(compareBy({ rank[it.optInt("itag")] ?: 0 }, { it.optInt("bitrate") }))
            ?: error("no audio stream")
        var url = if (best.has("url")) best.getString("url") else {
            val parts = best.getString("signatureCipher").split("&").associate { kv ->
                kv.substringBefore("=") to java.net.URLDecoder.decode(kv.substringAfter("=", ""), "UTF-8")
            }
            val sig = YoutubeJavaScriptPlayerManager.deobfuscateSignature(videoId, parts["s"] ?: error("no signature"))
            (parts["url"] ?: error("no link")) + "&" + (parts["sp"] ?: "signature") + "=" + java.net.URLEncoder.encode(sig, "UTF-8")
        }
        if (Uri.parse(url).getQueryParameter("n") != null) {
            url = YoutubeJavaScriptPlayerManager.getUrlWithThrottlingParameterDeobfuscated(videoId, url)
        }
        return url to (if (best.optString("mimeType").contains("mp4")) "m4a" else "webm")
    }

    private fun link(videoId: String): Pair<String, String> {
        initNewPipe()
        // Only the audio is needed: the extractor alone skips the rest of the song page.
        val extractor = ServiceList.YouTube.getStreamExtractor("https://www.youtube.com/watch?v=$videoId")
        extractor.fetchPage()
        val best = extractor.audioStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
            .maxByOrNull { it.averageBitrate }
            ?: error("no playable audio for this song")
        val ext = if (best.format == MediaFormat.M4A) "m4a" else "webm"
        return best.content to ext
    }

    // The apps' own user agents (NewPipe 0.27 no longer has them), the same as the PC's innertube.rs.
    private const val IOS_UA = "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)"
    private const val ANDROID_UA = "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip"

    internal fun userAgentFor(url: String): String = when (Uri.parse(url).getQueryParameter("c")?.uppercase()) {
        "IOS" -> IOS_UA
        "ANDROID", "ANDROID_MUSIC", "ANDROID_VR" -> ANDROID_UA
        else -> OkDownloader.UA
    }

    /**
     * A link that stopped working in the middle of a song (refused, or its data ran out early):
     * the song's link is looked up again. The new one is given only when it's the same file
     * (format and size), so the song can go on from the same byte; null otherwise. Blocking.
     */
    internal fun renewedLink(url: String): String? {
        val videoId = links.entries.firstOrNull { it.value.url == url }?.key ?: return null
        fun key(u: String) = Uri.parse(u).let { it.getQueryParameter("itag") to it.getQueryParameter("clen") }
        links.remove(videoId)
        val fresh = runCatching { cachedLink(videoId).url }.getOrNull() ?: return null
        android.util.Log.w("RyM", "link renewed for $videoId mid-song (same file: ${key(fresh) == key(url)})")
        return fresh.takeIf { key(it) == key(url) && it != url }
    }

    /**
     * Connections a song downloads over at once (1-16), and the piece size in MB: 0 is open, the
     * song is split evenly over the connections. Set from Library > Download connections / piece size.
     * The app sets no speed limit; YouTube limits each connection, so more use more of the line.
     */
    @Volatile var connections = 8
    @Volatile var pieceMb = 0

    /** The first request is small: it tells the song's size so the rest can be split up. */
    private const val FIRST = 256L shl 10

    internal fun save(url: String, userAgent: String, file: File) = save(url, mapOf("User-Agent" to userAgent), file)

    /** Writes the link to `file`: a small first piece tells the size, the rest comes over several connections at once. */
    internal fun save(url: String, headers: Map<String, String>, file: File) {
        java.io.RandomAccessFile(file, "rw").use { raf ->
            raf.setLength(0)
            val channel = raf.channel
            val total = piece(url, headers, 0, FIRST, channel) ?: return
            if (total <= FIRST) {
                raf.setLength(total)
                return
            }
            val parallel = connections.coerceIn(1, 16)
            val size = if (pieceMb <= 0) maxOf(FIRST, (total - FIRST + parallel - 1) / parallel) else pieceMb.toLong() shl 20
            val next = java.util.concurrent.atomic.AtomicLong(FIRST)
            val failed = java.util.concurrent.atomic.AtomicReference<Throwable?>(null)
            val workers = (1..parallel).map {
                Thread {
                    try {
                        while (failed.get() == null) {
                            val start = next.getAndAdd(size)
                            if (start >= total) break
                            piece(url, headers, start, minOf(size, total - start), channel)
                        }
                    } catch (e: Throwable) {
                        failed.compareAndSet(null, e)
                    }
                }.apply { start() }
            }
            workers.forEach { it.join() }
            failed.get()?.let { throw it }
            raf.setLength(total)
        }
    }

    /**
     * Writes bytes [start, start + length) of the link at their place in the file, asking again
     * from where an answer stopped early. Returns the song's size, or null when the server sent
     * the whole song at once (then it's all written).
     */
    private fun piece(url: String, headers: Map<String, String>, start: Long, length: Long, out: java.nio.channels.FileChannel): Long? {
        var pos = start
        val end = start + length
        var total = -1L
        val buf = ByteArray(64 * 1024)
        while (pos < end) {
            val req = Request.Builder().url(url)
                .apply { headers.forEach { (k, v) -> header(k, v) } }
                .header("Range", "bytes=$pos-${end - 1}")
                .build()
            media.newCall(req).execute().use { r ->
                check(r.code == 200 || r.code == 206) { "link refused (HTTP ${r.code})" }
                val input = checkNotNull(r.body) { "empty answer" }.byteStream()
                if (r.code == 200) {
                    // The server ignored the range and sent the whole song.
                    var p = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(java.nio.ByteBuffer.wrap(buf, 0, n), p)
                        p += n
                    }
                    return null
                }
                total = r.header("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: total
                var got = 0L
                while (pos < end) {
                    val n = input.read(buf, 0, minOf(buf.size.toLong(), end - pos).toInt())
                    if (n < 0) break
                    out.write(java.nio.ByteBuffer.wrap(buf, 0, n), pos)
                    pos += n
                    got += n
                }
                check(got > 0) { "download stopped at $pos bytes" }
            }
        }
        check(total > 0) { "the server didn't say the song's size" }
        return total
    }

    /** How much the played songs kept on this phone take (bytes). */
    fun cacheSize(ctx: Context): Long = dir(ctx).listFiles()?.sumOf { it.length() } ?: 0L

    /** "Clear cache": every played song kept on this phone goes, except [keep] (the playing one)
     *  and songs still downloading; the bytes freed. Downloads stay. The same as the PC's. */
    fun clearCache(ctx: Context, keep: String?): Long {
        var freed = 0L
        dir(ctx).listFiles { f -> !f.name.endsWith(".part") && (keep == null || f.nameWithoutExtension != keep) }?.forEach {
            val size = it.length()
            if (it.delete()) freed += size
        }
        return freed
    }

    /** Bytes as "1.4 GB" or "320 MB". */
    fun sizeText(bytes: Long): String {
        val mb = bytes / (1024.0 * 1024.0)
        return if (mb >= 1024) String.format(java.util.Locale.US, "%.1f GB", mb / 1024) else String.format(java.util.Locale.US, "%.0f MB", mb)
    }

    private fun trim(ctx: Context) {
        val files = dir(ctx).listFiles { f -> !f.name.endsWith(".part") }?.sortedByDescending { it.lastModified() } ?: return
        files.drop(KEEP).forEach { it.delete() }
    }
}
