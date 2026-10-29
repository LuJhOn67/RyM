package com.rymplayer.playback

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import org.schabi.newpipe.extractor.NewPipe
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.downloader.Downloader
import org.schabi.newpipe.extractor.downloader.Request
import org.schabi.newpipe.extractor.downloader.Response
import org.schabi.newpipe.extractor.stream.StreamInfo
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/** OkHttp behind the extractor library. */
private object OkDownloader : Downloader() {
    private val http = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .callTimeout(25, TimeUnit.SECONDS)
        .build()

    override fun execute(request: Request): Response {
        val body = request.dataToSend()?.toRequestBody()
        val builder = okhttp3.Request.Builder().url(request.url()).method(request.httpMethod(), body)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0")
        request.headers().forEach { (k, values) -> builder.removeHeader(k); values.forEach { builder.addHeader(k, it) } }
        http.newCall(builder.build()).execute().use { r ->
            return Response(r.code, r.message, r.headers.toMultimap(), r.body?.string(), r.request.url.toString())
        }
    }
}

/** Finds a playable audio URL: the best audio-only stream of a YouTube video. */
object Streams {
    @Volatile private var ready = false
    private val initLock = Any()
    private const val CACHE_SIZE = 128
    private const val CACHE_TTL_MS = 90 * 60 * 1000L
    private data class Entry(val url: String, val expiresAt: Long)
    private val cacheLock = Any()
    private val cache = LinkedHashMap<String, Entry>(CACHE_SIZE, 0.75f, true)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val inFlight = ConcurrentHashMap<String, Deferred<String?>>()
    private val warmSlots = Semaphore(4)

    /** Resolve once per song, reuse signed stream URLs briefly, and share concurrent lookups. */
    suspend fun resolve(videoId: String): String? {
        synchronized(cacheLock) {
            cache[videoId]?.let { entry ->
                if (entry.expiresAt > System.currentTimeMillis()) return entry.url
                cache.remove(videoId)
            }
        }
        val request = inFlight.computeIfAbsent(videoId) {
            scope.async { resolveFresh(videoId) }
        }
        return try {
            request.await()
        } finally {
            if (request.isCompleted) inFlight.remove(videoId, request)
        }
    }

    /** Resolve visible songs ahead of selection, with a small concurrency cap. */
    fun warm(videoId: String) {
        scope.launch { warmSlots.withPermit { runCatching { resolve(videoId) } } }
    }

    private fun resolveFresh(videoId: String): String? {
        if (!ready) synchronized(initLock) {
            if (!ready) {
                NewPipe.init(OkDownloader)
                ready = true
            }
        }
        val info = StreamInfo.getInfo(ServiceList.YouTube, "https://www.youtube.com/watch?v=$videoId")
        val url = info.audioStreams.filter { it.isUrl }.maxByOrNull { it.averageBitrate }?.content
        if (url != null) synchronized(cacheLock) {
            cache[videoId] = Entry(url, System.currentTimeMillis() + CACHE_TTL_MS)
            while (cache.size > CACHE_SIZE) cache.remove(cache.entries.first().key)
        }
        return url
    }
}
