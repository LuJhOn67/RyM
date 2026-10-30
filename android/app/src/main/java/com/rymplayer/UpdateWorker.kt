package com.rymplayer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.rymplayer.data.Updates
import java.util.concurrent.TimeUnit

/**
 * Looks for a new RyM on GitHub every 12 hours, even with the app closed, and says so in a
 * notification; tapping it opens the app, which shows the patch notes.
 */
class UpdateWorker(ctx: Context, params: WorkerParameters) : CoroutineWorker(ctx, params) {
    override suspend fun doWork(): Result {
        val prefs = applicationContext.getSharedPreferences("rym", Context.MODE_PRIVATE)
        if (!prefs.getBoolean("auto_updates", true)) return Result.success()
        val current = runCatching { applicationContext.packageManager.getPackageInfo(applicationContext.packageName, 0).versionName }.getOrNull().orEmpty()
        val release = runCatching { Updates.check(prefs.getBoolean("beta_updates", false), current) }.getOrElse { return Result.retry() }
        prefs.edit().putLong("last_update_check", System.currentTimeMillis()).apply()
        if (release != null && prefs.getString("notified_version", null) != release.version) {
            notify(applicationContext, release)
            prefs.edit().putString("notified_version", release.version).apply()
        }
        return Result.success()
    }

    companion object {
        private const val WORK = "rym-updates"
        private const val CHANNEL = "updates"

        /** Starts (or stops) the 12-hour checks. */
        fun schedule(ctx: Context, on: Boolean) {
            val wm = WorkManager.getInstance(ctx)
            if (!on) {
                wm.cancelUniqueWork(WORK)
                return
            }
            val request = PeriodicWorkRequestBuilder<UpdateWorker>(12, TimeUnit.HOURS)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            wm.enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun notify(ctx: Context, r: Updates.Release) {
            val nm = ctx.getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(NotificationChannel(CHANNEL, tr("Updates"), NotificationManager.IMPORTANCE_DEFAULT))
            val open = PendingIntent.getActivity(
                ctx, 7,
                Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP).putExtra("show_update", true),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            val kind = if (r.beta) tr("New beta") else tr("New update")
            val n = NotificationCompat.Builder(ctx, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_download_done)
                .setContentTitle("$kind: RyM ${r.version}")
                .setContentText(tr("Tap to see what's new"))
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            runCatching { nm.notify(7, n) }
        }
    }
}
