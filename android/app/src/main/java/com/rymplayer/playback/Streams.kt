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
    suspend fun playable(ctx: Context, videoId: String): String {
        cachedFile(ctx, videoId)?.let { return Uri.fromFile(it).toString() }
        return kotlinx.coroutines.withContext(Dispatchers.IO) { cachedLink(videoId).url }
    }

    /** Looks up the next song's link while this one plays. */
    fun prefetchLink(videoId: String) {
        scope.async { lookups.withPermit { runCatching { cachedLink(videoId) } } }
    }

    /** Links for songs about to be tapped (the first ones of a page), a few at a time. */
    fun prefetchLinks(videoIds: List<String>) {
        videoIds.filter { it.isNotEmpty() && links[it] == null }.forEach(::prefetchLink)
    }

    /** At most two lookups run ahead at once, so they don't slow down the song being played. */
    private val lookups = kotlinx.coroutines.sync.Semaphore(2)

    private fun cachedLink(videoId: String): Link {
        links[videoId]?.takeIf { it.until > System.currentTimeMillis() }?.let { return it }
        val (url, ext) = runCatching { link(videoId) }.getOrElse {
            android.util.Log.w("RyM", "no audio link for $videoId", it)
            throw IllegalStateException(it.message ?: it.javaClass.simpleName)
        }
        return Link(url, ext, System.currentTimeMillis() + 60 * 60 * 1000L).also { links[videoId] = it }
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
    private fun link(videoId: String): Pair<String, String> {
        if (!ready) synchronized(this) {
            if (!ready) {
                NewPipe.init(OkDownloader, Localization("en", "US"))
                ready = true
            }
        }
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

    /** Writes the link to `file` in ranges, like the PC version. */
    internal fun save(url: String, userAgent: String, file: File) {
        file.outputStream().use { out ->
            var pos = 0L
            var total = Long.MAX_VALUE
            while (pos < total) {
                val req = Request.Builder().url(url)
                    .header("User-Agent", userAgent)
                    .header("Range", "bytes=$pos-${pos + RANGE - 1}")
                    .build()
                media.newCall(req).execute().use { r ->
                    check(r.code == 200 || r.code == 206) { "link refused (HTTP ${r.code})" }
                    val body = checkNotNull(r.body) { "empty answer" }
                    if (r.code == 200) {
                        // The server sent the whole song at once.
                        body.byteStream().copyTo(out)
                        return
                    }
                    total = r.header("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: total
                    val n = body.byteStream().copyTo(out)
                    check(n > 0) { "download stopped at $pos bytes" }
                    pos += n
                }
            }
        }
    }

    private fun trim(ctx: Context) {
        val files = dir(ctx).listFiles { f -> !f.name.endsWith(".part") }?.sortedByDescending { it.lastModified() } ?: return
        files.drop(KEEP).forEach { it.delete() }
    }
}
