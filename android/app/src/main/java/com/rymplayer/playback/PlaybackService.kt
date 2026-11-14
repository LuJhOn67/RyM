package com.rymplayer.playback

import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import kotlinx.coroutines.launch
import androidx.media3.session.MediaSessionService

/**
 * The queue lives in the app's [com.rymplayer.playback.Player], which hands the media player one
 * song at a time. The notification's and lock screen's next/previous buttons ask it through here.
 */
object QueueBridge {
    @Volatile var next: (() -> Unit)? = null
    @Volatile var prev: (() -> Unit)? = null
    /** The focus timer's break: the music softer (1 = as set). */
    @Volatile var duck = 1f
    /** The duck changed: the service applies it (set by the service). */
    @Volatile var duckChanged: (() -> Unit)? = null
    /** A song comes after the playing one (the automix needs one to mix into). */
    @Volatile var hasNext: (() -> Boolean)? = null
    /**
     * The automix: the app moves its queue on to the next song and hands over that song's media
     * item (null when it can't, and then nothing moved), for the service to start itself.
     */
    @Volatile var mixNext: (((MediaItem?) -> Unit) -> Unit)? = null
    /** The playing song or the next one is Quran or a talk: they play whole, without a blend. */
    @Volatile var playsWhole: (() -> Boolean)? = null
    /** The sung line, for the lock screen and the notification (null: none, or turned off). */
    @Volatile var lyric: String? = null
    /** The line changed: the service shows it (set by the service, called on the main thread). */
    @Volatile var lyricChanged: (() -> Unit)? = null
    /** Android Auto or the assistant started a list on the player: the app's queue becomes it. */
    @Volatile var adopt: ((List<com.rymplayer.data.Track>, Int) -> Unit)? = null
    /** A song was picked by hand: a mix that hasn't started sounding yet is dropped. */
    @Volatile var cancelMix: (() -> Unit)? = null
}

/**
 * When the network drops mid-song, loading tries again (1, 2, 4, 8 then every 10 s, about two
 * minutes in all) and the song goes on once it's back. A refused or gone link (4xx) isn't retried.
 */
@OptIn(UnstableApi::class)
private class KeepTrying : androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy() {
    override fun getRetryDelayMsFor(info: androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo): Long {
        val e = info.exception
        if (e is androidx.media3.datasource.HttpDataSource.InvalidResponseCodeException && e.responseCode in 400..499) return C.TIME_UNSET
        if (e is androidx.media3.datasource.HttpDataSource.HttpDataSourceException) return minOf(1000L shl minOf(info.errorCount - 1, 3), 10_000L)
        return super.getRetryDelayMsFor(info)
    }

    override fun getMinimumLoadableRetryCount(dataType: Int) = 16
}

/**
 * Plays in the background and shows Android's media notification (with lock-screen controls).
 *
 * Two players: the one the media session shows, and the other, used by the smart automix. When a
 * mix starts, the session moves to the other player and the app starts the next song on it, while
 * the first one plays on, fading out with its bass taken away; both sound at once.
 */
class PlaybackService : androidx.media3.session.MediaLibraryService() {
    private var session: androidx.media3.session.MediaLibraryService.MediaLibrarySession? = null
    /** Android Auto's and the assistant's searches. */
    private val libraryScope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Main)
    private val players = arrayOfNulls<ExoPlayer>(2)
    private val meters = arrayOf(Automix.Meter(), Automix.Meter())
    private val effects = arrayOfNulls<SoundEffects>(2)
    /** The player the session shows. */
    private var active = 0

    /** When the music paused because headphones or Bluetooth went away (0: it didn't). */
    private var pausedByUnplug = 0L

    /** Headphones or Bluetooth back within half an hour of pausing for them: the music goes on. */
    private val plugged = object : android.media.AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out android.media.AudioDeviceInfo>) {
            val listening = setOf(
                android.media.AudioDeviceInfo.TYPE_WIRED_HEADPHONES, android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET,
                android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, android.media.AudioDeviceInfo.TYPE_USB_HEADSET,
                android.media.AudioDeviceInfo.TYPE_BLE_HEADSET,
            )
            val at = pausedByUnplug
            if (at == 0L || SystemClock.elapsedRealtime() - at > 30 * 60 * 1000L) return
            if (added.none { it.isSink && it.type in listening }) return
            pausedByUnplug = 0L
            // A moment for the sound to switch to them first.
            handler.postDelayed({ players[active]?.play() }, 800)
        }
    }
    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
    /** Sound settings changed in the app: applied at once. */
    private val soundChanged = android.content.SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> applySound() }
    private val attributes = AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build()

    /** A smart automix under way: the song going out, the one coming in. */
    private class Mix(
        val old: ExoPlayer,
        val oldMeter: Automix.Meter,
        val new: ExoPlayer,
        val newMeter: Automix.Meter,
        val ms: Long,
        val outBpm: Float?,
    ) {
        val asked = SystemClock.elapsedRealtime()
        /** When the incoming song started sounding (0: not yet). */
        var began = 0L
        /** The app asked the incoming song to play; its start was timed to a beat. */
        var playAsked = false
        var startTimed = false
        /** The tempo nudge given to the incoming song (null: not decided yet). */
        var nudge: Float? = null
    }

    private var mix: Mix? = null
    /** After a tempo nudge: back to the set speed, from that factor, since then. */
    private var easing: Pair<Long, Float>? = null

    @OptIn(UnstableApi::class)
    override fun onCreate() {
        super.onCreate()
        // The focus timer's break: the volume eases down at once (the fades loop keeps it there).
        QueueBridge.duckChanged = { handler.post { players.forEach { p -> p?.volume = QueueBridge.duck }; handler.removeCallbacks(fades); handler.post(fades) } }
        watchScreenOff()
        Streams.loadSource(this)
        for (i in 0..1) players[i] = build(i)
        Sound.listen(this, soundChanged)
        applySound()
        (getSystemService(AUDIO_SERVICE) as? android.media.AudioManager)?.registerAudioDeviceCallback(plugged, handler)
        // Tapping the notification (or the lock screen's player) opens RyM on the playing song.
        val open = android.app.PendingIntent.getActivity(
            this,
            0,
            android.content.Intent(this, com.rymplayer.MainActivity::class.java)
                .putExtra(com.rymplayer.MainActivity.OPEN_PLAYER, true)
                .addFlags(android.content.Intent.FLAG_ACTIVITY_SINGLE_TOP),
            android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
        )
        // Started by Android Auto or the assistant without the app: the account and source.
        if (Streams.cookie == null) Streams.cookie = com.rymplayer.data.SecureCookie.read(getSharedPreferences("rym", MODE_PRIVATE))
        Streams.loadSource(this)
        QueueBridge.lyricChanged = { handler.post { lyricViews.forEach { it() } } }
        // The home-screen widget follows the song and play / pause.
        players.forEach { it?.addListener(widgetFollows) }
        showOnWidget()
        session = androidx.media3.session.MediaLibraryService.MediaLibrarySession.Builder(this, wrap(players[0]!!), Library(this, libraryScope)).setSessionActivity(open).build()
        QueueBridge.cancelMix = { handler.post { cancelMix() } }
        // RyM Link: the PCs on this Wi-Fi hear what plays here.
        Link.appContext = applicationContext
        Link.start(android.os.Build.MODEL)
    }

    @OptIn(UnstableApi::class)
    private fun build(i: Int): ExoPlayer {
        // Web links stream in 4 MB ranges (see RangedHttpDataSource); files play directly.
        // Start after half a second of audio instead of the default 2.5 s; songs stream fast enough for it.
        val load = DefaultLoadControl.Builder().setBufferDurationsMs(15_000, 50_000, 500, 2_000).build()
        // The automix's measurements and bass filter sit in the player's sound chain.
        val renderers = object : DefaultRenderersFactory(this) {
            override fun buildAudioSink(context: android.content.Context, enableFloatOutput: Boolean, enableAudioTrackPlaybackParams: Boolean): AudioSink =
                DefaultAudioSink.Builder(context)
                    .setEnableFloatOutput(enableFloatOutput)
                    .setEnableAudioTrackPlaybackParams(enableAudioTrackPlaybackParams)
                    .setAudioProcessors(arrayOf(Automix.Processor(meters[i])))
                    .build()
        }
        val exo = ExoPlayer.Builder(this, renderers)
            .setLoadControl(load)
            .setMediaSourceFactory(
                DefaultMediaSourceFactory(
                    // rym://song/<id> (Android Auto, the assistant): the song's link is looked up as it loads.
                    androidx.media3.datasource.ResolvingDataSource.Factory(DefaultDataSource.Factory(this, WebDataSource.Factory())) { spec ->
                        if (spec.uri.scheme == "rym") spec.withUri(android.net.Uri.parse(Streams.resolve(spec.uri.lastPathSegment.orEmpty()))) else spec
                    },
                ).setLoadErrorHandlingPolicy(KeepTrying()),
            )
            // Only the session's player holds the audio focus (the other would take it away).
            .setAudioAttributes(attributes, i == 0)
            .setHandleAudioBecomingNoisy(true)
            // Keeps Wi-Fi awake only while streaming, and the CPU only while playing.
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        effects[i] = SoundEffects(exo)
        exo.addListener(object : Player.Listener {
            override fun onAudioSessionIdChanged(audioSessionId: Int) = applySound()
            // Headphones or Bluetooth went away and the music paused: noted, so it goes on when they come back.
            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                pausedByUnplug = if (!playWhenReady && reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY) SystemClock.elapsedRealtime() else 0L
            }
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                val m = mix
                if (isPlaying && m != null && m.new === exo && m.began == 0L) {
                    m.began = SystemClock.elapsedRealtime()
                    moveSession(m)
                }
                handler.removeCallbacks(fades)
                if (isPlaying || mix != null) handler.post(fades)
            }
            override fun onPlaybackStateChanged(state: Int) {
                if (state == Player.STATE_READY) startOnBeat(exo)
            }
            // Where the song's loudness blocks start counting (for the beats).
            override fun onPositionDiscontinuity(oldPosition: Player.PositionInfo, newPosition: Player.PositionInfo, reason: Int) {
                meters[i].anchorMs = newPosition.positionMs
            }
            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                meters[i].anchorMs = exo.currentPosition
            }
        })
        return exo
    }

    /** The session's view of a player: next/previous go to the app's queue; a mix holds its start for a beat. */
    private fun wrap(exo: ExoPlayer): Player {
        val skips = listOf(
            Player.COMMAND_SEEK_TO_NEXT,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        )
        fun held() = mix?.let { it.new === exo && it.began == 0L } == true
        return object : ForwardingPlayer(exo) {
            // The session's listeners, to tell them when the sung line changes.
            private val listening = java.util.concurrent.CopyOnWriteArraySet<Player.Listener>()
            init {
                lyricViews += { listening.forEach { l -> l.onMediaMetadataChanged(mediaMetadata) } }
            }
            override fun addListener(listener: Player.Listener) {
                super.addListener(listener)
                listening += listener
            }
            override fun removeListener(listener: Player.Listener) {
                super.removeListener(listener)
                listening -= listener
            }
            // Lyrics on the lock screen and in the notification: the sung line under the title,
            // where the artist is (the artist comes back between lines and after the song).
            override fun getMediaMetadata(): androidx.media3.common.MediaMetadata {
                val m = super.getMediaMetadata()
                val line = QueueBridge.lyric?.takeIf { it.isNotBlank() } ?: return m
                return m.buildUpon().setArtist("♪ $line").setSubtitle(m.artist).build()
            }
            override fun getAvailableCommands(): Player.Commands = super.getAvailableCommands().buildUpon().addAll(*skips.toIntArray()).build()
            override fun isCommandAvailable(command: Int) = command in skips || super.isCommandAvailable(command)
            override fun seekToNext() { QueueBridge.next?.invoke() ?: super.seekToNext() }
            override fun seekToNextMediaItem() { QueueBridge.next?.invoke() ?: super.seekToNextMediaItem() }
            override fun seekToPrevious() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun seekToPreviousMediaItem() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun play() {
                if (held()) askToStart(exo) else super.play()
            }
            override fun setPlayWhenReady(playWhenReady: Boolean) {
                if (playWhenReady && held()) askToStart(exo) else super.setPlayWhenReady(playWhenReady)
                if (!playWhenReady) endMix()
            }
            // Pausing in the middle of a mix ends it: both songs stop.
            override fun pause() {
                super.pause()
                mix?.new?.pause()
                endMix()
            }
        }
    }

    private val widgetFollows = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) {
            if (events.containsAny(Player.EVENT_MEDIA_METADATA_CHANGED, Player.EVENT_IS_PLAYING_CHANGED, Player.EVENT_MEDIA_ITEM_TRANSITION)) showOnWidget()
        }
    }
    private var widgetCoverFor: android.net.Uri? = null
    private var widgetCover: android.graphics.Bitmap? = null

    /** The widget shows the session's song (its cover loaded once per song). */
    private fun showOnWidget() {
        val p = players[active] ?: return
        val m = p.mediaMetadata
        val playing = p.isPlaying || (mix?.new?.isPlaying == true)
        val art = m.artworkUri
        com.rymplayer.playback.PlayerWidget.show(this, m.title?.toString(), m.artist?.toString(), playing, widgetCover.takeIf { art != null && art == widgetCoverFor })
        if (art != null && art != widgetCoverFor) {
            widgetCoverFor = art
            libraryScope.launch {
                val bmp = runCatching {
                    val req = coil.request.ImageRequest.Builder(this@PlaybackService).data(art).size(256).allowHardware(false).build()
                    (coil.Coil.imageLoader(this@PlaybackService).execute(req).drawable as? android.graphics.drawable.BitmapDrawable)?.bitmap
                }.getOrNull()
                if (widgetCoverFor == art) {
                    widgetCover = bmp
                    val now = players[active] ?: return@launch
                    com.rymplayer.playback.PlayerWidget.show(this@PlaybackService, now.mediaMetadata.title?.toString(), now.mediaMetadata.artist?.toString(), now.isPlaying, bmp)
                }
            }
        }
    }

    /** Each wrapped player's way to show a new sung line. */
    private val lyricViews = mutableListOf<() -> Unit>()

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): androidx.media3.session.MediaLibraryService.MediaLibrarySession? = session

    private fun applySound() {
        val s = Sound.load(this)
        Automix.Levels.vocalsOff = s.vocalsOff
        effects.forEach { it?.apply(s) }
        if (players.any { it?.isPlaying == true }) {
            handler.removeCallbacks(fades)
            handler.post(fades)
        }
    }

    private fun askToStart(exo: ExoPlayer) {
        val m = mix ?: return
        m.playAsked = true
        startOnBeat(exo)
    }

    /**
     * The incoming song, once loaded and asked to play, starts on the outgoing song's next beat
     * (when it has a clear one within 1.2 s; at once otherwise).
     */
    private fun startOnBeat(exo: ExoPlayer) {
        val m = mix ?: return
        if (m.new !== exo || !m.playAsked || m.startTimed || exo.playbackState != Player.STATE_READY) return
        m.startTimed = true
        val pos = m.old.currentPosition
        val speed = m.old.playbackParameters.speed.coerceAtLeast(0.1f)
        val wait = m.oldMeter.nextBeatMs(pos, 150)
            ?.let { (at, _) -> ((at - pos) / speed).toLong() - START_LATENCY_MS }
            ?.takeIf { it in 0..1200 } ?: 0L
        android.util.Log.w("RyM", "automix: next song in ${if (wait > 0) "on a beat, $wait ms" else "now"}")
        handler.postDelayed({ if (mix === m) exo.play() }, wait)
    }

    /** The song going out stops, the incoming one plays on alone. */
    private fun endMix(why: String = "paused") {
        val m = mix ?: return
        mix = null
        android.util.Log.w("RyM", "automix: done ($why) after ${SystemClock.elapsedRealtime() - m.asked} ms (nudge ${m.nudge})")
        // The next song is the one that goes on now, sounding or not.
        moveSession(m)
        m.old.stop()
        m.old.clearMediaItems()
        m.oldMeter.highpass = 0f
        val s = Sound.load(this)
        m.new.skipSilenceEnabled = s.skipSilence
        m.new.volume = 1f
        m.nudge?.takeIf { it != 1f }?.let { easing = SystemClock.elapsedRealtime() to it }
    }

    /**
     * The media session moves to the incoming player, once it sounds (or the mix ends). Moved
     * while it was still silent, the service would drop out of the foreground for a moment, and
     * with the screen off Android doesn't let it back in: the music stops on many phones.
     */
    private fun moveSession(m: Mix) {
        val i = players.indexOf(m.new)
        if (i < 0 || i == active) return
        active = i
        session?.player = wrap(m.new)
    }

    /** A song picked by hand while the next one is still loading for a mix: that one is dropped. */
    private fun cancelMix() {
        val m = mix ?: return
        if (m.began != 0L) return
        mix = null
        android.util.Log.w("RyM", "automix: dropped (a song was picked)")
        m.new.stop()
        m.new.clearMediaItems()
        m.old.volume = 1f
        m.oldMeter.highpass = 0f
        m.new.setAudioAttributes(attributes, false)
        m.old.setAudioAttributes(attributes, true)
    }

    /**
     * Starts a smart automix: the app moves its queue on and hands over the next song, which the
     * service loads on the other player and starts on a beat; the session moves there once it
     * sounds, while this one plays on, fading out (see [fades]).
     */
    private fun startMix(s: Sound.Settings) {
        val old = players[active] ?: return
        val i = 1 - active
        val new = players[i] ?: return
        val ask = QueueBridge.mixNext ?: return
        new.stop()
        new.clearMediaItems()
        new.volume = 0f
        // Cue-in: the next song's silent start is skipped, unless "Skip silence" is off.
        new.skipSilenceEnabled = s.skipSilence
        // The audio focus goes to the incoming player (the outgoing one would be paused by it).
        old.setAudioAttributes(attributes, false)
        new.setAudioAttributes(attributes, true)
        val outBeat = Automix.beat(meters[active].last(12f).first)
        mix = Mix(old, meters[active], new, meters[i], (Automix.mixSecs(s.crossfade) * 1000).toLong(), outBeat?.bpm)
        android.util.Log.w("RyM", "automix: start at ${old.currentPosition} of ${old.duration} ms, tempo ${outBeat?.bpm}")
        easing = null
        val m = mix
        ask { item ->
            if (mix !== m || m == null) return@ask
            if (item == null) {
                // No next song after all: this one plays to its end as usual.
                mix = null
                android.util.Log.w("RyM", "automix: no next song")
                new.setAudioAttributes(attributes, false)
                old.setAudioAttributes(attributes, true)
                return@ask
            }
            new.setMediaItem(item)
            new.prepare()
            m.playAsked = true
            startOnBeat(new)
        }
    }

    /**
     * Crossfade, the smart automix and the sleep timer (the last 10 seconds fade, then the music
     * pauses). Runs only while playing (or mixing), a few times a second when something is near,
     * otherwise it sleeps until something could start.
     */
    private val fades: Runnable = object : Runnable {
        override fun run() {
            val p = players[active] ?: return
            val m = mix
            if (!p.isPlaying && m == null) return
            val s = Sound.load(this@PlaybackService)
            var wait = 5_000L
            // The sleep timer's fade, on top of everything.
            var sleep = 1f
            if (s.sleepAt != 0L) {
                val dur = p.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: 0L
                val left = if (s.sleepAt > 0) s.sleepAt - System.currentTimeMillis()
                    else if (dur > 0) ((dur - p.currentPosition) / s.speed.coerceAtLeast(0.1f)).toLong() else Long.MAX_VALUE
                // Ran out long ago (while nothing played): just cleared, it doesn't stop new music.
                if (left < -10_000) {
                    Sound.save(this@PlaybackService, s.copy(sleepAt = 0))
                    handler.post(this)
                    return
                }
                if (left <= 250) {
                    endMix("sleep timer")
                    p.pause()
                    p.volume = 1f
                    Sound.save(this@PlaybackService, s.copy(sleepAt = 0))
                    return
                }
                if (left < 10_000) {
                    sleep = left / 10_000f
                    wait = 100L
                } else wait = minOf(wait, left - 10_000)
            }
            if (m != null) {
                mixStep(m, s, sleep)
                handler.postDelayed(this, 50L)
                return
            }
            // Back to the set speed over 4 s after a tempo nudge.
            easing?.let { (since, from) ->
                val t = ((SystemClock.elapsedRealtime() - since) / 4000f).coerceIn(0f, 1f)
                p.playbackParameters = PlaybackParameters(s.speed * (from + (1f - from) * t), s.pitch)
                if (t >= 1f) easing = null
                wait = 100L
            }
            val pos = p.currentPosition
            val dur = p.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: 0L
            // A long talk plays at its own speed; songs at theirs.
            val want = if (com.rymplayer.data.TalkPlace.isLong(dur / 1000f)) s.talkSpeed else s.speed
            if (easing == null && kotlin.math.abs(p.playbackParameters.speed - want) > 0.01f) p.playbackParameters = PlaybackParameters(want, s.pitch)
            val speed = want.coerceAtLeast(0.1f)
            var volume = 1f
            val whole = QueueBridge.playsWhole?.invoke() == true
            if (whole) {
                // Quran and talks: no blend, no fade; the song plays to its end.
            } else if (s.automix) {
                // Smart automix: on the fade-out in the last 15 s, or at the mix window.
                val mixMs = (Automix.mixSecs(s.crossfade) * 1000).toLong()
                val fromEnd = ((dur - pos) / speed).toLong()
                val windowMs = (Automix.WINDOW_SECS * 1000).toLong()
                val nextSong = QueueBridge.hasNext?.invoke() == true && s.sleepAt != -1L
                if (dur > mixMs + 10_000 && nextSong) {
                    if (fromEnd <= windowMs) {
                        // The tail plays as it is (skip silence would squeeze it and cut the mix short).
                        if (p.skipSilenceEnabled) p.skipSilenceEnabled = false
                        if (fromEnd <= mixMs + Automix.LOAD_LEAD_MS || Automix.fadedOut(meters[active].last(Automix.WINDOW_SECS).first)) {
                            startMix(s)
                            handler.postDelayed(this, 50L)
                            return
                        }
                        wait = 50L
                    } else wait = minOf(wait, fromEnd - windowMs)
                }
            } else {
                // Plain crossfade on one player: the end fades out, the next song fades in.
                val fadeMs = s.crossfade * 1000L
                if (fadeMs > 0 && dur > fadeMs * 3) {
                    val fromEnd = ((dur - pos) / speed).toLong()
                    val fromStart = (pos / speed).toLong()
                    if (fromStart < fadeMs) volume = minOf(volume, fromStart.toFloat() / fadeMs)
                    if (fromEnd < fadeMs) volume = minOf(volume, fromEnd.toFloat() / fadeMs)
                    wait = if (fromStart < fadeMs || fromEnd < fadeMs) 100L else minOf(wait, fromEnd - fadeMs)
                }
            }
            p.volume = (volume * sleep * QueueBridge.duck).coerceIn(0f, 1f)
            handler.postDelayed(this, wait.coerceIn(50L, 5_000L))
        }
    }

    /** One step of a mix: equal-power gains, the outgoing song's bass going, the incoming tempo nudged. */
    private fun mixStep(m: Mix, s: Sound.Settings, sleep: Float) {
        val now = SystemClock.elapsedRealtime()
        if (m.began == 0L) {
            // Waiting for the next song to load: the old one plays on (up to 20 s, or to its end).
            m.old.volume = sleep * QueueBridge.duck
            if (now - m.asked > 20_000 || m.old.playbackState == Player.STATE_ENDED) endMix("the next song didn't come")
            return
        }
        val t = ((now - m.began).toFloat() / m.ms).coerceIn(0f, 1f)
        val (out, inc) = Automix.gains(t)
        m.old.volume = out * sleep * QueueBridge.duck
        m.new.volume = inc * sleep * QueueBridge.duck
        m.oldMeter.highpass = Automix.bassCut(t)
        if (m.nudge == null && now - m.began > 3_000) {
            val inBeat = Automix.beat(m.newMeter.last(3f).first)
            val ratio = m.outBpm?.let { o -> inBeat?.let { Automix.nudge(o, it.bpm) } } ?: 1f
            m.nudge = ratio
            if (ratio != 1f) m.new.playbackParameters = PlaybackParameters(s.speed * ratio, s.pitch)
        }
        if (t >= 1f) endMix("mixed") else if (m.old.playbackState == Player.STATE_ENDED) endMix("the old song ended at ${m.old.currentPosition} of ${m.old.duration} ms")
    }

    /**
     * Swiping the app away while paused stops the service; while playing, music continues. (The
     * service stays in the foreground for 10 minutes after the music stops, so songs changing with
     * the screen off aren't refused by Android; it is stopped here on purpose.)
     */
    @OptIn(UnstableApi::class)
    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) pauseAllPlayersAndStopSelf()
    }

    /**
     * The screen was turned off: with music playing (and "Lyrics when the screen is off" on), RyM's
     * own always-on screen comes up over the lock screen; turned off again while it shows, the
     * phone sleeps.
     */
    private val screenOff = object : android.content.BroadcastReceiver() {
        override fun onReceive(context: android.content.Context, intent: android.content.Intent) {
            if (com.rymplayer.Aod.open) {
                com.rymplayer.Aod.close?.invoke()
                return
            }
            val on = getSharedPreferences("rym", MODE_PRIVATE).getBoolean("aod_lyrics", false)
            if (!on || players.none { it?.isPlaying == true } || !android.provider.Settings.canDrawOverlays(this@PlaybackService)) return
            runCatching {
                startActivity(
                    android.content.Intent(this@PlaybackService, com.rymplayer.AodActivity::class.java)
                        .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK or android.content.Intent.FLAG_ACTIVITY_NO_ANIMATION),
                )
            }.onFailure { android.util.Log.w("RyM", "always-on screen: ${it.message}") }
        }
    }
    private var screenOffWatched = false

    private fun watchScreenOff() {
        if (screenOffWatched) return
        screenOffWatched = true
        androidx.core.content.ContextCompat.registerReceiver(this, screenOff, android.content.IntentFilter(android.content.Intent.ACTION_SCREEN_OFF), androidx.core.content.ContextCompat.RECEIVER_NOT_EXPORTED)
    }

    override fun onDestroy() {
        if (screenOffWatched) runCatching { unregisterReceiver(screenOff) }
        QueueBridge.lyricChanged = null
        QueueBridge.lyric = null
        libraryScope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
        (getSystemService(AUDIO_SERVICE) as? android.media.AudioManager)?.unregisterAudioDeviceCallback(plugged)
        handler.removeCallbacksAndMessages(null)
        QueueBridge.cancelMix = null
        Link.stop()
        Sound.unlisten(this, soundChanged)
        effects.forEach { it?.release() }
        players.forEach { it?.release() }
        session?.release()
        session = null
        super.onDestroy()
    }

    private companion object {
        /** About how long a player takes from play() to sound (taken off the wait for a beat). */
        const val START_LATENCY_MS = 60L
    }
}
