package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.sujian.model.SyncStatus

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

        val exclusiveResult = SyncSession.runExclusive { taskId ->
            // 自动同步开始立即变黄；结束由成功/失败状态发布绿/红。
            SyncStatusRepository.notifySyncStarted()
            when (val result = settingsRepository.performSync(config)) {
                is BridgeResult.Error -> {
                    DiagnosticsLogger.w(TAG, "AutoSync failed: ${result.fullEnvelope}")
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "failed")
                    Result.retry()
                }
                BridgeResult.NotLoaded -> {
                    DiagnosticsLogger.w(TAG, "AutoSync skipped: native core not loaded")
                    Result.retry()
                }
                is BridgeResult.Success -> {
                    val status = result.data.status
                    if (isSuccessfulStatus(status)) {
                        SyncStatusRepository.notifySyncSuccess()
                    } else {
                        SyncStatusRepository.notifySyncFailed()
                    }
                    Result.success()
                }
            }
        }

        return when (exclusiveResult) {
            is ExclusiveResult.Busy -> Result.retry()
            is ExclusiveResult.Success -> exclusiveResult.value
        }
    }

    private fun isSuccessfulStatus(status: SyncStatus): Boolean {
        return status == SyncStatus.Success ||
            status == SyncStatus.NoChanges ||
            status == SyncStatus.LatestWinsApplied ||
            status == SyncStatus.BranchMissingRecovered
    }

    companion object {
        private const val TAG = "AutoSyncWorker"
        private const val DEFAULT_INTERVAL_SECONDS = 300L
    }
}
