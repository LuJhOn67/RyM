package com.rymplayer.playback

import android.content.Context
import android.media.audiofx.DynamicsProcessing
import android.media.audiofx.Equalizer
import android.media.audiofx.PresetReverb
import androidx.annotation.OptIn
import androidx.media3.common.AuxEffectInfo
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import kotlin.math.abs
import kotlin.math.ln

/**
 * Sound settings (saved in the "sound" preferences; the playback service applies them as they
 * change): crossfade, skip silence, a 5-band equalizer with presets, even loudness, speed and
 * pitch (Normal / Nightcore / Slowed, which also adds a little reverb), and the sleep timer.
 * The same choices as the PC apps' src/fx.rs.
 */
object Sound {
    const val PREFS = "sound"
    val BANDS = floatArrayOf(60f, 230f, 910f, 3600f, 14000f)
    val PRESETS = listOf(
        "Flat" to floatArrayOf(0f, 0f, 0f, 0f, 0f),
        "Bass Boost" to floatArrayOf(6f, 4f, 0f, 0f, 0f),
        "Vocal" to floatArrayOf(-2f, 0f, 3f, 4f, 1f),
        "Rock" to floatArrayOf(4f, 2f, -1f, 2f, 4f),
        "Pop" to floatArrayOf(-1f, 2f, 4f, 2f, -1f),
        "Custom" to floatArrayOf(0f, 0f, 0f, 0f, 0f),
    )
    /** Name, speed, pitch, reverb. */
    val MODES = listOf(Mode("Normal", 1f, 1f, false), Mode("Nightcore", 1.25f, 1.25f, false), Mode("Slowed", 0.8f, 0.85f, true))

    data class Mode(val name: String, val speed: Float, val pitch: Float, val reverb: Boolean)

    data class Settings(
        val crossfade: Int = 4,
        val skipSilence: Boolean = true,
        val eqPreset: Int = 0,
        val eqCustom: List<Float> = List(5) { 0f },
        val normalize: Boolean = true,
        val speed: Float = 1f,
        val pitch: Float = 1f,
        val reverb: Boolean = false,
        /** Sleep timer: when it stops (ms since 1970), -1 at the end of the song, 0 off. */
        val sleepAt: Long = 0,
        /** Smart automix (see [Automix]); off: the plain crossfade. */
        val automix: Boolean = false,
        /** Karaoke: the voice taken out (the middle of the stereo). */
        val vocalsOff: Boolean = false,
    ) {
        val gains: List<Float> get() = if (eqPreset == PRESETS.lastIndex) eqCustom else PRESETS[eqPreset.coerceIn(0, PRESETS.lastIndex)].second.toList()
        val modeName: String get() = MODES.firstOrNull { abs(it.speed - speed) < 0.01f && abs(it.pitch - pitch) < 0.01f }?.name ?: "Custom"
        /** Nothing changes the sound: the phone may decode it in its power-saving audio path. */
        val untouched: Boolean get() = crossfade == 0 && !automix && !skipSilence && gains.all { it == 0f } && !normalize && speed == 1f && pitch == 1f && !reverb && !vocalsOff
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun load(ctx: Context): Settings {
        val p = prefs(ctx)
        return Settings(
            crossfade = p.getInt("crossfade", 4),
            skipSilence = p.getBoolean("skip_silence", true),
            eqPreset = p.getInt("eq_preset", 0),
            eqCustom = (0 until 5).map { p.getFloat("eq_$it", 0f) },
            normalize = p.getBoolean("normalize", true),
            speed = p.getFloat("speed", 1f),
            pitch = p.getFloat("pitch", 1f),
            reverb = p.getBoolean("reverb", false),
            sleepAt = p.getLong("sleep_at", 0),
            automix = p.getBoolean("automix", false),
            vocalsOff = p.getBoolean("vocals_off", false),
        )
    }

    fun save(ctx: Context, s: Settings) {
        prefs(ctx).edit().apply {
            putInt("crossfade", s.crossfade)
            putBoolean("skip_silence", s.skipSilence)
            putInt("eq_preset", s.eqPreset)
            s.eqCustom.forEachIndexed { i, g -> putFloat("eq_$i", g) }
            putBoolean("normalize", s.normalize)
            putFloat("speed", s.speed)
            putFloat("pitch", s.pitch)
            putBoolean("reverb", s.reverb)
            putLong("sleep_at", s.sleepAt)
            putBoolean("automix", s.automix)
            putBoolean("vocals_off", s.vocalsOff)
        }.apply()
    }

    fun listen(ctx: Context, l: android.content.SharedPreferences.OnSharedPreferenceChangeListener) = prefs(ctx).registerOnSharedPreferenceChangeListener(l)
    fun unlisten(ctx: Context, l: android.content.SharedPreferences.OnSharedPreferenceChangeListener) = prefs(ctx).unregisterOnSharedPreferenceChangeListener(l)

    /** Our 5 bands' gain at [hz] (log-frequency interpolation between band centres). */
    fun gainAt(gains: List<Float>, hz: Float): Float {
        val x = ln(hz.coerceAtLeast(20f))
        val xs = BANDS.map { ln(it) }
        if (x <= xs.first()) return gains.first()
        if (x >= xs.last()) return gains.last()
        val i = xs.indexOfLast { it <= x }
        val t = (x - xs[i]) / (xs[i + 1] - xs[i])
        return gains[i] + (gains[i + 1] - gains[i]) * t
    }
}

/** The sound settings on one player (its audio session): equalizer, loudness, reverb, speed, skip silence. */
@OptIn(UnstableApi::class)
class SoundEffects(private val exo: ExoPlayer) {
    private var session = 0
    private var eq: Equalizer? = null
    private var dynamics: DynamicsProcessing? = null
    private var reverb: PresetReverb? = null

    fun apply(s: Sound.Settings) {
        exo.skipSilenceEnabled = s.skipSilence
        exo.playbackParameters = PlaybackParameters(s.speed, s.pitch)
        // Nothing changes the sound: the low-power audio path (offload) may be used.
        exo.trackSelectionParameters = exo.trackSelectionParameters.buildUpon().setAudioOffloadPreferences(
            TrackSelectionParameters.AudioOffloadPreferences.Builder()
                .setAudioOffloadMode(if (s.untouched) TrackSelectionParameters.AudioOffloadPreferences.AUDIO_OFFLOAD_MODE_ENABLED else TrackSelectionParameters.AudioOffloadPreferences.AUDIO_OFFLOAD_MODE_DISABLED)
                .build(),
        ).build()
        val id = exo.audioSessionId
        if (id != session) {
            release()
            session = id
        }
        if (id == 0) return
        runCatching {
            val e = eq ?: Equalizer(0, id).also { eq = it }
            val (low, high) = e.bandLevelRange.let { it[0].toInt() to it[1].toInt() }
            for (b in 0 until e.numberOfBands) {
                val hz = e.getCenterFreq(b.toShort()) / 1000f
                val mb = (Sound.gainAt(s.gains, hz) * 100).toInt().coerceIn(low, high)
                e.setBandLevel(b.toShort(), mb.toShort())
            }
            e.enabled = s.gains.any { it != 0f }
        }.onFailure { android.util.Log.w("RyM", "equalizer", it) }
        runCatching {
            if (s.normalize) {
                // Even loudness: a gentle compressor brings loud and quiet songs together, a limiter
                // catches the peaks.
                val d = dynamics ?: DynamicsProcessing(0, id, DynamicsProcessing.Config.Builder(
                    DynamicsProcessing.VARIANT_FAVOR_FREQUENCY_RESOLUTION, 2, false, 0, true, 1, false, 0, true,
                ).build()).also { dynamics = it }
                for (ch in 0 until 2) {
                    val band = d.getMbcBandByChannelIndex(ch, 0)
                    band.isEnabled = true
                    band.attackTime = 20f
                    band.releaseTime = 400f
                    band.ratio = 3f
                    band.threshold = -24f
                    band.kneeWidth = 6f
                    band.postGain = 6f
                    d.setMbcBandByChannelIndex(ch, 0, band)
                    val limiter = d.getLimiterByChannelIndex(ch)
                    limiter.isEnabled = true
                    limiter.threshold = -2f
                    limiter.ratio = 10f
                    d.setLimiterByChannelIndex(ch, limiter)
                }
                d.enabled = true
            } else {
                dynamics?.enabled = false
            }
        }.onFailure { android.util.Log.w("RyM", "loudness", it) }
        runCatching {
            if (s.reverb) {
                val r = reverb ?: PresetReverb(1, 0).also { reverb = it }
                r.preset = PresetReverb.PRESET_LARGEROOM
                r.enabled = true
                exo.setAuxEffectInfo(AuxEffectInfo(r.id, 0.35f))
            } else if (reverb != null) {
                exo.setAuxEffectInfo(AuxEffectInfo(AuxEffectInfo.NO_AUX_EFFECT_ID, 0f))
                reverb?.enabled = false
            }
        }.onFailure { android.util.Log.w("RyM", "reverb", it) }
    }

    fun release() {
        runCatching { eq?.release() }
        runCatching { dynamics?.release() }
        runCatching { reverb?.release() }
        eq = null
        dynamics = null
        reverb = null
    }
}
