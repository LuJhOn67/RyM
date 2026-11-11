package com.rymplayer

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.rymplayer.data.Downloads
import com.rymplayer.data.FeedMemory
import com.rymplayer.data.PageCache
import com.rymplayer.data.LIKED_ID
import com.rymplayer.data.Link
import com.rymplayer.data.Lrclib
import com.rymplayer.data.Lyrics
import com.rymplayer.data.LyricSources
import com.rymplayer.data.Recent
import com.rymplayer.data.LyricsCache
import com.rymplayer.data.Playlist
import com.rymplayer.data.SignedOutException
import com.rymplayer.data.Track
import com.rymplayer.data.MusicApi
import com.rymplayer.data.parseDuration
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import com.rymplayer.playback.Player
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/** The song a menu is open for, and its place in the queue when opened from "Up next". */
data class MenuFor(val track: Track, val queueIndex: Int? = null)

sealed interface Screen {
    data object Home : Screen
    data object Search : Screen
    data object Library : Screen
    data class Songs(val kind: Kind, val id: String, val title: String) : Screen

    enum class Kind { Playlist, Album, Mix, Radio, Artist, History, Results, Downloads, SoundCloud, SoundCloudLink, Spotify }
}

class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val prefs = app.getSharedPreferences("rym", Context.MODE_PRIVATE)

    var signedIn by mutableStateOf<Boolean?>(null)
        private set
    var api: MusicApi = MusicApi(null)
        private set
    var status by mutableStateOf("")

    /** Bottom tab; detail pages stack on top of it. */
    var tab by mutableStateOf<Screen>(Screen.Home)
        private set
    val stack = mutableStateListOf<Screen.Songs>()
    val screen: Screen get() = stack.lastOrNull() ?: tab

    var home by mutableStateOf<List<Track>>(emptyList())
        private set
    var homeLoading by mutableStateOf(false)
        private set
    var playlists by mutableStateOf<List<Playlist>>(emptyList())
        private set
    val pages = androidx.compose.runtime.mutableStateMapOf<Screen.Songs, List<Track>>()
    var query by mutableStateOf("")

    /** Suggestions under the search box: the account's search history first (the same as on the PC). */
    var suggestions by mutableStateOf<List<com.rymplayer.data.MusicApi.Suggestion>>(emptyList())
        private set
    private var suggestJob: Job? = null

    /** Typing in the search box: suggestions once it pauses for a moment. */
    fun typeQuery(q: String) {
        query = q
        askSuggestions(250)
    }

    fun askSuggestions(waitMs: Long = 0) {
        suggestJob?.cancel()
        val text = query.trim()
        suggestJob = viewModelScope.launch {
            delay(waitMs)
            suggestions = runCatching { api.searchSuggestions(text) }.getOrDefault(emptyList())
        }
    }

    /** A past search taken off the account's history. */
    fun forgetSearch(s: com.rymplayer.data.MusicApi.Suggestion) {
        suggestions = suggestions - s
        val token = s.forget?.takeIf { it.isNotEmpty() } ?: return
        viewModelScope.launch { runCatching { api.forgetSearch(token) } }
    }

    fun searchFor(text: String) {
        query = text
        search()
    }
    val liked = mutableStateListOf<String>()

    val player = Player(app, viewModelScope)
    var lyrics by mutableStateOf<Lyrics?>(null)
        private set
    var lyricsStatus by mutableStateOf("")
        private set
    var playerOpen by mutableStateOf(false)
    /** Song menu (bottom sheet), shown over everything; null when closed. */
    var menu by mutableStateOf<MenuFor?>(null)
    private var lyricsJob: Job? = null

    /** Every source's lyrics for the playing song (the lyrics view's switcher), once asked for. */
    var lyricsSources by mutableStateOf<List<Lyrics>>(emptyList())
        private set
    var lookingForSources by mutableStateOf(false)
        private set
    private var sourcesFor: String? = null
    private var sourcesJob: Job? = null

    init {
        I18n.arabic = prefs.getBoolean("arabic", false)
        com.rymplayer.data.YtMusicApi.init(app)
        com.rymplayer.data.YtMusicApi.arabic = I18n.arabic
        com.rymplayer.playback.Streams.warmUp()
        com.rymplayer.playback.Streams.connections = prefs.getInt("dl_connections", 8)
        com.rymplayer.playback.Streams.pieceMb = prefs.getInt("dl_piece_mb", 0)
        val cookie = com.rymplayer.data.SecureCookie.read(prefs)
        if (cookie != null) start(cookie) else signedIn = false
        player.onTrackChanged = ::loadLyrics
        followLyrics()
        player.likedSongs = { likedTracks }
        // What was playing last time, paused where it was left.
        player.restoreSession()
        player.useVlc = prefs.getBoolean("vlc", false)
    }

    // ---------------------------------------------------------------- settings, updates, plugins

    /** Full-screen pages over everything: settings, the plugin store, a video. */
    var settingsOpen by mutableStateOf(false)
    /** "Your recap" (listening stats) is open. */
    var recapOpen by mutableStateOf(false)

    /** Sound settings (applied by the playback service as they're saved). */
    var sound by mutableStateOf(com.rymplayer.playback.Sound.load(app))
        private set

    fun changeSound(s: com.rymplayer.playback.Sound.Settings) {
        sound = s
        com.rymplayer.playback.Sound.save(getApplication(), s)
    }

    /** Sleep timer: minutes from now, null for the end of the song, 0 to turn it off. */
    fun setSleep(minutes: Int?) {
        val at = when (minutes) {
            null -> -1L
            0 -> 0L
            else -> System.currentTimeMillis() + minutes * 60_000L
        }
        changeSound(com.rymplayer.playback.Sound.load(getApplication()).copy(sleepAt = at))
        status = tr(if (at == 0L) "Sleep timer off" else if (at < 0) "The music stops after this song" else "Sleep timer set")
    }

    /** The floating lyrics line over the app. */
    var lyricsPill by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("lyrics_pill", false))
        private set

    fun toggleLyricsPill() {
        lyricsPill = !lyricsPill
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("lyrics_pill", lyricsPill).apply()
    }

    /** RaM points: For you keeps to this kind (null: every kind heard). */
    var onlyKind by mutableStateOf(com.rymplayer.data.RamPoints.onlyKind(app))
        private set

    /** RaM points in the settings: songs with points and the top artists. */
    var ramSummary by mutableStateOf(com.rymplayer.data.RamPoints.Summary(0, emptyList()))
        private set

    fun refreshRamSummary() {
        viewModelScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            val s = com.rymplayer.data.RamPoints.summary(getApplication())
            launch(kotlinx.coroutines.Dispatchers.Main) { ramSummary = s }
        }
    }

    /** The day the points reset by themselves ("" = none), and the weekly reset (off unless turned on). */
    var ramResetDate by mutableStateOf(com.rymplayer.data.RamPoints.resetDate(app))
        private set
    var ramWeeklyReset by mutableStateOf(com.rymplayer.data.RamPoints.weeklyReset(app))
        private set

    fun chooseResetDate(date: String) {
        ramResetDate = date
        com.rymplayer.data.RamPoints.setResetDate(getApplication(), date)
    }

    fun chooseWeeklyReset(on: Boolean) {
        ramWeeklyReset = on
        com.rymplayer.data.RamPoints.setWeeklyReset(getApplication(), on)
    }

    /** The reset the user scheduled, when it's due (looked at when RyM comes to the front). */
    fun checkScheduledReset() {
        val why = com.rymplayer.data.RamPoints.scheduledReset(getApplication()) ?: return
        ramResetDate = com.rymplayer.data.RamPoints.resetDate(getApplication())
        refreshRamSummary()
        player.message = tr(if (why == "date") "Points reset: the day you picked came" else "Points reset: a week passed")
    }

    fun resetAllPoints() {
        com.rymplayer.data.RamPoints.resetAll(getApplication())
        refreshRamSummary()
        player.message = tr("All points are back to 0")
    }

    fun resetNegativePoints() {
        com.rymplayer.data.RamPoints.resetNegative(getApplication())
        refreshRamSummary()
        player.message = tr("Negative points are back to 0")
    }

    fun cycleOnlyKind(step: Int = 1) {
        val all = listOf<com.rymplayer.data.RamPoints.Kind?>(null) + com.rymplayer.data.RamPoints.Kind.entries
        onlyKind = all[Math.floorMod(all.indexOf(onlyKind) + step, all.size)]
        com.rymplayer.data.RamPoints.setOnlyKind(getApplication(), onlyKind)
    }

    /** The user says this song's artist is of [kind] (RaM points keeps it). */
    /** "Recommend more" on a song (RaM points). */
    fun recommendMore(t: Track) {
        com.rymplayer.data.RamPoints.recommendMore(getApplication(), t)
        status = "${tr("More like this")}: ${t.artist}"
    }

    /** "Don't recommend": never suggested again; out of Up next if it's waiting there. */
    fun dontRecommend(t: Track) {
        com.rymplayer.data.RamPoints.dontRecommend(getApplication(), t)
        val i = player.queue.indexOfFirst { it.videoId == t.videoId }
        if (i > player.index) player.remove(i)
        status = "${tr("Won't be suggested")}: ${t.title}"
    }

    fun setArtistKind(t: Track, kind: com.rymplayer.data.RamPoints.Kind) {
        com.rymplayer.data.RamPoints.correct(getApplication(), t, kind)
        status = "${com.rymplayer.data.Radio.firstArtist(t.artist).ifEmpty { t.artist }}: ${tr(kind.title)}"
    }

    /** yt-dlp from the nightly builds, and what the last update said. */
    var ytdlpNightly by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("ytdlp_nightly", false))
        private set
    var ytdlpStatus by mutableStateOf("")
        private set

    fun toggleYtdlpNightly() {
        ytdlpNightly = !ytdlpNightly
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("ytdlp_nightly", ytdlpNightly).apply()
    }

    /** Settings > Network & Parsers: which engine finds songs (ytmusicapi, the built-in one as the rollback). */
    var musicEngine by mutableStateOf(com.rymplayer.data.YtMusicApi.engine)
        private set
    var parsersStatus by mutableStateOf("")

    fun cycleEngine(step: Int = 1) {
        val all = com.rymplayer.data.YtMusicApi.Engine.entries
        musicEngine = all[Math.floorMod(musicEngine.ordinal + step, all.size)]
        com.rymplayer.data.YtMusicApi.setEngine(getApplication(), musicEngine)
    }

    /** yt-dlp and ytmusicapi to their newest, saved links and pages forgotten. */
    fun updateParsers() {
        parsersStatus = tr("Updating…")
        val nightly = ytdlpNightly
        viewModelScope.launch {
            val text = com.rymplayer.data.YtMusicApi.updateParsers(nightly)
            parsersStatus = "${tr("Up to date")} · $text"
        }
    }

    fun updateYtDlp() {
        ytdlpStatus = tr("Updating…")
        val nightly = ytdlpNightly
        viewModelScope.launch {
            ytdlpStatus = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { com.rymplayer.data.YtDlp.update(getApplication(), nightly) }
        }
    }

    /** Music Cross Ears: this phone's music can move to a PC of the same account (RyM Link). */
    var crossEars by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("cross_ears", true).also { com.rymplayer.playback.Link.enabled = it })
        private set

    fun toggleCrossEars() {
        crossEars = !crossEars
        com.rymplayer.playback.Link.enabled = crossEars
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("cross_ears", crossEars).apply()
    }

    /** The phone's look: RyM YTM (pure black, quick picks, big shelves) or the classic one. */
    var ytmTheme by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("theme_ytm", false))
        private set

    /** Settings > Colors: a built-in theme's name, "Custom" (imported), or "" (the app's own). */
    var palette by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getString("palette", "").orEmpty())
        private set
    private var customTheme by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getString("custom_theme", null)
        ?.let { runCatching { com.rymplayer.data.Themes.fromJson(it) }.getOrNull() })
    val colors: com.rymplayer.data.Themes.Theme?
        get() = when (palette) { "" -> null; "Custom" -> customTheme; else -> com.rymplayer.data.Themes.preset(palette) }

    private fun choosePalette(name: String) {
        palette = name
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putString("palette", name).apply()
    }

    fun cyclePalette(step: Int = 1) {
        val names = listOf("") + com.rymplayer.data.Themes.presets.map { it.name } + (if (customTheme != null) listOf("Custom") else emptyList())
        choosePalette(names[Math.floorMod(names.indexOf(palette).coerceAtLeast(0) + step, names.size)])
    }

    /** "Prefer lossless (FLAC)": the Internet Archive's FLAC first, YouTube when it isn't there. */
    var preferLossless by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("prefer_lossless", false).also { player.preferLossless = it })
        private set

    fun togglePreferLossless() {
        preferLossless = !preferLossless
        player.preferLossless = preferLossless
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("prefer_lossless", preferLossless).apply()
    }

    /** Where song links come from: 0 InnerTube + yt-dlp, 1 yt-dlp, 2 InnerTube (as on the PC). */
    var audioSource by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getInt("audio_source", 0).also { com.rymplayer.playback.Streams.useSource(app, it) })
        private set

    fun cycleSource() {
        audioSource = (audioSource + 1) % 3
        com.rymplayer.playback.Streams.useSource(getApplication(), audioSource)
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putInt("audio_source", audioSource).apply()
    }

    /** Animations' frames a second (0: as fast as the screen) and the performance badge. */
    var fpsCap by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getInt("fps_cap", 60).also { com.rymplayer.ui.animFps = it })
        private set
    var perfBadge by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("perf_badge", false))
        private set

    fun cycleFps(step: Int = 1) {
        val steps = listOf(24, 30, 60, 120, 144, 240, 0)
        fpsCap = steps[Math.floorMod(steps.indexOf(fpsCap).coerceAtLeast(0) + step, steps.size)]
        com.rymplayer.ui.animFps = fpsCap
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putInt("fps_cap", fpsCap).apply()
    }

    fun togglePerfBadge() {
        perfBadge = !perfBadge
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("perf_badge", perfBadge).apply()
    }

    /** Battery saver, or under 20% and not charging: animations stop (looked at every 30 s while RyM is in front). */
    fun refreshEco() {
        val ctx = getApplication<Application>()
        val power = ctx.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
        val battery = ctx.getSystemService(Context.BATTERY_SERVICE) as android.os.BatteryManager
        val level = battery.getIntProperty(android.os.BatteryManager.BATTERY_PROPERTY_CAPACITY)
        com.rymplayer.ui.animPaused = power.isPowerSaveMode || (level in 0..19 && !battery.isCharging)
    }

    private val ecoWatch = viewModelScope.launch {
        while (true) {
            if (player.visible) refreshEco()
            kotlinx.coroutines.delay(30_000)
        }
    }

    /** Liquid Aurora (the cover's colors moving with the bass) and the cover as a spinning record. */
    var aurora by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("aurora", false))
        private set
    var vinyl by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("vinyl", false))
        private set

    fun toggleAurora() {
        aurora = !aurora
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("aurora", aurora).apply()
    }

    fun toggleVinyl() {
        vinyl = !vinyl
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("vinyl", vinyl).apply()
    }

    /** The colors shown now as JSON, on the clipboard (to paste on the PC or share). */
    fun exportTheme() {
        val t = colors ?: com.rymplayer.data.Themes.Theme(name = "RyM")
        val cm = getApplication<Application>().getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        cm.setPrimaryClip(android.content.ClipData.newPlainText("RyM theme", t.toJson()))
        player.message = tr("Theme copied: paste it on the PC or share it")
    }

    /** A theme's JSON from the clipboard becomes "Custom". */
    fun importTheme() {
        val cm = getApplication<Application>().getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val text = cm.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(getApplication())?.toString().orEmpty()
        runCatching { com.rymplayer.data.Themes.fromJson(text) }
            .onSuccess { t ->
                customTheme = t
                getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putString("custom_theme", t.toJson()).apply()
                choosePalette("Custom")
                player.message = t.name
            }
            .onFailure { player.message = "${tr("Import colors")}: ${it.message}" }
    }

    fun toggleTheme() {
        ytmTheme = !ytmTheme
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("theme_ytm", ytmTheme).apply()
    }

    /** The sung line of the playing song's synced lyrics. */
    var lyricLine by mutableStateOf<Int?>(null)
        private set
    var storeOpen by mutableStateOf(false)
    /** Video plugin: the song whose video is showing. */
    var video by mutableStateOf<Track?>(null)
    /** Video plugin: the "download from a link" box is open. */
    var linkOpen by mutableStateOf(false)

    /** The computer's remote is open. */
    var remoteOpen by mutableStateOf(false)

    val versionName: String = runCatching { app.packageManager.getPackageInfo(app.packageName, 0).versionName }.getOrNull().orEmpty()

    var autoUpdates by mutableStateOf(prefs.getBoolean("auto_updates", true))
        private set
    var betaUpdates by mutableStateOf(prefs.getBoolean("beta_updates", false))
        private set
    /** A newer RyM, shown with its patch notes until closed. */
    var update by mutableStateOf<com.rymplayer.data.Updates.Release?>(null)
    var checking by mutableStateOf(false)
        private set
    var lastUpdateCheck by mutableStateOf(prefs.getLong("last_update_check", 0L))
        private set

    fun toggleAutoUpdates() {
        autoUpdates = !autoUpdates
        prefs.edit().putBoolean("auto_updates", autoUpdates).apply()
        UpdateWorker.schedule(getApplication(), autoUpdates)
    }

    fun toggleBetaUpdates() {
        betaUpdates = !betaUpdates
        prefs.edit().putBoolean("beta_updates", betaUpdates).apply()
    }

    /** Looks on GitHub for a newer RyM; [asked]: from the settings, so "up to date" is said too. */
    /** The update downloading in the app (0 to 1), or null. */
    var updateProgress by mutableStateOf<Float?>(null)
        private set

    /**
     * Downloads the new APK here and opens Android's installer on it (no browser). The first time,
     * Android asks to let RyM install apps: its settings page opens, then Update is pressed again.
     */
    fun installUpdate(r: com.rymplayer.data.Updates.Release) {
        val ctx = getApplication<Application>()
        val url = r.apk ?: return
        if (!ctx.packageManager.canRequestPackageInstalls()) {
            status = tr("Allow RyM to install updates, then press Update now again")
            runCatching {
                ctx.startActivity(
                    android.content.Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, android.net.Uri.parse("package:${ctx.packageName}"))
                        .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            }
            return
        }
        if (updateProgress != null) return
        updateProgress = 0f
        viewModelScope.launch {
            val file = runCatching { com.rymplayer.data.Updates.download(ctx, url) { p -> updateProgress = p } }
                .onFailure { status = "${tr("Couldn't update")}: ${it.message}" }
                .getOrNull()
            updateProgress = null
            if (file == null) return@launch
            // Signed differently from the app on the phone, Android only says it doesn't fit: say why.
            if (!sameSigner(ctx, file)) {
                status = tr("This update is signed differently from the RyM on this phone: uninstall RyM, then install the new APK from the release page")
                return@launch
            }
            update = null
            val uri = androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file)
            runCatching {
                ctx.startActivity(
                    android.content.Intent(android.content.Intent.ACTION_VIEW)
                        .setDataAndType(uri, "application/vnd.android.package-archive")
                        .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            }.onFailure { status = "${tr("Couldn't update")}: ${it.message}" }
        }
    }

    /** The APK at [file] is signed by the same key as the installed app (when it can't be read: assume so). */
    private fun sameSigner(ctx: Application, file: java.io.File): Boolean = runCatching {
        val pm = ctx.packageManager
        val flags = android.content.pm.PackageManager.GET_SIGNING_CERTIFICATES
        val new = pm.getPackageArchiveInfo(file.path, flags)?.signingInfo?.apkContentsSigners ?: return true
        val mine = pm.getPackageInfo(ctx.packageName, flags).signingInfo?.apkContentsSigners ?: return true
        new.map { it.toCharsString() }.toSet() == mine.map { it.toCharsString() }.toSet()
    }.getOrDefault(true)

    fun checkUpdates(asked: Boolean) {
        if (checking) return
        checking = true
        viewModelScope.launch {
            runCatching { com.rymplayer.data.Updates.check(betaUpdates, versionName) }
                .onSuccess { r ->
                    lastUpdateCheck = System.currentTimeMillis()
                    prefs.edit().putLong("last_update_check", lastUpdateCheck).apply()
                    if (r != null) update = r else if (asked) status = tr("You have the latest version")
                }
                .onFailure { if (asked) status = "${tr("Couldn't check for updates")}: ${it.message}" }
            checking = false
        }
    }

    /** The plugin store's list, and what's installed (id -> version). */
    var store by mutableStateOf<List<com.rymplayer.data.Plugins.Plugin>?>(null)
        private set
    val plugins = androidx.compose.runtime.mutableStateMapOf<String, String>().apply { putAll(com.rymplayer.data.Plugins.installed(app)) }
    val hasVideo: Boolean get() = com.rymplayer.data.Plugins.VIDEO in plugins
    fun hasPlugin(id: String) = id in plugins
    /** The search box looks on SoundCloud (its plugin installed and picked on the search page). */
    var searchSoundCloud by mutableStateOf(false)

    fun loadStore() = viewModelScope.launch {
        runCatching { com.rymplayer.data.Plugins.store(getApplication()) }.onSuccess { list ->
            store = list
            val news = list.filter { p -> plugins[p.id]?.let { com.rymplayer.data.Updates.newer(p.version, it) } == true }
            if (news.isNotEmpty()) status = "${tr("Plugin updates")}: ${news.joinToString { it.name }}"
        }
    }

    /** Installs (or updates) a plugin, or removes it; its features appear or go at once. */
    fun setPlugin(p: com.rymplayer.data.Plugins.Plugin, on: Boolean) {
        com.rymplayer.data.Plugins.set(getApplication(), p.id, if (on) p.version else null)
        if (on) plugins[p.id] = p.version else plugins.remove(p.id)
        status = tr(if (on) "Plugin installed" else "Plugin removed")
    }

    /** Video plugin: the song's music video, full screen (the song pauses). */
    fun watchVideo(t: Track) {
        player.pause()
        playerOpen = false
        video = t
    }

    /** Video plugin: downloads a link from YouTube or any site, as a video or just the sound. */
    fun downloadLink(url: String, asVideo: Boolean) = viewModelScope.launch {
        status = "${tr("Downloading")} $url"
        status = runCatching {
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                val file = com.rymplayer.data.YtDlp.downloadAny(getApplication(), url, asVideo, com.rymplayer.data.SecureCookie.read(prefs))
                "${tr("Saved to")} ${Downloads.publish(getApplication(), file, asVideo)}"
            }
        }.getOrElse { "${tr("Download failed")}: ${it.message}" }
    }

    // After the settings above exist (Kotlin runs these in order).
    init {
        // Updates: the 12-hour background check, and a look now when it's been that long.
        UpdateWorker.schedule(app, autoUpdates)
        if (autoUpdates && System.currentTimeMillis() - lastUpdateCheck > com.rymplayer.data.Updates.EVERY_MS) checkUpdates(false)
        loadStore()
    }

    // ---------------------------------------------------------------- session

    fun start(cookie: String?) {
        com.rymplayer.data.SecureCookie.write(prefs, cookie)
        api = MusicApi(cookie)
        com.rymplayer.playback.Streams.cookie = cookie
        signedIn = cookie != null
        player.api = api
        // Last saved home and playlists show at once; fresh ones replace them when they arrive.
        if (home.isEmpty()) home = PageCache.loadTracks(getApplication(), "home")
        if (cookie != null && playlists.isEmpty()) playlists = PageCache.loadPlaylists(getApplication())
        loadHome()
        if (api.signedIn) {
            viewModelScope.launch {
                try {
                    // Both at once instead of one after the other.
                    val (lists, likedSongs) = coroutineScope {
                        val lists = async { api.libraryPlaylists() }
                        val likedSongs = async { api.playlist(LIKED_ID) }
                        lists.await() to likedSongs.await()
                    }
                    playlists = lists
                    PageCache.savePlaylists(getApplication(), lists)
                    syncLiked(likedSongs)
                } catch (e: SignedOutException) {
                    status = tr("Your session ended. Sign in again.")
                    signOut()
                } catch (e: Exception) {
                    status = e.message.orEmpty()
                }
            }
        }
    }

    fun guest() = start(null).also { signedIn = true }

    fun signOut() {
        com.rymplayer.data.SecureCookie.write(prefs, null)
        // Forget the sign-in page's Google session too, so the next sign-in can pick another account.
        android.webkit.CookieManager.getInstance().removeAllCookies(null)
        player.stop()
        api = MusicApi(null)
        signedIn = false
        playlists = emptyList()
        liked.clear()
        localLikes.clear()
        stack.clear()
        // The saved home and playlists were this account's.
        PageCache.clear(getApplication())
        home = emptyList()
    }

    // ---------------------------------------------------------------- navigation

    fun selectTab(s: Screen) {
        stack.clear()
        tab = s
        playerOpen = false
    }

    fun open(page: Screen.Songs) {
        stack += page
        playerOpen = false
        // Downloads can change at any time; the rest is loaded once.
        if (pages[page] == null || page.kind == Screen.Kind.Downloads) load(page)
    }

    /** True when something was closed. */
    fun back(): Boolean = when {
        video != null -> { video = null; true }
        recapOpen -> { recapOpen = false; true }
        storeOpen -> { storeOpen = false; true }
        settingsOpen -> { settingsOpen = false; true }
        playerOpen -> { playerOpen = false; true }
        stack.isNotEmpty() -> { stack.removeAt(stack.lastIndex); true }
        tab != Screen.Home -> { tab = Screen.Home; true }
        else -> false
    }

    /** A YouTube Music (or YouTube) link opened in RyM: the song plays, a list opens. */
    fun openLink(link: String) {
        val target = com.rymplayer.data.YouTubeLinks.parse(link)
        if (target == null) {
            status = tr("Not a YouTube Music link")
            return
        }
        settingsOpen = false
        storeOpen = false
        recapOpen = false
        when (target) {
            is com.rymplayer.data.YouTubeLinks.Target.Song -> playSongId(target.videoId)
            is com.rymplayer.data.YouTubeLinks.Target.Playlist -> { playerOpen = false; open(Screen.Songs(Screen.Kind.Playlist, target.id, tr("Playlist"))) }
            is com.rymplayer.data.YouTubeLinks.Target.Mix -> { playerOpen = false; open(Screen.Songs(Screen.Kind.Mix, target.id, tr("Mix"))) }
            is com.rymplayer.data.YouTubeLinks.Target.Album -> { playerOpen = false; open(Screen.Songs(Screen.Kind.Album, target.id, tr("Album"))) }
            is com.rymplayer.data.YouTubeLinks.Target.Artist -> { playerOpen = false; open(Screen.Songs(Screen.Kind.Artist, target.id, tr("Artist"))) }
        }
    }

    /** Shared to RyM: a YouTube Music link opens; anything else (a Spotify link, words) is searched. */
    fun openShared(text: String) {
        val link = com.rymplayer.data.YouTubeLinks.firstLink(text)
        if (link != null && com.rymplayer.data.YouTubeLinks.parse(link) != null) return openLink(link)
        query = text.trim()
        playerOpen = false
        selectTab(Screen.Search)
        search()
    }

    /** A song by its id (from a link): its details and songs like it come from its radio. */
    private fun playSongId(videoId: String) {
        viewModelScope.launch {
            val radio = runCatching { api.radio(videoId) }.getOrNull().orEmpty()
            val song = radio.firstOrNull { it.videoId == videoId } ?: Track(videoId, tr("Song"), "")
            player.start(listOf(song), 0)
            playerOpen = true
            player.append(player.tidy(radio.filter { it.videoId != videoId }, song))
        }
    }

    fun openLink(t: Track) {
        FeedMemory.opened(getApplication(), t)
        when (val l = t.link) {
            is Link.Playlist -> open(Screen.Songs(Screen.Kind.Playlist, l.id, t.title))
            is Link.Album -> open(Screen.Songs(Screen.Kind.Album, l.id, t.title))
            is Link.Mix -> open(Screen.Songs(Screen.Kind.Mix, l.id, t.title))
            null -> playRadio(t)
        }
    }

    fun search() {
        val q = query.trim()
        if (q.isEmpty()) return
        val sp = com.rymplayer.data.Spotify.isLink(q)
        val scLink = com.rymplayer.data.SoundCloud.isLink(q)
        val plugins = com.rymplayer.data.Plugins
        when {
            sp && !hasPlugin(plugins.SPOTIFY) -> status = tr("Install the Spotify plugin to open Spotify links")
            scLink && !hasPlugin(plugins.SOUNDCLOUD) -> status = tr("Install the SoundCloud plugin to open SoundCloud links")
            sp -> {
                status = tr("Finding the Spotify songs on YouTube Music...")
                open(Screen.Songs(Screen.Kind.Spotify, q, "Spotify"))
            }
            scLink -> open(Screen.Songs(Screen.Kind.SoundCloudLink, q, "SoundCloud"))
            searchSoundCloud && hasPlugin(plugins.SOUNDCLOUD) -> open(Screen.Songs(Screen.Kind.SoundCloud, q, "SoundCloud: \"$q\""))
            else -> open(Screen.Songs(Screen.Kind.Results, q, "${tr("Results for")} \"$q\""))
        }
    }

    /** When the feed was last fetched; coming back after half an hour fetches a fresh one. */
    var homeLoadedAt = 0L
        private set

    fun refreshHomeIfOld() {
        if (!homeLoading && System.currentTimeMillis() - homeLoadedAt > 30 * 60 * 1000L) loadHome()
    }

    /**
     * A fresh home feed (the same as the PC apps' src/feed.rs): "Recently played" on top, then
     * "For you", a mix of three pools fetched at once (the radios of the last songs played, songs
     * never played or liked, and liked or most played ones, about a third), then YouTube's own
     * shelves (rested items left out, see [FeedMemory]). Scrolled to the bottom it goes on
     * ([loadMoreHome]). The liked songs are read on the way, so hearts are up to date too.
     */
    fun loadHome() {
        if (homeLoading && homeJob?.isActive == true) return
        homeLoading = true
        homeJob = viewModelScope.launch {
            val ctx = getApplication<Application>()
            // A failed refresh keeps what's on screen (the saved feed) instead of emptying it.
            runCatching {
                coroutineScope {
                    val user = api.signedIn
                    val recent = Recent.tracks(ctx)
                    val page = async { api.homePaged(3) }
                    val likedSongs = async { if (user) runCatching { api.playlist(LIKED_ID) }.getOrNull() else null }
                    val played = async { if (user) runCatching { api.history() }.getOrDefault(emptyList()) else emptyList() }
                    val home = page.await()
                    val arranged = FeedMemory.arrange(ctx, home.tracks)
                    val likedList = likedSongs.await()
                    likedList?.let(::syncLiked)
                    val history = played.await()
                    // RaM points. Fresh: the radios of the last songs played; new: radios of two liked or
                    // played songs at random (of a kind heard lately, when known).
                    val notes = com.rymplayer.data.RamPoints.load(ctx)
                    val freshSeedTracks = (recent + history).filter { com.rymplayer.data.isYouTube(it.videoId) }.distinctBy { it.videoId }.take(4)
                    val freshSeeds = freshSeedTracks.map { it.videoId }
                    val heardKinds = freshSeedTracks.mapNotNull { notes.kindOf(it) }.toSet()
                    val pool = (likedList.orEmpty().take(80) + history.take(40)).filter { it.videoId !in freshSeeds }
                        .filter { heardKinds.isEmpty() || (notes.kindOf(it)?.let { k -> k in heardKinds } ?: true) }
                    val newSeeds = FeedMemory.pickSeeds(ctx, pool, 2)
                    val newSeedTracks = newSeeds.mapNotNull { id -> pool.firstOrNull { it.videoId == id } }
                    val freshRadios = freshSeedTracks.map { s -> async { s to runCatching { api.radio(s.videoId) }.getOrDefault(emptyList()) } }
                    val newRadios = newSeedTracks.map { s -> async { s to runCatching { api.radio(s.videoId) }.getOrDefault(emptyList()) } }
                    val known = (likedList.orEmpty() + history + recent).map { it.videoId }.toSet()
                    val heard = mutableListOf<Pair<com.rymplayer.data.RamPoints.Kind, Float>>()
                    val cands = mutableListOf<com.rymplayer.data.RamPoints.Candidate>()
                    freshRadios.awaitAll().forEachIndexed { i, (seed, radio) ->
                        val kind = notes.seedKind(seed, radio)
                        notes.learn(kind, seed, radio)
                        heard += kind to 2f / (1 + i)
                        cands += com.rymplayer.data.RamPoints.fromRadio(radio, kind, 10)
                    }
                    (recent + history).filter { it.videoId !in freshSeeds }.take(12).forEachIndexed { i, t ->
                        notes.kindOf(t)?.let { heard += it to 0.6f / (1 + i * 0.3f) }
                    }
                    // "Only this kind" in the settings (a Muharram, say): the page keeps to it.
                    val kinds = com.rymplayer.data.RamPoints.onlyKind(ctx)?.let { listOf(it) } ?: com.rymplayer.data.RamPoints.pageKinds(heard)
                    for ((seed, radio) in newRadios.awaitAll()) {
                        val kind = notes.seedKind(seed, radio)
                        notes.learn(kind, seed, radio)
                        cands += com.rymplayer.data.RamPoints.fromRadio(radio, kind, 6)
                    }
                    // YouTube's own home-page songs and shelves: only the page's kinds.
                    val fitting = arranged.filter { t -> notes.kindOf(t)?.let { it in kinds } ?: true }
                    fitting.filter { it.link == null }.forEach { cands += com.rymplayer.data.RamPoints.Candidate(it, 14, null, false) }
                    (Recent.favourites(ctx, 40) + likedList.orEmpty().take(150)).shuffled().take(60).forEach { cands += com.rymplayer.data.RamPoints.Candidate(it, 4, null, true) }
                    val recentRow = recent.take(20).map { it.copy(album = FeedMemory.RECENTLY_PLAYED) }
                    val skip = (recentRow.map { it.videoId } + freshSeeds).toMutableSet()
                    val topArtists = Recent.favourites(ctx, 40).map { com.rymplayer.data.Radio.firstArtist(it.artist) }.filter { it.isNotEmpty() }.toSet()
                    val shown = FeedMemory.shownLookup(ctx)
                    // The main kind gets the shelf; a second kind, its own shelf (about two in five).
                    val sizes = if (kinds.size > 1) listOf(24, 16) else listOf(40)
                    val forYou = mutableListOf<Track>()
                    kinds.zip(sizes).forEachIndexed { i, (kind, n) ->
                        val section = if (i == 0) FeedMemory.FOR_YOU else kind.shelf
                        val picked = com.rymplayer.data.RamPoints.forYou(notes, cands, kind, n, skip, known, topArtists, shown, { Recent.playedAt(ctx, it) })
                        skip += picked.map { it.videoId }
                        forYou += picked.map { it.copy(album = section) }
                    }
                    com.rymplayer.data.RamPoints.save(ctx, notes)
                    FeedMemory.seen(ctx, forYou)
                    val inMix = forYou.map { it.videoId }.toSet()
                    val out = recentRow + forYou + fitting.filter { it.link != null || it.videoId !in inMix }
                    homeCursor = HomeCursor((home.more as? com.rymplayer.data.MusicApi.More.Home)?.token, (freshSeeds + newSeeds).toMutableList(),
                        out.map(FeedMemory::key).toMutableSet(), likedList.orEmpty(), history, kinds.first())
                    out
                }
            }
                .onFailure { if (home.isEmpty()) status = it.message.orEmpty() }
                .onSuccess { fresh ->
                    home = fresh
                    PageCache.saveTracks(ctx, "home", home)
                    homeLoadedAt = System.currentTimeMillis()
                }
            // Songs at the top start right away when tapped.
            com.rymplayer.playback.Streams.prefetchLinks(home.filter { it.link == null }.take(8).map { it.videoId })
            homeLoading = false
        }
    }

    /** Where the home feed goes on from: YouTube's next page, seeds used, what's on the page, the pools. */
    private class HomeCursor(
        var token: com.rymplayer.data.MusicApi.Token?,
        val used: MutableList<String>,
        val shown: MutableSet<String>,
        val liked: List<Track>,
        val history: List<Track>,
        /** The page's main kind of song (the later parts keep to it). */
        val kind: com.rymplayer.data.RamPoints.Kind,
    )
    private var homeCursor: HomeCursor? = null

    /** The next part of a page is on its way (the spinner at the bottom). */
    var loadingMore by mutableStateOf(false)
        private set

    /**
     * The home feed's next part: a mix from three new seeds (one of the songs played lately, two
     * at random) and YouTube's home page further down; nothing already on the page.
     */
    fun loadMoreHome() {
        val c = homeCursor ?: return
        if (loadingMore || homeLoading) return
        loadingMore = true
        viewModelScope.launch {
            val ctx = getApplication<Application>()
            runCatching {
                coroutineScope {
                    val recent = Recent.tracks(ctx)
                    val notes = com.rymplayer.data.RamPoints.load(ctx)
                    // More of the page's main kind (seeds not known yet: their radio tells).
                    val unused = (recent + c.history + c.liked).filter { com.rymplayer.data.isYouTube(it.videoId) && it.videoId !in c.used }
                        .filter { (notes.kindOf(it) ?: c.kind) == c.kind }
                    val seedTracks = (listOfNotNull(recent.firstOrNull { it in unused }) + unused.take(40).shuffled().take(2)).distinctBy { it.videoId }
                    val seeds = seedTracks.map { it.videoId }
                    val radios = seedTracks.map { s -> async { s to runCatching { api.radio(s.videoId) }.getOrDefault(emptyList()) } }
                    val token = c.token
                    val page = async { token?.let { runCatching { api.more(com.rymplayer.data.MusicApi.More.Home(it)) }.getOrNull() } }
                    val known = (c.liked + c.history + recent).map { it.videoId }.toSet()
                    val cands = mutableListOf<com.rymplayer.data.RamPoints.Candidate>()
                    for ((seed, radio) in radios.awaitAll()) {
                        val kind = notes.seedKind(seed, radio)
                        notes.learn(kind, seed, radio)
                        cands += com.rymplayer.data.RamPoints.fromRadio(radio, kind, 10)
                    }
                    val next = page.await()
                    val pageItems = next?.tracks.orEmpty().filter { t -> (notes.kindOf(t) ?: c.kind) == c.kind }
                    pageItems.filter { it.link == null }.forEach { cands += com.rymplayer.data.RamPoints.Candidate(it, 14, null, false) }
                    (Recent.favourites(ctx, 40) + c.liked.take(150)).shuffled().take(60).forEach { cands += com.rymplayer.data.RamPoints.Candidate(it, 4, null, true) }
                    val topArtists = Recent.favourites(ctx, 40).map { com.rymplayer.data.Radio.firstArtist(it.artist) }.filter { it.isNotEmpty() }.toSet()
                    val mixed = com.rymplayer.data.RamPoints.forYou(notes, cands, c.kind, 30, c.shown, known, topArtists, FeedMemory.shownLookup(ctx), { Recent.playedAt(ctx, it) })
                        .map { it.copy(album = FeedMemory.MORE_FOR_YOU) }
                    com.rymplayer.data.RamPoints.save(ctx, notes)
                    FeedMemory.seen(ctx, mixed)
                    c.token = (next?.more as? com.rymplayer.data.MusicApi.More.Home)?.token
                    val mixedKeys = mixed.map(FeedMemory::key).toSet()
                    val shelves = pageItems.filter { FeedMemory.key(it) !in c.shown && FeedMemory.key(it) !in mixedKeys }
                    c.used += seeds
                    (mixed + shelves).also { added -> c.shown += added.map(FeedMemory::key) }
                }
            }.onSuccess { added ->
                home = home + added
                if (added.isEmpty() && c.token == null) homeCursor = null
                com.rymplayer.playback.Streams.prefetchLinks(added.filter { it.link == null }.take(4).map { it.videoId })
            }.onFailure { android.util.Log.w("RyM", "more for you", it) }
            loadingMore = false
        }
    }

    /** Where each open list goes on (search results, playlists). */
    private val pageMore = HashMap<Screen.Songs, com.rymplayer.data.MusicApi.More>()

    /** The next part of a list near its bottom (nothing already in it). */
    fun loadMore(page: Screen.Songs) {
        val more = pageMore[page] ?: return
        if (loadingMore) return
        loadingMore = true
        viewModelScope.launch {
            runCatching { api.more(more) }.onSuccess { part ->
                val have = pages[page].orEmpty().map { it.videoId }.toMutableSet()
                val fresh = part.tracks.filter { have.add(it.videoId) }
                pages[page] = pages[page].orEmpty() + fresh
                if (part.more != null) pageMore[page] = part.more else pageMore.remove(page)
                com.rymplayer.playback.Streams.prefetchLinks(fresh.take(4).map { it.videoId })
            }.onFailure { android.util.Log.w("RyM", "more for ${page.kind}", it) }
            loadingMore = false
        }
    }

    private var homeJob: Job? = null

    // ---------------------------------------------------------------- liked songs, kept in step

    /** Likes made here lately, with when: they win over a synced list YouTube may not have updated yet. */
    private val localLikes = HashMap<String, Pair<Boolean, Long>>()
    private var syncJob: Job? = null

    /** The account's liked songs as YouTube Music has them now (likes from the PC show up here). */
    /** The liked songs as last read (mixed into Up next). */
    private var likedTracks: List<Track> = emptyList()

    private fun syncLiked(songs: List<Track>) {
        likedTracks = songs
        val now = System.currentTimeMillis()
        localLikes.entries.removeAll { now - it.value.second > 60_000 }
        val ids = LinkedHashSet(songs.map { it.videoId })
        var shown = songs
        for ((id, change) in localLikes) {
            if (change.first) ids += id else { ids -= id; shown = shown.filter { it.videoId != id } }
        }
        if (liked.toSet() != ids) {
            liked.clear()
            liked += ids
        }
        likedPages().forEach { pages[it] = shown }
    }

    private fun likedPages() = pages.keys.filter { it.kind == Screen.Kind.Playlist && it.id == LIKED_ID }

    fun syncLikesNow() {
        if (!api.signedIn || syncJob?.isActive == true) return
        syncJob = viewModelScope.launch { runCatching { api.playlist(LIKED_ID) }.onSuccess(::syncLiked) }
    }

    /**
     * The sung line, moved exactly when the next line is due (one wake-up per line, not a timer
     * ticking all the time), and only while the app is on screen and playing; a seek, a pause or
     * new lyrics start it over. It starts once the view model is fully made (on the next turn of
     * the main thread): started at once from init, it set [lyricLine] before that existed.
     */
    private fun followLyrics() = viewModelScope.launch(kotlinx.coroutines.Dispatchers.Main) {
        androidx.compose.runtime.snapshotFlow { listOf(lyrics, player.playing, player.visible, player.seeks, player.current?.videoId) }
            .collectLatest {
                val synced = lyrics?.synced.orEmpty()
                if (synced.isEmpty()) {
                    lyricLine = null
                    return@collectLatest
                }
                while (true) {
                    val pos = player.exactPosition()
                    val i = Lrclib.currentLine(synced, pos)
                    lyricLine = i
                    if (!player.playing || !player.visible) break
                    val next = synced.getOrNull((i ?: -1) + 1)?.first ?: break
                    delay(((next - pos) / player.speed.coerceAtLeast(0.1f) * 1000).toLong().coerceIn(30L, 10_000L) + 15)
                }
            }
    }

    private var foregroundJob: Job? = null

    /** App on screen: liked songs are re-read every two minutes (and right away). */
    fun onForeground() {
        player.visible = true
        refreshEco()
        checkScheduledReset()
        // Seamless Music: opening RyM here takes the PC's music (if it's playing there).
        player.takeFromPc()
        // RyM Link: the account's key, so a PC signed in to it can take the music over.
        if (com.rymplayer.playback.Link.account == null && api.signedIn) {
            viewModelScope.launch {
                runCatching { api.account() }
                    .onFailure { android.util.Log.w("RyM", "link: no account (${it.message})") }
                    .getOrNull()?.let { (name, handle) ->
                        com.rymplayer.playback.Link.account = com.rymplayer.playback.Link.accountKey(name, handle)
                        android.util.Log.w("RyM", "link: account key ${com.rymplayer.playback.Link.account}")
                    }
            }
        }
        if (foregroundJob?.isActive == true) return
        foregroundJob = viewModelScope.launch {
            while (isActive) {
                syncLikesNow()
                delay(120_000)
            }
        }
    }

    fun onBackground() {
        foregroundJob?.cancel()
        // Nothing on screen to update: the position and lyrics stop following until it's back.
        player.visible = false
        player.saveSession()
        Recent.flushListening(getApplication())
    }

    private fun load(page: Screen.Songs) {
        viewModelScope.launch {
            val r = runCatching {
                when (page.kind) {
                    Screen.Kind.Playlist -> api.playlistPaged(page.id).also { p -> p.more?.let { pageMore[page] = it } }.tracks
                    Screen.Kind.Album -> api.album(page.id)
                    Screen.Kind.Mix -> api.mix(page.id)
                    Screen.Kind.Radio -> api.radio(page.id)
                    Screen.Kind.Artist -> api.artist(page.id)
                    Screen.Kind.History -> api.history()
                    Screen.Kind.Results -> api.searchPaged(page.id).also { p -> p.more?.let { pageMore[page] = it } }.tracks
                    Screen.Kind.Downloads -> Downloads.list(getApplication())
                    Screen.Kind.SoundCloud -> com.rymplayer.data.SoundCloud.search(getApplication(), page.id)
                    Screen.Kind.SoundCloudLink -> com.rymplayer.data.SoundCloud.openLink(getApplication(), page.id)
                    Screen.Kind.Spotify -> com.rymplayer.data.Spotify.import(api, page.id)
                }
            }
            r.onFailure {
                android.util.Log.w("RyM", "loading ${page.kind} failed", it)
                status = it.message ?: it.javaClass.simpleName
            }
            pages[page] = r.getOrDefault(emptyList())
            com.rymplayer.playback.Streams.prefetchLinks(pages[page].orEmpty().filter { it.link == null }.take(8).map { it.videoId })
        }
    }

    // ---------------------------------------------------------------- playback

    /** Plays row `index` of a list; the queue becomes the list's songs. */
    fun playFrom(list: List<Track>, index: Int) {
        val songs = list.filter { it.link == null }
        val start = songs.indexOfFirst { it.videoId == list[index].videoId }.coerceAtLeast(0)
        // What was played stays above, so Previous goes back to it.
        player.start(songs, start)
    }

    /** One song, and YouTube Music's radio after it. */
    fun playRadio(t: Track) {
        FeedMemory.opened(getApplication(), t)
        // What was played stays above; Up next under it is made for this song.
        player.start(listOf(t), 0)
        viewModelScope.launch {
            runCatching { com.rymplayer.data.Radio.forTrack(api, t) }.getOrNull()?.let { radio -> player.append(player.tidy(radio, t)) }
        }
    }

    /** Download connections at once (1-16) and piece size in MB (0 = open: split evenly). */
    var connections by mutableStateOf(prefs.getInt("dl_connections", 8))
        private set
    var pieceMb by mutableStateOf(prefs.getInt("dl_piece_mb", 0))
        private set

    fun nextConnections() {
        val steps = listOf(1, 2, 4, 6, 8, 10, 12, 16)
        connections = steps[(steps.indexOf(connections).coerceAtLeast(0) + 1) % steps.size]
        prefs.edit().putInt("dl_connections", connections).apply()
        com.rymplayer.playback.Streams.connections = connections
    }

    fun nextPieceSize() {
        val steps = listOf(0, 1, 2, 4, 8)
        pieceMb = steps[(steps.indexOf(pieceMb).coerceAtLeast(0) + 1) % steps.size]
        prefs.edit().putInt("dl_piece_mb", pieceMb).apply()
        com.rymplayer.playback.Streams.pieceMb = pieceMb
    }

    /** The app's words in Arabic instead of English (Library > Language). */
    fun toggleArabic() {
        I18n.arabic = !I18n.arabic
        prefs.edit().putBoolean("arabic", I18n.arabic).apply()
    }

    /** Play songs in VLC instead of the built-in player (only offered when VLC is installed). */
    var playInVlc by mutableStateOf(prefs.getBoolean("vlc", false))
        private set

    fun togglePlayInVlc() {
        playInVlc = !playInVlc
        prefs.edit().putBoolean("vlc", playInVlc).apply()
        player.useVlc = playInVlc
    }

    fun openInVlc(t: Track) = viewModelScope.launch {
        runCatching { com.rymplayer.playback.Vlc.open(getApplication(), t) }.onFailure { status = "${tr("VLC couldn't open it")}: ${it.message}" }
    }

    /** Downloads in the best quality (yt-dlp, slow on a phone) instead of the playback file. */
    var downloadBest by mutableStateOf(prefs.getBoolean("download_best", false))
        private set

    fun toggleDownloadQuality() {
        downloadBest = !downloadBest
        prefs.edit().putBoolean("download_best", downloadBest).apply()
    }

    /** Saves a song to Music/RymPlayer; it plays from there afterwards. */
    fun download(t: Track) = viewModelScope.launch {
        status = "${tr("Downloading")} \"${t.title}\"..."
        status = runCatching { Downloads.save(getApplication(), t, com.rymplayer.data.SecureCookie.read(prefs), downloadBest) }.fold({ tr("Saved to Music/RymPlayer") }, {
            android.util.Log.w("RyM", "download of ${t.videoId} failed", it)
            "${tr("Download failed")}: ${it.message ?: it.javaClass.simpleName}"
        })
    }

    /** The artist's page (every song, then albums and singles); found by name when the song lacks the id. */
    fun goArtist(t: Track) {
        if (t.artist.isBlank()) return
        val name = t.artist.substringBefore(", ")
        playerOpen = false
        t.artistId?.let { open(Screen.Songs(Screen.Kind.Artist, it, name)); return }
        status = tr("Finding the artist...")
        viewModelScope.launch {
            val id = runCatching { api.artistIdFor(name) }.getOrNull()
            if (id == null) status = tr("Artist not found") else open(Screen.Songs(Screen.Kind.Artist, id, name))
        }
    }

    /** How the liked songs batch download is going (null: not started). */
    data class Batch(val done: Int, val total: Int, val saved: Int = 0, val skipped: Int = 0, val failed: Int = 0) {
        val finished get() = done >= total
    }

    var batch by mutableStateOf<Batch?>(null)
        private set

    /** Downloads the first [limit] liked songs, one after another, skipping ones already saved. */
    fun downloadAll(limit: Int) {
        if (batch?.finished == false || !api.signedIn) return
        batch = Batch(0, 0)
        viewModelScope.launch {
            val songs = runCatching { api.playlistAll(LIKED_ID, limit) }.getOrElse {
                status = "${tr("Download failed")}: ${it.message}"
                batch = null
                return@launch
            }
            var b = Batch(0, songs.size)
            batch = b
            for (t in songs) {
                val r = runCatching { Downloads.save(getApplication(), t, com.rymplayer.data.SecureCookie.read(prefs), downloadBest) }
                b = when {
                    r.getOrNull() == "already saved" -> b.copy(skipped = b.skipped + 1)
                    r.isSuccess -> b.copy(saved = b.saved + 1)
                    else -> b.copy(failed = b.failed + 1)
                }
                b = b.copy(done = b.done + 1)
                batch = b
            }
        }
    }

    fun toggleLike(t: Track) {
        val like = t.videoId !in liked
        setLiked(t, like)
        viewModelScope.launch {
            runCatching { api.rate(t.videoId, if (like) MusicApi.Rating.Like else MusicApi.Rating.None) }.onFailure {
                setLiked(t, !like)
                status = tr("Couldn't save the like")
            }
        }
    }

    /** Hearts and the Liked Music page follow at once. */
    private fun setLiked(t: Track, like: Boolean) {
        localLikes[t.videoId] = like to System.currentTimeMillis()
        if (like) { if (t.videoId !in liked) liked += t.videoId } else liked -= t.videoId
        for (key in likedPages()) {
            val list = pages[key].orEmpty()
            pages[key] = if (like) (if (list.none { it.videoId == t.videoId }) listOf(t) + list else list) else list.filter { it.videoId != t.videoId }
        }
    }

    fun addToPlaylist(p: Playlist, t: Track) = viewModelScope.launch {
        status = runCatching { api.addToPlaylist(p.id, t.videoId) }.fold({ "${tr("Added to")} \"${p.title}\"" }, { "${tr("Couldn't add")}: ${it.message}" })
    }

    fun createPlaylist(title: String, with: Track?) = viewModelScope.launch {
        runCatching { api.createPlaylist(title) }.onSuccess { p ->
            playlists = listOf(p) + playlists
            status = "Created \"$title\""
            if (with != null) addToPlaylist(p, with)
        }.onFailure { status = tr("Couldn't create the playlist") }
    }

    private fun loadLyrics(t: Track?) {
        lyricsJob?.cancel()
        sourcesJob?.cancel()
        sourcesFor = null
        lyricsSources = emptyList()
        lookingForSources = false
        wantedSource = null
        lyrics = null
        translateJob?.cancel()
        lyricsTranslation = emptyList()
        if (t == null) return
        lyricsStatus = tr("Looking for lyrics...")
        lyricsJob = viewModelScope.launch {
            val found = findLyrics(t)
            lyrics = found
            lyricsStatus = if (found == null) tr("No lyrics for this song") else ""
            translateShown()
            // Every source is asked right away, so the source buttons switch at once.
            loadLyricsSources()
            // The next song's lyrics are ready before it starts.
            player.queue.getOrNull(player.index + 1)?.let { next ->
                if (LyricsCache.get(getApplication(), next.videoId) == null) findLyrics(next)
            }
        }
    }

    /** Saved lyrics, or LRCLIB and YouTube Music asked at once (LRCLIB's preferred: it has timing). */
    private suspend fun findLyrics(t: Track): Lyrics? {
        val ctx = getApplication<Application>()
        LyricsCache.get(ctx, t.videoId)?.let { return it.lyrics }
        // The first synced lyrics in the order LRCLIB, NetEase, KuGou (all asked at once; a later one
        // is waited for only while the earlier ones had nothing synced), then plain ones: LRCLIB's,
        // YouTube Music's, Genius's.
        return coroutineScope {
            val ytm = async { if (com.rymplayer.data.isYouTube(t.videoId)) runCatching { api.lyrics(t.videoId) }.getOrNull() else null }
            val netease = async { runCatching { LyricSources.netease(t) }.getOrNull() }
            val kugou = async { runCatching { LyricSources.kugou(t) }.getOrNull() }
            val lrc = Lrclib.find(t.title, t.artist, t.album, parseDuration(t.duration))
            val synced = lrc?.takeIf { it.synced.isNotEmpty() }
                ?: netease.await()?.takeIf { it.synced.isNotEmpty() }
                ?: kugou.await()?.takeIf { it.synced.isNotEmpty() }
            val found = synced ?: lrc ?: ytm.await() ?: netease.await() ?: kugou.await() ?: runCatching { LyricSources.genius(t) }.getOrNull()
            ytm.cancel()
            netease.cancel()
            kugou.cancel()
            LyricsCache.put(ctx, t.videoId, found)
            found
        }
    }

    /** Asks every source for the playing song's lyrics (once per song): LRCLIB's matches, NetEase, KuGou, Genius, YouTube Music. */
    fun loadLyricsSources() {
        val t = player.current ?: return
        if (sourcesFor == t.videoId) return
        sourcesFor = t.videoId
        lookingForSources = true
        sourcesJob = viewModelScope.launch {
            val found = coroutineScope {
                val lrc = async { Lrclib.all(t.title, t.artist, parseDuration(t.duration)) }
                val ytm = async { if (com.rymplayer.data.isYouTube(t.videoId)) runCatching { api.lyrics(t.videoId) }.getOrNull() else null }
                val others = listOf(
                    async { runCatching { LyricSources.netease(t) }.getOrNull() },
                    async { runCatching { LyricSources.kugou(t) }.getOrNull() },
                    async { runCatching { LyricSources.genius(t) }.getOrNull() },
                )
                // The lyrics shown now stay first, so they can be picked again.
                listOfNotNull(lyrics) + lrc.await() + others.awaitAll().filterNotNull() + listOfNotNull(ytm.await())
            }
            lyricsSources = Lrclib.distinct(found)
            lookingForSources = false
            wantedSource?.let { wantedSource = null; pickSource(it) }
        }
    }

    /** A source pill pressed before the sources were in: shown once they are. */
    private var wantedSource: String? = null

    /** A source pill: that provider's lyrics (its next version when it already shows); the sources are asked first when they aren't in. */
    fun pickSource(p: String) {
        val t = player.current ?: return
        if (sourcesFor != t.videoId || lookingForSources) {
            wantedSource = p
            loadLyricsSources()
            return
        }
        val versions = lyricsSources.filter { LyricSources.provider(it) == p }
        if (versions.isEmpty()) return
        val at = versions.indexOfFirst { it == lyrics }
        showLyricsFrom(versions[if (at < 0) 0 else (at + 1) % versions.size])
    }

    /** Shows the lyrics picked in the switcher, and keeps them for the song next time. */
    fun showLyricsFrom(l: Lyrics) {
        val t = player.current ?: return
        lyrics = l
        lyricsStatus = ""
        LyricsCache.put(getApplication(), t.videoId, l)
        translateShown()
    }

    /** English under the lines shown (Japanese songs), or nothing. */
    var lyricsTranslation by mutableStateOf<List<String>>(emptyList())
        private set

    /** Japanese lyrics with English under each line (the setting). */
    var translateLyrics by mutableStateOf(app.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("lyrics_translation", true))
        private set

    fun toggleTranslateLyrics() {
        translateLyrics = !translateLyrics
        getApplication<Application>().getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("lyrics_translation", translateLyrics).apply()
        translateShown()
    }

    private var translateJob: Job? = null

    /** The lyrics shown get their English: saved with them, or looked for (the site's own, else a machine translation). */
    private fun translateShown() {
        translateJob?.cancel()
        val l = lyrics
        val t = player.current
        lyricsTranslation = if (translateLyrics) l?.translation.orEmpty() else emptyList()
        if (!translateLyrics || l == null || t == null || l.translation.isNotEmpty() || !com.rymplayer.data.Translate.isJapanese(l)) return
        translateJob = viewModelScope.launch {
            val tr = com.rymplayer.data.Translate.translate(t, l)
            if (tr.isNotEmpty() && lyrics === l) {
                l.translation = tr
                lyricsTranslation = tr
                LyricsCache.put(getApplication(), t.videoId, l)
            }
        }
    }

    override fun onCleared() {
        player.release()
    }
}

