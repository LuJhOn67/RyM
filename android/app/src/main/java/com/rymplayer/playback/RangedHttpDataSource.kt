package com.rymplayer.playback

import android.net.Uri
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.BaseDataSource
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.IOException
import java.io.InputStream
import java.util.concurrent.TimeUnit

/**
 * Lets the player stream YouTube audio the way the downloads fetch it: bounded 4 MB `Range`
 * requests with the right user agent, one after the other. Playback starts as soon as the first
 * bytes arrive, and seeking opens a new range where it lands.
 */
@OptIn(UnstableApi::class)
class RangedHttpDataSource private constructor(private val http: OkHttpClient) : BaseDataSource(true) {
    class Factory : DataSource.Factory {
        override fun createDataSource(): DataSource = RangedHttpDataSource(client)
    }

    private var uri: Uri? = null
    private var url = ""
    private var agent = ""
    private var spec: DataSpec? = null
    private var response: Response? = null
    private var body: InputStream? = null
    /** Next byte to read, end of the open range (exclusive), and the whole file's size. */
    private var pos = 0L
    private var end = 0L
    private var total = C.LENGTH_UNSET.toLong()
    private var remaining = C.LENGTH_UNSET.toLong()

    override fun open(dataSpec: DataSpec): Long {
        spec = dataSpec
        uri = dataSpec.uri
        url = dataSpec.uri.toString()
        agent = Streams.userAgentFor(url)
        transferInitializing(dataSpec)
        pos = dataSpec.position
        openRange()
        transferStarted(dataSpec)
        remaining = when {
            dataSpec.length != C.LENGTH_UNSET.toLong() -> dataSpec.length
            total > 0 -> total - dataSpec.position
            else -> C.LENGTH_UNSET.toLong()
        }
        return remaining
    }

    private fun openRange() {
        closeRange()
        val req = Request.Builder().url(url)
            .header("User-Agent", agent)
            .header("Range", "bytes=$pos-${pos + Streams.RANGE - 1}")
            .build()
        val r = http.newCall(req).execute()
        if (r.code != 200 && r.code != 206) {
            r.close()
            throw IOException("link refused (HTTP ${r.code})")
        }
        val length = r.body?.contentLength() ?: -1
        if (r.code == 206) {
            total = r.header("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: total
            end = if (length >= 0) pos + length else Long.MAX_VALUE
        } else {
            // The whole file came back: skip to where we are.
            total = if (length >= 0) length else total
            end = if (total > 0) total else Long.MAX_VALUE
            r.body?.byteStream()?.skip(pos)
        }
        response = r
        body = r.body?.byteStream()
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (length == 0) return 0
        if (remaining == 0L || (total > 0 && pos >= total)) return C.RESULT_END_OF_INPUT
        if (pos >= end) openRange()
        val want = if (remaining != C.LENGTH_UNSET.toLong()) minOf(length.toLong(), remaining).toInt() else length
        val n = body?.read(buffer, offset, want) ?: -1
        if (n == -1) {
            if (total > 0 && pos < total) {
                // The range ended early: ask again from here.
                end = pos
                return read(buffer, offset, length)
            }
            return C.RESULT_END_OF_INPUT
        }
        pos += n
        if (remaining != C.LENGTH_UNSET.toLong()) remaining -= n
        bytesTransferred(n)
        return n
    }

    override fun getUri(): Uri? = uri

    private fun closeRange() {
        runCatching { body?.close() }
        runCatching { response?.close() }
        body = null
        response = null
    }

    override fun close() {
        closeRange()
        if (spec != null) {
            spec = null
            transferEnded()
        }
        uri = null
    }

    private companion object {
        val client: OkHttpClient = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(30, TimeUnit.SECONDS).build()
    }
}
