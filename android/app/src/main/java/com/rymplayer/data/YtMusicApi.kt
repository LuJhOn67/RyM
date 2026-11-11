package com.rymplayer.data

import android.content.Context
import java.io.BufferedWriter
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.zip.ZipInputStream
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * ytmusicapi (github.com/sigma67/ytmusicapi, Python, MIT) as the song finder on the phone, with
 * the built-in engine ([MusicApi], InnerTube) as the fallback. The same bridge as the PC apps
 * (assets/ytm_bridge.py), run by the Python that youtubedl-android unpacks; ytmusicapi and its
 * pure-Python libraries (requests, urllib3, idna, certifi, charset-normalizer) come in
 * assets/ytmusicapi.zip, and "Update parsers" fetches the newest ytmusicapi from PyPI.
 */
object YtMusicApi {
    enum class Engine(val label: String) {
        /** ytmusicapi first, the built-in engine when it fails or finds nothing. */
        Auto("Auto (ytmusicapi, built-in when it fails)"),
        YtMusicApi("ytmusicapi only (new)"),
        /** The rollback, when a YouTube change breaks ytmusicapi. */
        Builtin("Built-in InnerTube only (rollback)"),
    }

    @Volatile var engine = Engine.Auto
        private set
    /** The last time Auto fell back, and why (for the settings). */
    @Volatile var lastFailure: String? = null
        private set
    @Volatile private var app: Context? = null
    @Volatile var arabic = false

    private const val TIMEOUT_S = 25L
    /** After the bridge couldn't start, the next try waits this long. */
    private const val RETRY_AFTER_MS = 120_000L

    fun init(ctx: Context) {
        app = ctx.applicationContext
        engine = Engine.entries[ctx.getSharedPreferences("rym", Context.MODE_PRIVATE).getInt("music_engine", 0).coerceIn(0, 2)]
    }

    /** For tests (no settings saved). */
    internal fun useEngine(e: Engine) {
        engine = e
    }

    fun setEngine(ctx: Context, e: Engine) {
        engine = e
        ctx.getSharedPreferences("rym", Context.MODE_PRIVATE).edit().putInt("music_engine", e.ordinal).apply()
    }

    /** Runs [new] (ytmusicapi) and/or [builtin] as the engine setting says. */
    suspend fun <T> hybrid(what: String, found: (T) -> Boolean, new: suspend () -> T, builtin: suspend () -> T): T = when (engine) {
        Engine.Builtin -> builtin()
        Engine.YtMusicApi -> new()
        Engine.Auto -> try {
            val v = new()
            if (found(v)) v else builtin()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            lastFailure = "$what: ${e.message}"
            // (Not in plain JVM tests, where Android's Log isn't there.)
            runCatching { android.util.Log.w("RyM", "ytmusicapi: $what fell back to the built-in engine: ${e.message}") }
            builtin()
        }
    }

    // ------------------------------------------------------------ the bridge process

    private class Bridge(val process: Process, val input: BufferedWriter, val lines: LinkedBlockingQueue<String>, val version: String) {
        var nextId = 1
        var signedAs = 0
    }

    private val lock = Any()
    private var bridge: Bridge? = null
    private var failedAt = 0L
    private var failedWhy = ""

    private fun ctx(): Context = app ?: error("ytmusicapi isn't set up")

    private fun siteDir() = File(ctx().filesDir, "ytmusicapi")

    /** ytmusicapi and its libraries, unpacked from the app the first time (or after an app update). */
    private fun ensureSite() {
        val c = ctx()
        val marker = File(siteDir(), ".from")
        val want = "app ${c.packageManager.getPackageInfo(c.packageName, 0).longVersionCode}"
        val have = marker.takeIf { it.exists() }?.readText().orEmpty()
        // A newer ytmusicapi from "Update parsers" stays until the app itself updates.
        if (have == want || (have.startsWith("pypi") && have.substringAfter("|") == want)) return
        siteDir().deleteRecursively()
        unzip(c.assets.open("ytmusicapi.zip"), siteDir())
        marker.writeText(want)
    }

    private fun unzip(input: java.io.InputStream, into: File, only: (String) -> Boolean = { true }) {
        ZipInputStream(input.buffered()).use { z ->
            while (true) {
                val e = z.nextEntry ?: break
                val f = File(into, e.name)
                if (!f.canonicalPath.startsWith(into.canonicalPath) || !only(e.name)) continue
                if (e.isDirectory) f.mkdirs() else {
                    f.parentFile?.mkdirs()
                    f.outputStream().use { z.copyTo(it) }
                }
            }
        }
    }

    private fun start(): Bridge {
        val c = ctx()
        YtDlp.ready(c)
        ensureSite()
        val script = File(c.filesDir, "ytm_bridge.py")
        val text = c.assets.open("ytm_bridge.py").bufferedReader().readText()
        if (!script.exists() || script.readText() != text) script.writeText(text)
        val python = File(c.noBackupFilesDir, "youtubedl-android/packages/python")
        val bin = File(c.applicationInfo.nativeLibraryDir, "libpython.so")
        val pb = ProcessBuilder(bin.path, "-u", script.path)
        pb.environment().apply {
            put("LD_LIBRARY_PATH", "${python.path}/usr/lib")
            put("SSL_CERT_FILE", "${python.path}/usr/etc/tls/cert.pem")
            put("PYTHONHOME", "${python.path}/usr")
            put("PYTHONPATH", siteDir().path)
            put("HOME", c.filesDir.path)
            put("TMPDIR", c.cacheDir.path)
        }
        pb.redirectError(File(c.cacheDir, "ytm_bridge.log"))
        val p = pb.start()
        val lines = LinkedBlockingQueue<String>()
        Thread {
            runCatching { p.inputStream.bufferedReader().forEachLine { lines.put(it) } }
            lines.put("")
        }.apply { isDaemon = true }.start()
        // Python's first start on a phone is slow (it compiles ytmusicapi once).
        val first = lines.poll(60, TimeUnit.SECONDS) ?: run { p.destroy(); error("ytmusicapi didn't start") }
        val hello = runCatching { JSONObject(first) }.getOrNull()
        if (hello?.optBoolean("ready") != true) {
            p.destroy()
            error(hello?.optString("error")?.takeIf { it.isNotEmpty() } ?: "ytmusicapi didn't load")
        }
        android.util.Log.w("RyM", "ytmusicapi: bridge ready, version ${hello.optString("version")}")
        return Bridge(p, p.outputStream.bufferedWriter(), lines, hello.optString("version"))
    }

    private fun send(b: Bridge, req: JSONObject): Any? {
        val id = b.nextId++
        req.put("id", id).put("lang", if (arabic) "ar" else "en")
        b.input.write(req.toString())
        b.input.newLine()
        b.input.flush()
        val until = System.currentTimeMillis() + TIMEOUT_S * 1000
        while (true) {
            val left = until - System.currentTimeMillis()
            if (left <= 0) error("ytmusicapi took too long")
            val line = b.lines.poll(left, TimeUnit.MILLISECONDS) ?: error("ytmusicapi took too long")
            if (line.isEmpty()) error("ytmusicapi stopped")
            val answer = runCatching { JSONObject(line) }.getOrNull() ?: continue
            if (answer.optInt("id") != id) continue
            if (answer.optBoolean("ok")) {
                android.util.Log.w("RyM", "ytmusicapi: ${req.optString("op")} answered")
                return answer.opt("data")
            }
            error(answer.optString("error", "ytmusicapi failed"))
        }
    }

    /** One request: starts (or restarts) the bridge as needed and signs it in with [cookie]. */
    private fun askBlocking(req: JSONObject, cookie: String?): Any? = synchronized(lock) {
        if (bridge?.process?.isAlive == false) bridge = null
        if (bridge == null) {
            if (System.currentTimeMillis() - failedAt < RETRY_AFTER_MS) error(failedWhy)
            bridge = runCatching { start() }.getOrElse {
                failedAt = System.currentTimeMillis()
                failedWhy = it.message ?: "ytmusicapi didn't start"
                throw it
            }
        }
        val b = bridge!!
        val want = cookie?.hashCode()?.or(1) ?: 0
        try {
            if (b.signedAs != want) {
                send(b, JSONObject().put("op", "auth").put("cookie", cookie.orEmpty()))
                b.signedAs = want
            }
            send(b, req)
        } catch (e: IllegalStateException) {
            if (e.message?.contains("took too long") == true || e.message?.contains("stopped") == true) {
                b.process.destroy()
                bridge = null
            }
            throw e
        }
    }

    /** A request waiting for the bridge: searches (priority 0) go before radios and mixes (1). */
    private class Job(val req: JSONObject, val cookie: String?, val priority: Int, val seq: Long, val done: kotlinx.coroutines.CompletableDeferred<Result<Any?>>)

    private val seq = java.util.concurrent.atomic.AtomicLong()
    private val jobs = java.util.concurrent.PriorityBlockingQueue<Job>(16, compareBy<Job>({ it.priority }, { it.seq }))

    /** One thread answers the jobs in order of priority (a search typed now doesn't wait behind the home page's radios). */
    private val worker: Thread by lazy {
        Thread {
            while (true) {
                val job = jobs.take()
                job.done.complete(runCatching { askBlocking(job.req, job.cookie) })
            }
        }.apply {
            isDaemon = true
            name = "ytmusicapi"
            start()
        }
    }

    private suspend fun ask(req: JSONObject, cookie: String?): Any? {
        worker
        val done = kotlinx.coroutines.CompletableDeferred<Result<Any?>>()
        val priority = if (req.optString("op") == "search") 0 else 1
        jobs.put(Job(req, cookie, priority, seq.incrementAndGet(), done))
        return done.await().getOrThrow()
    }

    private fun tracks(data: Any?): List<Track> {
        val arr = data as? JSONArray ?: return emptyList()
        return (0 until arr.length()).mapNotNull { i ->
            val o = arr.optJSONObject(i) ?: return@mapNotNull null
            val id = o.optString("video_id").takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            fun opt(k: String) = o.optString(k).takeIf { it.isNotEmpty() && it != "null" }
            Track(id, o.optString("title"), o.optString("artist"), o.optString("album"), o.optString("duration"), opt("thumb"), opt("album_id"), opt("artist_id"))
        }
    }

    suspend fun search(q: String, filter: String, cookie: String?): List<Track> =
        tracks(ask(JSONObject().put("op", "search").put("query", q).put("filter", filter).put("limit", 40), cookie))

    suspend fun radio(videoId: String, cookie: String?): List<Track> =
        tracks(ask(JSONObject().put("op", "radio").put("videoId", videoId).put("limit", 50), cookie))

    suspend fun mix(playlistId: String, cookie: String?): List<Track> =
        tracks(ask(JSONObject().put("op", "mix").put("playlistId", playlistId).put("limit", 100), cookie))

    /** "ytmusicapi 1.12.3", or why it isn't working. */
    suspend fun status(): String = withContext(Dispatchers.IO) {
        synchronized(lock) { bridge?.let { return@withContext "ytmusicapi ${it.version}" } }
        runCatching { "ytmusicapi ${askBlocking(JSONObject().put("op", "version"), null)}" }.getOrElse { it.message ?: "ytmusicapi isn't working" }
    }

    /** The newest ytmusicapi from PyPI into the bridge's folder (it's pure Python). */
    private fun updateFromPyPI(): String {
        val c = ctx()
        val http = okhttp3.OkHttpClient()
        val json = http.newCall(okhttp3.Request.Builder().url("https://pypi.org/pypi/ytmusicapi/json").build()).execute().use { r ->
            if (!r.isSuccessful) error("PyPI answered ${r.code}")
            JSONObject(r.body!!.string())
        }
        val version = json.getJSONObject("info").getString("version")
        val urls = json.getJSONArray("urls")
        val wheel = (0 until urls.length()).map { urls.getJSONObject(it) }.firstOrNull { it.optString("packagetype") == "bdist_wheel" && it.optString("filename").endsWith("py3-none-any.whl") }
            ?: error("no wheel for ytmusicapi $version")
        val current = siteDir().listFiles()?.firstOrNull { it.name.startsWith("ytmusicapi-") && it.name.endsWith(".dist-info") }
            ?.name?.removePrefix("ytmusicapi-")?.removeSuffix(".dist-info")
        if (current == version) return "ytmusicapi $version is up to date"
        val bytes = http.newCall(okhttp3.Request.Builder().url(wheel.getString("url")).build()).execute().use { r ->
            if (!r.isSuccessful) error("PyPI answered ${r.code}")
            r.body!!.bytes()
        }
        val sha = wheel.optJSONObject("digests")?.optString("sha256").orEmpty()
        val got = java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        if (sha.isNotEmpty() && sha != got) error("the ytmusicapi download didn't match PyPI's checksum")
        File(siteDir(), "ytmusicapi").deleteRecursively()
        siteDir().listFiles()?.filter { it.name.startsWith("ytmusicapi-") && it.name.endsWith(".dist-info") }?.forEach { it.deleteRecursively() }
        unzip(bytes.inputStream(), siteDir()) { it.startsWith("ytmusicapi") }
        val appMark = "app ${c.packageManager.getPackageInfo(c.packageName, 0).longVersionCode}"
        File(siteDir(), ".from").writeText("pypi $version|$appMark")
        return "ytmusicapi updated to $version"
    }

    /** "Update parsers": yt-dlp and ytmusicapi to their newest, the bridge restarted, saved links and pages forgotten. */
    suspend fun updateParsers(nightly: Boolean): String = withContext(Dispatchers.IO) {
        val c = ctx()
        val ytdlp = YtDlp.update(c, nightly)
        val ytm = runCatching { synchronized(lock) { ensureSite() }; updateFromPyPI() }.getOrElse { "ytmusicapi: ${it.message}" }
        synchronized(lock) {
            bridge?.process?.destroy()
            bridge = null
            failedAt = 0L
        }
        com.rymplayer.playback.Streams.forgetAllLinks()
        PageCache.clear(c)
        "$ytdlp · $ytm · ${status()}"
    }
}
