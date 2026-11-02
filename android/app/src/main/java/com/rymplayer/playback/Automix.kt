package com.rymplayer.playback

import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import androidx.media3.common.util.UnstableApi
import java.nio.ByteBuffer
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.ln
import kotlin.math.log10
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Smart automix: one song turns into the next like a DJ mix, with light signal processing (no
 * models). The same rules as the PC apps' src/automix.rs:
 * - cue-out: the change starts as soon as the song has faded out (below -28 dB for 400 ms in its
 *   last 15 seconds), or at the mix window before its end;
 * - cue-in: the next song's silent start is skipped, so it comes in on its first sound;
 * - equal-power gains (cos / sin), so the loudness holds through the change;
 * - bass swap: the outgoing song loses its lows (a high-pass rising to 200 Hz) while the
 *   incoming one brings its own;
 * - beats: the incoming song starts on one of the outgoing song's beats, and when their tempos
 *   are within 3% it's nudged to the same tempo while they overlap (pitch kept).
 */
object Automix {
    /** Loudness blocks per second (one every 10 ms). */
    const val BLOCKS = 100
    private const val KEEP = 16 * BLOCKS
    const val CUE_OUT_DB = -28f
    const val CUE_OUT_HOLD = 40
    /** The end of a song where a fade-out starts the change (seconds). */
    const val WINDOW_SECS = 15f
    const val BASS_HZ = 200f
    const val MAX_NUDGE = 0.03f

    /** The next song takes a moment to load: the mix is asked for this much earlier, so they overlap fully. */
    const val LOAD_LEAD_MS = 1500L

    /** The mix's length: the crossfade setting, or 6 seconds when that's off. */
    fun mixSecs(crossfade: Int): Float = if (crossfade == 0) 6f else crossfade.toFloat()

    fun db(rms: Float): Float = 20f * log10(rms.coerceAtLeast(1e-6f))

    /** Equal-power gains at [t] (0 to 1): the outgoing song's and the incoming one's. */
    fun gains(t: Float): Pair<Float, Float> {
        val a = t.coerceIn(0f, 1f) * (Math.PI / 2).toFloat()
        return cos(a) to sin(a)
    }

    /** The song has faded out: its last 400 ms below -28 dB, and well below its own usual level. */
    fun fadedOut(env: FloatArray): Boolean {
        if (env.size < CUE_OUT_HOLD * 3) return false
        val levels = env.map(::db)
        val tail = levels.takeLast(CUE_OUT_HOLD)
        val usual = levels.dropLast(CUE_OUT_HOLD).sorted().let { it[it.size / 2] }
        val threshold = minOf(CUE_OUT_DB, usual - 8f)
        return tail.all { it < threshold }
    }

    /** The outgoing song's high-pass at [t]: off at first, eased up to 200 Hz by two thirds in. */
    fun bassCut(t: Float): Float {
        val x = (t / 0.66f).coerceIn(0f, 1f)
        val eased = x * x * (3 - 2 * x)
        return if (eased <= 0f) 0f else 20f + (BASS_HZ - 20f) * eased
    }

    /** [period]: blocks between beats; [last]: blocks back from the envelope's end to its last beat. */
    data class Beat(val period: Float, val last: Float, val confidence: Float) {
        val bpm: Float get() = 60f * BLOCKS / period
    }

    /** Tempo and beat phase from onsets' autocorrelation over 70 to 180 BPM; null without a clear beat. */
    fun beat(env: FloatArray): Beat? {
        if (env.size < 3 * BLOCKS) return null
        val log = FloatArray(env.size) { ln(env[it] + 1e-4f) }
        val onset = FloatArray(env.size - 1) { maxOf(0f, log[it + 1] - log[it]) }
        val mean = onset.average().toFloat()
        for (i in onset.indices) onset[i] -= mean
        val n = onset.size
        val energy = onset.fold(0f) { s, o -> s + o * o }
        if (energy <= 1e-9f) return null
        fun corr(lag: Int): Float {
            var s = 0f
            for (i in lag until n) s += onset[i] * onset[i - lag]
            return s / (n - lag) * n
        }
        val lo = BLOCKS * 60 / 180
        val hi = BLOCKS * 60 / 70
        val scores = FloatArray(hi - lo + 3) { corr(lo - 1 + it) }
        var best = 1
        for (i in 1 until scores.size - 1) if (scores[i] > scores[best]) best = i
        // Every second (or 1.5th) beat lines up as well: the shorter period wins when nearly as high.
        val top = scores[best]
        for ((num, den) in listOf(1 to 2, 2 to 3)) {
            val lag = (lo - 1 + best) * num / den
            if (lag > lo && lag + 1 <= hi) {
                val i = (lag - 1 - (lo - 1)..lag + 1 - (lo - 1)).maxBy { scores[it] }
                if (scores[i] >= 0.8f * top && i < best) {
                    best = i
                    break
                }
            }
        }
        val confidence = scores[best] / energy
        if (confidence < 0.2f) return null
        val (a, b, c) = Triple(scores[best - 1], scores[best], scores[best + 1])
        val shift = if (a - 2 * b + c != 0f) 0.5f * (a - c) / (a - 2 * b + c) else 0f
        val period = (lo - 1 + best) + shift.coerceIn(-0.5f, 0.5f)
        var last = 0f
        var most = -Float.MAX_VALUE
        for (p in 0 until ceil(period).toInt()) {
            var sum = 0f
            var at = (n - 1).toFloat() - p
            while (at >= 0f) {
                sum += onset[at.roundToInt()]
                at -= period
            }
            if (sum > most) {
                most = sum
                last = p.toFloat()
            }
        }
        return Beat(period, last, confidence.coerceAtMost(1f))
    }

    /** The incoming song's speed factor to meet the outgoing tempo (double and half time count), within 3%. */
    fun nudge(outBpm: Float, inBpm: Float): Float? =
        listOf(0.5f, 1f, 2f).map { outBpm / (inBpm * it) }.minBy { abs(it - 1f) }.takeIf { abs(it - 1f) <= MAX_NUDGE }

    /**
     * One player's measurements and controls: its loudness every 10 ms (written by the playback
     * thread), the high-pass on it, and where its loudness blocks sit in the song.
     */
    class Meter {
        private val ring = FloatArray(KEEP)
        private var written = 0L
        /** The high-pass on it (Hz; 0 = off). */
        @Volatile var highpass = 0f
        /** The song's position (ms) where its blocks start counting (set on each seek and song). */
        @Volatile var anchorMs = 0L

        @Synchronized fun push(rms: Float) {
            ring[(written % KEEP).toInt()] = rms
            written++
        }

        @Synchronized fun flushed() {
            written = 0
        }

        /** The last [secs] of loudness, oldest first, and how many blocks since the anchor. */
        @Synchronized fun last(secs: Float): Pair<FloatArray, Long> {
            val want = minOf((secs * BLOCKS).toLong(), written, KEEP.toLong()).toInt()
            val out = FloatArray(want) { ring[((written - want + it) % KEEP).toInt()] }
            return out to written
        }

        /** The song position (ms) of its next beat at least [aheadMs] after [positionMs], and the beat. */
        fun nextBeatMs(positionMs: Long, aheadMs: Long): Pair<Long, Beat>? {
            val (env, blocks) = last(12f)
            val b = beat(env) ?: return null
            val step = b.period * 1000f / BLOCKS
            var at = anchorMs + (blocks - 1 - b.last) * 1000f / BLOCKS
            // The blocks run ahead of what's heard: the first beat after the position, either way.
            while (at < positionMs + aheadMs) at += step
            while (at - step >= positionMs + aheadMs) at -= step
            return at.toLong() to b
        }
    }

    /** A high-pass filter (RBJ cookbook, Butterworth Q). */
    private class HighPass {
        private var b0 = 1f
        private var b1 = 0f
        private var b2 = 0f
        private var a1 = 0f
        private var a2 = 0f
        private var z1 = 0f
        private var z2 = 0f

        fun set(hz: Float, rate: Float) {
            val w0 = (2 * Math.PI * minOf(hz, rate * 0.45f) / rate).toFloat()
            val cs = cos(w0)
            val alpha = sin(w0) / (2f * 0.70710677f)
            val a0 = 1 + alpha
            b0 = (1 + cs) / 2 / a0
            b1 = -(1 + cs) / a0
            b2 = (1 + cs) / 2 / a0
            a1 = -2 * cs / a0
            a2 = (1 - alpha) / a0
        }

        fun reset() {
            z1 = 0f
            z2 = 0f
        }

        fun run(x: Float): Float {
            val y = b0 * x + z1
            z1 = b1 * x - a1 * y + z2
            z2 = b2 * x - a2 * y
            return y
        }
    }

    /**
     * Sits in the player's audio chain (before speed and skip silence): measures the song's
     * loudness into [meter] and runs its high-pass. 16-bit sound only; otherwise it stands aside.
     */
    @OptIn(UnstableApi::class)
    class Processor(private val meter: Meter) : BaseAudioProcessor() {
        private var channels = 2
        private var rate = 48_000
        private var filters = emptyArray<HighPass>()
        private var cutoff = 0f
        private var blockFrames = 480
        private var sum = 0.0
        private var inBlock = 0

        override fun onConfigure(inputAudioFormat: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
            if (inputAudioFormat.encoding != C.ENCODING_PCM_16BIT) return AudioProcessor.AudioFormat.NOT_SET
            channels = inputAudioFormat.channelCount
            rate = inputAudioFormat.sampleRate
            blockFrames = maxOf(1, rate / BLOCKS)
            filters = Array(channels) { HighPass() }
            cutoff = 0f
            return inputAudioFormat
        }

        override fun queueInput(inputBuffer: ByteBuffer) {
            val bytes = inputBuffer.remaining()
            if (bytes == 0) return
            val out = replaceOutputBuffer(bytes)
            val want = meter.highpass
            if (abs(want - cutoff) > 0.5f) {
                if (cutoff < 1f) filters.forEach { it.reset() }
                cutoff = want
                if (want >= 1f) filters.forEach { it.set(want, rate.toFloat()) }
            }
            val frames = bytes / (2 * channels)
            repeat(frames) {
                var power = 0f
                for (c in 0 until channels) {
                    var x = inputBuffer.short / 32768f
                    power += x * x
                    if (cutoff >= 1f) x = filters[c].run(x)
                    out.putShort((x * 32768f).roundToInt().coerceIn(-32768, 32767).toShort())
                }
                sum += power / channels
                if (++inBlock >= blockFrames) {
                    meter.push(sqrt(sum / inBlock).toFloat())
                    sum = 0.0
                    inBlock = 0
                }
            }
            // A partial last frame (never expected) passes through untouched.
            while (inputBuffer.hasRemaining()) out.put(inputBuffer.get())
            out.flip()
        }

        override fun onFlush() {
            sum = 0.0
            inBlock = 0
            filters.forEach { it.reset() }
            meter.flushed()
        }
    }
}
