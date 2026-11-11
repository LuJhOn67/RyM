package com.rymplayer.data

import android.content.Context
import android.os.Build
import java.io.File
import java.net.URLEncoder

/**
 * Crash reports, like the PC apps': when the app crashes, a short report (the version, Android,
 * the phone's model, and the top of the error) is kept; the next start offers to open a GitHub
 * issue filled in with it, and you decide whether to send it. Nothing is ever sent on its own.
 */
object CrashReport {
    private fun file(ctx: Context) = File(ctx.filesDir, "crash-report.txt")

    /** Notes crashes from now on (then lets Android close the app as usual). */
    fun watch(ctx: Context) {
        val app = ctx.applicationContext
        val before = Thread.getDefaultUncaughtExceptionHandler()
        if (before is Watcher) return
        Thread.setDefaultUncaughtExceptionHandler(Watcher(app, before))
    }

    private class Watcher(val app: Context, val before: Thread.UncaughtExceptionHandler?) : Thread.UncaughtExceptionHandler {
        override fun uncaughtException(t: Thread, e: Throwable) {
            runCatching {
                val version = app.packageManager.getPackageInfo(app.packageName, 0).versionName
                val stack = e.stackTraceToString().lineSequence().take(25).joinToString("\n")
                file(app).writeText(
                    "App: RyM for Android $version\nSystem: Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), ${Build.MANUFACTURER} ${Build.MODEL}\n" +
                        "Thread: ${t.name}\nWhat: ${e.javaClass.simpleName}: ${e.message.orEmpty().take(300)}\nWhen: ${System.currentTimeMillis() / 1000} (seconds since 1970)\n\n$stack",
                )
            }
            before?.uncaughtException(t, e)
        }
    }

    /** The report of the last crash, if one is waiting. */
    fun pending(ctx: Context): String? = file(ctx).takeIf { it.exists() }?.readText()?.takeIf { it.isNotBlank() }

    /** The waiting report is dealt with (sent or not): it isn't offered again. */
    fun dismiss(ctx: Context) {
        file(ctx).delete()
    }

    /** A new GitHub issue on RyM's page, filled in with [report] (you press "Submit"). */
    fun issueUrl(report: String): String {
        val what = report.lineSequence().firstOrNull { it.startsWith("What: ") }?.removePrefix("What: ") ?: "a crash"
        val title = "Crash (phone): $what".take(100)
        val body = "RyM closed unexpectedly. What it noted:\n\n```\n${report.take(5000)}\n```\n\nWhat were you doing when it happened? (optional)\n"
        fun enc(s: String) = URLEncoder.encode(s, "UTF-8").replace("+", "%20")
        return "https://github.com/LuJhOn67/RyM/issues/new?title=${enc(title)}&body=${enc(body)}"
    }
}
