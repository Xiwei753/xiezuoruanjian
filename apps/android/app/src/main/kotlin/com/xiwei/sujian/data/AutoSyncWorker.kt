package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.sujian.model.SyncTrigger

class AutoSyncWorker(
    appContext: Context,
    workerParams: WorkerParameters
) : CoroutineWorker(appContext, workerParams) {
    override suspend fun doWork(): Result {
        val settingsRepository = SettingsRepository(applicationContext)
        val config = try {
            settingsRepository.loadSyncConfig()
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Unable to load sync config", e)
            return Result.success()
        }
        val secrets = try {
            settingsRepository.loadSyncSecrets()
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Unable to load sync secrets", e)
            return Result.success()
        }
        if (!AutoSyncScheduler.shouldSync(config, secrets)) return Result.success()

        val state = try {
            settingsRepository.loadSyncState()
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Unable to load sync state", e)
            return Result.retry()
        }

        val interval = when {
            config.syncIntervalSeconds != null && config.syncIntervalSeconds > 0 -> config.syncIntervalSeconds.toLong()
            else -> DEFAULT_INTERVAL_SECONDS
        }
        val elapsed = if (state.lastSyncTime != null && state.lastSyncTime > 0) {
            (System.currentTimeMillis() / 1000) - state.lastSyncTime
        } else {
            null
        }
        if (elapsed != null && elapsed < interval) return Result.success()

        SyncCoordinator.initialize(settingsRepository)
        val result = SyncCoordinator.runSync(SyncTrigger.Auto)
        return if (result != null) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "completed")
            Result.success()
        } else {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "skipped")
            Result.retry()
        }
    }

    companion object {
        private const val TAG = "AutoSyncWorker"
        private const val DEFAULT_INTERVAL_SECONDS = 300L
    }
}
