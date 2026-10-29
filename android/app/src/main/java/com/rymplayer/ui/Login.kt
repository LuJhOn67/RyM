package com.rymplayer.ui

import android.annotation.SuppressLint
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import com.rymplayer.AppViewModel
import com.rymplayer.data.YtMusic

private const val SIGN_IN = "https://accounts.google.com/ServiceLogin?service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F"

@Composable
fun LoginScreen(vm: AppViewModel) {
    var web by remember { mutableStateOf(false) }
    if (web) {
        SignInWeb { cookie -> vm.start(cookie) }
        return
    }
    Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Color(0xFF3A0B1A), Color.Black))), contentAlignment = Alignment.Center) {
        Column(Modifier.fillMaxWidth().padding(32.dp).appear("login"), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            Box(Modifier.size(84.dp).clip(CircleShape).background(Accent), contentAlignment = Alignment.Center) {
                Icon(Icons.Default.PlayArrow, null, tint = Color.White, modifier = Modifier.size(48.dp))
            }
            Spacer(Modifier.height(18.dp))
            Text("RyM", fontSize = 36.sp, fontWeight = FontWeight.Bold)
            Text("YouTube Music, your way", color = Dim)
            Spacer(Modifier.height(36.dp))
            Button({ web = true }, Modifier.fillMaxWidth().height(52.dp)) { Text("Sign in with Google", fontSize = 16.sp) }
            TextButton(vm::guest, Modifier.padding(top = 8.dp)) { Text("Continue without an account", color = Color.White) }
            if (vm.status.isNotEmpty()) Text(vm.status, color = Dim, fontSize = 13.sp, modifier = Modifier.padding(top = 12.dp))
        }
    }
}

/** Google's sign-in page; done once YouTube Music has the session cookies. */
@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun SignInWeb(onDone: (String) -> Unit) {
    AndroidView(
        modifier = Modifier.fillMaxSize().statusBarsPadding(),
        factory = { context ->
            CookieManager.getInstance().setAcceptCookie(true)
            WebView(context).apply {
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                CookieManager.getInstance().setAcceptThirdPartyCookies(this, true)
                webViewClient = object : WebViewClient() {
                    override fun onPageFinished(view: WebView, url: String) {
                        if (!url.startsWith(YtMusic.ORIGIN)) return
                        val cookie = CookieManager.getInstance().getCookie(YtMusic.ORIGIN) ?: return
                        if (cookie.split(';').any { it.trim().startsWith("SAPISID=") || it.trim().startsWith("__Secure-3PAPISID=") }) {
                            CookieManager.getInstance().flush()
                            onDone(cookie)
                        }
                    }
                }
                loadUrl(SIGN_IN)
            }
        },
    )
}
