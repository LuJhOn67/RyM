package com.rymplayer

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
        enableEdgeToEdge()
        setContent { RymTheme { RymApp(viewModel()) } }
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
    // Short messages ("Added to ...", errors) as toasts.
    val context = LocalContext.current
    LaunchedEffect(vm.status, vm.player.message) {
        listOf(vm.status, vm.player.message).filter { it.isNotEmpty() }.forEach { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
        vm.status = ""
        vm.player.message = ""
    }
    when (vm.signedIn) {
        null -> Loading()
        false -> LoginScreen(vm)
        true -> Main(vm)
    }
}

@Composable
private fun Main(vm: AppViewModel) {
    BackHandler(vm.playerOpen || vm.stack.isNotEmpty() || vm.tab != Screen.Home) { vm.back() }
    val tint = coverColor(vm.player.current?.thumb)
    Box(Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = Color(0xFF0B0B0D),
            bottomBar = {
                Column {
                    MiniPlayer(vm, tint)
                    NavigationBar(containerColor = Color(0xFF111114)) {
                        val tabs = listOf(Triple(Screen.Home, "Home", Icons.Default.Home), Triple(Screen.Search, "Search", Icons.Default.Search), Triple(Screen.Library, "Library", Icons.Default.LibraryMusic))
                        for ((screen, label, icon) in tabs) {
                            NavigationBarItem(vm.tab == screen && vm.stack.isEmpty(), { vm.selectTab(screen) }, { Icon(icon, label) }, label = { Text(label) })
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
            FullPlayer(vm, tint)
        }
        vm.menu?.let { m -> com.rymplayer.ui.SongSheet(vm, m) { vm.menu = null } }
    }
}
