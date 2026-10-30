package com.rymplayer.playback

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.stream.DeliveryMethod

/**
 * The video plugin's links: a song's music video as a picture stream (up to 1080p) and the sound
 * stream to play with it, found the way NewPipe finds them.
 */
object VideoStreams {
    data class Links(val video: String, val audio: String?, val userAgent: String)

    suspend fun links(videoId: String): Links = withContext(Dispatchers.IO) {
        Streams.initNewPipe()
        val extractor = ServiceList.YouTube.getStreamExtractor("https://www.youtube.com/watch?v=$videoId")
        extractor.fetchPage()
        val picture = extractor.videoOnlyStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP && it.height in 1..1080 }
            .maxByOrNull { it.height }
        val sound = extractor.audioStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
            .maxByOrNull { it.averageBitrate }
        if (picture != null && sound != null) {
            return@withContext Links(picture.content, sound.content, Streams.userAgentFor(picture.content))
        }
        // Picture and sound together (lower quality), when they aren't offered apart.
        val both = extractor.videoStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
            .maxByOrNull { it.height }
            ?: error("no video for this song")
        Links(both.content, null, Streams.userAgentFor(both.content))
    }
}
