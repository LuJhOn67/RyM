package com.rymplayer.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.Lyrics
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rymplayer.AppViewModel
import com.rymplayer.data.Lrclib
import com.rymplayer.data.fmtTime

@Composable
fun MiniPlayer(vm: AppViewModel, tint: Color) {
    val p = vm.player
    val t = p.current ?: return
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp).clip(RoundedCornerShape(14.dp))
            .background(lerp(Color(0xFF1C1C21), tint, 0.35f)).clickable { vm.playerOpen = true },
    ) {
        Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Cover(t.thumb, 46.dp, 120, 8.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Marquee(t.title, 15, FontWeight.SemiBold, Color.White)
                Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
            }
            PlayPause(vm, 28)
            IconButton(p::next) { Icon(Icons.Default.SkipNext, "Next", tint = Color.White) }
        }
        val dur = p.duration
        LinearProgressIndicator(
            progress = { if (dur > 0) (p.position / dur).coerceIn(0f, 1f) else 0f },
            modifier = Modifier.fillMaxWidth().height(2.dp),
            color = Color.White,
            trackColor = Color.White.copy(alpha = 0.15f),
            drawStopIndicator = {},
        )
    }
}

@Composable
private fun PlayPause(vm: AppViewModel, size: Int) {
    val p = vm.player
    if (p.loading) {
        CircularProgressIndicator(Modifier.size((size + 14).dp).padding(8.dp), strokeWidth = 2.dp, color = Color.White)
    } else {
        IconButton(p::toggle) {
            Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, "Play", tint = Color.White, modifier = Modifier.size(size.dp))
        }
    }
}

private enum class Panel { Cover, Lyrics, Queue }

@Composable
fun FullPlayer(vm: AppViewModel, tint: Color) {
    val p = vm.player
    val t = p.current ?: return
    var panel by remember { mutableStateOf(Panel.Cover) }
    val bg = Brush.verticalGradient(listOf(lerp(Color.Black, tint, 0.55f), lerp(Color.Black, tint, 0.18f), Color.Black))
    Column(Modifier.fillMaxSize().background(bg).statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton({ vm.playerOpen = false }) { Icon(Icons.Default.KeyboardArrowDown, "Close", tint = Color.White, modifier = Modifier.size(30.dp)) }
            Text("Now playing", Modifier.weight(1f), textAlign = TextAlign.Center, fontWeight = FontWeight.SemiBold, color = Color.White.copy(alpha = 0.8f))
            Spacer(Modifier.width(48.dp))
        }
        AnimatedContent(panel, transitionSpec = { fadeIn(tween(250)) togetherWith fadeOut(tween(200)) }, modifier = Modifier.weight(1f), label = "panel") { shown ->
            when (shown) {
                Panel.Cover -> BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    // The cover breathes: full size while playing, slightly smaller when paused.
                    val scale by animateFloatAsState(if (p.playing) 1f else 0.86f, spring(Spring.DampingRatioMediumBouncy, Spring.StiffnessLow), label = "cover")
                    val side = minOf(maxWidth, maxHeight) - 48.dp
                    Box(Modifier.scale(scale).shadow(24.dp, RoundedCornerShape(16.dp))) { Cover(t.thumb, side, 720, 16.dp) }
                }
                Panel.Lyrics -> LyricsView(vm)
                Panel.Queue -> QueueView(vm)
            }
        }
        Column(Modifier.padding(horizontal = 24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Marquee(t.title, 22, FontWeight.Bold, Color.White)
                    Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Color.White.copy(alpha = 0.7f), fontSize = 16.sp)
                }
                if (vm.api.signedIn) {
                    val liked = t.videoId in vm.liked
                    IconButton({ vm.toggleLike(t) }) {
                        Icon(if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder, "Like", tint = if (liked) MaterialTheme.colorScheme.primary else Color.White)
                    }
                }
            }
            val dur = p.duration
            var dragging by remember { mutableStateOf<Float?>(null) }
            Slider(
                value = (dragging ?: p.position).coerceIn(0f, dur.coerceAtLeast(1f)),
                onValueChange = { dragging = it },
                onValueChangeFinished = { dragging?.let(p::seekTo); dragging = null },
                valueRange = 0f..dur.coerceAtLeast(1f),
                colors = SliderDefaults.colors(thumbColor = Color.White, activeTrackColor = Color.White, inactiveTrackColor = Color.White.copy(alpha = 0.2f)),
            )
            Row {
                Text(fmtTime(dragging ?: p.position), color = Dim, fontSize = 12.sp)
                Spacer(Modifier.weight(1f))
                Text(fmtTime(dur), color = Dim, fontSize = 12.sp)
            }
            Row(Modifier.fillMaxWidth().padding(vertical = 10.dp), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                IconButton(p::prev, Modifier.size(56.dp)) { Icon(Icons.Default.SkipPrevious, "Previous", tint = Color.White, modifier = Modifier.size(36.dp)) }
                Box(Modifier.size(76.dp).clip(CircleShape).background(Color.White).clickable(onClick = p::toggle), contentAlignment = Alignment.Center) {
                    if (p.loading) {
                        CircularProgressIndicator(Modifier.size(34.dp), color = Color.Black, strokeWidth = 3.dp)
                    } else {
                        Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, "Play", tint = Color.Black, modifier = Modifier.size(40.dp))
                    }
                }
                IconButton(p::next, Modifier.size(56.dp)) { Icon(Icons.Default.SkipNext, "Next", tint = Color.White, modifier = Modifier.size(36.dp)) }
            }
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                PanelButton(Icons.Default.Lyrics, "Lyrics", panel == Panel.Lyrics) { panel = if (panel == Panel.Lyrics) Panel.Cover else Panel.Lyrics }
                PanelButton(Icons.AutoMirrored.Filled.QueueMusic, "Up next", panel == Panel.Queue) { panel = if (panel == Panel.Queue) Panel.Cover else Panel.Queue }
            }
        }
    }
}

@Composable
private fun PanelButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, active: Boolean, onClick: () -> Unit) {
    val color by animateColorAsState(if (active) Color.White else Color.White.copy(alpha = 0.55f), label = "panelBtn")
    Row(Modifier.clip(RoundedCornerShape(20.dp)).clickable(onClick = onClick).padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = color)
        Spacer(Modifier.width(8.dp))
        Text(label, color = color, fontWeight = FontWeight.SemiBold)
    }
}

/** Synced lyrics glide so the sung line stays in the upper third; it grows and lights up. */
@Composable
private fun LyricsView(vm: AppViewModel) {
    val l = vm.lyrics ?: return Box(Modifier.fillMaxSize(), Alignment.Center) { Text(vm.lyricsStatus, color = Dim) }
    val state = rememberLazyListState()
    if (l.synced.isEmpty()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(24.dp)) {
            itemsIndexed(l.plain) { _, s -> Text(s, color = Color.White, fontSize = 20.sp, modifier = Modifier.padding(vertical = 4.dp)) }
            item { Text("Source: ${l.source}", color = Dim, fontSize = 12.sp, modifier = Modifier.padding(top = 16.dp)) }
        }
        return
    }
    val cur = Lrclib.currentLine(l.synced, vm.player.position)
    var viewport by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(cur) {
        if (cur != null) state.animateScrollToItem(cur, -(viewport / 3).toInt())
    }
    LazyColumn(
        state = state,
        modifier = Modifier.fillMaxSize().onSizeChanged { viewport = it.height.toFloat() },
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 120.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        itemsIndexed(l.synced) { i, (time, line) ->
            val active = i == cur
            val passed = cur != null && i < cur
            val alpha by animateFloatAsState(if (active) 1f else if (passed) 0.35f else 0.55f, tween(450), label = "a")
            val scale by animateFloatAsState(if (active) 1.06f else 1f, spring(Spring.DampingRatioLowBouncy, Spring.StiffnessLow), label = "s")
            Text(
                line.ifEmpty { "♪" },
                color = Color.White,
                fontSize = 26.sp,
                lineHeight = 32.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.graphicsLayer { this.alpha = alpha; scaleX = scale; scaleY = scale; transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0f, 0.5f) }
                    .clickable { vm.player.seekTo(time) },
            )
        }
        item { Text("Source: ${l.source}", color = Dim, fontSize = 12.sp) }
    }
}

@Composable
private fun QueueView(vm: AppViewModel) {
    val p = vm.player
    val state = rememberLazyListState(initialFirstVisibleItemIndex = p.index.coerceAtLeast(0))
    LazyColumn(state = state, modifier = Modifier.fillMaxSize()) {
        itemsIndexed(p.queue) { i, t ->
            SongRow(vm, t, i == p.index, { p.play(i) }, queueIndex = i)
        }
    }
}
