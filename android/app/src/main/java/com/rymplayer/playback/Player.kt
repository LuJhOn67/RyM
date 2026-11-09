package com.rymplayer.playback

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player as ExoListener
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.rymplayer.data.Downloads
import com.rymplayer.data.Track
import com.rymplayer.data.MusicApi
import com.rymplayer.data.parseDuration
import com.rymplayer.data.thumbSized
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * The queue, as Compose state. Audio plays in [PlaybackService] (so it keeps going in the
 * background and shows the media notification); this drives it through a [MediaController].
 */
class Player(private val context: Context, private val scope: CoroutineScope) {
    companion object {
        /** Played songs kept above a new start, at most. */
        const val HISTORY = 100
    }

    lateinit var api: MusicApi
    /** Hand songs to VLC instead of playing them here (see [Vlc]). */
    @Volatile var useVlc = false
    var onTrackChanged: (Track?) -> Unit = {}

    val queue = mutableStateListOf<Track>()
    var index by mutableIntStateOf(-1)
        private set
    var playing by mutableStateOf(false)
        private set
    var loading by mutableStateOf(false)
        private set
    var position by mutableFloatStateOf(0f)
        private set
    var message by mutableStateOf("")

    val current: Track? get() = queue.getOrNull(index)
    val duration: Float
        get() = controller?.duration?.takeIf { it != C.TIME_UNSET && it > 0 }?.let { it / 1000f }
            ?: current?.let { parseDuration(it.duration) } ?: 0f

    private val future = MediaController.Builder(
        context,
        SessionToken(context, ComponentName(context, PlaybackService::class.java)),
    ).buildAsync()
    private var controller: MediaController? = null
    private val ready = CompletableDeferred<MediaController>()
    private var ticker: Job? = null
    private var loadJob: Job? = null

    init {
        QueueBridge.next = { next() }
        QueueBridge.prev = { prev() }
        QueueBridge.hasNext = { (index + 1 < queue.size || repeatMode == 1) && !repeatOne }
        QueueBridge.mixNext = { done -> scope.launch { done(advanceForMix()) } }
        Link.pause = { pause() }
        // Seamless Music from the PC: while nothing plays here, a PC of the same account's music moves here.
        Link.listen(context) { fromPc(it) }
        watchNonMusic()
        future.addListener({
            val c = runCatching { future.get() }.getOrElse {
                ready.completeExceptionally(it)
                return@addListener
            }
            controller = c
            c.addListener(object : ExoListener.Listener {
                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    playing = isPlaying
                    countListening()
                    if (isPlaying) {
                        startTicker()
                        linkSnapshot()
                    } else saveSession()
                }

                override fun onPlaybackStateChanged(state: Int) {
                    if (state == ExoListener.STATE_READY && tapped > 0) {
                        android.util.Log.w("RyM", "timing: ${current?.title} started ${System.currentTimeMillis() - tapped} ms after the tap")
                        tapped = 0
                    }
                    loading = state == ExoListener.STATE_BUFFERING
                    if (state == ExoListener.STATE_ENDED) {
                        // Once per song, and only for the one playing now (a late "ended" from a
                        // song already skipped by hand must not skip the new one too).
                        val ended = controller?.currentMediaItem?.mediaId
                        if (ended != null && ended == current?.videoId && endedFor != playCount && repeatOne) {
                            // Repeat this song: from the start again.
                            controller?.seekTo(0L)
                            controller?.play()
                        } else if (ended != null && ended == current?.videoId && endedFor != playCount) {
                            endedFor = playCount
                            // Nothing after it yet: the radio is asked again and plays once it's in (see append).
                            if (index + 1 < queue.size) next() else if (repeatMode == 1 && queue.isNotEmpty()) play(0) else current?.let(::keepGoing)
                        }
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    loading = false
                    // A cached file or link the player can't use is dropped, so the next try fetches it again.
                    current?.let {
                        Streams.forget(context, it.videoId)
                        Streams.forgetLink(it.videoId)
                    }
                    message = "${com.rymplayer.tr("Can't play")}: ${error.errorCodeName}"
                }
            })
            ready.complete(c)
        }, ContextCompat.getMainExecutor(context))
    }

    /**
     * While playing: the shown position once a second, only while the app is on screen (the
     * lyrics follow on their own, see the view model); where it is, kept every 15 seconds, and
     * listening time written once a minute, in the background too.
     */
    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && controller?.isPlaying == true) {
                if (visible) position = exactPosition()
                if (System.currentTimeMillis() - sessionSavedAt > 15_000) saveSession()
                if (System.currentTimeMillis() - statsWrittenAt > 60_000) {
                    countListening()
                    com.rymplayer.data.Recent.flushListening(context)
                    statsWrittenAt = System.currentTimeMillis()
                }
                delay(if (visible) 1000 else 15_000)
            }
            position = exactPosition()
        }
    }

    /** The app is on screen (the position is shown; otherwise it isn't kept up to date). */
    var visible by mutableStateOf(true)

    /** Changes when the user seeks (the lyrics jump with it). */
    var seeks by mutableIntStateOf(0)
        private set

    /** The player's own position, in seconds. */
    fun exactPosition(): Float = (controller?.currentPosition ?: 0L) / 1000f

    /** Playback speed (1 = normal). */
    val speed: Float get() = controller?.playbackParameters?.speed ?: 1f

    /** Listening time: counted from play/pause and song changes, added to the stats in batches. */
    private var playingSince = 0L
    private var statsWrittenAt = System.currentTimeMillis()

    private fun countListening() {
        val now = System.currentTimeMillis()
        val t = current
        if (playingSince > 0 && t != null) com.rymplayer.data.Recent.listened(t, now - playingSince)
        playingSince = if (controller?.isPlaying == true) now else 0L
    }

    /** Songs played this session (Up next leaves them out). */
    val played = mutableSetOf<String>()

    /** The song left off last time and the millisecond to go on from, until it plays. */
    private var resumeAt: Pair<String, Long>? = null
    private var sessionSavedAt = 0L
    /** The queue the session file holds (its songs are written again only when it changes). */
    private var savedQueue: List<String> = emptyList()

    /**
     * What plays, where in it, and the queue around it (100 songs before, 400 after), so the
     * next start picks up there even after a crash. Written on every new song and every 3 s.
     */
    fun saveSession(atMs: Long? = null) {
        sessionSavedAt = System.currentTimeMillis()
        val i = index
        val from = (i - 100).coerceAtLeast(0)
        val to = (if (i < 0) queue.size else i + 401).coerceAtMost(queue.size)
        val songs = queue.toList().subList(from, to)
        val ms = resumeAt?.takeIf { it.first == current?.videoId }?.second ?: atMs ?: (controller?.currentPosition ?: 0L)
        val ids = songs.map { it.videoId }
        if (ids != savedQueue) {
            com.rymplayer.data.PageCache.saveTracks(context, "session", songs)
            savedQueue = ids
        }
        context.getSharedPreferences("session", Context.MODE_PRIVATE).edit()
            .putInt("index", if (i < 0) -1 else i - from).putLong("ms", ms.coerceAtLeast(0)).apply()
        linkSnapshot(ms)
    }

    /** What plays, for RyM Link (a PC on the Wi-Fi can take it over from this second). */
    private fun linkSnapshot(ms: Long = controller?.currentPosition ?: 0L) {
        Link.snapshot = if (index < 0) null else Link.Snapshot(queue.toList(), index, ms.coerceAtLeast(0), System.currentTimeMillis(), controller?.isPlaying == true, speed)
    }

    /** The last session's queue, its song paused where it was left: play goes on from there. */
    fun restoreSession() {
        if (queue.isNotEmpty()) return
        val songs = com.rymplayer.data.PageCache.loadTracks(context, "session")
        val prefs = context.getSharedPreferences("session", Context.MODE_PRIVATE)
        val i = prefs.getInt("index", -1)
        if (i !in songs.indices) return
        queue += songs
        savedQueue = songs.map { it.videoId }
        index = i
        val ms = prefs.getLong("ms", 0)
        resumeAt = songs[i].videoId to ms
        position = ms / 1000f
    }

    /** How many songs added with "Add to queue" / "Play next" wait right after the playing one. */
    private var added = 0

    /** Counts play() calls, so a song's end is acted on once. */
    private var playCount = 0
    private var endedFor = -1

    /** Replaces the queue (and its history) with [list] from [start]. */
    fun setQueue(list: List<Track>, start: Int) {
        queue.clear()
        queue += list
        added = 0
        index = -1
        play(start)
    }

    /**
     * Plays [list] from [start], keeping the songs played so far (up to the playing one) above
     * it, so Previous goes back to them; the songs that were still to come are dropped.
     */
    /** The next song was picked by hand (RaM points: its minus points go). */
    var userPick = false

    /** "Prefer lossless (FLAC)": the Internet Archive's FLAC first; and whether the playing song is lossless (the gold badge). */
    var preferLossless = false
    var lossless by mutableStateOf(false)
        private set

    fun start(list: List<Track>, start: Int) {
        if (list.isEmpty()) return
        userPick = true
        val keep = (index + 1).coerceIn(0, queue.size)
        while (queue.size > keep) queue.removeAt(queue.lastIndex)
        while (queue.size > HISTORY) queue.removeAt(0)
        val at = queue.size + start.coerceIn(0, list.size - 1)
        queue += list
        added = 0
        play(at)
    }

    fun append(list: List<Track>) {
        queue += list.filter { t -> queue.none { it.videoId == t.videoId } }
        // The queue had run out waiting for these: go on.
        if (endedFor == playCount && index + 1 < queue.size) play(index + 1)
    }

    /** Right after the playing song (it joins the songs waiting there, before them). */
    fun playNext(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else {
            queue.add(index + 1, t)
            added++
        }
    }

    /** Right after the playing song and the songs added before it, not at the end of the queue. */
    fun addToQueue(t: Track) {
        if (index < 0) setQueue(listOf(t), 0) else {
            queue.add((index + 1 + added).coerceAtMost(queue.size), t)
            added++
        }
    }

    /** Moves row [from] to [to]; the playing song stays the playing one. */
    fun move(from: Int, to: Int) {
        if (from !in queue.indices || to !in queue.indices || from == to) return
        val t = queue.removeAt(from)
        queue.add(to, t)
        index = when {
            index == from -> to
            from < index && to >= index -> index - 1
            from > index && to <= index -> index + 1
            else -> index
        }
        added = 0
    }

    /** When a song was asked for, for the timing log. */
    private var tapped = 0L

    fun play(i: Int) {
        val t = queue.getOrNull(i) ?: return
        // A mix still loading its song gives way to the one picked here.
        QueueBridge.cancelMix?.invoke()
        // The song that was playing gets its listening time before the new one starts.
        countListening()
        tapped = System.currentTimeMillis()
        android.util.Log.w("RyM", "timing: play ${t.title} (${t.videoId})")
        // Moving on to the next song uses up one added song; jumping elsewhere forgets them.
        added = if (i == index + 1) (added - 1).coerceAtLeast(0) else 0
        // Another song than the one left off last time: that one's place no longer counts.
        if (resumeAt?.first != t.videoId) resumeAt = null
        // RaM points: the song that was playing gets its listening time, then this one its play.
        val left = current?.let { Triple(it, (controller?.currentPosition ?: 0L) / 1000f, duration) }
        val manual = userPick.also { userPick = false }
        val ctx = context
        scope.launch(kotlinx.coroutines.Dispatchers.IO) { com.rymplayer.data.RamPoints.movedOn(ctx, left, t, manual) }
        index = i
        playCount++
        played += t.videoId
        com.rymplayer.data.Recent.played(context, t)
        position = 0f
        // The player still holds the last song's place: the new one starts at 0.
        saveSession(0)
        controller?.stop()
        onTrackChanged(t)
        loading = true
        loadJob?.cancel()
        loadJob = scope.launch {
            // Play songs in VLC (Library setting), when it's installed; the built-in player otherwise.
            if (useVlc && Vlc.installed(context) && com.rymplayer.data.isYouTube(t.videoId)) {
                val sent = runCatching { Vlc.open(context, t) }
                    .onFailure { if (it !is kotlinx.coroutines.CancellationException) message = "VLC couldn't open it: ${it.message}" }
                loading = false
                if (sent.isSuccess) return@launch
            }
            // A downloaded song plays from Music/RymPlayer; then the Archive's FLAC when lossless is
            // preferred and it has the song; others stream (or play from the cache).
            val flac = if (preferLossless && com.rymplayer.data.isYouTube(t.videoId) && Downloads.uriFor(context, t.videoId) == null)
                kotlinx.coroutines.withTimeoutOrNull(8000) { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { com.rymplayer.data.Lossless.find(t) } }
            else null
            lossless = flac != null
            val url = flac ?: Downloads.uriFor(context, t.videoId)?.toString()
                ?: runCatching { Streams.playable(context, t.videoId) }
                    .onFailure { if (it !is kotlinx.coroutines.CancellationException) message = "${com.rymplayer.tr("Can't play")}: ${it.message}" }
                    .getOrNull()
            if (url == null) {
                loading = false
                return@launch
            }
            val c = runCatching { ready.await() }.getOrElse {
                loading = false
                message = "${com.rymplayer.tr("Can't start the player")}: ${it.message}"
                return@launch
            }
            if (current?.videoId != t.videoId) return@launch
            val item = mediaItem(t, url)
            // The song left off last time starts where it was (given with the song: a seek sent
            // right after loading it was lost).
            val from = resumeAt?.takeIf { it.first == t.videoId && it.second > 1000 }?.second
            resumeAt = null
            if (from != null) c.setMediaItem(item, from) else c.setMediaItem(item)
            c.prepare()
            c.play()
            // In the account's history, so the PC shows it too.
            reportPlay(t)
            // The next song's link is looked up while this one plays.
            Streams.prefetchLinks(queue.drop(i + 1).take(2).map { it.videoId })
            // Last song of the queue: a radio of similar songs follows it.
            keepGoing(t)
        }
    }

    /** The player's item for [t]: its link, and title, artist and cover for the notification. */
    private fun mediaItem(t: Track, url: String): MediaItem {
        val meta = MediaMetadata.Builder()
            .setTitle(t.title)
            .setArtist(t.artist)
            .apply { thumbSized(t.thumb, 544)?.let { setArtworkUri(Uri.parse(it)) } }
            .build()
        // SoundCloud's songs come in pieces (HLS).
        val hls = ".m3u8" in url
        return MediaItem.Builder().setMediaId(t.videoId).setUri(url).setMediaMetadata(meta).apply { if (hls) setMimeType(androidx.media3.common.MimeTypes.APPLICATION_M3U8) }.build()
    }

    /**
     * The automix moves on to the next song: its link first (nothing changes when there isn't
     * one), then the queue, the stats and the lyrics, like [play]; the service starts the song.
     */
    private suspend fun advanceForMix(): MediaItem? {
        val i = index + 1
        val t = queue.getOrNull(i) ?: return null
        if (useVlc && Vlc.installed(context)) return null
        val url = Downloads.uriFor(context, t.videoId)?.toString()
            ?: runCatching { Streams.playable(context, t.videoId) }.getOrNull()
            ?: return null
        if (index + 1 != i || queue.getOrNull(i)?.videoId != t.videoId) return null
        countListening()
        android.util.Log.w("RyM", "timing: mix into ${t.title} (${t.videoId})")
        added = (added - 1).coerceAtLeast(0)
        resumeAt = null
        index = i
        playCount++
        played += t.videoId
        com.rymplayer.data.Recent.played(context, t)
        position = 0f
        saveSession(0)
        onTrackChanged(t)
        Streams.prefetchLinks(queue.drop(i + 1).take(2).map { it.videoId })
        keepGoing(t)
        reportPlay(t)
        return mediaItem(t, url)
    }

    private fun reportPlay(t: Track) {
        if (!::api.isInitialized || !com.rymplayer.data.isYouTube(t.videoId)) return
        scope.launch {
            runCatching { api.reportPlay(t.videoId) }.onFailure { android.util.Log.w("RyM", "history: not reported (${it.message})") }
        }
    }

    fun pause() {
        controller?.pause()
    }

    fun toggle() {
        val c = controller ?: return
        when {
            c.isPlaying -> c.pause()
            c.playbackState != ExoListener.STATE_IDLE && c.mediaItemCount > 0 -> c.play()
            else -> play(index)
        }
    }

    fun next() {
        if (index + 1 < queue.size) play(index + 1) else if (repeatMode == 1 && queue.isNotEmpty()) play(0)
    }

    /** When Previous was last pressed: twice in a row goes back even mid-song. */
    private var lastPrev = 0L

    /**
     * Previous: back to the start of the song after its first 3 seconds; before that (or pressed
     * twice in a row), the song before it. Reads the player's own position: the shown one only
     * moves while the screen is on.
     */
    fun prev() {
        val now = System.currentTimeMillis()
        val again = now - lastPrev < 1500
        lastPrev = now
        val pos = (controller?.currentPosition ?: 0L) / 1000f
        if (index > 0 && (pos < 3f || again)) play(index - 1) else seekTo(0f)
    }

    /** Songs like the one playing, added when the queue reaches its last song (the radio). */
    private fun keepGoing(t: Track) {
        if (index != queue.lastIndex || !::api.isInitialized) return
        scope.launch {
            runCatching { com.rymplayer.data.Radio.forTrack(api, t) }.getOrNull()?.let { radio -> append(tidy(radio, t)) }
        }
    }

    /**
     * Up next for [seed] from its radio by RaM points: only its kind (Husseini brings Husseini,
     * songs bring songs), its artist and songs like it first, new songs over ones played lately,
     * nothing played or queued already.
     */
    fun tidy(radio: List<Track>, seed: Track): List<Track> {
        val heard = queue.take(index + 1).map { com.rymplayer.data.Radio.firstArtist(it.artist) }.filter { it.isNotEmpty() }.toSet()
        val queued = queue.map { it.videoId }.toSet()
        val notes = com.rymplayer.data.RamPoints.load(context)
        val out = com.rymplayer.data.RamPoints.upNext(notes, radio, seed, likedSongs(), played + queued, heard, { com.rymplayer.data.Recent.playedAt(context, it) })
        com.rymplayer.data.RamPoints.save(context, notes)
        return out
    }

    /** The PC song already taken or skipped (device, song), so it's once. */
    private var pcSeen: Pair<String, String>? = null

    /** The PC's latest note (address, port, name, song, when heard). */
    private var pcNote: PcNote? = null
    private data class PcNote(val ip: String, val port: Int, val device: String, val vid: String, val at: Long)

    /** SponsorBlock: a music video's non-music parts (intro, talk, outro) are skipped. */
    var skipNonMusic by mutableStateOf(context.getSharedPreferences("rym", Context.MODE_PRIVATE).getBoolean("sponsorblock", true))
        private set

    fun toggleSkipNonMusic() {
        skipNonMusic = !skipNonMusic
        context.getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putBoolean("sponsorblock", skipNonMusic).apply()
    }

    /** Twice a second while a song plays: its non-music parts (asked once per song) are jumped over. */
    private fun watchNonMusic() {
        scope.launch {
            var forId: String? = null
            var parts: List<Pair<Float, Float>> = emptyList()
            while (isActive) {
                delay(500)
                val id = current?.videoId
                if (id != forId) {
                    forId = id
                    parts = emptyList()
                    if (id != null && skipNonMusic) {
                        val found = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { com.rymplayer.data.SponsorBlock.segments(id) }
                        if (current?.videoId == id) parts = found
                    }
                }
                val c = controller ?: continue
                if (!skipNonMusic || parts.isEmpty() || !c.isPlaying) continue
                val to = com.rymplayer.data.SponsorBlock.skipTo(parts, c.currentPosition / 1000f) ?: continue
                seekTo(to)
                message = com.rymplayer.tr("Skipped a part that isn't music (SponsorBlock)")
            }
        }
    }

    /** The computer heard last (its note), for the remote. */
    var pc by mutableStateOf<Link.PcState?>(null)
        private set

    /** "Play along": the PC's song plays here too, in step (the PC goes on playing). */
    var playAlong by mutableStateOf(false)
        private set

    /** Added to the PC's second while playing along (headphones and speakers lag differently). */
    var alongOffsetMs by mutableIntStateOf(context.getSharedPreferences("rym", Context.MODE_PRIVATE).getInt("along_offset_ms", 0))
        private set

    fun nudgeAlongOffset(ms: Int) {
        alongOffsetMs = (alongOffsetMs + ms).coerceIn(-1000, 1000)
        context.getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putInt("along_offset_ms", alongOffsetMs).apply()
        lastFix = 0
    }

    private fun fromPc(n: Link.PcState) {
        pc = n
        if (n.playing && n.videoId.isNotEmpty()) pcNote = PcNote(n.ip, n.port, n.device, n.videoId, n.at)
        if (playAlong) follow(n)
    }

    /** The phone's remote for the PC heard last. */
    fun remote(what: String, value: Float = 0f) {
        val n = pc ?: return
        scope.launch(kotlinx.coroutines.Dispatchers.IO) {
            if (!Link.control(n.ip, n.port, what, value)) {
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) { message = com.rymplayer.tr("The computer didn't answer") }
            }
        }
    }

    fun startPlayAlong() {
        val n = pc ?: return
        playAlong = true
        lastFix = 0
        join(n)
    }

    fun stopPlayAlong() {
        playAlong = false
    }

    private var joining = false
    private var lastFix = 0L

    /** Plays the PC's queue from its song and second (plus the offset and a moment to start). */
    private fun join(n: Link.PcState) {
        if (joining) return
        joining = true
        scope.launch {
            val got = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { Link.pcQueue(n.ip, n.port) }
            joining = false
            if (got == null || !playAlong) return@launch
            val (songs, current, position) = got
            resumeAt = songs[current].videoId to ((position + alongOffsetMs / 1000f + 0.4f) * 1000).toLong().coerceAtLeast(0)
            start(songs, current)
            lastFix = System.currentTimeMillis()
        }
    }

    /** Each PC note while playing along: the same song, playing or paused like it, within a third of a second. */
    private fun follow(n: Link.PcState) {
        if (loading || joining || n.videoId.isEmpty()) return
        if (current?.videoId != n.videoId) {
            join(n)
            return
        }
        val c = controller ?: return
        if (n.playing != c.isPlaying) {
            if (n.playing) c.play() else c.pause()
            return
        }
        if (!n.playing) return
        val now = System.currentTimeMillis()
        if (now - lastFix < 4000) return
        val want = n.positionNow(now) + alongOffsetMs / 1000f
        val here = c.currentPosition / 1000f
        if (kotlin.math.abs(want - here) > 0.35f) {
            lastFix = now
            seekTo(want + 0.05f)
        }
    }

    /** RyM was opened on the phone: if the PC is playing (heard in the last seconds) and nothing plays here, the music comes here. */
    fun takeFromPc() {
        val n = pcNote?.takeIf { System.currentTimeMillis() - it.at < 8000 } ?: return
        if (playing || loading || pcSeen == (n.device to n.vid)) return
        pcSeen = n.device to n.vid
        val (ip, port, device) = Triple(n.ip, n.port, n.device)
        scope.launch {
            val got = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { Link.takeOver(ip, port) } ?: return@launch
            val (songs, current, position) = got
            resumeAt = songs[current].videoId to (position * 1000).toLong()
            start(songs, current)
            message = "${com.rymplayer.tr("Playing here, moved from")} $device"
        }
    }

    /** Repeat: 0 off, 1 all (the list starts over at its end), 2 this song (for good, no mix). */
    var repeatMode by mutableIntStateOf(0)
    val repeatOne: Boolean get() = repeatMode == 2

    fun cycleRepeat() {
        repeatMode = (repeatMode + 1) % 3
    }

    /** The liked songs, mixed into Up next (set by the app). */
    var likedSongs: () -> List<Track> = { emptyList() }

    fun seekTo(secs: Float) {
        controller?.seekTo((secs * 1000).toLong())
        position = secs
        seeks++
        linkSnapshot((secs * 1000).toLong())
    }

    fun remove(i: Int) {
        if (i !in queue.indices || i == index) return
        queue.removeAt(i)
        if (i < index) index--
        // One of the added songs waiting after the playing one.
        if (i > index && i <= index + added) added--
    }

    fun stop() {
        controller?.stop()
        controller?.clearMediaItems()
        queue.clear()
        index = -1
        onTrackChanged(null)
    }

    /** Lets go of the service; music keeps playing if it was. */
    fun release() {
        saveSession()
        QueueBridge.next = null
        QueueBridge.prev = null
        QueueBridge.hasNext = null
        QueueBridge.mixNext = null
        Link.pause = null
        loadJob?.cancel()
        ticker?.cancel()
        MediaController.releaseFuture(future)
    }
}
