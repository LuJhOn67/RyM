package com.rymplayer.ui

import androidx.compose.foundation.background
import android.graphics.drawable.BitmapDrawable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.palette.graphics.Palette
import coil.imageLoader
import coil.request.ImageRequest
import coil.request.SuccessResult
import com.rymplayer.data.thumbSized
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.min
import kotlin.math.sin

/** Animations stop on battery saver or a low battery (set by the app). */
var animPaused by androidx.compose.runtime.mutableStateOf(false)

/** Animations' frames a second (0: as fast as the screen). */
var animFps by androidx.compose.runtime.mutableIntStateOf(60)

/** Waits out the rest of a frame at the chosen frames a second. */
private suspend fun frameGap() {
    if (animFps > 0) kotlinx.coroutines.delay(1000L / animFps)
}

/** A drawing only once its size is real (0 or NaN right after unlocking drew stray lines). */
private fun ready(s: androidx.compose.ui.geometry.Size) = s.width.isFinite() && s.height.isFinite() && s.width > 1f && s.height > 1f

/** A level that is a real number between 0 and 1. */
private fun Float.safe() = if (isFinite()) coerceIn(0f, 1f) else 0f

/** Dot-matrix digits for the times (Dot Matrix theme; Doto, SIL OFL). */
val DotDigits = FontFamily(Font(com.rymplayer.R.font.doto, FontWeight.ExtraBold))

/** The cover's four main colors (Liquid Aurora); the last ones are darker shades when it has fewer. */
@Composable
fun coverLights(url: String?): List<Color> {
    val context = LocalContext.current
    var lights by remember { mutableStateOf(listOf(Color(0xFFEA814A), Color(0xFF8E44AD), Color(0xFF2980B9), Color(0xFF16A085))) }
    LaunchedEffect(url) {
        if (url == null) return@LaunchedEffect
        val req = ImageRequest.Builder(context).data(thumbSized(url, 96)).allowHardware(false).build()
        val bmp = ((context.imageLoader.execute(req) as? SuccessResult)?.drawable as? BitmapDrawable)?.bitmap ?: return@LaunchedEffect
        val p = Palette.from(bmp).generate()
        val found = listOfNotNull(p.vibrantSwatch, p.lightVibrantSwatch, p.darkVibrantSwatch, p.mutedSwatch, p.dominantSwatch, p.lightMutedSwatch, p.darkMutedSwatch)
            .map { it.rgb }.distinct().take(4).map { Color(it) }.toMutableList()
        if (found.isEmpty()) return@LaunchedEffect
        while (found.size < 4) found += androidx.compose.ui.graphics.lerp(found.last(), Color.Black, 0.35f)
        lights = found
    }
    return lights
}

/** The playing song's bass, left and right (0 to 1), read while [active]: Liquid Aurora and the meters. */
@Composable
fun rememberLevels(active: Boolean): Triple<Float, Float, Float> {
    var bass by remember { mutableFloatStateOf(0f) }
    var left by remember { mutableFloatStateOf(0f) }
    var right by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(active, animPaused) {
        var peak = 0.05f
        if (!active || animPaused) { bass = 0f; left = 0f; right = 0f; return@LaunchedEffect }
        while (true) {
            val l = com.rymplayer.playback.Automix.Levels
            peak = maxOf(peak * 0.995f, l.bass, 0.01f)
            bass = (l.bass / peak).let { it * kotlin.math.sqrt(it) }.coerceIn(0f, 1f)
            fun vu(x: Float) = ((20f * log10(x.coerceAtLeast(1e-5f)) + 30f) / 30f).coerceIn(0f, 1f)
            left = vu(l.left)
            right = vu(l.right)
            kotlinx.coroutines.delay(50)
        }
    }
    return Triple(bass, left, right)
}

/** Liquid Aurora: the cover's four colors as soft lights, bigger and brighter with the bass. */
@Composable
fun Aurora(lights: List<Color>, energy: Float, playing: Boolean, dark: Boolean, modifier: Modifier = Modifier) {
    val e by animateFloatAsState(energy, tween(140), label = "aurora")
    var t by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(playing, animPaused, animFps) {
        var last = 0L
        while (playing && !animPaused) {
            withFrameNanos { now -> if (last > 0) t += (now - last) / 1e9f / 6f; last = now }
            frameGap()
        }
    }
    Canvas(modifier.fillMaxSize()) {
        if (!ready(size)) return@Canvas
        val alpha = (if (dark) 0.5f else 0.4f) * (0.55f + e.safe() * 0.45f)
        listOf(Triple(0.18f, 0.22f, 0.8f), Triple(0.84f, 0.2f, 0.7f), Triple(0.28f, 0.86f, 0.75f), Triple(0.82f, 0.8f, 0.65f)).forEachIndexed { i, (bx, by, s) ->
            val r = min(size.width, size.height) * s * (1f + e * 0.4f) * 0.35f
            val c = Offset(size.width * (bx + sin(t + i * 1.7f) * 0.05f), size.height * (by + cos(t * 0.8f + i * 2.3f) * 0.05f))
            val color = lights.getOrElse(i) { lights.last() }
            drawCircle(Brush.radialGradient(listOf(color.copy(alpha = alpha), color.copy(alpha = 0f)), c, r), r, c)
        }
    }
}

/** The cover as a record: grooves, the cover as its label turning while it plays (33⅓ rpm × [speed]). */
@Composable
fun Vinyl(url: String?, side: Dp, playing: Boolean, speed: Float, glow: Color?, modifier: Modifier = Modifier, muted: Boolean = false) {
    var angle by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(playing, speed, animPaused, animFps) {
        var last = 0L
        while (playing && !animPaused) {
            withFrameNanos { now -> if (last > 0) angle = (angle + (now - last) / 1e9f * 200f * speed) % 360f; last = now }
            frameGap()
        }
    }
    Box(modifier.size(side), contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            if (!ready(size)) return@Canvas
            val r = size.minDimension / 2
            if (glow != null) drawCircle(Brush.radialGradient(listOf(glow.copy(alpha = 0.55f), Color.Transparent), center, r * 1.15f), r * 1.15f)
            drawCircle(Color(0xFF0C0C0D), r)
            for (i in 0 until 10) drawCircle(Color.White.copy(alpha = if (i % 3 == 0) 0.13f else 0.055f), r * (0.97f - i * 0.052f), style = Stroke(1f))
        }
        Box(Modifier.size(side * 0.4f).clip(CircleShape).rotate(angle)) { Cover(url, side * 0.4f, 480, 0.dp, muted = muted) }
        Canvas(Modifier.fillMaxSize()) {
            if (!ready(size)) return@Canvas
            val r = size.minDimension / 2
            drawCircle(Color(0xFF0C0C0D), r * 0.0175f)
            drawCircle(Brush.linearGradient(0.28f to Color.Transparent, 0.46f to Color.White.copy(alpha = 0.09f), 0.55f to Color.Transparent, 0.82f to Color.White.copy(alpha = 0.05f), 0.9f to Color.Transparent, start = Offset.Zero, end = Offset(size.width, size.height)), r)
        }
    }
}

/** The performance badge: frames a second, frame time and memory (it keeps frames coming while shown). */
@Composable
fun PerfBadge(modifier: Modifier = Modifier) {
    var fps by remember { mutableFloatStateOf(0f) }
    var ms by remember { mutableFloatStateOf(0f) }
    var ram by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(Unit) {
        var frames = 0
        var start = 0L
        var last = 0L
        while (true) withFrameNanos { now ->
            if (start == 0L) start = now
            if (last > 0) {
                val d = (now - last) / 1e6f
                ms = if (ms == 0f) d else ms * 0.9f + d * 0.1f
            }
            last = now
            frames++
            if (now - start >= 1_000_000_000L) {
                fps = frames * 1e9f / (now - start)
                frames = 0
                start = now
                val rt = Runtime.getRuntime()
                ram = ((rt.totalMemory() - rt.freeMemory()) + android.os.Debug.getNativeHeapAllocatedSize()) / 1048576f
            }
        }
    }
    val text = fps.toInt().toString() + " FPS · " + String.format(java.util.Locale.US, "%.1f", ms) + " ms · " + ram.toInt() + " MB" + (if (animPaused) " · eco" else "")
    androidx.compose.material3.Text(
        text,
        color = Color(0xFFE8E8E8),
        fontSize = androidx.compose.ui.unit.TextUnit(11f, androidx.compose.ui.unit.TextUnitType.Sp),
        modifier = modifier.clip(androidx.compose.foundation.shape.RoundedCornerShape(6.dp)).background(Color(0xB0000000)).padding(horizontal = 8.dp, vertical = 4.dp),
    )
}

/** A slow, heavy glow in the accent that beats with the bass (Remorse). */
@Composable
fun Heartbeat(accent: Color, bass: Float, modifier: Modifier = Modifier) {
    val e by animateFloatAsState(bass.safe(), tween(650), label = "heartbeat")
    Canvas(modifier.fillMaxSize()) {
        if (!ready(size)) return@Canvas
        val r = size.minDimension * (0.45f + e * 0.15f)
        drawCircle(Brush.radialGradient(listOf(accent.copy(alpha = 0.08f + e * 0.3f), accent.copy(alpha = 0f)), center, r), r, center)
    }
}

/** Two needle meters, left and right (Hi-Fi Rack). */
@Composable
fun VuMeters(left: Float, right: Float, modifier: Modifier = Modifier) {
    val l by animateFloatAsState(left, tween(90), label = "vuL")
    val r by animateFloatAsState(right, tween(90), label = "vuR")
    Row(modifier.fillMaxWidth().height(64.dp)) {
        for ((level, name) in listOf(l to "L", r to "R")) {
            Canvas(Modifier.weight(1f).fillMaxSize().padding(horizontal = 4.dp)) {
                if (!ready(size)) return@Canvas
                drawRect(Brush.verticalGradient(listOf(Color(0xFFF4ECD4), Color(0xFFDDD0AD))))
                drawRect(Color(0xFF0D0D0D), style = Stroke(2.dp.toPx()))
                val p = Offset(size.width / 2, size.height * 0.96f)
                val reach = size.height * 0.8f
                for (k in 0..10) {
                    val a = Math.toRadians(-50.0 + k * 10).toFloat()
                    val len = (if (k >= 8) 11 else 7).dp.toPx()
                    drawLine(if (k >= 8) Color(0xFFC0392B) else Color(0xFF2B2B2B), Offset(p.x + sin(a) * reach, p.y - cos(a) * reach), Offset(p.x + sin(a) * (reach - len), p.y - cos(a) * (reach - len)), 2.dp.toPx())
                }
                val a = Math.toRadians(-50.0 + level.safe() * 100).toFloat()
                drawLine(Color(0xFF161616), p, Offset(p.x + sin(a) * size.height * 0.86f, p.y - cos(a) * size.height * 0.86f), 2.dp.toPx(), StrokeCap.Round)
                drawCircle(Color(0xFF161616), 5.dp.toPx(), p)
                drawContext.canvas.nativeCanvas.drawText(name, 8.dp.toPx(), size.height - 6.dp.toPx(), android.graphics.Paint().apply { textSize = 11.dp.toPx(); color = 0xFF5A5A5A.toInt(); isAntiAlias = true })
            }
        }
    }
}

