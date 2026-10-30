package com.rymplayer.ui

import androidx.compose.material.icons.filled.PlayArrow

import androidx.compose.material3.ExperimentalMaterial3Api

import androidx.compose.material3.ModalBottomSheet

import android.graphics.drawable.BitmapDrawable
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Radio
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.palette.graphics.Palette
import coil.compose.SubcomposeAsyncImage
import coil.imageLoader
import coil.request.ImageRequest
import coil.request.SuccessResult
import com.rymplayer.AppViewModel
import com.rymplayer.MenuFor
import com.rymplayer.Screen
import com.rymplayer.data.Track
import com.rymplayer.data.thumbSized

val Accent = Color(0xFFFF2D55)
val Dim = Color(0xFFAAAAAA)

@Composable
fun RymTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Accent,
            background = Color(0xFF0B0B0D),
            surface = Color(0xFF0B0B0D),
            surfaceVariant = Color(0xFF1C1C21),
            onSurfaceVariant = Dim,
            // Pure white text on the dark pages (the default is a light grey).
            onBackground = Color.White,
            onSurface = Color.White,
        ),
        content = content,
    )
}

/** Cover art with a rounded corner and a note placeholder while it loads. */
@Composable
fun Cover(url: String?, size: Dp, px: Int, corner: Dp = 8.dp, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(corner)
    SubcomposeAsyncImage(
        model = ImageRequest.Builder(LocalContext.current).data(thumbSized(url, px)).crossfade(250).build(),
        contentDescription = null,
        contentScale = ContentScale.Crop,
        modifier = modifier.size(size).clip(shape),
        loading = { Placeholder(size, shape) },
        error = { Placeholder(size, shape) },
    )
}

@Composable
private fun Placeholder(size: Dp, shape: RoundedCornerShape) {
    Box(Modifier.size(size).clip(shape).background(Color(0xFF26262C)), contentAlignment = Alignment.Center) {
        Icon(Icons.Default.MusicNote, null, tint = Dim, modifier = Modifier.size(size * 0.4f))
    }
}

/** The cover's most colorful shade, darkened for a background; fades when the song changes. */
@Composable
fun coverColor(url: String?): Color {
    val context = LocalContext.current
    var target by remember { mutableStateOf(Color(0xFF2A2A30)) }
    LaunchedEffect(url) {
        if (url == null) return@LaunchedEffect
        val req = ImageRequest.Builder(context).data(thumbSized(url, 96)).allowHardware(false).build()
        val bmp = ((context.imageLoader.execute(req) as? SuccessResult)?.drawable as? BitmapDrawable)?.bitmap ?: return@LaunchedEffect
        val p = Palette.from(bmp).generate()
        val rgb = (p.vibrantSwatch ?: p.mutedSwatch ?: p.dominantSwatch)?.rgb ?: return@LaunchedEffect
        target = Color(rgb)
    }
    return animateColorAsState(target, tween(900), label = "cover").value
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SongRow(vm: AppViewModel, t: Track, current: Boolean, onClick: () -> Unit, queueIndex: Int? = null) {
    Row(
        Modifier.fillMaxWidth()
            .combinedClickable(onClick = onClick, onLongClick = { vm.menu = MenuFor(t, queueIndex) })
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(t.thumb, 50.dp, 120, 6.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(
                t.title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold,
                color = if (current) MaterialTheme.colorScheme.primary else Color.White,
            )
            val sub = listOf(t.artist, t.album, t.duration).filter { it.isNotBlank() }.joinToString(" • ")
            Text(sub, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
        }
        if (t.videoId in vm.liked) Icon(Icons.Default.Favorite, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
        IconButton(onClick = { vm.menu = MenuFor(t, queueIndex) }) { Icon(Icons.Default.MoreVert, "More", tint = Dim) }
    }
}

/**
 * The song menu: one sheet from the bottom of the screen for the whole app (rows open it with ⋮ or
 * a long-press, the player with its ⋮). A single shared sheet can't get stuck the way a small
 * menu attached to each row of a scrolling list did.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SongSheet(vm: AppViewModel, m: MenuFor, onDismiss: () -> Unit) {
    val t = m.track
    val queueIndex = m.queueIndex
    var pick by remember(t.videoId) { mutableStateOf(false) }
    if (pick) {
        SaveToPlaylist(vm, t) { pick = false; onDismiss() }
        return
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = AppColorsSheet, contentColor = Color.White) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Cover(t.thumb, 48.dp, 120, 6.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(t.title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, color = Color.White)
                Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
            }
        }
        Spacer(Modifier.height(8.dp))
        fun item(label: String, icon: ImageVector, action: () -> Unit) = @Composable {
            Row(
                Modifier.fillMaxWidth().clickable { onDismiss(); action() }.padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(icon, null, tint = Dim)
                Spacer(Modifier.width(20.dp))
                Text(label, fontSize = 15.sp, color = Color.White)
            }
        }
        item("Start radio", Icons.Default.Radio) { vm.playRadio(t) }()
        item("Play next", Icons.Default.SkipNext) { vm.player.playNext(t) }()
        item("Add to queue", Icons.AutoMirrored.Filled.QueueMusic) { vm.player.append(listOf(t)) }()
        item("Download", Icons.Default.Download) { vm.download(t) }()
        if (com.rymplayer.playback.Vlc.installed(LocalContext.current)) {
            item("Open in VLC", Icons.Default.PlayArrow) { vm.openInVlc(t) }()
        }
        if (vm.api.signedIn) {
            val liked = t.videoId in vm.liked
            item(if (liked) "Remove from liked songs" else "Add to liked songs", if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder) { vm.toggleLike(t) }()
            Row(
                Modifier.fillMaxWidth().clickable { pick = true }.padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.AutoMirrored.Filled.PlaylistAdd, null, tint = Dim)
                Spacer(Modifier.width(20.dp))
                Text("Save to playlist", fontSize = 15.sp, color = Color.White)
            }
        }
        t.albumId?.let { id -> item("Go to album", Icons.Default.Album) { vm.open(Screen.Songs(Screen.Kind.Album, id, t.album)) }() }
        t.artistId?.let { id -> item("Go to artist", Icons.Default.Person) { vm.open(Screen.Songs(Screen.Kind.Artist, id, t.artist.substringBefore(", "))) }() }
        if (queueIndex != null && queueIndex != vm.player.index) {
            item("Remove from queue", Icons.Default.MoreVert) { vm.player.remove(queueIndex) }()
        }
        Spacer(Modifier.height(24.dp))
    }
}

/** A sheet a little lighter than the page, so it stands out. */
private val AppColorsSheet: Color get() = androidx.compose.ui.graphics.lerp(Color(0xFF0B0B0D), Color.White, 0.08f)

@Composable
fun SaveToPlaylist(vm: AppViewModel, t: Track, onDismiss: () -> Unit) {
    var naming by remember { mutableStateOf(false) }
    if (naming) {
        NewPlaylist(onDismiss = onDismiss) { name -> vm.createPlaylist(name, t); onDismiss() }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Save to playlist") },
        text = {
            LazyColumn(Modifier.heightIn(max = 380.dp)) {
                item {
                    Row(Modifier.fillMaxWidth().clickable { naming = true }.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.AutoMirrored.Filled.PlaylistAdd, null)
                        Spacer(Modifier.width(14.dp))
                        Text("New playlist", fontWeight = FontWeight.SemiBold)
                    }
                }
                items(vm.playlists) { p ->
                    Row(Modifier.fillMaxWidth().clickable { vm.addToPlaylist(p, t); onDismiss() }.padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Cover(p.thumb, 40.dp, 120, 4.dp)
                        Spacer(Modifier.width(14.dp))
                        Text(p.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        },
        confirmButton = { TextButton(onDismiss) { Text("Close") } },
    )
}

@Composable
fun NewPlaylist(onDismiss: () -> Unit, onCreate: (String) -> Unit) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New playlist") },
        text = { OutlinedTextField(name, { name = it }, label = { Text("Title") }, singleLine = true) },
        confirmButton = { TextButton({ if (name.isNotBlank()) onCreate(name.trim()) }) { Text("Create") } },
        dismissButton = { TextButton(onDismiss) { Text("Cancel") } },
    )
}

@Composable
fun Marquee(text: String, size: Int, weight: FontWeight, color: Color) {
    Text(text, maxLines = 1, fontSize = size.sp, fontWeight = weight, color = color, modifier = Modifier.basicMarquee())
}

val CardGap = Arrangement.spacedBy(12.dp)
