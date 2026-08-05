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
        val deps = (applicationContext as? com.xiwei.sujian.runtime.SujianAppDependenciesProvider)
            ?.dependencies
            ?: return Result.failure()
        val settingsRepository = deps.settingsRepository
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

        val outcome = deps.syncCoordinator.runSync(SyncTrigger.Auto)
        return when (outcome) {
            is com.xiwei.sujian.data.SyncOutcome.Completed -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "completed")
                Result.success()
            }
            is com.xiwei.sujian.data.SyncOutcome.Unconfigured,
            is com.xiwei.sujian.data.SyncOutcome.Disabled -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "unconfigured")
                Result.success()
            }
            is com.xiwei.sujian.data.SyncOutcome.Busy -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "busy")
                Result.retry()
            }
            is com.xiwei.sujian.data.SyncOutcome.RetryableFailure -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "retryable_failure")
                Result.retry()
            }
            is com.xiwei.sujian.data.SyncOutcome.TerminalFailure -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "terminal_failure")
                Result.failure()
            }
        }
    }

    companion object {
        private const val TAG = "AutoSyncWorker"
        private const val DEFAULT_INTERVAL_SECONDS = 300L
    }
}
