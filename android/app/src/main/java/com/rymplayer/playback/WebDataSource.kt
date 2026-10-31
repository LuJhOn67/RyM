package com.rymplayer.playback

import android.net.Uri
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.TransferListener

/**
 * Web links for the player: YouTube's streams (googlevideo.com) through [RangedHttpDataSource],
 * any other (SoundCloud's HLS pieces) through a plain HTTP source.
 */
@OptIn(UnstableApi::class)
class WebDataSource private constructor() : DataSource {
    class Factory : DataSource.Factory {
        override fun createDataSource(): DataSource = WebDataSource()
    }

    private val ranged = RangedHttpDataSource.Factory().createDataSource()
    private val plain = DefaultHttpDataSource.Factory().setAllowCrossProtocolRedirects(true).createDataSource()
    private var open: DataSource? = null

    override fun addTransferListener(transferListener: TransferListener) {
        ranged.addTransferListener(transferListener)
        plain.addTransferListener(transferListener)
    }

    override fun open(dataSpec: DataSpec): Long {
        val source = if (dataSpec.uri.host.orEmpty().endsWith("googlevideo.com")) ranged else plain
        open = source
        return source.open(dataSpec)
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int = checkNotNull(open) { "not open" }.read(buffer, offset, length)

    override fun getUri(): Uri? = open?.uri

    override fun getResponseHeaders(): Map<String, List<String>> = open?.responseHeaders ?: emptyMap()

    override fun close() {
        open?.close()
        open = null
    }
}
