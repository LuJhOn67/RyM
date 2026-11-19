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

    /** The report of the last crash, if one is waiting (from the watcher, or from Android's own
     *  record of how the app last ended). */
    fun pending(ctx: Context): String? {
        file(ctx).takeIf { it.exists() }?.readText()?.takeIf { it.isNotBlank() }?.let { report ->
            // Android noted the same crash: not offered a second time.
            runCatching { fromAndroid(ctx) }
            return report
        }
        return runCatching { fromAndroid(ctx) }.getOrNull()
    }

    /**
     * Crashes the watcher can't see (in native code, or the app frozen until Android closed it, an
     * "ANR"): Android 11 and newer keep why the app last ended. Each ending is offered once.
     */
    private fun fromAndroid(ctx: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        val am = ctx.getSystemService(android.app.ActivityManager::class.java) ?: return null
        val last = am.getHistoricalProcessExitReasons(ctx.packageName, 0, 1).firstOrNull() ?: return null
        val prefs = ctx.getSharedPreferences("crash", Context.MODE_PRIVATE)
        if (last.timestamp <= prefs.getLong("seen", 0)) return null
        prefs.edit().putLong("seen", last.timestamp).apply()
        val what = when (last.reason) {
            android.app.ApplicationExitInfo.REASON_CRASH_NATIVE -> "a crash in native code"
            android.app.ApplicationExitInfo.REASON_ANR -> "frozen, closed by Android (ANR)"
            android.app.ApplicationExitInfo.REASON_CRASH -> "a crash"
            android.app.ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "failed to start"
            else -> return null
        }
        val version = runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull()
        // What Android kept: the native crash's or the frozen threads' trace (its top part).
        val trace = runCatching { last.traceInputStream?.bufferedReader()?.useLines { it.take(60).joinToString("\n") } }.getOrNull().orEmpty()
        return "App: RyM for Android $version\nSystem: Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), ${Build.MANUFACTURER} ${Build.MODEL}\n" +
            "What: $what${last.description?.let { " ($it)" }.orEmpty()}\nWhen: ${last.timestamp / 1000} (seconds since 1970)\nMemory: ${last.pss / 1024} MB\n\n$trace"
    }

    /** The report sent with any app (WhatsApp, Telegram, email...): no GitHub account needed. */
    fun share(ctx: Context, report: String) {
        val send = android.content.Intent(android.content.Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(android.content.Intent.EXTRA_SUBJECT, "RyM crash report")
            .putExtra(android.content.Intent.EXTRA_TEXT, report.take(20000))
        runCatching { ctx.startActivity(android.content.Intent.createChooser(send, null).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)) }
    }

    /** The report copied, to paste anywhere. */
    fun copy(ctx: Context, report: String) {
        val cm = ctx.getSystemService(android.content.ClipboardManager::class.java) ?: return
        cm.setPrimaryClip(android.content.ClipData.newPlainText("RyM crash report", report))
    }

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
