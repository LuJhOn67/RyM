package com.rymplayer

import com.rymplayer.ui.takesAllTouches
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.ui.unit.dp
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.viewModels
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rymplayer.ui.FullPlayer
import com.rymplayer.ui.HomePage
import com.rymplayer.ui.LibraryPage
import com.rymplayer.ui.Loading
import com.rymplayer.ui.LoginScreen
import com.rymplayer.ui.MiniPlayer
import com.rymplayer.ui.RymTheme
import com.rymplayer.ui.SearchPage
import com.rymplayer.ui.SongsPage
import com.rymplayer.ui.coverColor

class MainActivity : ComponentActivity() {
    private val vm: AppViewModel by viewModels()

    // Liked songs stay in step with the PC while the app is on screen.
    override fun onStart() {
        super.onStart()
        vm.onForeground()
    }

    override fun onStop() {
        super.onStop()
        vm.onBackground()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        com.rymplayer.data.CrashReport.watch(this)
        // Edge to edge with see-through bars: the pages and the player's colors go under the
        // status and navigation bars (no grey strip behind the buttons).
        enableEdgeToEdge(
            statusBarStyle = androidx.activity.SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = androidx.activity.SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) window.isNavigationBarContrastEnforced = false
        setContent {
            val model: AppViewModel = viewModel()
            RymTheme(model.ytmTheme, model.colors, model.noirTheme) {
                androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.fillMaxSize()) {
                    RymApp(model)
                    // Rain and ash over the app (Lo-Fi Rain, Remorse), landing on the mini player.
                    model.colors?.let { c ->
                        val (bass, _, _) = com.rymplayer.ui.rememberLevels(model.player.playing && model.player.visible && (c.style == "rain" || c.style == "ash"))
                        com.rymplayer.ui.FallingParticles(c.style, androidx.compose.ui.graphics.Color(com.rymplayer.data.Themes.argb(if (c.style == "ash") c.text else c.accent2) ?: -1), model.player.playing && model.player.visible, bass)
                    }
                    if (model.perfBadge) {
                        com.rymplayer.ui.PerfBadge(
                            androidx.compose.ui.Modifier.align(androidx.compose.ui.Alignment.TopEnd)
                                .then(androidx.compose.ui.Modifier.statusBarsPadding()).then(androidx.compose.ui.Modifier.padding(8.dp)),
                        )
                    }
                }
            }
        }
        showUpdateFrom(intent)
        // Not again when the screen turns (the same intent comes back with the saved state).
        if (savedInstanceState == null) openFrom(intent)
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        showUpdateFrom(intent)
        openFrom(intent)
    }

    /** The notification (the playing song), a YouTube Music link, or something shared to RyM. */
    private fun openFrom(intent: android.content.Intent?) {
        when {
            intent == null -> {}
            intent.getBooleanExtra(OPEN_PLAYER, false) -> vm.playerOpen = vm.player.current != null
            intent.action == android.content.Intent.ACTION_VIEW -> intent.dataString?.let(vm::openLink)
            intent.action == android.content.Intent.ACTION_SEND -> intent.getStringExtra(android.content.Intent.EXTRA_TEXT)?.let(vm::openShared)
        }
    }

    companion object {
        const val OPEN_PLAYER = "open_player"
    }

    /** Opened from the "new update" notification: look again and show the patch notes. */
    private fun showUpdateFrom(intent: android.content.Intent?) {
        if (intent?.getBooleanExtra("show_update", false) == true) vm.checkUpdates(false)
    }
}

@Composable
fun RymApp(vm: AppViewModel) {
    // Android 13+ asks before an app may show notifications (the music controls are one).
    val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    val ctx = LocalContext.current
    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
    // The last run crashed: the report is offered (it shows what would be sent).
    var crash by remember { mutableStateOf(com.rymplayer.data.CrashReport.pending(ctx)) }
    crash?.let { report ->
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { com.rymplayer.data.CrashReport.dismiss(ctx); crash = null },
            title = { Text(tr("RyM closed unexpectedly last time")) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    Text(tr("Open report opens a GitHub issue with this report filled in, and you send it yourself. Nothing is sent otherwise."))
                    Text(report, fontSize = 11.sp, modifier = Modifier.padding(top = 10.dp))
                }
            },
            confirmButton = {
                androidx.compose.material3.TextButton({
                    com.rymplayer.data.CrashReport.dismiss(ctx)
                    crash = null
                    runCatching { ctx.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(com.rymplayer.data.CrashReport.issueUrl(report)))) }
                }) { Text(tr("Open report")) }
            },
            dismissButton = {
                androidx.compose.material3.TextButton({ com.rymplayer.data.CrashReport.dismiss(ctx); crash = null }) { Text(tr("Not now")) }
            },
        )
    }
    // Short messages ("Added to ...", errors) as toasts.
    val context = LocalContext.current
    LaunchedEffect(vm.status, vm.player.message) {
        listOf(vm.status, vm.player.message).filter { it.isNotEmpty() }.forEach { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
        vm.status = ""
        vm.player.message = ""
    }
    // Arabic reads right to left: the whole layout turns around.
    val direction = if (I18n.arabic) androidx.compose.ui.unit.LayoutDirection.Rtl else androidx.compose.ui.unit.LayoutDirection.Ltr
    androidx.compose.runtime.CompositionLocalProvider(androidx.compose.ui.platform.LocalLayoutDirection provides direction) {
        when (vm.signedIn) {
            null -> Loading()
            false -> LoginScreen(vm)
            true -> Main(vm)
        }
    }
}

@Composable
private fun Main(vm: AppViewModel) {
    BackHandler(vm.video != null || vm.recapOpen || vm.storeOpen || vm.settingsOpen || vm.playerOpen || vm.stack.isNotEmpty() || vm.tab != Screen.Home) { vm.back() }
    val tint = coverColor(vm.player.current?.thumb)
    Box(Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            bottomBar = {
                Column {
                    // Its top edge is where rain and ash land (FallingParticles).
                    androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.onGloballyPositioned { com.rymplayer.ui.miniBar = if (vm.player.current == null) androidx.compose.ui.geometry.Rect.Zero else it.boundsInRoot() }) {
                        MiniPlayer(vm, tint)
                    }
                    // RyM YTM: a black bar, the chosen tab in white without a pill behind it.
                    NavigationBar(containerColor = if (vm.colors != null) MaterialTheme.colorScheme.surfaceVariant else if (vm.ytmTheme) Color.Black else Color(0xFF111114)) {
                        val tabs = listOf(Triple(Screen.Home, tr("Home"), Icons.Default.Home), Triple(Screen.Search, tr("Search"), Icons.Default.Search), Triple(Screen.Library, tr("Library"), Icons.Default.LibraryMusic))
                        val colors = if (vm.colors == null && vm.ytmTheme) androidx.compose.material3.NavigationBarItemDefaults.colors(
                            selectedIconColor = Color.White,
                            selectedTextColor = Color.White,
                            unselectedIconColor = com.rymplayer.ui.Dim,
                            unselectedTextColor = com.rymplayer.ui.Dim,
                            indicatorColor = Color.Transparent,
                        ) else androidx.compose.material3.NavigationBarItemDefaults.colors()
                        for ((screen, label, icon) in tabs) {
                            NavigationBarItem(vm.tab == screen && vm.stack.isEmpty(), { vm.selectTab(screen) }, { Icon(icon, label) }, label = { Text(label) }, colors = colors)
                        }
                    }
                }
            },
        ) { padding ->
            AnimatedContent(
                targetState = vm.screen,
                transitionSpec = {
                    // Detail pages slide in from the side; tab switches cross-fade.
                    if (targetState is Screen.Songs) {
                        (slideInHorizontally(tween(300)) { it / 3 } + fadeIn(tween(300))) togetherWith fadeOut(tween(150))
                    } else {
                        fadeIn(tween(220)) togetherWith (fadeOut(tween(150)) + slideOutHorizontally(tween(220)) { it / 6 })
                    }
                },
                modifier = Modifier.padding(padding),
                label = "page",
            ) { s ->
                when (s) {
                    Screen.Home -> HomePage(vm)
                    Screen.Search -> SearchPage(vm)
                    Screen.Library -> LibraryPage(vm)
                    is Screen.Songs -> SongsPage(vm, s)
                }
            }
        }
        AnimatedVisibility(vm.playerOpen, enter = slideInVertically(tween(350)) { it }, exit = slideOutVertically(tween(300)) { it }) {
            Box(Modifier.fillMaxSize().takesAllTouches()) { FullPlayer(vm, tint) }
        }
        vm.menu?.let { m -> com.rymplayer.ui.SongSheet(vm, m) { vm.menu = null } }
        AnimatedVisibility(vm.settingsOpen, enter = slideInHorizontally(tween(300)) { it }, exit = slideOutHorizontally(tween(250)) { it }) {
            Box(Modifier.fillMaxSize().takesAllTouches()) { com.rymplayer.ui.SettingsPage(vm) }
        }
        AnimatedVisibility(vm.storeOpen, enter = slideInHorizontally(tween(300)) { it }, exit = slideOutHorizontally(tween(250)) { it }) {
            Box(Modifier.fillMaxSize().takesAllTouches()) { com.rymplayer.ui.PluginStorePage(vm) }
        }
        AnimatedVisibility(vm.recapOpen, enter = slideInHorizontally(tween(300)) { it }, exit = slideOutHorizontally(tween(250)) { it }) {
            Box(Modifier.fillMaxSize().takesAllTouches()) { com.rymplayer.ui.RecapPage(vm) }
        }
        if (vm.lyricsPill && !vm.playerOpen) com.rymplayer.ui.LyricsPill(vm)
        vm.video?.let { Box(Modifier.fillMaxSize().takesAllTouches()) { com.rymplayer.ui.VideoPage(vm, it) } }
        vm.update?.let { com.rymplayer.ui.UpdateDialog(vm, it) }
        if (vm.linkOpen) com.rymplayer.ui.LinkDialog(vm)
        if (vm.remoteOpen) com.rymplayer.ui.RemoteSheet(vm) { vm.remoteOpen = false }
        // A song taken out of the queue: Undo, for a few seconds.
        if (vm.player.undoNote.isNotEmpty()) {
            LaunchedEffect(vm.player.undoNoteId) {
                kotlinx.coroutines.delay(5000)
                vm.player.undoNote = ""
            }
            androidx.compose.material3.Snackbar(
                action = { androidx.compose.material3.TextButton({ vm.player.undo() }) { Text(tr("Undo"), color = MaterialTheme.colorScheme.primary) } },
                modifier = Modifier.align(androidx.compose.ui.Alignment.BottomCenter).padding(start = 12.dp, end = 12.dp, bottom = if (vm.playerOpen) 40.dp else 150.dp).takesAllTouches(),
            ) { Text(vm.player.undoNote, maxLines = 2) }
        }
    }
}
