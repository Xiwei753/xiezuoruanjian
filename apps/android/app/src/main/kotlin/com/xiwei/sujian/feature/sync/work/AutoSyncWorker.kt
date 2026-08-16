package com.xiwei.sujian.feature.sync.work
import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.sync.data.SyncOutcome
import com.xiwei.sujian.feature.sync.data.SyncProfileGate
import com.xiwei.sujian.feature.sync.data.SyncProfileReadResult
import com.xiwei.sujian.feature.sync.data.SyncProfileSnapshot
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger

/**
 * #630 评论 #1：全量自动同步 Worker。
 *
 * 每次只读一个全局 profile、判断一次 enabled/autoSync/interval，然后调用一次
 * [com.xiwei.sujian.feature.sync.data.SyncCoordinator.runFullSync]（Auto）。
 * 作品枚举由 Core 的 full-sync 完成，Android 不再自己循环调用两套 API。
 */
class AutoSyncWorker(
    appContext: Context,
    workerParams: WorkerParameters,
) : CoroutineWorker(appContext, workerParams) {
    override suspend fun doWork(): Result {
        // 没有外部存储权限时（例如首次启动尚未授权）不得触碰 appContainer /
        // AppServiceProvider / WriterAppServiceHolder，避免提前初始化 Rust Core（Issue #600）。
        if (!com.xiwei.sujian.core.platform.storage.AndroidDataRoot.hasStorageAccess()) {
            return Result.success()
        }
        val deps =
            (applicationContext as? com.xiwei.sujian.app.di.SujianAppDependenciesProvider)
                ?.dependencies
                ?: return Result.failure()

        val settingsRepository = deps.syncRepository
        // #630 评论 #1：只读一份全局 profile snapshot。
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
                    DiagnosticsLogger.w(
                        TAG,
                        "Sync profile snapshot failed: ${snapshotResult.message}",
                    )
                    return if (snapshotResult.kind.isTransientReadFailure()) {
                        Result.retry()
                    } else {
                        Result.success()
                    }
                }
            }
        if (!AutoSyncScheduler.shouldSync(snapshot.config, snapshot.secrets)) return Result.success()
        if (!shouldSyncNow(settingsRepository, snapshot)) return Result.success()

        val outcome = deps.syncCoordinator.runFullSync(SyncTrigger.Auto, snapshot)
        return when (outcome) {
            is SyncOutcome.Completed -> {
                DiagnosticsEvents.syncEvent("autosync", "completed")
                Result.success()
            }
            is SyncOutcome.Unconfigured,
            is SyncOutcome.Disabled,
            -> {
                DiagnosticsEvents.syncEvent("autosync", "unconfigured")
                Result.success()
            }
            is SyncOutcome.Busy -> {
                DiagnosticsEvents.syncEvent("autosync", "busy")
                Result.retry()
            }
            is SyncOutcome.RetryableFailure -> {
                DiagnosticsEvents.syncEvent("autosync", "retryable_failure")
                Result.retry()
            }
            is SyncOutcome.TerminalFailure -> {
                DiagnosticsEvents.syncEvent("autosync", "terminal_failure")
                Result.failure()
            }
        }
    }

    /**
     * #630 评论 #1：判定全量同步是否到时间点（interval/elapsed 检查）。
     *
     * 全量同步间隔由全局 config.syncIntervalSeconds 决定；上次同步时间取自
     * App target 的本地 state（<app_data_root>/app-meta/sync/state.local.json），
     * 因为全量同步的总体完成时间由 App target 状态最贴近。
     */
    private fun shouldSyncNow(
        settingsRepository: com.xiwei.sujian.feature.sync.data.SyncRepository,
        snapshot: SyncProfileSnapshot,
    ): Boolean {
        val state =
            try {
                settingsRepository.loadAppSyncState()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load app sync state", e)
                return false
            }
        return AutoSyncScheduler.shouldSyncByInterval(
            intervalSeconds = snapshot.config.syncIntervalSeconds?.toLong(),
            lastSyncTime = state.lastSyncTime,
            nowEpochSeconds = System.currentTimeMillis() / 1000,
        )
    }

    companion object {
        private const val TAG = "AutoSyncWorker"
    }
}
