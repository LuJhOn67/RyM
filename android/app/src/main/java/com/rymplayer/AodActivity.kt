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
}

/**
 * RyM's own always-on screen (Android doesn't let apps draw on the phone's): when the screen is
 * turned off while music plays, a dim black screen over the lock screen with the clock, the sung
 * line in the cover's colour, the next line, and the song. It moves a little every minute (no
 * burn-in), and goes with a touch, when the music stops, or when the screen is turned off again.
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
        setContent { AodScreen(::finish) }
    }

    override fun onDestroy() {
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
    Box(
        Modifier.fillMaxSize().background(Color.Black)
            .clickable(remember { MutableInteractionSource() }, indication = null) { onClose() },
    ) {
        Column(
            Modifier.fillMaxSize().offset(shift.first, shift.second).padding(horizontal = 28.dp, vertical = 72.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date(now)), fontSize = 64.sp, fontWeight = FontWeight.Light, color = Color.White.copy(alpha = 0.85f))
            Text(SimpleDateFormat("EEE d MMM", Locale.getDefault()).format(Date(now)), fontSize = 15.sp, color = Color.White.copy(alpha = 0.5f))
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
