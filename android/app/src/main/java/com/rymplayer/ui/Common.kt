package com.rymplayer.ui

import androidx.compose.ui.input.pointer.pointerInput
import com.rymplayer.tr
import androidx.compose.material.icons.filled.PlayArrow

import androidx.compose.material3.ExperimentalMaterial3Api

import androidx.compose.material3.ModalBottomSheet

import android.content.Context
import android.content.Intent
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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.asImageBitmap
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
import androidx.compose.material.icons.filled.Category
import androidx.compose.material.icons.filled.ThumbUp
import androidx.compose.material.icons.filled.ThumbDown
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Radio
import androidx.compose.material.icons.filled.Share
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

/** The color theme's background (Settings > Colors); null: the app's own. */
var themeBg by androidx.compose.runtime.mutableStateOf<Color?>(null)

/** Cyber HUD: corners cut at 45 degrees instead of rounded. */
var themeCut by androidx.compose.runtime.mutableStateOf(false)

/** A theme's corner: cut at 45 degrees on Cyber HUD, rounded otherwise. */
fun themeShape(corner: Dp): androidx.compose.ui.graphics.Shape =
    if (themeCut) androidx.compose.foundation.shape.CutCornerShape(corner.coerceAtLeast(8.dp)) else RoundedCornerShape(corner)

/** The pages' background: the color theme's, else near black, or pure black in RyM YTM. */
fun pageColor(ytm: Boolean) = themeBg ?: if (ytm) Color.Black else Color(0xFF0B0B0D)

private fun hex(h: String) = Color(com.rymplayer.data.Themes.argb(h) ?: 0xFF808080.toInt())

/** The theme's pattern (grid, dots, rain...) as a small picture repeated over the app; null: plain. */
private fun patternBrush(t: com.rymplayer.data.Themes.Theme, s: Float): androidx.compose.ui.graphics.ShaderBrush? {
    val text = com.rymplayer.data.Themes.argb(t.text) ?: return null
    val acc = com.rymplayer.data.Themes.argb(t.accent2) ?: return null
    fun a(c: Int, alpha: Int) = (c and 0xFFFFFF) or (alpha shl 24)
    val (w, h) = when (t.style) {
        "grid" -> 40 to 40; "dots" -> 12 to 12; "halftone" -> 14 to 14; "rain" -> 60 to 120; "scanlines" -> 4 to 4
        "hud" -> 64 to 64; "aqua" -> 120 to 120; "stars" -> 160 to 160; "ash" -> 150 to 150; else -> return null
    }
    val bmp = android.graphics.Bitmap.createBitmap((w * s).toInt().coerceAtLeast(1), (h * s).toInt().coerceAtLeast(1), android.graphics.Bitmap.Config.ARGB_8888)
    val c = android.graphics.Canvas(bmp)
    val p = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply { strokeWidth = s }
    val rnd = java.util.Random(7)
    val (W, H) = w * s to h * s
    when (t.style) {
        "grid" -> { p.color = a(text, 30); c.drawLine(0f, H - s / 2, W, H - s / 2, p); c.drawLine(W - s / 2, 0f, W - s / 2, H, p) }
        "dots" -> { p.color = a(text, 36); c.drawCircle(6 * s, 6 * s, 1.6f * s, p) }
        "halftone" -> { p.color = a(0, 42); c.drawCircle(7 * s, 7 * s, 3.2f * s, p) }
        "scanlines" -> { p.color = a(0, 60); c.drawRect(0f, 0f, W, s, p) }
        "rain" -> { p.color = a(acc, 60); repeat(7) { val x = rnd.nextInt(w) * s; val y = rnd.nextInt(h) * s; c.drawLine(x, y, x, y + (10 + rnd.nextInt(18)) * s, p) } }
        "hud" -> {
            p.color = a(acc, 80); val k = 6 * s
            c.drawLine(0f, 0f, k, 0f, p); c.drawLine(0f, 0f, 0f, k, p); c.drawLine(W, 0f, W - k, 0f, p); c.drawLine(W - s / 2, 0f, W - s / 2, k, p)
            c.drawLine(0f, H - s / 2, k, H - s / 2, p); c.drawLine(0f, H, 0f, H - k, p); c.drawLine(W, H - s / 2, W - k, H - s / 2, p); c.drawLine(W - s / 2, H, W - s / 2, H - k, p)
        }
        "aqua" -> { p.style = android.graphics.Paint.Style.STROKE; p.color = a(-1, 80); repeat(5) { c.drawCircle(rnd.nextInt(w) * s, rnd.nextInt(h) * s, (3 + rnd.nextInt(9)) * s, p) } }
        "ash" -> repeat(16) { p.color = a(text, 35 + rnd.nextInt(55)); c.drawCircle(rnd.nextInt(w) * s, rnd.nextInt(h) * s, (0.7f + rnd.nextInt(10) / 8f) * s, p) }
        "stars" -> repeat(26) { p.color = a(if (rnd.nextInt(4) == 0) acc else text, 50 + rnd.nextInt(150)); c.drawCircle(rnd.nextInt(w) * s, rnd.nextInt(h) * s, 0.8f * s, p) }
    }
    return androidx.compose.ui.graphics.ShaderBrush(androidx.compose.ui.graphics.ImageShader(bmp.asImageBitmap(), androidx.compose.ui.graphics.TileMode.Repeated, androidx.compose.ui.graphics.TileMode.Repeated))
}

@Composable
fun RymTheme(ytm: Boolean = false, colors: com.rymplayer.data.Themes.Theme? = null, content: @Composable () -> Unit) {
    androidx.compose.runtime.SideEffect {
        themeBg = colors?.let { hex(it.bg) }
        themeCut = colors?.cut == true
    }
    val scheme = if (colors == null) darkColorScheme(
        primary = if (ytm) Color(0xFFFF3048) else Accent,
        background = if (ytm) Color.Black else Color(0xFF0B0B0D),
        surface = if (ytm) Color.Black else Color(0xFF0B0B0D),
        surfaceVariant = if (ytm) Color(0xFF212121) else Color(0xFF1C1C21),
        onSurfaceVariant = Dim,
        // Pure white text on the dark pages (the default is a light grey).
        onBackground = Color.White,
        onSurface = Color.White,
    ) else if (colors.dark) darkColorScheme(
        primary = hex(colors.accent), secondary = hex(colors.accent2), onPrimary = hex(colors.bg),
        background = hex(colors.bg), surface = hex(colors.bg), surfaceVariant = hex(colors.raised),
        onSurfaceVariant = hex(colors.dim), onBackground = hex(colors.text), onSurface = hex(colors.text), outline = hex(colors.border),
    ) else androidx.compose.material3.lightColorScheme(
        primary = hex(colors.accent), secondary = hex(colors.accent2), onPrimary = hex(colors.bg),
        background = hex(colors.bg), surface = hex(colors.bg), surfaceVariant = hex(colors.raised),
        onSurfaceVariant = hex(colors.dim), onBackground = hex(colors.text), onSurface = hex(colors.text), outline = hex(colors.border),
    )
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    val brush = remember(colors, density) { colors?.let { patternBrush(it, density) } }
    MaterialTheme(colorScheme = scheme) {
        val vignette = colors?.vignette == true
        Box(Modifier.fillMaxSize().drawWithContent {
            drawContent()
            brush?.let { drawRect(it) }
            if (vignette && size.minDimension > 1f) {
                drawRect(androidx.compose.ui.graphics.Brush.radialGradient(0.45f to Color.Transparent, 1f to Color.Black.copy(alpha = 0.6f), center = center, radius = size.maxDimension * 0.62f))
            }
        }) { content() }
    }
}

/** Cover art with a rounded corner and a note placeholder while it loads. */
@Composable
fun Cover(url: String?, size: Dp, px: Int, corner: Dp = 8.dp, modifier: Modifier = Modifier, muted: Boolean = false) {
    val shape = if (themeCut && corner > 0.dp) androidx.compose.foundation.shape.CutCornerShape(size * 0.12f) else RoundedCornerShape(corner)
    SubcomposeAsyncImage(
        model = ImageRequest.Builder(LocalContext.current).data(thumbSized(url, px)).crossfade(250).build(),
        contentDescription = null,
        contentScale = ContentScale.Crop,
        // Remorse: the cover keeps 30% of its color.
        colorFilter = if (muted) androidx.compose.ui.graphics.ColorFilter.colorMatrix(androidx.compose.ui.graphics.ColorMatrix().apply { setToSaturation(0.3f) }) else null,
        modifier = modifier.size(size).clip(shape),
        loading = { Placeholder(size, shape) },
        error = { Placeholder(size, shape) },
    )
}

@Composable
private fun Placeholder(size: Dp, shape: androidx.compose.ui.graphics.Shape) {
    Box(Modifier.size(size).clip(shape).background(Color(0xFF26262C)), contentAlignment = Alignment.Center) {
        Icon(Icons.Default.MusicNote, null, tint = Dim, modifier = Modifier.size(size * 0.4f))
    }
}

/** The cover's most colorful shade, darkened for a background; fades when the song changes. */
@Composable
fun coverColor(url: String?): Color {
    val context = LocalContext.current
    var target by remember { mutableStateOf(url?.let { CoverColors.get(it) }?.let { Color(it) } ?: Color(0xFF2A2A30)) }
    LaunchedEffect(url) {
        if (url == null) return@LaunchedEffect
        CoverColors.get(url)?.let { target = Color(it); return@LaunchedEffect }
        val req = ImageRequest.Builder(context).data(thumbSized(url, 96)).allowHardware(false).build()
        val bmp = ((context.imageLoader.execute(req) as? SuccessResult)?.drawable as? BitmapDrawable)?.bitmap ?: return@LaunchedEffect
        val p = Palette.from(bmp).generate()
        val rgb = (p.vibrantSwatch ?: p.mutedSwatch ?: p.dominantSwatch)?.rgb ?: return@LaunchedEffect
        CoverColors.put(url, rgb)
        target = Color(rgb)
    }
    return animateColorAsState(target, tween(900), label = "cover").value
}

/** Covers' colours already worked out (Palette runs once per cover). */
private val CoverColors = android.util.LruCache<String, Int>(128)

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SongRow(vm: AppViewModel, t: Track, current: Boolean, onClick: () -> Unit, queueIndex: Int? = null, menuOnHold: Boolean = true) {
    Row(
        Modifier.fillMaxWidth()
            .combinedClickable(onClick = onClick, onLongClick = if (menuOnHold) ({ vm.menu = MenuFor(t, queueIndex) }) else null)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(t.thumb, 50.dp, 120, 6.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(
                t.title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold,
                color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onBackground,
            )
            // The artist's name opens their page; album and length follow it.
            Row {
                if (t.artist.isNotBlank()) {
                    Text(
                        t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp,
                        modifier = Modifier.weight(1f, fill = false).clickable(enabled = t.link == null && com.rymplayer.data.isYouTube(t.videoId)) { vm.goArtist(t) },
                    )
                }
                val rest = listOf(t.album, t.duration).filter { it.isNotBlank() }.joinToString(" • ")
                if (rest.isNotEmpty()) Text((if (t.artist.isNotBlank()) " • " else "") + rest, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
            }
        }
        if (t.videoId in vm.liked) Icon(Icons.Default.Favorite, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
        IconButton(onClick = { vm.menu = MenuFor(t, queueIndex) }) { Icon(Icons.Default.MoreVert, tr("More"), tint = Dim) }
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
    val context = LocalContext.current
    var pick by remember(t.videoId) { mutableStateOf(false) }
    var kinds by remember(t.videoId) { mutableStateOf(false) }
    if (pick) {
        SaveToPlaylist(vm, t) { pick = false; onDismiss() }
        return
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = AppColorsSheet, contentColor = MaterialTheme.colorScheme.onBackground) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Cover(t.thumb, 48.dp, 120, 6.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(t.title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onBackground)
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
                Text(label, fontSize = 15.sp, color = MaterialTheme.colorScheme.onBackground)
            }
        }
        item(tr("Start radio"), Icons.Default.Radio) { vm.playRadio(t) }()
        item(tr("Play next"), Icons.Default.SkipNext) { vm.player.playNext(t) }()
        item(tr("Add to queue"), Icons.AutoMirrored.Filled.QueueMusic) { vm.player.addToQueue(t) }()
        item(tr("Recommend more"), Icons.Default.ThumbUp) { vm.recommendMore(t) }()
        item(tr("Don't recommend"), Icons.Default.ThumbDown) { vm.dontRecommend(t) }()
        // RaM points: the artist's kind, put right by hand.
        Row(
            Modifier.fillMaxWidth().clickable { kinds = !kinds }.padding(horizontal = 20.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Default.Category, null, tint = Dim)
            Spacer(Modifier.width(20.dp))
            Text(tr("This artist's kind"), fontSize = 15.sp, color = MaterialTheme.colorScheme.onBackground)
        }
        if (kinds) {
            for (k in com.rymplayer.data.RamPoints.Kind.entries) {
                Text(
                    tr(k.title),
                    fontSize = 15.sp,
                    color = MaterialTheme.colorScheme.onBackground,
                    modifier = Modifier.fillMaxWidth().clickable { onDismiss(); vm.setArtistKind(t, k) }.padding(start = 64.dp, top = 10.dp, bottom = 10.dp),
                )
            }
        }
        item(tr("Download"), Icons.Default.Download) { vm.download(t) }()
        if (com.rymplayer.playback.Vlc.installed(LocalContext.current) && com.rymplayer.data.isYouTube(t.videoId)) {
            item(tr("Open in VLC"), Icons.Default.PlayArrow) { vm.openInVlc(t) }()
        }
        val youtube = com.rymplayer.data.isYouTube(t.videoId)
        if (vm.api.signedIn && youtube) {
            val liked = t.videoId in vm.liked
            item(if (liked) tr("Remove from liked songs") else tr("Add to liked songs"), if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder) { vm.toggleLike(t) }()
            Row(
                Modifier.fillMaxWidth().clickable { pick = true }.padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.AutoMirrored.Filled.PlaylistAdd, null, tint = Dim)
                Spacer(Modifier.width(20.dp))
                Text(tr("Save to playlist"), fontSize = 15.sp, color = MaterialTheme.colorScheme.onBackground)
            }
        }
        t.albumId?.let { id -> item(tr("Go to album"), Icons.Default.Album) { vm.open(Screen.Songs(Screen.Kind.Album, id, t.album)) }() }
        if (t.artist.isNotBlank() && youtube) item(tr("Go to artist"), Icons.Default.Person) { vm.goArtist(t) }()
        item(tr("Share"), Icons.Default.Share) { shareSong(context, t) }()
        if (vm.hasVideo && youtube) {
            item(tr("Watch video"), Icons.Default.PlayArrow) { vm.watchVideo(t) }()
            item(tr("Download video"), Icons.Default.Download) { vm.downloadLink("https://www.youtube.com/watch?v=${t.videoId}", true) }()
        }
        if (queueIndex != null && queueIndex != vm.player.index) {
            item(tr("Remove from queue"), Icons.Default.MoreVert) { vm.player.remove(queueIndex) }()
        }
        Spacer(Modifier.height(24.dp))
    }
}

/** Shares the song's link (with its name) through the phone's share sheet. */
fun shareSong(context: Context, t: Track) {
    val link = if (com.rymplayer.data.SoundCloud.isSoundCloud(t.videoId)) com.rymplayer.data.SoundCloud.shareLink(t.videoId) else "https://music.youtube.com/watch?v=${t.videoId}"
    val text = "${t.title} - ${t.artist}\n$link"
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text).putExtra(Intent.EXTRA_TITLE, t.title)
    context.startActivity(Intent.createChooser(send, tr("Share")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

/** A sheet a little lighter than the page, so it stands out. */
internal val AppColorsSheet: Color
    @Composable get() = if (themeBg == null) androidx.compose.ui.graphics.lerp(Color(0xFF0B0B0D), Color.White, 0.08f) else MaterialTheme.colorScheme.surfaceVariant

@Composable
fun SaveToPlaylist(vm: AppViewModel, t: Track, onDismiss: () -> Unit) {
    var naming by remember { mutableStateOf(false) }
    if (naming) {
        NewPlaylist(onDismiss = onDismiss) { name -> vm.createPlaylist(name, t); onDismiss() }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("Save to playlist")) },
        text = {
            LazyColumn(Modifier.heightIn(max = 380.dp)) {
                item {
                    Row(Modifier.fillMaxWidth().clickable { naming = true }.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.AutoMirrored.Filled.PlaylistAdd, null)
                        Spacer(Modifier.width(14.dp))
                        Text(tr("New playlist"), fontWeight = FontWeight.SemiBold)
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
        confirmButton = { TextButton(onDismiss) { Text(tr("Close")) } },
    )
}

@Composable
fun NewPlaylist(onDismiss: () -> Unit, onCreate: (String) -> Unit) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("New playlist")) },
        text = { OutlinedTextField(name, { name = it }, label = { Text(tr("Title")) }, singleLine = true) },
        confirmButton = { TextButton({ if (name.isNotBlank()) onCreate(name.trim()) }) { Text(tr("Create")) } },
        dismissButton = { TextButton(onDismiss) { Text(tr("Cancel")) } },
    )
}

@Composable
fun Marquee(text: String, size: Int, weight: FontWeight, color: Color) {
    Text(text, maxLines = 1, fontSize = size.sp, fontWeight = weight, color = color, modifier = Modifier.basicMarquee())
}

val CardGap = Arrangement.spacedBy(12.dp)

/**
 * A page over the others (the player, Settings): its empty spots take the touch, so a tap there
 * can't reach what's behind it (a tab switched, the mini player paused).
 */
fun androidx.compose.ui.Modifier.takesAllTouches(): androidx.compose.ui.Modifier = this.then(
    androidx.compose.ui.Modifier.pointerInput(Unit) {
        awaitPointerEventScope {
            while (true) awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Final).changes.forEach { it.consume() }
        }
    },
)
