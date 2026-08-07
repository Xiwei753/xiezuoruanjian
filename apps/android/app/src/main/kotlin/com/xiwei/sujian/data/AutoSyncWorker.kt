package com.xiwei.sujian.data

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.SyncTrigger

class AutoSyncWorker(
    appContext: Context,
    workerParams: WorkerParameters,
) : CoroutineWorker(appContext, workerParams) {
    override suspend fun doWork(): Result {
        val deps =
            (applicationContext as? com.xiwei.sujian.runtime.SujianAppDependenciesProvider)
                ?.dependencies
                ?: return Result.failure()
        val settingsRepository = deps.settingsRepository
        // #592 六：一次只读取一份完整不可变快照（generation + config + secrets），
        // 后续整个操作（shouldSync 判定 + runSync）只使用这份 snapshot。
        val snapshotResult =
            try {
                SyncProfileGate.snapshotExclusive { settingsRepository.snapshotSyncProfile() }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync profile snapshot", e)
                return Result.retry()
            }
        val snapshot =
            when (snapshotResult) {
                is SyncProfileReadResult.Found -> snapshotResult.snapshot
                is SyncProfileReadResult.NotConfigured -> snapshotResult.snapshot
                is SyncProfileReadResult.Failed -> {
                    DiagnosticsLogger.w(TAG, "Sync profile snapshot failed: ${snapshotResult.message}")
                    // #595 四：按失败类型映射 — 临时网络/IO/原生库故障交给 WorkManager
                    // 退避重试；Fatal/协议/配置损坏是确定性失败，重试没有意义。
                    return if (snapshotResult.kind.isTransientReadFailure()) {
                        Result.retry()
                    } else {
                        Result.failure()
                    }
                }
            }
        if (!AutoSyncScheduler.shouldSync(snapshot.config, snapshot.secrets)) return Result.success()

        // #600：sync 已改为 per-project — 后台自动同步针对当前活动作品。
        // 无活动作品时无需同步（用户未打开任何作品）。
        val projectId = ActiveProjectGate.currentProjectId() ?: return Result.success()

        val state =
            try {
                settingsRepository.loadSyncState(projectId)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync state", e)
                return Result.retry()
            }

        val interval =
            when {
                snapshot.config.syncIntervalSeconds != null && snapshot.config.syncIntervalSeconds > 0 ->
                    snapshot.config.syncIntervalSeconds.toLong()
                else -> DEFAULT_INTERVAL_SECONDS
            }
        val elapsed =
            if (state.lastSyncTime != null && state.lastSyncTime > 0) {
                (System.currentTimeMillis() / 1000) - state.lastSyncTime
            } else {
                null
            }
        if (elapsed != null && elapsed < interval) return Result.success()

        val outcome = deps.syncCoordinator.runSync(SyncTrigger.Auto, projectId, snapshot)
        return when (outcome) {
            is com.xiwei.sujian.data.SyncOutcome.Completed -> {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.syncEvent("autosync", "completed")
                Result.success()
            }
            is com.xiwei.sujian.data.SyncOutcome.Unconfigured,
            is com.xiwei.sujian.data.SyncOutcome.Disabled,
            -> {
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
