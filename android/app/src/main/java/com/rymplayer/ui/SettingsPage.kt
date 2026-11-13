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
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Translate
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.NewReleases
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.AutoAwesome
import kotlin.math.roundToInt
import androidx.compose.material.icons.filled.Equalizer
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.ContentCut
import androidx.compose.material.icons.filled.SwapHoriz
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
import androidx.compose.material.icons.filled.Delete
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

private val Page: Color @Composable get() = MaterialTheme.colorScheme.background
private val CardColor: Color
    @androidx.compose.runtime.Composable get() = if (themeBg == null) Color(0xFF17171B) else MaterialTheme.colorScheme.surfaceVariant

/** A page's top bar: back arrow and title. */
@Composable
private fun TopBar(title: String, onBack: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, null, tint = MaterialTheme.colorScheme.onBackground) }
        Text(title, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onBackground)
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
        shape = themeShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = CardColor),
    ) { Column { content() } }
}

private val itemColors @Composable get() = ListItemDefaults.colors(
    containerColor = Color.Transparent,
    headlineColor = MaterialTheme.colorScheme.onBackground,
    // A color theme's own second color (the grey can't be read on the light ones).
    supportingColor = if (themeBg == null) Dim else MaterialTheme.colorScheme.onSurfaceVariant,
    leadingIconColor = MaterialTheme.colorScheme.primary,
    trailingIconColor = if (themeBg == null) Dim else MaterialTheme.colorScheme.onSurfaceVariant,
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
/** A calendar to pick the reset day; "Clear" takes the day away. */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun ResetDatePicker(current: String, onDismiss: () -> Unit, onPick: (String) -> Unit) {
    val start = runCatching { java.time.LocalDate.parse(current) }.getOrNull() ?: java.time.LocalDate.now().plusDays(7)
    val state = androidx.compose.material3.rememberDatePickerState(initialSelectedDateMillis = start.atStartOfDay(java.time.ZoneOffset.UTC).toInstant().toEpochMilli())
    androidx.compose.material3.DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            androidx.compose.material3.TextButton({
                val ms = state.selectedDateMillis
                if (ms != null) onPick(java.time.Instant.ofEpochMilli(ms).atZone(java.time.ZoneOffset.UTC).toLocalDate().toString()) else onDismiss()
            }) { androidx.compose.material3.Text(tr("OK")) }
        },
        dismissButton = { androidx.compose.material3.TextButton({ onPick("") }) { androidx.compose.material3.Text(tr("Clear")) } },
    ) { androidx.compose.material3.DatePicker(state) }
}

@Composable
private fun TapRow(icon: ImageVector, title: String, about: String, value: String = "", onPrev: (() -> Unit)? = null, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(title, fontWeight = FontWeight.SemiBold) },
        supportingContent = if (about.isNotEmpty()) ({ Text(about, fontSize = 13.sp) }) else null,
        leadingContent = { Icon(icon, null) },
        trailingContent = when {
            value.isEmpty() -> null
            // A setting with steps: ‹ value › like the PC (tapping the row still moves forward).
            onPrev != null -> ({
                Row(verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onPrev, contentPadding = PaddingValues(0.dp), modifier = Modifier.size(36.dp)) { Text("‹", fontSize = 22.sp, fontWeight = FontWeight.Bold) }
                    Text(value, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
                    TextButton(onClick, contentPadding = PaddingValues(0.dp), modifier = Modifier.size(36.dp)) { Text("›", fontSize = 22.sp, fontWeight = FontWeight.Bold) }
                }
            })
            else -> ({ Text(value, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold) })
        },
        colors = itemColors,
        modifier = Modifier.clickable(onClick = onClick),
    )
}

@Composable
private fun Divider() = HorizontalDivider(Modifier.padding(start = 56.dp), color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.06f))

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
                TapRow(
                    Icons.Default.SettingsEthernet, tr("Audio source"),
                    tr("InnerTube + yt-dlp: fast, and yt-dlp takes over when YouTube refuses. yt-dlp: slower start, Premium gets 256 kbps. InnerTube: straight from YouTube only · tap to change"),
                    listOf("InnerTube + yt-dlp", "yt-dlp", "InnerTube")[vm.audioSource.coerceIn(0, 2)],
                ) { vm.cycleSource() }
                Divider()
                SwitchRow(Icons.Default.Download, tr("Best download quality"), tr("yt-dlp with your account, 256 kbps for Premium, slower"), vm.downloadBest) { vm.toggleDownloadQuality() }
                Divider()
                TapRow(Icons.Default.SettingsEthernet, tr("Download connections"), tr("How many at once (1-16); more use more of your line · tap to change"), "${vm.connections}") { vm.nextConnections() }
                Divider()
                TapRow(
                    Icons.Default.ViewModule, tr("Download piece size"),
                    if (vm.pieceMb == 0) tr("No fixed size: the song is split evenly over the connections") else "${tr("Each connection fetches")} ${vm.pieceMb} MB ${tr("at a time · tap to change")}",
                    if (vm.pieceMb == 0) tr("Open") else "${vm.pieceMb} MB",
                ) { vm.nextPieceSize() }
                Divider()
                SwitchRow(Icons.Default.Tune, tr("Skip non-music parts"), tr("SponsorBlock: a music video's intro, talk and outro are skipped"), vm.player.skipNonMusic) { vm.player.toggleSkipNonMusic() }
                if (com.rymplayer.playback.Vlc.installed(ctx)) {
                    Divider()
                    SwitchRow(Icons.Default.PlayArrow, tr("Play songs in VLC"), tr("Each song opens in VLC instead of the built-in player"), vm.playInVlc) { vm.togglePlayInVlc() }
                }
            }
            Section(tr("Lyrics")) {
                SwitchRow(Icons.Default.Translate, tr("Translate Japanese lyrics"), tr("English in small letters under each line: the lyrics site's own translation, else a machine translation"), vm.translateLyrics) { vm.toggleTranslateLyrics() }
                SwitchRow(Icons.Default.Translate, tr("Romaji"), tr("Japanese lines in Latin letters under them"), vm.romajiLyrics) { vm.toggleRomajiLyrics() }
            }
            Section(tr("Devices")) {
                SwitchRow(Icons.Default.Devices, "Seamless Music", tr("Music moves between this phone and RyM on your PC (same Wi-Fi and Google account): to the PC when it's idle, and here when this phone is idle"), vm.crossEars) { vm.toggleCrossEars() }
            }
            Section(tr("Suggestions (RaM points)")) {
                TapRow(Icons.Default.Tune, tr("Familiar or new"), tr("Up next: songs you like, or songs you haven't heard"), tr(com.rymplayer.data.RamPoints.DISCOVERY_NAMES[vm.discovery]), onPrev = { vm.cycleDiscovery(-1) }) { vm.cycleDiscovery() }
                TapRow(Icons.Default.Tune, tr("Only this kind"), tr("For you shows only this kind (a Muharram, say) · tap to change"), vm.onlyKind?.let { tr(it.title) } ?: tr("Every kind heard"), onPrev = { vm.cycleOnlyKind(-1) }) { vm.cycleOnlyKind() }
                androidx.compose.runtime.LaunchedEffect(Unit) { vm.refreshRamSummary() }
                val sum = vm.ramSummary
                val about = if (sum.tracked == 0) tr("Nothing learned yet") else
                    "${sum.tracked} ${tr("songs with points")}" + if (sum.topArtists.isEmpty()) "" else " · ${tr("Top artists")}: ${sum.topArtists.joinToString(", ")}"
                var askReset by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
                TapRow(Icons.Default.AutoAwesome, tr("Reset all points"), about, tr("Reset all")) { askReset = true }
                TapRow(Icons.Default.Tune, tr("Reset negative points only"), tr("Skipped songs come back; your favorites stay"), tr("Reset")) { vm.resetNegativePoints() }
                var pickDate by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
                TapRow(Icons.Default.Tune, tr("Reset on a date"), tr("On that day all points go back to 0 · tap to pick"), vm.ramResetDate.ifEmpty { tr("None") }) { pickDate = true }
                if (pickDate) ResetDatePicker(vm.ramResetDate, { pickDate = false }) { vm.chooseResetDate(it); pickDate = false }
                var askWeekly by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
                SwitchRow(Icons.Default.Tune, tr("Weekly reset"), tr("Every 7 days all points go back to 0 (off unless you turn it on)"), vm.ramWeeklyReset) {
                    if (vm.ramWeeklyReset) vm.chooseWeeklyReset(false) else askWeekly = true
                }
                if (askWeekly) {
                    androidx.compose.material3.AlertDialog(
                        onDismissRequest = { askWeekly = false },
                        title = { androidx.compose.material3.Text(tr("Turn on the weekly reset?")) },
                        text = { androidx.compose.material3.Text(tr("Every 7 days, all the points RyM learned from what you play go back to 0, and the suggestions start learning again from scratch. Your likes, playlists and history stay. You can still reset by hand or pick a date instead.")) },
                        confirmButton = { androidx.compose.material3.TextButton({ askWeekly = false; vm.chooseWeeklyReset(true) }) { androidx.compose.material3.Text(tr("Turn on")) } },
                        dismissButton = { androidx.compose.material3.TextButton({ askWeekly = false }) { androidx.compose.material3.Text(tr("Cancel")) } },
                    )
                }
                if (askReset) {
                    androidx.compose.material3.AlertDialog(
                        onDismissRequest = { askReset = false },
                        title = { androidx.compose.material3.Text(tr("Reset all points")) },
                        text = { androidx.compose.material3.Text(tr("Every song and artist goes back to 0. Kinds and hidden songs stay.")) },
                        confirmButton = { androidx.compose.material3.TextButton({ askReset = false; vm.resetAllPoints() }) { androidx.compose.material3.Text(tr("Reset all")) } },
                        dismissButton = { androidx.compose.material3.TextButton({ askReset = false }) { androidx.compose.material3.Text(tr("Cancel")) } },
                    )
                }
            }
            Section(tr("Network & Parsers")) {
                TapRow(Icons.Default.Search, tr("Song finder"), tr("Search, radio and mixes; the built-in one is the rollback if a YouTube change breaks ytmusicapi") + " · " + tr("tap to change"), tr(vm.musicEngine.label), onPrev = { vm.cycleEngine(-1) }) { vm.cycleEngine() }
                Divider()
                val why = com.rymplayer.data.YtMusicApi.lastFailure
                TapRow(Icons.Default.Sync, tr("Update parsers"), vm.parsersStatus.ifEmpty { why?.let { "${tr("Last fallback")}: $it" } ?: tr("yt-dlp and ytmusicapi to their newest, saved answers cleared") }, tr("Update")) { vm.updateParsers() }
            }
            Section("yt-dlp") {
                SwitchRow(Icons.Default.NewReleases, tr("Nightly builds"), tr("The newest yt-dlp every day (fixes come sooner)"), vm.ytdlpNightly) { vm.toggleYtdlpNightly() }
                Divider()
                TapRow(Icons.Default.SystemUpdate, tr("Update yt-dlp"), vm.ytdlpStatus.ifEmpty { tr("yt-dlp finds songs YouTube refuses, and downloads") }, tr("Update")) { vm.updateYtDlp() }
            }
            Section(tr("Long talks")) {
                // Over 20 minutes: they go on from where they stopped, at their own speed.
                TapRow(Icons.Default.SystemUpdate, tr("Long talks' speed"), tr("Talks over 20 minutes go on from where they stopped, at this speed; songs keep theirs"), "${vm.sound.talkSpeed}×") {
                    val all = com.rymplayer.data.TalkPlace.SPEEDS
                    val at = all.indexOfFirst { kotlin.math.abs(it - vm.sound.talkSpeed) < 0.01f }
                    vm.changeSound(vm.sound.copy(talkSpeed = all[(at + 1) % all.size]))
                }
            }
            Section(tr("Storage")) {
                val ctx = androidx.compose.ui.platform.LocalContext.current
                val streams = com.rymplayer.playback.Streams
                var cacheBytes by androidx.compose.runtime.remember { androidx.compose.runtime.mutableLongStateOf(streams.cacheSize(ctx)) }
                TapRow(Icons.Default.Delete, tr("Clear cache"), "${tr("Played songs kept on this phone")}: ${streams.sizeText(cacheBytes)}", tr("Clear")) {
                    val freed = streams.clearCache(ctx, vm.player.current?.videoId)
                    cacheBytes = streams.cacheSize(ctx)
                    vm.player.message = "${tr("Cache cleared")}: ${streams.sizeText(freed)}"
                }
            }
            Section(tr("Theme")) {
                TapRow(Icons.Default.Palette, tr("Theme"), tr("Classic, RyM YTM (pure black, quick picks and big shelves) or RyM Noir (grey tiles, green, a floating player) · tap to switch"), vm.themeName) { vm.toggleTheme() }
                TapRow(Icons.Default.Palette, tr("Colors"), tr("12 looks, the same as on the PC · tap to change"), vm.palette.ifEmpty { tr("The layout's own") }, onPrev = { vm.cyclePalette(-1) }) { vm.cyclePalette() }
                SwitchRow(Icons.Default.AutoAwesome, tr("Liquid Aurora"), tr("The cover's colors move behind the player with the bass"), vm.aurora) { vm.toggleAurora() }
                SwitchRow(Icons.Default.GraphicEq, tr("Vinyl record"), tr("The cover turns like a record: faster with Nightcore, slower with Slowed"), vm.vinyl) { vm.toggleVinyl() }
                TapRow(Icons.Default.Download, tr("Import colors"), tr("Copy a theme's JSON (from the PC's export), then tap here"), tr("Import")) { vm.importTheme() }
                TapRow(Icons.Default.SwapHoriz, tr("Export colors"), tr("Copies these colors as JSON"), tr("Export")) { vm.exportTheme() }
                TapRow(Icons.Default.Tune, tr("Animation frame rate"), tr("Animations stop on battery saver or under 20% · tap to change"), if (vm.fpsCap == 0) tr("As fast as the screen") else "${vm.fpsCap} FPS", onPrev = { vm.cycleFps(-1) }) { vm.cycleFps() }
                SwitchRow(Icons.Default.Tune, tr("Performance monitor"), tr("Frames a second, frame time and memory in a corner"), vm.perfBadge) { vm.togglePerfBadge() }
                SwitchRow(Icons.Default.GraphicEq, tr("Prefer lossless (FLAC)"), tr("Looks for the song as FLAC on the Internet Archive first (free and legal); YouTube when it isn't there"), vm.preferLossless) { vm.togglePreferLossless() }
            }
            Section(tr("Sound")) {
                val snd = vm.sound
                TapRow(Icons.Default.SwapHoriz, tr("Crossfade"), tr("The end of a song fades out as the next fades in · tap to change"), if (snd.crossfade == 0) tr("Off") else "${snd.crossfade} s", onPrev = { vm.changeSound(snd.copy(crossfade = Math.floorMod(snd.crossfade - 1, 11))) }) {
                    vm.changeSound(snd.copy(crossfade = (snd.crossfade + 1) % 11))
                }
                Divider()
                SwitchRow(Icons.Default.AutoAwesome, tr("Smart Automix"), tr("Songs blend like a DJ mix: on the fade-out, on the beat, without the basses clashing"), snd.automix) { vm.changeSound(snd.copy(automix = !snd.automix)) }
                SwitchRow(Icons.Default.MicOff, tr("Lyrics on the lock screen"), tr("The sung line under the song's name on the lock screen and in the notification"), vm.lockLyrics) { vm.toggleLockLyrics() }
                SwitchRow(Icons.Default.MicOff, tr("Remove vocals (karaoke)"), tr("The voice taken out (it sits in the middle of the stereo); the music stays"), snd.vocalsOff) { vm.changeSound(snd.copy(vocalsOff = !snd.vocalsOff)) }
                Divider()
                SwitchRow(Icons.Default.ContentCut, tr("Skip silence"), tr("Silent parts at the start and end of songs are skipped"), snd.skipSilence) { vm.changeSound(snd.copy(skipSilence = !snd.skipSilence)) }
                Divider()
                SwitchRow(Icons.Default.GraphicEq, tr("Even loudness"), tr("Every song at about the same loudness, without sudden loud parts"), snd.normalize) { vm.changeSound(snd.copy(normalize = !snd.normalize)) }
                Divider()
                TapRow(Icons.Default.Equalizer, tr("Equalizer"), tr("Flat, Bass Boost, Vocal, Rock, Pop, or your own (move a band)"), tr(com.rymplayer.playback.Sound.PRESETS[snd.eqPreset].first), onPrev = { vm.changeSound(snd.copy(eqPreset = Math.floorMod(snd.eqPreset - 1, com.rymplayer.playback.Sound.PRESETS.size))) }) {
                    vm.changeSound(snd.copy(eqPreset = (snd.eqPreset + 1) % com.rymplayer.playback.Sound.PRESETS.size))
                }
                Column(Modifier.padding(horizontal = 20.dp, vertical = 4.dp)) {
                    snd.gains.forEachIndexed { i, g ->
                        val hz = com.rymplayer.playback.Sound.BANDS[i]
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(if (hz >= 1000) "${(hz / 1000).toString().removeSuffix(".0")}k" else "${hz.toInt()}", color = Dim, fontSize = 12.sp, modifier = Modifier.width(44.dp))
                            androidx.compose.material3.Slider(g, { v ->
                                // Moving a band makes the preset "Custom".
                                val custom = snd.gains.toMutableList().also { it[i] = (v * 2).roundToInt() / 2f }
                                vm.changeSound(snd.copy(eqPreset = com.rymplayer.playback.Sound.PRESETS.lastIndex, eqCustom = custom))
                            }, valueRange = -12f..12f, modifier = Modifier.weight(1f))
                            Text("${if (g > 0) "+" else ""}${g.toInt()}", color = Dim, fontSize = 12.sp, modifier = Modifier.width(32.dp))
                        }
                    }
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
                // Downloading in the app.
                vm.updateProgress?.let { p ->
                    Spacer(Modifier.padding(top = 10.dp))
                    Text("${tr("Downloading the update...")} ${(p * 100).toInt()}%", color = Dim, fontSize = 13.sp)
                    androidx.compose.material3.LinearProgressIndicator(progress = { p }, modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
                }
            }
        },
        confirmButton = {
            Button({
                // The APK downloads here and Android's installer opens; without one, the release page.
                if (r.apk != null) vm.installUpdate(r) else {
                    runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(r.page))) }
                    vm.update = null
                }
            }, enabled = vm.updateProgress == null) { Text(if (r.apk != null) tr("Update now") else tr("Download")) }
        },
        dismissButton = { TextButton({ vm.update = null }, enabled = vm.updateProgress == null) { Text(tr("Later")) } },
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
