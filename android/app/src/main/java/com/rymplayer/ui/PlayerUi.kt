package com.rymplayer.ui

import androidx.compose.ui.text.withStyle
import com.rymplayer.tr
import androidx.compose.animation.AnimatedContent
import kotlin.math.roundToInt
import androidx.compose.material.icons.filled.Subtitles
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Speed
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.foundation.border
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.OndemandVideo
import androidx.compose.material.icons.filled.Lyrics
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
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
    if (vm.ytmTheme) {
        // RyM YTM: a flat bar across the screen, the progress as a thin line under it.
        Column(Modifier.fillMaxWidth().background(Color(0xFF212121)).clickable { vm.playerOpen = true }) {
            Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Cover(t.thumb, 44.dp, 120, 4.dp)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Marquee(t.title, 15, FontWeight.SemiBold, Color.White)
                    Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
                }
                PlayPause(vm, 30)
            }
            val dur = p.duration
            LinearProgressIndicator(
                progress = { played(p.position, dur) },
                modifier = Modifier.fillMaxWidth().height(2.dp),
                color = Color.White,
                trackColor = Color.Transparent,
                drawStopIndicator = {},
            )
        }
        return
    }
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
            progress = { played(p.position, dur) },
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
            Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, tr(if (p.playing) "Pause" else "Play"), tint = Color.White, modifier = Modifier.size(size.dp))
        }
    }
}

private enum class Panel { Cover, Lyrics, Queue }

@Composable
fun FullPlayer(vm: AppViewModel, coverTint: Color) {
    // Remorse: the cover's color goes grey everywhere in the player.
    val tint = if (vm.colors?.muted == true) lerp(coverTint, Color(0xFF2A2F38), 0.7f) else coverTint
    val p = vm.player
    val t = p.current ?: return
    var panel by remember { mutableStateOf(Panel.Cover) }
    val bg = Brush.verticalGradient(listOf(lerp(Color.Black, tint, 0.55f), lerp(Color.Black, tint, 0.18f), Color.Black))
    // The color theme's extras: glass over the cover, Liquid Aurora, the needle meters, dot digits.
    val colors = vm.colors
    val accent = MaterialTheme.colorScheme.primary
    val (bass, vuL, vuR) = rememberLevels((vm.aurora || colors?.vu == true || colors?.heartbeat == true) && p.playing && p.visible)
    Box(Modifier.fillMaxSize().background(bg)) {
    if (colors != null && colors.glass > 0f) {
        // The cover blurred behind see-through panels (the blur needs Android 12; before, a soft fade).
        coil.compose.AsyncImage(
            com.rymplayer.data.thumbSized(t.thumb, 480), null,
            contentScale = androidx.compose.ui.layout.ContentScale.Crop,
            modifier = Modifier.fillMaxSize().blur(28.dp).alpha(0.3f + colors.glass * 0.5f),
        )
    }
    if (vm.aurora) Aurora(coverLights(t.thumb), bass, p.playing, colors?.dark ?: true)
    if (colors?.heartbeat == true) Heartbeat(accent, bass)
    Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton({ vm.playerOpen = false }) { Icon(Icons.Default.KeyboardArrowDown, tr("Close"), tint = Color.White, modifier = Modifier.size(30.dp)) }
            Text(tr("Now playing"), Modifier.weight(1f), textAlign = TextAlign.Center, fontWeight = FontWeight.SemiBold, color = Color.White.copy(alpha = 0.8f))
            val context = LocalContext.current
            var sheet by remember { mutableStateOf<String?>(null) }
            IconButton({ sheet = "speed" }) { Icon(Icons.Default.Speed, tr("Speed and pitch"), tint = Color.White) }
            IconButton({ sheet = "sleep" }) { Icon(Icons.Default.Bedtime, tr("Sleep timer"), tint = if (vm.sound.sleepAt != 0L) MaterialTheme.colorScheme.primary else Color.White) }
            IconButton({ vm.toggleLyricsPill() }) { Icon(Icons.Default.Subtitles, tr("Floating lyrics"), tint = if (vm.lyricsPill) MaterialTheme.colorScheme.primary else Color.White) }
            IconButton({ shareSong(context, t) }) { Icon(Icons.Default.Share, tr("Share"), tint = Color.White) }
            when (sheet) {
                "speed" -> SpeedSheet(vm) { sheet = null }
                "sleep" -> SleepSheet(vm) { sheet = null }
            }
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
                    if (vm.vinyl) {
                        Vinyl(t.thumb, side, p.playing && !p.loading, vm.sound.speed, if (colors?.glow == true) accent else null, Modifier.clickable { panel = Panel.Lyrics }, muted = colors?.muted == true)
                    } else {
                        val shape = if (colors?.cut == true) androidx.compose.foundation.shape.CutCornerShape(side * 0.12f) else RoundedCornerShape(16.dp * (colors?.round ?: 1f))
                        Box(
                            Modifier.scale(scale)
                                .shadow(if (colors?.glow == true) 36.dp else 24.dp, shape, ambientColor = if (colors?.glow == true) accent else Color.Black, spotColor = if (colors?.glow == true) accent else Color.Black)
                                .then(if (colors?.coverBorder == true) Modifier.border(2.dp, tint, shape) else Modifier)
                                .clickable { panel = Panel.Lyrics },
                        ) { Cover(t.thumb, side, 1080, 16.dp * (colors?.round ?: 1f), muted = colors?.muted == true) }
                    }
                }
                Panel.Lyrics -> LyricsView(vm, tint)
                Panel.Queue -> QueueView(vm)
            }
        }
        Column(Modifier.padding(horizontal = 24.dp)) {
            if (colors?.vu == true) VuMeters(vuL, vuR, Modifier.padding(bottom = 10.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    // The gold badge: the song plays lossless (FLAC from the Internet Archive).
                    if (p.lossless) {
                        Text(
                            "Hi-Res FLAC", color = Color(0xFFE6C35C), fontSize = 11.sp, fontWeight = FontWeight.Bold,
                            modifier = Modifier.padding(bottom = 4.dp).clip(RoundedCornerShape(6.dp)).background(Color(0x1FD4AF37)).padding(horizontal = 7.dp, vertical = 2.dp),
                        )
                    }
                    Marquee(t.title, 22, FontWeight.Bold, Color.White)
                    Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Color.White.copy(alpha = 0.7f), fontSize = 16.sp, modifier = Modifier.clickable(enabled = com.rymplayer.data.isYouTube(t.videoId)) { vm.goArtist(t) })
                }
                val youtube = com.rymplayer.data.isYouTube(t.videoId)
                if (vm.hasVideo && youtube) {
                    IconButton({ vm.watchVideo(t) }) { Icon(Icons.Default.OndemandVideo, tr("Watch video"), tint = Color.White) }
                }
                if (vm.api.signedIn && youtube) {
                    val liked = t.videoId in vm.liked
                    IconButton({ vm.toggleLike(t) }) {
                        Icon(if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder, tr("Like"), tint = if (liked) MaterialTheme.colorScheme.primary else Color.White)
                    }
                }
            }
            val dur = p.duration.takeIf { it.isFinite() && it > 0f } ?: 0f
            // A new song forgets an old drag.
            var dragging by remember(t.videoId) { mutableStateOf<Float?>(null) }
            // Locking the screen mid-touch cancels the drag without "finished": the thumb (a white
            // line) stayed stuck where the finger was while the song went on. Leaving forgets it.
            val lifecycle = androidx.compose.ui.platform.LocalLifecycleOwner.current
            androidx.compose.runtime.DisposableEffect(lifecycle) {
                val watch = androidx.lifecycle.LifecycleEventObserver { _, e ->
                    if (e == androidx.lifecycle.Lifecycle.Event.ON_STOP || e == androidx.lifecycle.Lifecycle.Event.ON_RESUME) dragging = null
                }
                lifecycle.lifecycle.addObserver(watch)
                onDispose { lifecycle.lifecycle.removeObserver(watch) }
            }
            val shown = (dragging ?: p.position).takeIf { it.isFinite() } ?: 0f
            Slider(
                value = shown.coerceIn(0f, dur.coerceAtLeast(1f)),
                onValueChange = { dragging = it },
                onValueChangeFinished = { dragging?.let(p::seekTo); dragging = null },
                valueRange = 0f..dur.coerceAtLeast(1f),
                // The cover's color on the bar (it eases into the next song's).
                colors = SliderDefaults.colors(thumbColor = Color.White, activeTrackColor = lerp(tint, Color.White, 0.45f), inactiveTrackColor = Color.White.copy(alpha = 0.2f)),
            )
            Row {
                val digits = if (colors?.dotClock == true) DotDigits else null
                Text(fmtTime(dragging ?: p.position), color = Dim, fontSize = if (digits != null) 15.sp else 12.sp, fontFamily = digits)
                Spacer(Modifier.weight(1f))
                Text(fmtTime(dur), color = Dim, fontSize = if (digits != null) 15.sp else 12.sp, fontFamily = digits)
            }
            Row(Modifier.fillMaxWidth().padding(vertical = 10.dp), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.size(48.dp))
                IconButton(p::prev, Modifier.size(56.dp)) { Icon(Icons.Default.SkipPrevious, tr("Previous"), tint = Color.White, modifier = Modifier.size(36.dp)) }
                Box(Modifier.size(76.dp).clip(CircleShape).background(Color.White).clickable(onClick = p::toggle), contentAlignment = Alignment.Center) {
                    if (p.loading) {
                        CircularProgressIndicator(Modifier.size(34.dp), color = Color.Black, strokeWidth = 3.dp)
                    } else {
                        Icon(if (p.playing) Icons.Default.Pause else Icons.Default.PlayArrow, tr(if (p.playing) "Pause" else "Play"), tint = Color.Black, modifier = Modifier.size(40.dp))
                    }
                }
                IconButton(p::next, Modifier.size(56.dp)) { Icon(Icons.Default.SkipNext, tr("Next"), tint = Color.White, modifier = Modifier.size(36.dp)) }
                // Repeat this song.
                IconButton({ p.cycleRepeat() }, Modifier.size(48.dp)) {
                    Icon(
                        if (p.repeatOne) Icons.Default.RepeatOne else Icons.Default.Repeat,
                        tr(when (p.repeatMode) { 2 -> "Repeat this song"; 1 -> "Repeat all"; else -> "Repeat" }),
                        tint = if (p.repeatMode != 0) MaterialTheme.colorScheme.primary else Color.White.copy(alpha = 0.7f),
                        modifier = Modifier.size(28.dp),
                    )
                }
            }
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                PanelButton(Icons.Default.Lyrics, tr("Lyrics"), panel == Panel.Lyrics) { panel = if (panel == Panel.Lyrics) Panel.Cover else Panel.Lyrics }
                PanelButton(Icons.AutoMirrored.Filled.QueueMusic, tr("Up next"), panel == Panel.Queue) { panel = if (panel == Panel.Queue) Panel.Cover else Panel.Queue }
            }
        }
    }
    }
}

/** How much of the song has played (0 to 1); 0 until both numbers are real (right after unlocking). */
private fun played(position: Float, duration: Float): Float =
    if (duration > 0f && duration.isFinite() && position.isFinite()) (position / duration).coerceIn(0f, 1f) else 0f

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
        Column(Modifier.fillMaxSize()) {
            LyricsPills(vm, sung)
            Box(Modifier.weight(1f).fillMaxWidth()) { LyricsLines(vm, sung, next) }
        }
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
            itemsIndexed(l.plain) { i, s ->
                Column(Modifier.padding(vertical = 4.dp)) {
                    Text(s, color = Color.White, fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.Bold)
                    vm.lyricsTranslation.getOrNull(i)?.takeIf { it.isNotBlank() }?.let { Text(it, color = Color.White.copy(alpha = 0.75f), fontSize = 14.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold) }
                }
            }
            item { Text("${tr("Lyrics from")} ${l.source}", color = sung, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 18.dp)) }
        }
        return
    }
    val cur = vm.lyricLine
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
            Column(Modifier.fillMaxWidth().clickable { vm.player.seekTo(time) }) {
                // Word by word (sources with word times): the sung line fills as it's sung.
                val words = l.words.getOrNull(i)?.takeIf { it.isNotEmpty() && i == cur }
                if (words != null) {
                    val end = l.synced.getOrNull(i + 1)?.first ?: (words.last().first + 3f)
                    var split by remember(i, l) { mutableStateOf("" to line) }
                    LaunchedEffect(i, l, vm.player.playing) {
                        // The exact place, a dozen times a second, only for this line.
                        while (true) {
                            split = com.rymplayer.data.WordTimes.sungSplit(words, end, vm.player.exactPosition())
                            if (!vm.player.playing) break
                            kotlinx.coroutines.delay(80)
                        }
                    }
                    Text(
                        androidx.compose.ui.text.buildAnnotatedString {
                            withStyle(androidx.compose.ui.text.SpanStyle(color = Color.White)) { append(split.first) }
                            withStyle(androidx.compose.ui.text.SpanStyle(color = Color.White.copy(alpha = 0.35f))) { append(split.second) }
                        },
                        fontSize = 26.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold,
                    )
                } else Text(line.ifEmpty { "♪" }, color = color, fontSize = 26.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold)
                // A Japanese song's English, small under the line.
                vm.lyricsTranslation.getOrNull(i)?.takeIf { it.isNotBlank() }?.let {
                    Text(it, color = color.copy(alpha = 0.8f), fontSize = 15.sp, lineHeight = 19.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 2.dp))
                }
            }
        }
        item { Text("${tr("Lyrics from")} ${l.source}", color = sung, fontSize = 12.sp, fontWeight = FontWeight.SemiBold) }
    }
}

/**
 * One pill per source (LRCLIB | NetEase | KuGou | Genius | YouTube Music), the one showing lit;
 * a tap shows that source's lyrics (the next LRCLIB version when LRCLIB already shows). Sources
 * that had nothing are dimmed once every source was asked.
 */
@Composable
private fun LyricsPills(vm: AppViewModel, sung: Color) {
    val shown = vm.lyrics
    val active = shown?.let(com.rymplayer.data.LyricSources::provider)
    val asked = vm.lyricsSources.isNotEmpty()
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (p in com.rymplayer.data.LyricSources.PROVIDERS) {
            val versions = vm.lyricsSources.filter { com.rymplayer.data.LyricSources.provider(it) == p }
            val at = versions.indexOfFirst { it == shown }
            val text = if (p == com.rymplayer.data.LyricSources.LRCLIB && versions.size > 1 && at >= 0) "$p ${at + 1}/${versions.size}" else p
            val on = active == p
            val enabled = !asked || versions.isNotEmpty()
            Text(
                text,
                color = if (on) Color.Black else Color.White.copy(alpha = if (enabled) 0.9f else 0.35f),
                fontSize = 12.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.clip(RoundedCornerShape(14.dp))
                    .background(if (on) Color.White else Color.Black.copy(alpha = 0.25f))
                    .clickable(enabled = enabled) { vm.pickSource(p) }
                    .padding(horizontal = 12.dp, vertical = 5.dp),
            )
        }
        if (vm.lookingForSources) CircularProgressIndicator(Modifier.size(16.dp), color = sung, strokeWidth = 2.dp)
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

/** Speed and pitch: two sliders and the quick picks (Normal, Nightcore, Slowed). */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun SpeedSheet(vm: AppViewModel, onDismiss: () -> Unit) {
    androidx.compose.material3.ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 8.dp)) {
            Text(tr("Speed and pitch"), fontSize = 18.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(12.dp))
            Text("${tr("Speed")}: ${"%.2f".format(vm.sound.speed)}x", color = Dim)
            Slider(vm.sound.speed, { vm.changeSound(vm.sound.copy(speed = (it * 20).roundToInt() / 20f)) }, valueRange = 0.5f..2f)
            Text("${tr("Pitch")}: ${"%.2f".format(vm.sound.pitch)}", color = Dim)
            Slider(vm.sound.pitch, { vm.changeSound(vm.sound.copy(pitch = (it * 20).roundToInt() / 20f)) }, valueRange = 0.5f..2f)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (m in com.rymplayer.playback.Sound.MODES) {
                    androidx.compose.material3.FilterChip(
                        vm.sound.modeName == m.name,
                        { vm.changeSound(vm.sound.copy(speed = m.speed, pitch = m.pitch, reverb = m.reverb)) },
                        { Text(tr(m.name)) },
                    )
                }
            }
            // Smart automix on or off, from the player too.
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().padding(top = 14.dp)) {
                Column(Modifier.weight(1f)) {
                    Text(tr("Smart Automix"), fontWeight = FontWeight.SemiBold)
                    Text(tr("Songs blend like a DJ mix"), color = Dim, fontSize = 13.sp)
                }
                androidx.compose.material3.Switch(vm.sound.automix, { vm.changeSound(vm.sound.copy(automix = it)) })
            }
            // Skip silence too (the quiet starts and ends of songs, in mixes as well).
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().padding(top = 10.dp)) {
                Column(Modifier.weight(1f)) {
                    Text(tr("Skip silence"), fontWeight = FontWeight.SemiBold)
                    Text(tr("Silent parts at the start and end of songs are skipped"), color = Dim, fontSize = 13.sp)
                }
                androidx.compose.material3.Switch(vm.sound.skipSilence, { vm.changeSound(vm.sound.copy(skipSilence = it)) })
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

/** The sleep timer: 15, 30, 45, 60 minutes, the end of the song, or off; the last 10 seconds fade out. */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun SleepSheet(vm: AppViewModel, onDismiss: () -> Unit) {
    androidx.compose.material3.ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(bottom = 24.dp)) {
            Text(tr("Sleep timer"), fontSize = 18.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(horizontal = 22.dp, vertical = 8.dp))
            val choices = listOf(15, 30, 45, 60).map { "$it ${tr("minutes")}" to it as Int? } + listOf(tr("End of this song") to null, tr("Off") to 0)
            for ((text, minutes) in choices) {
                Text(text, fontSize = 16.sp, modifier = Modifier.fillMaxWidth().clickable { vm.setSleep(minutes); onDismiss() }.padding(horizontal = 22.dp, vertical = 14.dp))
            }
        }
    }
}

/**
 * The floating lyrics: the sung line in a pill over the app (drag it anywhere); it changes only
 * when the line does.
 */
@Composable
fun LyricsPill(vm: AppViewModel) {
    val l = vm.lyrics ?: return
    val line = vm.lyricLine?.let { l.synced.getOrNull(it)?.second } ?: return
    var offset by remember { mutableStateOf(androidx.compose.ui.geometry.Offset(0f, 0f)) }
    Box(Modifier.fillMaxSize().statusBarsPadding().padding(top = 8.dp), contentAlignment = Alignment.TopCenter) {
        Text(
            line.ifEmpty { "♪" },
            color = Color.White,
            fontWeight = FontWeight.Bold,
            fontSize = 15.sp,
            textAlign = TextAlign.Center,
            maxLines = 2,
            modifier = Modifier
                .graphicsLayer { translationX = offset.x; translationY = offset.y }
                .pointerInput(Unit) { detectDragGestures { _, d -> offset += d } }
                .padding(horizontal = 24.dp)
                .clip(RoundedCornerShape(22.dp))
                .background(Color.Black.copy(alpha = 0.72f))
                .padding(horizontal = 18.dp, vertical = 10.dp),
        )
    }
}
