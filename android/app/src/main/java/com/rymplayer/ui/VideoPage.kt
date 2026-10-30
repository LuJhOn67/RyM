package com.rymplayer.ui

import androidx.annotation.OptIn
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Download
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.MergingMediaSource
import androidx.media3.exoplayer.source.ProgressiveMediaSource
import androidx.media3.ui.PlayerView
import com.rymplayer.AppViewModel
import com.rymplayer.data.Track
import com.rymplayer.playback.VideoStreams
import com.rymplayer.tr

/**
 * The video plugin's player: the song's music video full screen, picture and sound streamed
 * apart and played together (its own player, so the music queue is left as it was).
 */
@OptIn(UnstableApi::class)
@Composable
fun VideoPage(vm: AppViewModel, t: Track) {
    val ctx = LocalContext.current
    var links by remember(t.videoId) { mutableStateOf<VideoStreams.Links?>(null) }
    var error by remember(t.videoId) { mutableStateOf<String?>(null) }
    val player = remember { ExoPlayer.Builder(ctx).build() }
    DisposableEffect(Unit) { onDispose { player.release() } }
    LaunchedEffect(t.videoId) {
        runCatching { VideoStreams.links(t.videoId) }.onSuccess { links = it }.onFailure { error = it.message ?: it.javaClass.simpleName }
    }
    LaunchedEffect(links) {
        val l = links ?: return@LaunchedEffect
        val http = DefaultHttpDataSource.Factory().setUserAgent(l.userAgent).setAllowCrossProtocolRedirects(true)
        val factory = ProgressiveMediaSource.Factory(http)
        val picture = factory.createMediaSource(MediaItem.fromUri(l.video))
        val source = l.audio?.let { MergingMediaSource(picture, factory.createMediaSource(MediaItem.fromUri(it))) } ?: picture
        player.setMediaSource(source)
        player.prepare()
        player.play()
    }
    Column(Modifier.fillMaxSize().background(Color.Black).statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton({ vm.video = null }) { Icon(Icons.Default.Close, tr("Close"), tint = Color.White) }
            Column(Modifier.weight(1f)) {
                Text(t.title, color = Color.White, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(t.artist, color = Dim, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton({ vm.downloadLink("https://www.youtube.com/watch?v=${t.videoId}", true) }) {
                Icon(Icons.Default.Download, tr("Download video"), tint = Color.White)
            }
        }
        Box(Modifier.fillMaxSize(), Alignment.Center) {
            when {
                error != null -> Text("${tr("Can't open the video")}: $error", color = Color.White, modifier = Modifier.padding(24.dp))
                links == null -> CircularProgressIndicator(color = Color.White)
                else -> AndroidView({ PlayerView(it).apply { this.player = player } }, Modifier.fillMaxSize())
            }
        }
    }
}
