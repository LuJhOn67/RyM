package com.rymplayer.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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

@Composable
fun HomePage(vm: AppViewModel) {
    if (vm.home.isEmpty()) {
        if (vm.homeLoading) Loading() else Box(Modifier.fillMaxSize(), Alignment.Center) {
            TextButton(vm::loadHome) { Text("Nothing here yet. Tap to retry") }
        }
        return
    }
    val sections = remember(vm.home) { vm.home.groupBy { it.album }.toList() }
    LazyColumn(contentPadding = pagePadding) {
        item { Title("For you", Modifier.padding(top = 8.dp)) }
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

@Composable
private fun HomeCard(vm: AppViewModel, t: Track) {
    Column(Modifier.width(150.dp).clip(RoundedCornerShape(10.dp)).clickable { vm.openLink(t) }) {
        Cover(t.thumb, 150.dp, 360, 10.dp)
        Spacer(Modifier.height(8.dp))
        Text(t.title, maxLines = 2, overflow = TextOverflow.Ellipsis, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
        Text(t.artist, maxLines = 1, overflow = TextOverflow.Ellipsis, color = Dim, fontSize = 12.sp)
    }
}

@Composable
fun SearchPage(vm: AppViewModel) {
    Column(Modifier.fillMaxSize()) {
        Title("Search")
        OutlinedTextField(
            value = vm.query,
            onValueChange = { vm.query = it },
            placeholder = { Text("Songs, artists, albums") },
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
    LazyColumn(contentPadding = pagePadding) {
        item { Title("Library") }
        item {
            LibraryRow("Downloads", "Saved on this phone, play without internet", Brush.linearGradient(listOf(Color(0xFF19B6A5), Color(0xFF2878D0))), Icons.Default.Download) {
                vm.open(Screen.Songs(Screen.Kind.Downloads, "downloads", "Downloads"))
            }
        }
        if (vm.api.signedIn) {
            item {
                LibraryRow("Liked Music", "Auto playlist", Brush.linearGradient(listOf(Color(0xFFFF2D55), Color(0xFF8E44FF))), Icons.Default.Favorite) {
                    vm.open(Screen.Songs(Screen.Kind.Playlist, LIKED_ID, "Liked Music"))
                }
            }
            item {
                LibraryRow("History", "What you played", Brush.linearGradient(listOf(Color(0xFF2D9CFF), Color(0xFF00C2A8))), Icons.Default.History) {
                    vm.open(Screen.Songs(Screen.Kind.History, "history", "History"))
                }
            }
            item {
                LibraryRow("New playlist", "", Brush.linearGradient(listOf(Color(0xFF3A3A42), Color(0xFF3A3A42))), Icons.AutoMirrored.Filled.PlaylistAdd) { naming = true }
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
                        Text("Playlist", color = Dim, fontSize = 13.sp)
                    }
                }
            }
        } else {
            item { Text("Sign in to see your playlists", color = Dim, modifier = Modifier.padding(16.dp)) }
        }
        // Account: sign out, or sign in when browsing without an account (both show the sign-in screen).
        item {
            if (vm.api.signedIn) {
                LibraryRow("Sign out", "Use another account or none", Brush.linearGradient(listOf(Color(0xFF3A3A42), Color(0xFF3A3A42))), Icons.AutoMirrored.Filled.Logout) { vm.signOut() }
            } else {
                LibraryRow("Sign in with Google", "Your playlists, likes and history", Brush.linearGradient(listOf(Color(0xFFFF2D55), Color(0xFF8E44FF))), Icons.AutoMirrored.Filled.Login) { vm.signOut() }
            }
        }
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
                Text(page.title, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text("${songs.size} songs", color = Dim, fontSize = 13.sp)
                Spacer(Modifier.height(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button({ vm.playFrom(tracks, tracks.indexOfFirst { it.link == null }.coerceAtLeast(0)) }, enabled = songs.isNotEmpty()) {
                        Icon(Icons.Default.PlayArrow, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Play")
                    }
                    OutlinedButton({ vm.playFrom(songs.shuffled(), 0) }, enabled = songs.isNotEmpty(), colors = ButtonDefaults.outlinedButtonColors(contentColor = Color.White)) {
                        Icon(Icons.Default.Shuffle, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Shuffle")
                    }
                }
            }
        }
        if (tracks.isEmpty()) item { Text(vm.status.ifEmpty { "Nothing here" }, color = Dim, modifier = Modifier.padding(16.dp)) }
        itemsIndexed(tracks) { i, t ->
            val onClick = { if (t.link != null) vm.openLink(t) else vm.playFrom(tracks, i) }
            SongRow(vm, t, current == t.videoId && t.videoId.isNotEmpty(), onClick)
        }
    }
}

@Composable
fun SectionLabel(text: String) = Text(text.uppercase(), color = MaterialTheme.colorScheme.primary, fontSize = 12.sp, fontWeight = FontWeight.Bold)
