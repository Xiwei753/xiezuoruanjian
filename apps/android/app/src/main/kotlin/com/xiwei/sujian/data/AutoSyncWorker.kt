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
        // #592 六：一次只读取一份完整不可变快照（generation + config + secrets），
        // 后续整个操作（shouldSync 判定 + runSync）只使用这份 snapshot。
        val snapshot = try {
            SyncProfileGate.snapshotExclusive { settingsRepository.snapshotSyncProfile() }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Unable to load sync profile snapshot", e)
            return Result.retry()
        } ?: run {
            DiagnosticsLogger.w(TAG, "Sync profile snapshot unavailable")
            return Result.success()
        }
        if (!AutoSyncScheduler.shouldSync(snapshot.config, snapshot.secrets)) return Result.success()

        val state = try {
            settingsRepository.loadSyncState()
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Unable to load sync state", e)
            return Result.retry()
        }

        val interval = when {
            snapshot.config.syncIntervalSeconds != null && snapshot.config.syncIntervalSeconds > 0 -> snapshot.config.syncIntervalSeconds.toLong()
            else -> DEFAULT_INTERVAL_SECONDS
        }
        val elapsed = if (state.lastSyncTime != null && state.lastSyncTime > 0) {
            (System.currentTimeMillis() / 1000) - state.lastSyncTime
        } else {
            null
        }
        if (elapsed != null && elapsed < interval) return Result.success()

        val outcome = deps.syncCoordinator.runSync(SyncTrigger.Auto, snapshot)
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
