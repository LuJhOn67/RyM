package com.rymplayer

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/**
 * Smart downloads: on Wi-Fi and charging, your top songs by RaM points are saved on the phone by
 * themselves (the ones not saved yet), so they play without the internet. Settings turns it on.
 */
class OfflineWorker(ctx: Context, params: WorkerParameters) : CoroutineWorker(ctx, params) {
    override suspend fun doWork(): Result {
        val ctx = applicationContext
        val prefs = ctx.getSharedPreferences("rym", Context.MODE_PRIVATE)
        if (!prefs.getBoolean(ON, false)) return Result.success()
        val saved = com.rymplayer.data.Downloads.list(ctx).map { it.videoId }.toSet()
        val top = com.rymplayer.playback.Link.topSongs(ctx, prefs.getInt(COUNT, 30))
            .filter { it.videoId !in saved && com.rymplayer.data.isYouTube(it.videoId) }
        val cookie = com.rymplayer.data.SecureCookie.read(prefs)
        val best = prefs.getBoolean("download_best", false)
        for (t in top) {
            if (isStopped) break
            runCatching { com.rymplayer.data.Downloads.save(ctx, t, cookie, best) }
        }
        return Result.success()
    }

    companion object {
        private const val WORK = "smart-downloads"
        const val ON = "smart_downloads"
        const val COUNT = "smart_downloads_count"

        fun schedule(ctx: Context, on: Boolean) {
            val wm = WorkManager.getInstance(ctx)
            if (!on) {
                wm.cancelUniqueWork(WORK)
                return
            }
            val request = PeriodicWorkRequestBuilder<OfflineWorker>(12, TimeUnit.HOURS)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.UNMETERED).setRequiresCharging(true).build())
                .build()
            wm.enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.KEEP, request)
        }
    }
}
