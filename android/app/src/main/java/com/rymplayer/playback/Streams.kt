package com.rymplayer.playback

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import org.schabi.newpipe.extractor.NewPipe
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.downloader.Downloader
import org.schabi.newpipe.extractor.downloader.Request
import org.schabi.newpipe.extractor.downloader.Response
import org.schabi.newpipe.extractor.stream.StreamInfo
import java.util.concurrent.TimeUnit

/** OkHttp behind the extractor library. */
private object OkDownloader : Downloader() {
    private val http = OkHttpClient.Builder().readTimeout(30, TimeUnit.SECONDS).build()

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

    suspend fun resolve(videoId: String): String? = withContext(Dispatchers.IO) {
        if (!ready) {
            NewPipe.init(OkDownloader)
            ready = true
        }
        val info = StreamInfo.getInfo(ServiceList.YouTube, "https://www.youtube.com/watch?v=$videoId")
        info.audioStreams.filter { it.isUrl }.maxByOrNull { it.averageBitrate }?.content
    }
}
