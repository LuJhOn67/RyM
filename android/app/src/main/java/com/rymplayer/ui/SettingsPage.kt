package com.rymplayer.ui

import android.content.Intent
import android.net.Uri
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Login
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Extension
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.NewReleases
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Science
import androidx.compose.material.icons.filled.SettingsEthernet
import androidx.compose.material.icons.filled.Storefront
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.ViewModule
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rymplayer.AppViewModel
import com.rymplayer.I18n
import com.rymplayer.data.Plugins
import com.rymplayer.data.Updates
import com.rymplayer.tr

private val Page = Color(0xFF0B0B0D)
private val CardColor = Color(0xFF17171B)

/** A page's top bar: back arrow and title. */
@Composable
private fun TopBar(title: String, onBack: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, null, tint = Color.White) }
        Text(title, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = Color.White)
    }
}

/** A titled group of settings on its own card. */
@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Text(
        title.uppercase(), color = MaterialTheme.colorScheme.primary, fontSize = 12.sp, fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(start = 28.dp, top = 22.dp, bottom = 8.dp),
    )
    Card(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = CardColor),
    ) { Column { content() } }
}

private val itemColors @Composable get() = ListItemDefaults.colors(
    containerColor = Color.Transparent,
    headlineColor = Color.White,
    supportingColor = Dim,
    leadingIconColor = MaterialTheme.colorScheme.primary,
    trailingIconColor = Dim,
)

/** One setting: icon, title, a line about it, and an on/off switch. */
@Composable
private fun SwitchRow(icon: ImageVector, title: String, about: String, on: Boolean, onChange: () -> Unit) {
    ListItem(
        headlineContent = { Text(title, fontWeight = FontWeight.SemiBold) },
        supportingContent = { Text(about, fontSize = 13.sp) },
        leadingContent = { Icon(icon, null) },
        trailingContent = { Switch(on, { onChange() }) },
        colors = itemColors,
        modifier = Modifier.clickable(onClick = onChange),
    )
}

/** One setting that shows its value and changes (or opens something) on tap. */
@Composable
private fun TapRow(icon: ImageVector, title: String, about: String, value: String = "", onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(title, fontWeight = FontWeight.SemiBold) },
        supportingContent = if (about.isNotEmpty()) ({ Text(about, fontSize = 13.sp) }) else null,
        leadingContent = { Icon(icon, null) },
        trailingContent = if (value.isNotEmpty()) ({ Text(value, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold) }) else null,
        colors = itemColors,
        modifier = Modifier.clickable(onClick = onClick),
    )
}

@Composable
private fun Divider() = HorizontalDivider(Modifier.padding(start = 56.dp), color = Color.White.copy(alpha = 0.06f))

/** Settings, in sections: general, playback and quality, extensions, updates, about, account. */
@Composable
fun SettingsPage(vm: AppViewModel) {
    val ctx = LocalContext.current
    fun open(url: String) = runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) }
    Column(Modifier.fillMaxSize().background(Page).statusBarsPadding().navigationBarsPadding()) {
        TopBar(tr("Settings")) { vm.settingsOpen = false }
        Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
            Section(tr("General")) {
                TapRow(Icons.Default.Language, tr("Language"), tr("The words in the app"), if (I18n.arabic) "العربية" else "English") { vm.toggleArabic() }
            }
            Section(tr("Playback and quality")) {
                SwitchRow(Icons.Default.Download, tr("Best download quality"), tr("yt-dlp with your account, 256 kbps for Premium, slower"), vm.downloadBest) { vm.toggleDownloadQuality() }
                Divider()
                TapRow(Icons.Default.SettingsEthernet, tr("Download connections"), tr("How many at once (1-16); more use more of your line · tap to change"), "${vm.connections}") { vm.nextConnections() }
                Divider()
                TapRow(
                    Icons.Default.ViewModule, tr("Download piece size"),
                    if (vm.pieceMb == 0) tr("No fixed size: the song is split evenly over the connections") else "${tr("Each connection fetches")} ${vm.pieceMb} MB ${tr("at a time · tap to change")}",
                    if (vm.pieceMb == 0) tr("Open") else "${vm.pieceMb} MB",
                ) { vm.nextPieceSize() }
                if (com.rymplayer.playback.Vlc.installed(ctx)) {
                    Divider()
                    SwitchRow(Icons.Default.PlayArrow, tr("Play songs in VLC"), tr("Each song opens in VLC instead of the built-in player"), vm.playInVlc) { vm.togglePlayInVlc() }
                }
            }
            Section(tr("Extensions")) {
                TapRow(Icons.Default.Storefront, tr("Plugin store"), tr("Add features to RyM; without them it stays a music player"), if (vm.plugins.isEmpty()) "" else "${vm.plugins.size}") { vm.storeOpen = true }
                for ((id, version) in vm.plugins) {
                    val name = vm.store?.firstOrNull { it.id == id }?.name ?: id
                    Divider()
                    TapRow(Icons.Default.Extension, name, "${tr("Installed")} $version") { vm.storeOpen = true }
                }
                if (vm.hasVideo) {
                    Divider()
                    TapRow(Icons.Default.Link, tr("Download from a link"), tr("Paste a link from YouTube or any site")) { vm.linkOpen = true }
                }
            }
            Section(tr("Updates and beta")) {
                SwitchRow(Icons.Default.SystemUpdate, tr("Check for updates automatically"), tr("Every 12 hours, and when the app opens"), vm.autoUpdates) { vm.toggleAutoUpdates() }
                Divider()
                SwitchRow(Icons.Default.Science, tr("Beta updates"), tr("Test versions (pre-releases) count as updates too"), vm.betaUpdates) { vm.toggleBetaUpdates() }
                Divider()
                ListItem(
                    headlineContent = { Text(tr("Check for updates"), fontWeight = FontWeight.SemiBold) },
                    supportingContent = { Text("${tr("You have")} ${vm.versionName}", fontSize = 13.sp) },
                    leadingContent = { Icon(Icons.Default.NewReleases, null) },
                    trailingContent = {
                        if (vm.checking) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                        else OutlinedButton({ vm.checkUpdates(true) }) { Text(tr("Check")) }
                    },
                    colors = itemColors,
                )
            }
            Section(tr("About")) {
                TapRow(Icons.Default.Info, "RyM ${vm.versionName}", tr("Releases and patch notes on GitHub")) { open("https://github.com/${Updates.REPO}/releases") }
                Divider()
                TapRow(Icons.Default.Extension, tr("Plugins on GitHub"), Plugins.PAGE.removePrefix("https://")) { open(Plugins.PAGE) }
            }
            Section(tr("Account")) {
                if (vm.api.signedIn) {
                    TapRow(Icons.AutoMirrored.Filled.Logout, tr("Sign out"), tr("Use another account or none")) { vm.settingsOpen = false; vm.signOut() }
                } else {
                    TapRow(Icons.AutoMirrored.Filled.Login, tr("Sign in with Google"), tr("Your playlists, likes and history")) { vm.settingsOpen = false; vm.signOut() }
                }
            }
        }
    }
}

/** The plugin store: every plugin with Install, Update or Remove. */
@Composable
fun PluginStorePage(vm: AppViewModel) {
    LaunchedEffect(Unit) { if (vm.store == null) vm.loadStore() }
    Column(Modifier.fillMaxSize().background(Page).statusBarsPadding().navigationBarsPadding()) {
        TopBar(tr("Plugin store")) { vm.storeOpen = false }
        Text(tr("Add features to RyM; without them it stays a music player"), color = Dim, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 20.dp))
        val list = vm.store
        if (list == null) {
            Box(Modifier.fillMaxSize(), Alignment.Center) { CircularProgressIndicator() }
            return@Column
        }
        LazyColumn(contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            items(list, key = { it.id }) { p ->
                val installed = vm.plugins[p.id]
                val newer = installed != null && Updates.newer(p.version, installed)
                Card(shape = RoundedCornerShape(18.dp), colors = CardDefaults.cardColors(containerColor = CardColor)) {
                    Column(Modifier.padding(18.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.primary.copy(alpha = 0.18f), RoundedCornerShape(12.dp)), Alignment.Center) {
                                Icon(Icons.Default.Extension, null, tint = MaterialTheme.colorScheme.primary)
                            }
                            Spacer(Modifier.width(14.dp))
                            Column(Modifier.weight(1f)) {
                                Text(p.name, fontWeight = FontWeight.Bold, color = Color.White)
                                Text(if (installed != null) "${tr("Installed")} $installed" else "${tr("Version")} ${p.version}", color = Dim, fontSize = 12.sp)
                            }
                        }
                        Spacer(Modifier.padding(top = 10.dp))
                        Text(p.description, color = Color.White.copy(alpha = 0.85f), fontSize = 14.sp)
                        Spacer(Modifier.padding(top = 12.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            when {
                                installed == null -> Button({ vm.setPlugin(p, true) }) { Text(tr("Install")) }
                                newer -> {
                                    Button({ vm.setPlugin(p, true) }) { Text(tr("Update")) }
                                    OutlinedButton({ vm.setPlugin(p, false) }) { Text(tr("Remove")) }
                                }
                                else -> OutlinedButton({ vm.setPlugin(p, false) }) { Text(tr("Remove")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** A newer RyM: its version, patch notes and a download button. */
@Composable
fun UpdateDialog(vm: AppViewModel, r: Updates.Release) {
    val ctx = LocalContext.current
    AlertDialog(
        onDismissRequest = { vm.update = null },
        icon = { Icon(Icons.Default.NewReleases, null) },
        title = { Text("${if (r.beta) tr("New beta") else tr("New update")}: RyM ${r.version}") },
        text = {
            Column {
                Text("${tr("You have")} ${vm.versionName}", color = Dim, fontSize = 13.sp)
                Spacer(Modifier.padding(top = 8.dp))
                Text(tr("What's new"), fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.primary)
                Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()).padding(top = 6.dp)) {
                    Text(r.plainNotes, fontSize = 14.sp)
                }
            }
        },
        confirmButton = {
            Button({
                runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(r.apk ?: r.page))) }
                vm.update = null
            }) { Text(tr("Download")) }
        },
        dismissButton = { TextButton({ vm.update = null }) { Text(tr("Later")) } },
    )
}

/** Video plugin: download a link from YouTube or any site, as a video or just the sound. */
@Composable
fun LinkDialog(vm: AppViewModel) {
    var url by remember { mutableStateOf("") }
    var asVideo by remember { mutableStateOf(true) }
    AlertDialog(
        onDismissRequest = { vm.linkOpen = false },
        icon = { Icon(Icons.Default.Link, null) },
        title = { Text(tr("Download from a link")) },
        text = {
            Column {
                OutlinedTextField(url, { url = it }, placeholder = { Text(tr("Paste a link from YouTube or any site")) }, singleLine = true)
                Row(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(asVideo, { asVideo = true }, label = { Text(tr("Video")) })
                    FilterChip(!asVideo, { asVideo = false }, label = { Text(tr("Audio only")) })
                }
            }
        },
        confirmButton = {
            Button({ if (url.isNotBlank()) { vm.downloadLink(url.trim(), asVideo); vm.linkOpen = false } }) { Text(tr("Download")) }
        },
        dismissButton = { TextButton({ vm.linkOpen = false }) { Text(tr("Cancel")) } },
    )
}
