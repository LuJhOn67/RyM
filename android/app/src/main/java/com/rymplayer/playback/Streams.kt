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

    suspend fun playable(ctx: Context, videoId: String): String {
        cachedFile(ctx, videoId)?.let { return Uri.fromFile(it).toString() }
        return kotlinx.coroutines.withContext(Dispatchers.IO) {
            runCatching { cachedLink(videoId).url }.getOrElse { refused ->
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

    /** Looks up the next song's link while this one plays. */
    fun prefetchLink(videoId: String) {
        scope.async { lookups.withPermit { runCatching { RangedHttpDataSource.warm(cachedLink(videoId).url) } } }
    }

    /** Links for songs about to be tapped (the first ones of a page), a few at a time. */
    fun prefetchLinks(videoIds: List<String>) {
        videoIds.filter { it.isNotEmpty() && links[it] == null }.forEach(::prefetchLink)
    }

    /** At most two lookups run ahead at once, so they don't slow down the song being played. */
    private val lookups = kotlinx.coroutines.sync.Semaphore(2)

    /** One lookup per song at a time: a play waits for a running prefetch of it instead of starting another. */
    private val pending = ConcurrentHashMap<String, Any>()

    private fun cachedLink(videoId: String): Link {
        fun fresh() = links[videoId]?.takeIf { it.until > System.currentTimeMillis() }
        fresh()?.let { return it }
        synchronized(pending.computeIfAbsent(videoId) { Any() }) {
            fresh()?.let { return it }
            val started = System.currentTimeMillis()
            val fast = runCatching { fastLink(videoId) }
                .onFailure { android.util.Log.w("RyM", "quick lookup failed for $videoId: ${it.message}") }
                .getOrNull()
            val (url, ext) = fast ?: runCatching { link(videoId) }.getOrElse {
                android.util.Log.w("RyM", "no audio link for $videoId", it)
                throw IllegalStateException(it.message ?: it.javaClass.simpleName)
            }
            val how = if (fast != null) "quick" else "full NewPipe"
            android.util.Log.w("RyM", "timing: link $videoId took ${System.currentTimeMillis() - started} ms ($how, $ext)")
            return Link(url, ext, System.currentTimeMillis() + 60 * 60 * 1000L).also { links[videoId] = it }
        }
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
    private fun initNewPipe() {
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

    internal fun userAgentFor(url: String): String = when (Uri.parse(url).getQueryParameter("c")?.uppercase()) {
        "IOS" -> YoutubeParsingHelper.getIosUserAgent(null)
        "ANDROID", "ANDROID_MUSIC", "ANDROID_VR" -> YoutubeParsingHelper.getAndroidUserAgent(null)
        else -> OkDownloader.UA
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

    private fun trim(ctx: Context) {
        val files = dir(ctx).listFiles { f -> !f.name.endsWith(".part") }?.sortedByDescending { it.lastModified() } ?: return
        files.drop(KEEP).forEach { it.delete() }
    }
}
