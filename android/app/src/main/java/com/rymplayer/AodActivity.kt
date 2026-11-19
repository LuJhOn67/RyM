package com.rymplayer

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import com.rymplayer.ui.Cover
import com.rymplayer.ui.coverColor
import kotlinx.coroutines.delay
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.random.Random

/** What RyM's always-on screen shows: the sung line and the next, and the song (set by the app). */
object Aod {
    var line by mutableStateOf<String?>(null)
    var next by mutableStateOf<String?>(null)
    var title by mutableStateOf("")
    var artist by mutableStateOf("")
    var thumb by mutableStateOf<String?>(null)
    var playing by mutableStateOf(false)
    /** The screen is up (from its start to its end), and how to close it. */
    @Volatile var open = false
    @Volatile var close: (() -> Unit)? = null
    /** The proximity sensor is covered (a pocket, face down). */
    var covered by mutableStateOf(false)
}

/**
 * RyM's own always-on screen (Android doesn't let apps draw on the phone's): when the screen is
 * turned off while music plays, a dim black screen over the lock screen with the clock (12 or 24
 * hours, as the phone has it), the date and the battery, the sung line in the cover's colour, the
 * next line, and the song. It moves a little every minute (no burn-in). A tap shows play / pause,
 * previous and next for 5 s; a double tap closes it (a touch in a pocket doesn't); in a pocket or
 * face down (the proximity sensor) it goes all black and takes no touches. It also goes when the
 * music stops, or when the screen is turned off again.
 */
class AodActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        // Light on the phone: as dim as the screen goes (black pixels are off on OLED screens, the
        // text barely lit), and the screen's slowest refresh rate (it draws once a minute or a line).
        @Suppress("DEPRECATION")
        val d = if (android.os.Build.VERSION.SDK_INT >= 30) display else windowManager.defaultDisplay
        val slowest = d?.let { now -> now.supportedModes.filter { it.physicalWidth == now.mode.physicalWidth && it.physicalHeight == now.mode.physicalHeight }.minByOrNull { it.refreshRate } }
        window.attributes = window.attributes.apply {
            screenBrightness = 0.04f
            slowest?.let { preferredDisplayModeId = it.modeId }
        }
        // The bars stay (black, over black): hiding them brought Android's "Viewing full screen"
        // notice over the lock screen.
        WindowCompat.setDecorFitsSystemWindows(window, false)
        Aod.open = true
        Aod.close = { runOnUiThread { finish() } }
        // Brought by the full-screen notification: it's done.
        getSystemService(android.app.NotificationManager::class.java)?.cancel(com.rymplayer.playback.AOD_NOTIFICATION)
        // In a pocket or face down: all black, touches ignored.
        val sensors = getSystemService(android.hardware.SensorManager::class.java)
        sensors?.getDefaultSensor(android.hardware.Sensor.TYPE_PROXIMITY)?.let { near ->
            val l = object : android.hardware.SensorEventListener {
                override fun onSensorChanged(e: android.hardware.SensorEvent) { Aod.covered = e.values[0] < near.maximumRange.coerceAtMost(5f) }
                override fun onAccuracyChanged(s: android.hardware.Sensor?, a: Int) {}
            }
            sensors.registerListener(l, near, android.hardware.SensorManager.SENSOR_DELAY_NORMAL)
            proximity = { sensors.unregisterListener(l) }
        }
        setContent { AodScreen(::finish) }
    }

    private var proximity: (() -> Unit)? = null

    override fun onDestroy() {
        proximity?.invoke()
        Aod.covered = false
        Aod.open = false
        Aod.close = null
        super.onDestroy()
    }
}

@Composable
private fun AodScreen(onClose: () -> Unit) {
    // The clock moves once a minute (woken at the minute's turn, not every second).
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            now = System.currentTimeMillis()
            delay(60_000 - now % 60_000 + 50)
        }
    }
    // The music stopped (for 5 s): the screen goes back to sleep.
    LaunchedEffect(Aod.playing) {
        if (!Aod.playing) {
            delay(5000)
            onClose()
        }
    }
    // A small shift every minute, so nothing stays lit on the same pixels.
    val minute = now / 60_000
    val shift = remember(minute) { Random.nextInt(-24, 25).dp to Random.nextInt(-48, 49).dp }
    val tint = coverColor(Aod.thumb)
    val ctx = androidx.compose.ui.platform.LocalContext.current
    // A tap shows the controls for 5 s; a double tap closes the screen.
    var controls by remember { mutableStateOf(false) }
    LaunchedEffect(controls) {
        if (controls) {
            delay(5000)
            controls = false
        }
    }
    val battery = remember(minute) {
        ctx.getSystemService(android.os.BatteryManager::class.java)?.getIntProperty(android.os.BatteryManager.BATTERY_PROPERTY_CAPACITY)?.takeIf { it in 0..100 }
    }
    val clock = if (android.text.format.DateFormat.is24HourFormat(ctx)) "HH:mm" else "h:mm"
    fun key(code: Int) = runCatching { com.rymplayer.playback.PlayerWidget.button(ctx, code).send() }
    if (Aod.covered) {
        // In a pocket: nothing lit, no touches.
        Box(Modifier.fillMaxSize().background(Color.Black).clickable(remember { MutableInteractionSource() }, indication = null) {})
        return
    }
    Box(
        Modifier.fillMaxSize().background(Color.Black)
            .pointerInput(Unit) { detectTapGestures(onTap = { controls = !controls }, onDoubleTap = { onClose() }) },
    ) {
        Column(
            Modifier.fillMaxSize().offset(shift.first, shift.second).padding(horizontal = 28.dp, vertical = 72.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(SimpleDateFormat(clock, Locale.getDefault()).format(Date(now)), fontSize = 64.sp, fontWeight = FontWeight.Light, color = Color.White.copy(alpha = 0.85f))
            Text(SimpleDateFormat("EEE d MMM", Locale.getDefault()).format(Date(now)) + (battery?.let { "  ·  $it%" } ?: ""), fontSize = 15.sp, color = Color.White.copy(alpha = 0.5f))
            Spacer(Modifier.weight(1f))
            AnimatedContent(Aod.line, transitionSpec = { fadeIn(tween(400)) togetherWith fadeOut(tween(250)) }, label = "line") { line ->
                Text(
                    line ?: "♪",
                    fontSize = 30.sp,
                    fontWeight = FontWeight.Bold,
                    color = lerp(tint, Color.White, 0.55f),
                    textAlign = TextAlign.Center,
                    lineHeight = 38.sp,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Spacer(Modifier.height(14.dp))
            Aod.next?.let {
                Text(it, fontSize = 18.sp, color = Color.White.copy(alpha = 0.35f), textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth())
            }
            Spacer(Modifier.weight(1f))
            // Previous, play / pause, next: shown by a tap, dim.
            androidx.compose.animation.AnimatedVisibility(controls) {
                Row(Modifier.padding(bottom = 18.dp), verticalAlignment = Alignment.CenterVertically) {
                    val dim = Color.White.copy(alpha = 0.55f)
                    androidx.compose.material3.IconButton({ key(android.view.KeyEvent.KEYCODE_MEDIA_PREVIOUS); controls = true }) {
                        androidx.compose.material3.Icon(androidx.compose.material.icons.Icons.Default.SkipPrevious, tr("Previous"), tint = dim)
                    }
                    androidx.compose.material3.IconButton({ key(android.view.KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE); controls = true }, Modifier.padding(horizontal = 18.dp)) {
                        androidx.compose.material3.Icon(if (Aod.playing) androidx.compose.material.icons.Icons.Default.Pause else androidx.compose.material.icons.Icons.Default.PlayArrow, tr("Play"), tint = dim, modifier = Modifier.padding(2.dp))
                    }
                    androidx.compose.material3.IconButton({ key(android.view.KeyEvent.KEYCODE_MEDIA_NEXT); controls = true }) {
                        androidx.compose.material3.Icon(androidx.compose.material.icons.Icons.Default.SkipNext, tr("Next"), tint = dim)
                    }
                }
            }
            if (!controls) Text(tr("Tap for controls · double tap to close"), fontSize = 11.sp, color = Color.White.copy(alpha = 0.22f), modifier = Modifier.padding(bottom = 14.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Cover(Aod.thumb, 40.dp, 120, 6.dp)
                Spacer(Modifier.width(12.dp))
                Column {
                    Text(Aod.title, fontSize = 14.sp, color = Color.White.copy(alpha = 0.7f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(Aod.artist, fontSize = 12.sp, color = Color.White.copy(alpha = 0.45f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
