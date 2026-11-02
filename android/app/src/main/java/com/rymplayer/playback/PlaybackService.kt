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
import androidx.media3.session.MediaSessionService

/**
 * The queue lives in the app's [com.rymplayer.playback.Player], which hands the media player one
 * song at a time. The notification's and lock screen's next/previous buttons ask it through here.
 */
object QueueBridge {
    @Volatile var next: (() -> Unit)? = null
    @Volatile var prev: (() -> Unit)? = null
    /** A song comes after the playing one (the automix needs one to mix into). */
    @Volatile var hasNext: (() -> Boolean)? = null
}

/**
 * Plays in the background and shows Android's media notification (with lock-screen controls).
 *
 * Two players: the one the media session shows, and the other, used by the smart automix. When a
 * mix starts, the session moves to the other player and the app starts the next song on it, while
 * the first one plays on, fading out with its bass taken away; both sound at once.
 */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private val players = arrayOfNulls<ExoPlayer>(2)
    private val meters = arrayOf(Automix.Meter(), Automix.Meter())
    private val effects = arrayOfNulls<SoundEffects>(2)
    /** The player the session shows. */
    private var active = 0
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
        for (i in 0..1) players[i] = build(i)
        Sound.listen(this, soundChanged)
        applySound()
        session = MediaSession.Builder(this, wrap(players[0]!!)).build()
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
            .setMediaSourceFactory(DefaultMediaSourceFactory(DefaultDataSource.Factory(this, WebDataSource.Factory())))
            // Only the session's player holds the audio focus (the other would take it away).
            .setAudioAttributes(attributes, i == 0)
            .setHandleAudioBecomingNoisy(true)
            // Keeps Wi-Fi awake only while streaming, and the CPU only while playing.
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        effects[i] = SoundEffects(exo)
        exo.addListener(object : Player.Listener {
            override fun onAudioSessionIdChanged(audioSessionId: Int) = applySound()
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                val m = mix
                if (isPlaying && m != null && m.new === exo && m.began == 0L) m.began = SystemClock.elapsedRealtime()
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
            override fun getAvailableCommands(): Player.Commands = super.getAvailableCommands().buildUpon().addAll(*skips.toIntArray()).build()
            override fun isCommandAvailable(command: Int) = command in skips || super.isCommandAvailable(command)
            override fun seekToNext() { QueueBridge.next?.invoke() }
            override fun seekToNextMediaItem() { QueueBridge.next?.invoke() }
            override fun seekToPrevious() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun seekToPreviousMediaItem() { QueueBridge.prev?.invoke() ?: super.seekToPrevious() }
            override fun play() {
                if (held()) askToStart(exo) else super.play()
            }
            override fun setPlayWhenReady(playWhenReady: Boolean) {
                if (playWhenReady && held()) askToStart(exo) else super.setPlayWhenReady(playWhenReady)
                if (!playWhenReady) endMix()
            }
            // Pausing in the middle of a mix ends it: the song going out stops.
            override fun pause() {
                super.pause()
                endMix()
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    private fun applySound() {
        val s = Sound.load(this)
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
        m.old.stop()
        m.old.clearMediaItems()
        m.oldMeter.highpass = 0f
        val s = Sound.load(this)
        m.new.skipSilenceEnabled = s.skipSilence
        m.new.volume = 1f
        m.nudge?.takeIf { it != 1f }?.let { easing = SystemClock.elapsedRealtime() to it }
    }

    /**
     * Starts a smart automix: the session moves to the other player, the app plays the next song
     * on it, and this one plays on, fading out (see [fades]).
     */
    private fun startMix(s: Sound.Settings) {
        val old = players[active] ?: return
        val i = 1 - active
        val new = players[i] ?: return
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
        active = i
        session?.player = wrap(new)
        QueueBridge.next?.invoke()
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
            val speed = s.speed.coerceAtLeast(0.1f)
            var volume = 1f
            if (s.automix) {
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
            p.volume = (volume * sleep).coerceIn(0f, 1f)
            handler.postDelayed(this, wait.coerceIn(50L, 5_000L))
        }
    }

    /** One step of a mix: equal-power gains, the outgoing song's bass going, the incoming tempo nudged. */
    private fun mixStep(m: Mix, s: Sound.Settings, sleep: Float) {
        val now = SystemClock.elapsedRealtime()
        if (m.began == 0L) {
            // Waiting for the next song to load: the old one plays on (up to 20 s, or to its end).
            m.old.volume = sleep
            if (now - m.asked > 20_000 || m.old.playbackState == Player.STATE_ENDED) endMix("the next song didn't come")
            return
        }
        val t = ((now - m.began).toFloat() / m.ms).coerceIn(0f, 1f)
        val (out, inc) = Automix.gains(t)
        m.old.volume = out * sleep
        m.new.volume = inc * sleep
        m.oldMeter.highpass = Automix.bassCut(t)
        if (m.nudge == null && now - m.began > 3_000) {
            val inBeat = Automix.beat(m.newMeter.last(3f).first)
            val ratio = m.outBpm?.let { o -> inBeat?.let { Automix.nudge(o, it.bpm) } } ?: 1f
            m.nudge = ratio
            if (ratio != 1f) m.new.playbackParameters = PlaybackParameters(s.speed * ratio, s.pitch)
        }
        if (t >= 1f) endMix("mixed") else if (m.old.playbackState == Player.STATE_ENDED) endMix("the old song ended at ${m.old.currentPosition} of ${m.old.duration} ms")
    }

    /** Swiping the app away while paused stops the service; while playing, music continues. */
    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
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
