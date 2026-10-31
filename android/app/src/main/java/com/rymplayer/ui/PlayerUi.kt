package com.rymplayer.ui

import com.rymplayer.tr
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChangeIgnoreConsumed
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.zIndex
import androidx.compose.foundation.lazy.LazyListLayoutInfo
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
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
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.OndemandVideo
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
            IconButton(p::next) { Icon(Icons.Default.SkipNext, tr("Next"), tint = Color.White) }
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
            Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, tr("Play"), tint = Color.White, modifier = Modifier.size(size.dp))
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
            IconButton({ vm.playerOpen = false }) { Icon(Icons.Default.KeyboardArrowDown, tr("Close"), tint = Color.White, modifier = Modifier.size(30.dp)) }
            Text(tr("Now playing"), Modifier.weight(1f), textAlign = TextAlign.Center, fontWeight = FontWeight.SemiBold, color = Color.White.copy(alpha = 0.8f))
            val context = LocalContext.current
            IconButton({ shareSong(context, t) }) { Icon(Icons.Default.Share, tr("Share"), tint = Color.White) }
            IconButton({ vm.player.current?.let { vm.menu = com.rymplayer.MenuFor(it, vm.player.index) } }) {
                Icon(Icons.Default.MoreVert, tr("More"), tint = Color.White)
            }
        }
        AnimatedContent(panel, transitionSpec = { fadeIn(tween(250)) togetherWith fadeOut(tween(200)) }, modifier = Modifier.weight(1f), label = "panel") { shown ->
            when (shown) {
                Panel.Cover -> BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    // The cover breathes: full size while playing, slightly smaller when paused.
                    val scale by animateFloatAsState(if (p.playing) 1f else 0.86f, spring(Spring.DampingRatioMediumBouncy, Spring.StiffnessLow), label = "cover")
                    val side = minOf(maxWidth, maxHeight) - 48.dp
                    // A tap on the cover shows the lyrics (it never skips the song).
                    Box(Modifier.scale(scale).shadow(24.dp, RoundedCornerShape(16.dp)).clickable { panel = Panel.Lyrics }) { Cover(t.thumb, side, 720, 16.dp) }
                }
                Panel.Lyrics -> LyricsView(vm, tint)
                Panel.Queue -> QueueView(vm)
            }
        }
        Column(Modifier.padding(horizontal = 24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Marquee(t.title, 22, FontWeight.Bold, Color.White)
                    Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Color.White.copy(alpha = 0.7f), fontSize = 16.sp, modifier = Modifier.clickable { vm.goArtist(t) })
                }
                if (vm.hasVideo) {
                    IconButton({ vm.watchVideo(t) }) { Icon(Icons.Default.OndemandVideo, tr("Watch video"), tint = Color.White) }
                }
                if (vm.api.signedIn) {
                    val liked = t.videoId in vm.liked
                    IconButton({ vm.toggleLike(t) }) {
                        Icon(if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder, tr("Like"), tint = if (liked) MaterialTheme.colorScheme.primary else Color.White)
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
                IconButton(p::prev, Modifier.size(56.dp)) { Icon(Icons.Default.SkipPrevious, tr("Previous"), tint = Color.White, modifier = Modifier.size(36.dp)) }
                Box(Modifier.size(76.dp).clip(CircleShape).background(Color.White).clickable(onClick = p::toggle), contentAlignment = Alignment.Center) {
                    if (p.loading) {
                        CircularProgressIndicator(Modifier.size(34.dp), color = Color.Black, strokeWidth = 3.dp)
                    } else {
                        Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, tr("Play"), tint = Color.Black, modifier = Modifier.size(40.dp))
                    }
                }
                IconButton(p::next, Modifier.size(56.dp)) { Icon(Icons.Default.SkipNext, tr("Next"), tint = Color.White, modifier = Modifier.size(36.dp)) }
            }
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                PanelButton(Icons.Default.Lyrics, tr("Lyrics"), panel == Panel.Lyrics) { panel = if (panel == Panel.Lyrics) Panel.Cover else Panel.Lyrics }
                PanelButton(Icons.AutoMirrored.Filled.QueueMusic, tr("Up next"), panel == Panel.Queue) { panel = if (panel == Panel.Queue) Panel.Cover else Panel.Queue }
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

/** The cover's color at the level all the RyM apps use for lyrics: dark enough for white lines. */
private fun lyricsBackground(tint: Color): Color {
    val max = maxOf(tint.red, tint.green, tint.blue)
    if (max < 0.04f) return Color(0xFF484850)
    val k = (150f / 255f) / max
    return Color(tint.red * k, tint.green * k, tint.blue * k)
}

/**
 * Lyrics on a panel in the cover's color: big bold lines, the ones to come dark, the sung one
 * white, sung ones soft white; they glide so the sung line stays in the upper third. Tap a line
 * to jump there.
 */
@Composable
private fun LyricsView(vm: AppViewModel, tint: Color) {
    val bg = lyricsBackground(tint)
    val sung = lerp(bg, Color.White, 0.72f)
    val next = lerp(bg, Color.Black, 0.55f)
    Box(Modifier.fillMaxSize().padding(horizontal = 14.dp, vertical = 6.dp).clip(RoundedCornerShape(16.dp)).background(bg)) {
        LyricsLines(vm, sung, next)
        LyricsSourceSwitch(vm, Modifier.align(Alignment.TopEnd).padding(10.dp))
    }
}

@Composable
private fun BoxScope.LyricsLines(vm: AppViewModel, sung: Color, next: Color) {
    val l = vm.lyrics
    if (l == null) {
        Text(vm.lyricsStatus, color = sung, fontWeight = FontWeight.Bold, fontSize = 18.sp, modifier = Modifier.align(Alignment.Center))
        return
    }
    val state = rememberLazyListState()
    if (l.synced.isEmpty()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(22.dp)) {
            item { Text(tr("These lyrics aren't synced to the song"), color = sung, fontSize = 13.sp, modifier = Modifier.padding(bottom = 12.dp)) }
            itemsIndexed(l.plain) { _, s -> Text(s, color = Color.White, fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(vertical = 4.dp)) }
            item { Text("${tr("Lyrics from")} ${l.source}", color = sung, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 18.dp)) }
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
        contentPadding = PaddingValues(horizontal = 22.dp, vertical = 90.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        itemsIndexed(l.synced) { i, (time, line) ->
            val target = when {
                i == cur -> Color.White
                cur != null && i < cur -> sung
                else -> next
            }
            val color by animateColorAsState(target, tween(350), label = "line")
            Text(
                line.ifEmpty { "♪" },
                color = color,
                fontSize = 26.sp,
                lineHeight = 32.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.fillMaxWidth().clickable { vm.player.seekTo(time) },
            )
        }
        item { Text("${tr("Lyrics from")} ${l.source}", color = sung, fontSize = 12.sp, fontWeight = FontWeight.SemiBold) }
    }
}

/** Where the lyrics come from; a tap lists every source to pick another (when the timing is off, say). */
@Composable
private fun LyricsSourceSwitch(vm: AppViewModel, modifier: Modifier) {
    var open by remember { mutableStateOf(false) }
    val shown = vm.lyrics
    Box(modifier) {
        Row(
            Modifier.clip(RoundedCornerShape(14.dp)).background(Color.Black.copy(alpha = 0.28f))
                .clickable { open = true; vm.loadLyricsSources() }
                .padding(start = 12.dp, end = 6.dp, top = 5.dp, bottom = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(shown?.source ?: tr("Lyrics sources"), color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            Icon(Icons.Default.ArrowDropDown, tr("Lyrics sources"), tint = Color.White, modifier = Modifier.size(18.dp))
        }
        DropdownMenu(open, { open = false }) {
            when {
                vm.lookingForSources -> DropdownMenuItem(
                    { Text(tr("Looking for lyrics...")) }, onClick = {}, enabled = false,
                    leadingIcon = { CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp) },
                )
                vm.lyricsSources.isEmpty() -> DropdownMenuItem({ Text(tr("No lyrics for this song")) }, onClick = {}, enabled = false)
                else -> vm.lyricsSources.forEach { l ->
                    DropdownMenuItem(
                        text = {
                            Column {
                                Text(l.source, fontWeight = FontWeight.SemiBold)
                                Text(if (l.synced.isNotEmpty()) tr("Synced") else tr("Not synced"), fontSize = 12.sp, color = Dim)
                            }
                        },
                        onClick = { vm.showLyricsFrom(l); open = false },
                        trailingIcon = { if (l == shown) Icon(Icons.Default.Check, null) },
                    )
                }
            }
        }
    }
}

/** How long a song in Up next is held before it lifts out to be moved. */
private const val HOLD_MS = 2000L

/** The lifted song's row: a little lighter than the panel. */
private val LiftedRow = Color(0xFF2A2A30)

/**
 * Moving a song in Up next: held for 2 s it lifts (pops out), follows the finger while the rows
 * it passes make room, and on release it settles into its new place (pops back in).
 */
private class QueueDrag(private val list: LazyListState, private val scope: CoroutineScope, private val move: (Int, Int) -> Unit) {
    /** The lifted row, while it is held. */
    var index by mutableStateOf<Int?>(null)
        private set
    /** The row just let go, gliding back into its place by [settle]. */
    var dropped by mutableStateOf<Int?>(null)
        private set
    val settle = Animatable(0f)
    private var startOffset by mutableIntStateOf(0)
    private var delta by mutableFloatStateOf(0f)
    /** The list as laid out when a row last moved: the next move waits for a new layout. */
    private var movedIn: LazyListLayoutInfo? = null

    private val lifted get() = index?.let { i -> list.layoutInfo.visibleItemsInfo.firstOrNull { it.index == i } }

    /** How far the lifted row is drawn from its place in the list, so it stays under the finger. */
    val offset: Float get() = lifted?.let { startOffset + delta - it.offset } ?: 0f

    fun start(at: Offset): Boolean {
        val row = list.layoutInfo.visibleItemsInfo.firstOrNull { at.y.toInt() in it.offset..(it.offset + it.size) } ?: return false
        index = row.index
        startOffset = row.offset
        delta = 0f
        return true
    }

    fun drag(by: Float) {
        delta += by
        if (list.layoutInfo === movedIn) return
        val row = lifted ?: return
        val middle = (row.offset + offset + row.size / 2f).toInt()
        val target = list.layoutInfo.visibleItemsInfo.firstOrNull { it.index != row.index && middle in it.offset..(it.offset + it.size) } ?: return
        // The list keeps its first row in place; when that row moves, keep the list still instead.
        if (row.index == list.firstVisibleItemIndex || target.index == list.firstVisibleItemIndex) {
            list.requestScrollToItem(list.firstVisibleItemIndex, list.firstVisibleItemScrollOffset)
        }
        move(row.index, target.index)
        movedIn = list.layoutInfo
        index = target.index
    }

    /** Near the top or bottom edge, in the way it's being moved: how far the list scrolls this frame. */
    fun edgeScroll(): Float {
        val row = lifted ?: return 0f
        val info = list.layoutInfo
        val top = row.offset + offset
        val bottom = top + row.size
        val zone = row.size / 2f
        return when {
            delta < 0 && top < info.viewportStartOffset + zone -> -((info.viewportStartOffset + zone - top) / 4f).coerceAtMost(30f)
            delta > 0 && bottom > info.viewportEndOffset - zone -> ((bottom - info.viewportEndOffset + zone) / 4f).coerceAtMost(30f)
            else -> 0f
        }
    }

    fun end() {
        val i = index ?: return
        val from = offset
        dropped = i
        index = null
        // Started at once, so the row doesn't show at its place for a frame before gliding there.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            settle.snapTo(from)
            settle.animateTo(0f, spring(Spring.DampingRatioMediumBouncy, Spring.StiffnessMediumLow))
            dropped = null
        }
    }
}

/**
 * Up next: songs played before (dimmed; Previous goes back to them), the playing one, what comes
 * next. A tap plays a song, ⋮ opens its menu; held for 2 seconds it lifts out and can be dragged
 * to its new place.
 */
@Composable
private fun QueueView(vm: AppViewModel) {
    val p = vm.player
    val state = rememberLazyListState(initialFirstVisibleItemIndex = p.index.coerceAtLeast(0))
    val scope = rememberCoroutineScope()
    val drag = remember { QueueDrag(state, scope) { from, to -> p.move(from, to) } }
    val haptic = LocalHapticFeedback.current
    // While a song is lifted near an edge, the list scrolls under it.
    val holding = drag.index != null
    LaunchedEffect(holding) {
        while (holding) {
            val by = drag.edgeScroll()
            if (by != 0f) {
                state.scrollBy(by)
                drag.drag(0f)
            }
            withFrameNanos { }
        }
    }
    val songs = p.queue.toList()
    // Keys that follow each row when it moves (a song can be in the queue twice).
    val keys = remember(songs) {
        val seen = HashMap<String, Int>()
        songs.map { t -> val n = seen[t.videoId] ?: 0; seen[t.videoId] = n + 1; "${t.videoId}#$n" }
    }
    Column(Modifier.fillMaxSize()) {
        Text(tr("Hold a song for 2 seconds, then drag it to move it"), color = Dim, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        LazyColumn(
            state = state,
            modifier = Modifier.fillMaxSize().pointerInput(drag) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    // Null when the time ran out with the finger still down and still: a hold.
                    val notHeld = withTimeoutOrNull(HOLD_MS) {
                        while (true) {
                            val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
                            if (c == null || !c.pressed || c.isConsumed || (c.position - down.position).getDistance() > viewConfiguration.touchSlop) break
                        }
                        true
                    }
                    if (notHeld != null || !drag.start(down.position)) return@awaitEachGesture
                    haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                    // The lifted song takes every move before the list (no scrolling) and the row
                    // (no tap on release) see it.
                    try {
                        while (true) {
                            val c = awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull { it.id == down.id } ?: break
                            val by = c.positionChangeIgnoreConsumed().y
                            c.consume()
                            if (!c.pressed) break
                            drag.drag(by)
                        }
                    } finally {
                        drag.end()
                    }
                }
            },
        ) {
            itemsIndexed(songs, key = { i, _ -> keys[i] }) { i, t ->
                val lifted = drag.index == i
                val pop by animateFloatAsState(if (lifted) 1.04f else 1f, spring(Spring.DampingRatioMediumBouncy, Spring.StiffnessMedium), label = "pop")
                val raise by animateDpAsState(if (lifted) 12.dp else 0.dp, label = "raise")
                val shade by animateColorAsState(if (lifted) LiftedRow else Color.Transparent, label = "shade")
                val place = when {
                    lifted -> Modifier.zIndex(1f).graphicsLayer { translationY = drag.offset }
                    i == drag.dropped -> Modifier.zIndex(1f).graphicsLayer { translationY = drag.settle.value }
                    else -> Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null)
                }
                Box(
                    place.graphicsLayer { scaleX = pop; scaleY = pop; alpha = if (i < p.index && !lifted) 0.5f else 1f }
                        .shadow(raise, RoundedCornerShape(10.dp), clip = false)
                        .background(shade, RoundedCornerShape(10.dp)),
                ) {
                    SongRow(vm, t, i == p.index, { p.play(i) }, queueIndex = i, menuOnHold = false)
                }
            }
        }
    }
}
