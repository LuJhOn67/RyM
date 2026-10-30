package com.rymplayer.ui

import com.rymplayer.I18n
import com.rymplayer.tr
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Login
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rymplayer.AppViewModel
import com.rymplayer.Screen
import com.rymplayer.data.LIKED_ID
import com.rymplayer.data.Track

private val pagePadding = PaddingValues(bottom = 24.dp)

@Composable
fun Title(text: String, modifier: Modifier = Modifier) {
    Text(text, fontSize = 24.sp, fontWeight = FontWeight.Bold, modifier = modifier.padding(horizontal = 16.dp, vertical = 10.dp))
}

@Composable
fun Loading() = Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }

/** Rises and fades in on first show. */
@Composable
fun Modifier.appear(key: Any, delayMs: Int = 0): Modifier {
    var shown by remember(key) { mutableStateOf(false) }
    LaunchedEffect(key) { shown = true }
    val k by animateFloatAsState(if (shown) 1f else 0f, tween(420, delayMs), label = "appear")
    return graphicsLayer { alpha = k; translationY = (1 - k) * 40f }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
fun HomePage(vm: AppViewModel) {
    if (vm.home.isEmpty()) {
        if (vm.homeLoading) Loading() else Box(Modifier.fillMaxSize(), Alignment.Center) {
            TextButton(vm::loadHome) { Text(tr("Nothing here yet. Tap to retry")) }
        }
        return
    }
    val sections = remember(vm.home) { vm.home.groupBy { it.album }.toList() }
    LaunchedEffect(Unit) { vm.refreshHomeIfOld() }
    // Pull down for a fresh feed.
    androidx.compose.material3.pulltorefresh.PullToRefreshBox(isRefreshing = vm.homeLoading, onRefresh = vm::loadHome) {
    LazyColumn(contentPadding = pagePadding) {
        item { Title(tr("For you"), Modifier.padding(top = 8.dp)) }
        itemsIndexed(sections, key = { _, s -> s.first }) { i, (name, items) ->
            Column(Modifier.appear(name, 60 * i.coerceAtMost(6))) {
                Text(name, fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 16.dp, top = 14.dp, bottom = 10.dp))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = CardGap) {
                    items(items) { t -> HomeCard(vm, t) }
                }
            }
        }
    }
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun HomeCard(vm: AppViewModel, t: Track) {
    val hold: (() -> Unit)? = if (t.link == null) ({ vm.menu = com.rymplayer.MenuFor(t) }) else null
    Column(Modifier.width(150.dp).clip(RoundedCornerShape(10.dp)).combinedClickable(onClick = { vm.openLink(t) }, onLongClick = hold)) {
        Cover(t.thumb, 150.dp, 360, 10.dp)
        Spacer(Modifier.height(8.dp))
        Text(t.title, maxLines = 2, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
        Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 12.sp)
    }
}

@Composable
fun SearchPage(vm: AppViewModel) {
    Column(Modifier.fillMaxSize()) {
        Title(tr("Search"))
        OutlinedTextField(
            value = vm.query,
            onValueChange = { vm.query = it },
            placeholder = { Text(tr("Songs, artists, albums")) },
            leadingIcon = { Icon(Icons.Default.Search, null) },
            singleLine = true,
            shape = RoundedCornerShape(28.dp),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { vm.search() }),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
    }
}

@Composable
fun LibraryPage(vm: AppViewModel) {
    var naming by remember { mutableStateOf(false) }
    val context = androidx.compose.ui.platform.LocalContext.current
    LazyColumn(contentPadding = pagePadding) {
        item { Title(tr("Library")) }
        item {
            LibraryRow(tr("Settings"), tr("Language, quality, plugins, updates, account"), Brush.linearGradient(listOf(Color(0xFF2D9CFF), Color(0xFF8E44FF))), Icons.Default.Settings) {
                vm.settingsOpen = true
            }
        }
        item {
            LibraryRow(tr("Downloads"), tr("Saved on this phone, play without internet"), Brush.linearGradient(listOf(Color(0xFF19B6A5), Color(0xFF2878D0))), Icons.Default.Download) {
                vm.open(Screen.Songs(Screen.Kind.Downloads, "downloads", tr("Downloads")))
            }
        }
        if (vm.api.signedIn) {
            item {
                LibraryRow(tr("Liked Music"), tr("Auto playlist"), Brush.linearGradient(listOf(Color(0xFFFF2D55), Color(0xFF8E44FF))), Icons.Default.Favorite) {
                    vm.open(Screen.Songs(Screen.Kind.Playlist, LIKED_ID, tr("Liked Music")))
                }
            }
            item {
                LibraryRow(tr("History"), tr("What you played"), Brush.linearGradient(listOf(Color(0xFF2D9CFF), Color(0xFF00C2A8))), Icons.Default.History) {
                    vm.open(Screen.Songs(Screen.Kind.History, "history", tr("History")))
                }
            }
            item {
                LibraryRow(tr("New playlist"), "", Brush.linearGradient(listOf(Color(0xFF3A3A42), Color(0xFF3A3A42))), Icons.AutoMirrored.Filled.PlaylistAdd) { naming = true }
            }
            items(vm.playlists) { p ->
                Row(
                    Modifier.fillMaxWidth().clickable { vm.open(Screen.Songs(Screen.Kind.Playlist, p.id, p.title)) }.padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Cover(p.thumb, 56.dp, 160, 8.dp)
                    Spacer(Modifier.width(14.dp))
                    Column {
                        Text(p.title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(tr("Playlist"), color = Dim, fontSize = 13.sp)
                    }
                }
            }
        } else {
            item { Text(tr("Sign in to see your playlists"), color = Dim, modifier = Modifier.padding(16.dp)) }
        }
        item { Text("RyM ${vm.versionName}", color = Dim, fontSize = 12.sp, modifier = Modifier.padding(16.dp)) }
    }
    if (naming) NewPlaylist({ naming = false }) { vm.createPlaylist(it, null); naming = false }
}

@Composable
private fun LibraryRow(title: String, sub: String, brush: Brush, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(56.dp).clip(RoundedCornerShape(8.dp)).background(brush), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = Color.White)
        }
        Spacer(Modifier.width(14.dp))
        Column {
            Text(title, fontWeight = FontWeight.SemiBold)
            if (sub.isNotEmpty()) Text(sub, color = Dim, fontSize = 13.sp)
        }
    }
}

/** A playlist, album, mix, artist, history or search results. */
@Composable
fun SongsPage(vm: AppViewModel, page: Screen.Songs) {
    val tracks = vm.pages[page] ?: return Loading()
    val current = vm.player.current?.videoId
    val songs = tracks.filter { it.link == null }
    LazyColumn(contentPadding = pagePadding) {
        item {
            Column(Modifier.fillMaxWidth().padding(16.dp).appear(page), horizontalAlignment = Alignment.CenterHorizontally) {
                Cover(tracks.firstOrNull()?.thumb, 200.dp, 480, 12.dp)
                Spacer(Modifier.height(14.dp))
                Text(tr(page.title), fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text("${songs.size} ${tr("songs")}", color = Dim, fontSize = 13.sp)
                Spacer(Modifier.height(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button({ vm.playFrom(tracks, tracks.indexOfFirst { it.link == null }.coerceAtLeast(0)) }, enabled = songs.isNotEmpty()) {
                        Icon(Icons.Default.PlayArrow, null)
                        Spacer(Modifier.width(6.dp))
                        Text(tr("Play"))
                    }
                    OutlinedButton({ vm.playFrom(songs.shuffled(), 0) }, enabled = songs.isNotEmpty(), colors = ButtonDefaults.outlinedButtonColors(contentColor = Color.White)) {
                        Icon(Icons.Default.Shuffle, null)
                        Spacer(Modifier.width(6.dp))
                        Text(tr("Shuffle"))
                    }
                }
                if (page.kind == Screen.Kind.Playlist && page.id == LIKED_ID && vm.api.signedIn) DownloadAll(vm)
            }
        }
        if (tracks.isEmpty()) item { Text(vm.status.ifEmpty { tr("Nothing here") }, color = Dim, modifier = Modifier.padding(16.dp)) }
        itemsIndexed(tracks) { i, t ->
            val onClick = {
                when {
                    t.link != null -> vm.openLink(t)
                    // A search result: the song, then a radio of songs like it.
                    page.kind == Screen.Kind.Results -> vm.playRadio(t)
                    else -> vm.playFrom(tracks, i)
                }
            }
            SongRow(vm, t, current == t.videoId && t.videoId.isNotEmpty(), onClick)
        }
    }
}

/** Liked Music: download the first N (empty = all), one after another, with progress. */
@Composable
private fun DownloadAll(vm: AppViewModel) {
    var count by remember { mutableStateOf("") }
    val b = vm.batch
    val running = b != null && !b.finished
    Spacer(Modifier.height(12.dp))
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        OutlinedTextField(
            value = count,
            onValueChange = { v -> count = v.filter(Char::isDigit).take(5) },
            placeholder = { Text(tr("How many (empty = all)"), fontSize = 13.sp) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Number),
            modifier = Modifier.width(190.dp),
        )
        Button({ vm.downloadAll(count.toIntOrNull()?.takeIf { it > 0 } ?: 5000) }, enabled = !running) {
            Icon(Icons.Default.Download, null)
            Spacer(Modifier.width(6.dp))
            Text(tr("Download all"))
        }
    }
    if (b != null) {
        Spacer(Modifier.height(8.dp))
        val words = buildString {
            append("${tr("Downloading songs")} ${b.done} / ${b.total}")
            for ((n, what) in listOf(b.saved to "saved", b.skipped to "already there", b.failed to "failed")) if (n > 0) append(" · $n ${tr(what)}")
        }
        Text(words, color = if (b.finished) Dim else MaterialTheme.colorScheme.primary, fontSize = 13.sp)
        LinearProgressIndicator(progress = { if (b.total > 0) b.done.toFloat() / b.total else 0f }, modifier = Modifier.width(260.dp).padding(top = 6.dp))
    }
}

@Composable
fun SectionLabel(text: String) = Text(text.uppercase(), color = MaterialTheme.colorScheme.primary, fontSize = 12.sp, fontWeight = FontWeight.Bold)
