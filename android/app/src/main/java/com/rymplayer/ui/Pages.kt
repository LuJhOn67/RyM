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
import androidx.compose.material.icons.filled.NorthWest
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.IconButton
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.filled.Insights
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
    val blocks = remember(vm.home) { homeBlocks(vm.home) }
    val current = vm.player.current?.videoId
    val state = androidx.compose.foundation.lazy.rememberLazyListState()
    LaunchedEffect(Unit) { vm.refreshHomeIfOld() }
    NearTheEnd(state) { vm.loadMoreHome() }
    // Pull down for a fresh feed.
    androidx.compose.material3.pulltorefresh.PullToRefreshBox(isRefreshing = vm.homeLoading, onRefresh = vm::loadHome) {
    LazyColumn(state = state, contentPadding = pagePadding) {
        item { HomeTitle(vm) }
        if (vm.ytmTheme) {
            itemsIndexed(ytmSections(vm.home), key = { i, _ -> "ytm$i" }) { i, sec -> YtmSection(vm, sec, current, Modifier.appear(sec.name, 60 * i.coerceAtMost(6))) }
            if (vm.loadingMore) item { MoreSpinner() }
            return@LazyColumn
        }
        itemsIndexed(blocks, key = { i, _ -> i }) { i, b ->
            when (b) {
                is HomeBlock.Header -> Text(tr(b.name), fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.appear(b.name, 60 * i.coerceAtMost(6)).padding(start = 16.dp, top = 14.dp, bottom = 10.dp))
                is HomeBlock.Shelf -> LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = CardGap) {
                    items(b.items) { t -> HomeCard(vm, t) }
                }
                // The mix: one song per row, so the page grows downward as it goes on.
                // (The shelf's name is kept in `album`: not shown as the song's album.)
                is HomeBlock.Song -> SongRow(vm, b.track.copy(album = ""), current == b.track.videoId, { vm.playRadio(b.track) })
            }
        }
        if (vm.loadingMore) item { MoreSpinner() }
    }
    }
}

/** "For you" and the button that switches the phone's theme (classic / RyM YTM). */
@Composable
private fun HomeTitle(vm: AppViewModel) {
    Row(Modifier.fillMaxWidth().padding(top = 8.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Title(tr("For you"), Modifier.weight(1f))
        // The computer playing RyM nearby: its remote.
        if (vm.pcNearby() != null) {
            IconButton({ vm.remoteOpen = true }) {
                Icon(androidx.compose.material.icons.Icons.Default.Computer, contentDescription = tr("Remote"), tint = MaterialTheme.colorScheme.primary)
            }
        }
        IconButton(vm::toggleTheme) {
            Icon(Icons.Default.Palette, tr("Switch theme"), tint = if (vm.ytmTheme) MaterialTheme.colorScheme.primary else Dim)
        }
        // The signed-in account's picture: opens Settings.
        vm.account?.let { a ->
            IconButton({ vm.settingsOpen = true }) { Avatar(a, 30.dp) }
        }
    }
}

/** The account's picture in a circle (its first letter while it loads or when there's none). */
@Composable
fun Avatar(a: com.rymplayer.data.MusicApi.Account, size: androidx.compose.ui.unit.Dp) {
    Box(Modifier.size(size).clip(androidx.compose.foundation.shape.CircleShape).background(MaterialTheme.colorScheme.primary), contentAlignment = Alignment.Center) {
        Text(a.name.take(1).uppercase(), color = Color.White, fontWeight = FontWeight.Bold, fontSize = (size.value * 0.45f).sp)
        if (a.photo != null) {
            coil.compose.AsyncImage(a.photo, a.name, contentScale = androidx.compose.ui.layout.ContentScale.Crop, modifier = Modifier.size(size))
        }
    }
}

/** RyM YTM: a shelf with its songs (as quick picks) or its cards. */
private data class YtmShelf(val name: String, val items: List<Track>, val songs: Boolean)

private fun ytmSections(home: List<Track>): List<YtmShelf> {
    val out = mutableListOf<YtmShelf>()
    var i = 0
    while (i < home.size) {
        val name = home[i].album
        var end = i
        while (end < home.size && home[end].album == name) end++
        val run = home.subList(i, end).toList()
        out += YtmShelf(name, run, run.all { it.link == null })
        i = end
    }
    return out
}

/**
 * RyM YTM: a shelf's name in large type; songs as quick picks (four rows a column, columns side
 * by side) with "Play all", other shelves as big covers sideways.
 */
@Composable
private fun YtmSection(vm: AppViewModel, sec: YtmShelf, current: String?, modifier: Modifier) {
    Column(modifier.padding(top = 18.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(tr(sec.name), fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            if (sec.songs) {
                OutlinedButton(
                    { vm.playFrom(sec.items.map { it.copy(album = "") }, 0) },
                    border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.onBackground.copy(alpha = 0.3f)),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 0.dp),
                    modifier = Modifier.height(32.dp),
                ) { Text(tr("Play all"), fontSize = 13.sp, color = MaterialTheme.colorScheme.onBackground) }
            }
        }
        Spacer(Modifier.height(12.dp))
        if (sec.songs) {
            val width = (androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp * 0.88f).dp
            androidx.compose.foundation.lazy.grid.LazyHorizontalGrid(
                rows = androidx.compose.foundation.lazy.grid.GridCells.Fixed(minOf(4, sec.items.size)),
                modifier = Modifier.height((minOf(4, sec.items.size) * 68).dp),
                contentPadding = PaddingValues(horizontal = 8.dp),
            ) {
                items(sec.items.size) { k -> QuickPick(vm, sec.items[k], current == sec.items[k].videoId, width) }
            }
        } else {
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                items(sec.items) { t -> BigCard(vm, t) }
            }
        }
    }
}

/** A quick pick: cover, title, artist (and length), and its menu. */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun QuickPick(vm: AppViewModel, t: Track, playing: Boolean, width: androidx.compose.ui.unit.Dp) {
    val song = t.copy(album = "")
    Row(
        Modifier.width(width).height(68.dp).clip(RoundedCornerShape(6.dp))
            .combinedClickable(onClick = { vm.playRadio(song) }, onLongClick = { vm.menu = com.rymplayer.MenuFor(song) })
            .padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(t.thumb, 54.dp, 160, 4.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(t.title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = if (playing) MaterialTheme.colorScheme.primary else Color.White)
            Text(listOf(t.artist, t.duration).filter { it.isNotBlank() }.joinToString(" • "), maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
        }
        IconButton({ vm.menu = com.rymplayer.MenuFor(song) }) { Icon(Icons.Default.MoreVert, tr("More"), tint = MaterialTheme.colorScheme.onBackground) }
    }
}

/** A big cover (a playlist, album, mix or song) with its title and what it is. */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun BigCard(vm: AppViewModel, t: Track) {
    val hold: (() -> Unit)? = if (t.link == null) ({ vm.menu = com.rymplayer.MenuFor(t) }) else null
    Column(Modifier.width(176.dp).combinedClickable(onClick = { vm.openLink(t) }, onLongClick = hold)) {
        Cover(t.thumb, 176.dp, 360, 6.dp)
        Spacer(Modifier.height(8.dp))
        Text(t.title, maxLines = 2, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
        Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 13.sp)
    }
}

/** The home page as rows: a title per shelf, the mix song by song, other shelves sideways. */
private sealed interface HomeBlock {
    data class Header(val name: String) : HomeBlock
    data class Shelf(val items: List<Track>) : HomeBlock
    data class Song(val track: Track) : HomeBlock
}

private fun homeBlocks(home: List<Track>): List<HomeBlock> {
    val out = mutableListOf<HomeBlock>()
    var i = 0
    while (i < home.size) {
        val name = home[i].album
        var end = i
        while (end < home.size && home[end].album == name) end++
        val run = home.subList(i, end)
        out += HomeBlock.Header(name)
        if (name == com.rymplayer.data.FeedMemory.FOR_YOU || name == com.rymplayer.data.FeedMemory.MORE_FOR_YOU || name in com.rymplayer.data.RamPoints.SHELVES) run.mapTo(out) { HomeBlock.Song(it) }
        else out += HomeBlock.Shelf(run)
        i = end
    }
    return out
}

/** Calls [load] when the list is scrolled to within five rows of its end. */
@Composable
private fun NearTheEnd(state: androidx.compose.foundation.lazy.LazyListState, load: () -> Unit) {
    LaunchedEffect(state) {
        androidx.compose.runtime.snapshotFlow {
            val info = state.layoutInfo
            (info.visibleItemsInfo.lastOrNull()?.index ?: 0) to info.totalItemsCount
        }.collect { (last, total) -> if (total > 0 && last >= total - 5) load() }
    }
}

@Composable
private fun MoreSpinner() {
    Box(Modifier.fillMaxWidth().padding(16.dp), Alignment.Center) { CircularProgressIndicator(Modifier.size(26.dp), strokeWidth = 2.5.dp) }
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
            onValueChange = vm::typeQuery,
            placeholder = { Text(tr("Songs, artists, albums")) },
            leadingIcon = { Icon(Icons.Default.Search, null) },
            singleLine = true,
            shape = RoundedCornerShape(28.dp),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { vm.search() }),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
        // With the SoundCloud plugin: where the search looks.
        if (vm.hasPlugin(com.rymplayer.data.Plugins.SOUNDCLOUD)) {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                androidx.compose.material3.FilterChip(!vm.searchSoundCloud, { vm.searchSoundCloud = false }, { Text("YouTube Music") })
                androidx.compose.material3.FilterChip(vm.searchSoundCloud, { vm.searchSoundCloud = true }, { Text("SoundCloud") })
            }
        }
        val links = listOfNotNull(
            "Spotify".takeIf { vm.hasPlugin(com.rymplayer.data.Plugins.SPOTIFY) },
            "SoundCloud".takeIf { vm.hasPlugin(com.rymplayer.data.Plugins.SOUNDCLOUD) },
        )
        if (links.isNotEmpty()) {
            Text("${tr("You can also paste a link from")} ${links.joinToString(" / ")}", color = Dim, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp))
        }
        // The account's search history (nothing typed), or suggestions for what's typed.
        LaunchedEffect(Unit) { vm.askSuggestions() }
        LazyColumn(Modifier.fillMaxWidth().weight(1f), contentPadding = PaddingValues(vertical = 6.dp)) {
            items(vm.suggestions, key = { (if (it.fromHistory) "h:" else "s:") + it.text }) { sug ->
                Row(
                    Modifier.fillMaxWidth().clickable { vm.searchFor(sug.text) }.padding(start = 20.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(if (sug.fromHistory) Icons.Default.History else Icons.Default.Search, null, tint = Dim)
                    Spacer(Modifier.width(16.dp))
                    Text(sug.text, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 16.sp)
                    if (sug.fromHistory) {
                        IconButton({ vm.forgetSearch(sug) }) { Icon(Icons.Default.Close, tr("Remove from search history"), tint = Dim) }
                    } else {
                        IconButton({ vm.typeQuery(sug.text) }) { Icon(Icons.Default.NorthWest, tr("Fill in"), tint = Dim) }
                    }
                }
            }
        }
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
            LibraryRow(tr("Your recap"), tr("Hours listened, top songs and artists"), Brush.linearGradient(listOf(Color(0xFFFF8A00), Color(0xFFE52E71))), Icons.Default.Insights) {
                vm.recapOpen = true
            }
        }
        item {
            LibraryRow(tr("Downloads"), tr("Saved on this phone, play without internet"), Brush.linearGradient(listOf(Color(0xFF19B6A5), Color(0xFF2878D0))), Icons.Default.Download) {
                vm.open(Screen.Songs(Screen.Kind.Downloads, "downloads", tr("Downloads")))
            }
        }
        item {
            // Every song on the phone (Android asks for "Music and audio" the first time).
            val ctx = androidx.compose.ui.platform.LocalContext.current
            val openDevice = { vm.open(Screen.Songs(Screen.Kind.Device, "device", tr("Songs on this phone"))) }
            val ask = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { ok ->
                if (ok) openDevice() else vm.status = tr("RyM needs \"Music and audio\" to show the phone's songs")
            }
            LibraryRow(tr("Songs on this phone"), tr("Your own music files"), Brush.linearGradient(listOf(Color(0xFF7B61FF), Color(0xFF00B4D8))), Icons.Default.PhoneAndroid) {
                if (com.rymplayer.data.LocalSongs.allowed(ctx)) openDevice() else ask.launch(com.rymplayer.data.LocalSongs.permission)
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
    val state = androidx.compose.foundation.lazy.rememberLazyListState()
    NearTheEnd(state) { vm.loadMore(page) }
    LazyColumn(state = state, contentPadding = pagePadding) {
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
                    page.kind == Screen.Kind.Results || page.kind == Screen.Kind.SoundCloud -> vm.playRadio(t)
                    else -> vm.playFrom(tracks, i)
                }
            }
            SongRow(vm, t, current == t.videoId && t.videoId.isNotEmpty(), onClick)
        }
        if (vm.loadingMore) item { MoreSpinner() }
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

/** "Your recap": hours listened (this month and in all), the most played songs, the top artists this month. */
@Composable
fun RecapPage(vm: AppViewModel) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val r = remember { com.rymplayer.data.Recent.recap(ctx) }
    LazyColumn(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).statusBarsPadding(), contentPadding = PaddingValues(bottom = 32.dp)) {
        item {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(4.dp)) {
                IconButton({ vm.recapOpen = false }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, null, tint = MaterialTheme.colorScheme.onBackground) }
                Text(tr("Your recap"), fontSize = 22.sp, fontWeight = FontWeight.Bold)
            }
        }
        item {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(36.dp)) {
                for ((n, what) in listOf(r.hoursMonth to tr("hours this month"), r.hoursTotal to tr("hours in all"))) {
                    Column {
                        Text("%.1f".format(n), fontSize = 30.sp, fontWeight = FontWeight.Bold)
                        Text(what, color = Dim)
                    }
                }
            }
        }
        item { Text(tr("Top songs"), fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 6.dp)) }
        if (r.topSongs.isEmpty()) item { Text(tr("Play some songs and they'll show here"), color = Dim, modifier = Modifier.padding(16.dp)) }
        items(r.topSongs) { (t, plays) -> SongRow(vm, t.copy(album = "$plays ${tr("plays")}"), false, { vm.playRadio(t) }) }
        item { Text(tr("Top artists this month"), fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 6.dp)) }
        items(r.topArtists) { (artist, hours) ->
            Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 10.dp)) {
                Text(artist, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                Text("${(hours * 60).toInt()} ${tr("minutes")}", color = Dim)
            }
        }
        // RaM points' own favorites (songs and artists by points).
        item {
            val ctx = androidx.compose.ui.platform.LocalContext.current
            val fav by androidx.compose.runtime.produceState<Pair<List<Pair<Track, Float>>, List<Pair<String, Float>>>?>(null) {
                value = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                    com.rymplayer.data.RamPoints.favorites(ctx, com.rymplayer.data.Recent.tracks(ctx), 8)
                }
            }
            fav?.let { (songs, artists) ->
                Column {
                    if (songs.isNotEmpty()) {
                        Text(tr("Your favorite songs (RaM points)"), fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 6.dp))
                        songs.forEach { (t, p) -> SongRow(vm, t.copy(album = "${p.toInt()} ${tr("points")}"), false, { vm.playRadio(t) }) }
                    }
                    if (artists.isNotEmpty()) {
                        Text(tr("Your favorite artists (RaM points)"), fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 6.dp))
                        artists.forEach { (a, p) ->
                            Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 10.dp)) {
                                Text(a, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                                Text("${p.toInt()} ${tr("points")}", color = Dim)
                            }
                        }
                    }
                }
            }
        }
    }
}
