package com.rymplayer.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rymplayer.AppViewModel
import com.rymplayer.tr

/** The computer heard in the last 12 seconds (the remote's button shows then). */
fun AppViewModel.pcNearby(now: Long = System.currentTimeMillis()) = player.pc?.takeIf { now - it.at < 12_000 }

/**
 * The remote for the computer playing RyM on the same Wi-Fi (same account): its song, play,
 * pause, skip, seek and volume, and "Play along" (its song here too, in step).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteSheet(vm: AppViewModel, onDismiss: () -> Unit) {
    val p = vm.player
    // The position moves on between the computer's notes.
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(250)
            now = System.currentTimeMillis()
        }
    }
    val pc = vm.pcNearby(now)
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = AppColorsSheet, contentColor = MaterialTheme.colorScheme.onBackground) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Default.Computer, null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(10.dp))
                Text(pc?.device?.ifEmpty { null } ?: tr("Computer"), fontWeight = FontWeight.Bold, fontSize = 18.sp)
            }
            if (pc == null) {
                Spacer(Modifier.height(16.dp))
                Text(tr("No computer heard: open RyM on it (same Wi-Fi, same account)"), color = Dim)
                return@Column
            }
            Spacer(Modifier.height(16.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Cover(pc.thumb, 64.dp, 200, 8.dp)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(pc.title.ifEmpty { tr("Nothing playing") }, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold)
                    Text(pc.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
                }
            }
            if (!pc.remote) {
                Spacer(Modifier.height(16.dp))
                Text(tr("Turn on \"Remote from the phone\" in RyM's settings on the computer"), color = Dim)
            } else {
                // Seek: shown where the finger is while dragging, sent when let go.
                var dragging by remember { mutableStateOf(false) }
                var drag by remember { mutableFloatStateOf(0f) }
                val length = pc.duration.coerceAtLeast(1f)
                Spacer(Modifier.height(12.dp))
                Slider(
                    value = if (dragging) drag else (pc.positionNow(now) / length).coerceIn(0f, 1f),
                    onValueChange = { dragging = true; drag = it },
                    onValueChangeFinished = { p.remote("seek", drag * length); dragging = false },
                )
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(clock(if (dragging) drag * length else pc.positionNow(now)), color = Dim, fontSize = 12.sp)
                    Text(clock(pc.duration), color = Dim, fontSize = 12.sp)
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                    IconButton({ p.remote("prev") }) { Icon(Icons.Default.SkipPrevious, tr("Previous"), Modifier.size(34.dp)) }
                    FilledIconButton({ p.remote("toggle") }, Modifier.size(64.dp)) {
                        Icon(if (pc.playing) Icons.Default.Pause else Icons.Default.PlayArrow, tr("Play / pause"), Modifier.size(36.dp))
                    }
                    IconButton({ p.remote("next") }) { Icon(Icons.Default.SkipNext, tr("Next"), Modifier.size(34.dp)) }
                }
                Spacer(Modifier.height(8.dp))
                var vol by remember(pc.device) { mutableFloatStateOf(pc.volume) }
                var moving by remember { mutableStateOf(false) }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.AutoMirrored.Filled.VolumeUp, tr("Volume"), tint = Dim)
                    Spacer(Modifier.width(8.dp))
                    Slider(
                        value = if (moving) vol else pc.volume,
                        onValueChange = { moving = true; vol = it },
                        onValueChangeFinished = { p.remote("volume", vol); moving = false },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(tr("Play along"), fontWeight = FontWeight.SemiBold)
                    Text(tr("The computer's song plays here too, in step"), color = Dim, fontSize = 13.sp)
                }
                Switch(p.playAlong, { if (it) p.startPlayAlong() else p.stopPlayAlong() })
            }
            if (p.playAlong) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(tr("Delay"), color = Dim, modifier = Modifier.weight(1f))
                    TextButton({ p.nudgeAlongOffset(-50) }) { Text("−50 ms") }
                    Text("${p.alongOffsetMs} ms", fontWeight = FontWeight.SemiBold)
                    TextButton({ p.nudgeAlongOffset(50) }) { Text("+50 ms") }
                }
            }
        }
    }
}

private fun clock(secs: Float): String {
    val s = secs.toInt().coerceAtLeast(0)
    return "%d:%02d".format(s / 60, s % 60)
}
