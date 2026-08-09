package com.xiwei.sujian.feature.sync.work
import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import com.xiwei.sujian.feature.sync.data.model.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.AppSyncProfileReadResult
import com.xiwei.sujian.feature.sync.data.AppSyncProfileSnapshot
import com.xiwei.sujian.feature.sync.data.ProjectSyncProfileSnapshot
import com.xiwei.sujian.feature.sync.data.SyncOutcome
import com.xiwei.sujian.feature.sync.data.SyncProfileGate
import com.xiwei.sujian.feature.sync.data.SyncProfileReadResult

/**
 * #600 评论 #3 问题三：自动同步不再依赖 ActiveProjectGate（进程重启后 null）。
 *
 * doWork 遍历 ProjectRepository.getProjects()，逐个读取 snapshotSyncProfile(projectId)，
 * 只处理 enabled && autoSync 的作品，分别调用 runSync(..., projectId, snapshot)。
 * 一个 WorkManager 周期任务负责所有已配置作品，不需要给每个作品造一套 Worker。
 */
class AutoSyncWorker(
    appContext: Context,
    workerParams: WorkerParameters,
) : CoroutineWorker(appContext, workerParams) {
    override suspend fun doWork(): Result {
        // 没有外部存储权限时（例如首次启动尚未授权）不得触碰 appContainer /
        // AppServiceProvider / WriterAppServiceHolder，避免提前初始化 Rust Core（Issue #600）。
        if (!com.xiwei.sujian.core.platform.AndroidDataRoot.hasStorageAccess()) {
            return Result.success()
        }
        val deps =
            (applicationContext as? com.xiwei.sujian.app.di.SujianAppDependenciesProvider)
                ?.dependencies
                ?: return Result.failure()

        // #600 评论 #4 问题三：先执行应用级同步（设置/全局星图/主题调色板），
        // 再遍历所有作品逐个同步。应用级同步目标唯一，不依赖 ActiveProjectGate。
        val appOutcome = syncApp(deps)

        // #600 评论 #3 问题三：遍历所有作品，逐个尝试自动同步。
        // 不再依赖 ActiveProjectGate.currentProjectId()（进程重启后 null）。
        val projects =
            try {
                deps.projectRepository.getProjects()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to list projects for auto sync", e)
                return Result.retry()
            }

        var anyTransientFailure = false
        var anyTerminalFailure = false
        // 应用级同步结果纳入整体 outcome 聚合。
        when (appOutcome) {
            ProjectSyncOutcome.TransientFailure -> anyTransientFailure = true
            ProjectSyncOutcome.TerminalFailure -> anyTerminalFailure = true
            else -> { }
        }
        for (project in projects) {
            val outcome = syncOneProject(deps, project.id)
            when (outcome) {
                ProjectSyncOutcome.TransientFailure -> anyTransientFailure = true
                ProjectSyncOutcome.TerminalFailure -> anyTerminalFailure = true
                else -> { }
            }
        }

        // 任一目标（应用级或作品级）出现确定性失败时整体 failure；
        // 仅临时故障时 retry；全部成功/跳过时 success。
        return when {
            anyTerminalFailure -> Result.failure()
            anyTransientFailure -> Result.retry()
            else -> Result.success()
        }
    }

    /**
     * #600 评论 #4/#5：应用级自动同步 — 设置/全局星图/主题调色板。
     *
     * 读 [SyncRepository.snapshotAppSyncProfile]，若 enabled && autoSync 则按
     * [shouldAppSyncNow] 判断是否到同步时间点，到点后调用 [SyncCoordinator.runAppSync]。
     *
     * 返回与作品级相同的 [ProjectSyncOutcome] 分类，纳入 doWork 整体 outcome 聚合。
     */
    private suspend fun syncApp(deps: com.xiwei.sujian.app.di.SujianAppDependencies): ProjectSyncOutcome {
        val settingsRepository = deps.syncRepository
        val snapshotResult =
            try {
                SyncProfileGate.snapshotExclusive { settingsRepository.snapshotAppSyncProfile() }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load app sync profile snapshot", e)
                return ProjectSyncOutcome.TransientFailure
            }
        val snapshot =
            when (snapshotResult) {
                is AppSyncProfileReadResult.Found -> snapshotResult.snapshot
                is AppSyncProfileReadResult.NotConfigured -> snapshotResult.snapshot
                is AppSyncProfileReadResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "App sync profile snapshot failed: ${snapshotResult.message}",
                    )
                    return if (snapshotResult.kind.isTransientReadFailure()) {
                        ProjectSyncOutcome.TransientFailure
                    } else {
                        ProjectSyncOutcome.Skipped
                    }
                }
            }
        if (!AutoSyncScheduler.shouldSync(snapshot.config, snapshot.secrets)) return ProjectSyncOutcome.Skipped
        if (!shouldAppSyncNow(settingsRepository, snapshot)) return ProjectSyncOutcome.Skipped

        val outcome = deps.syncCoordinator.runAppSync(SyncTrigger.Auto, snapshot)
        return when (outcome) {
            is SyncOutcome.Completed -> {
                DiagnosticsEvents.syncEvent("autosync_app", "completed")
                ProjectSyncOutcome.Success
            }
            is SyncOutcome.Unconfigured,
            is SyncOutcome.Disabled,
            -> {
                DiagnosticsEvents.syncEvent("autosync_app", "unconfigured")
                ProjectSyncOutcome.Skipped
            }
            is SyncOutcome.Busy -> {
                DiagnosticsEvents.syncEvent("autosync_app", "busy")
                ProjectSyncOutcome.TransientFailure
            }
            is SyncOutcome.RetryableFailure -> {
                DiagnosticsEvents.syncEvent("autosync_app", "retryable_failure")
                ProjectSyncOutcome.TransientFailure
            }
            is SyncOutcome.TerminalFailure -> {
                DiagnosticsEvents.syncEvent("autosync_app", "terminal_failure")
                ProjectSyncOutcome.TerminalFailure
            }
        }
    }

    /**
     * 单个作品的自动同步 — 提取为独立方法降低 doWork 长度。
     * 返回该作品的同步结果分类（成功/跳过/临时失败/确定性失败）。
     */
    private suspend fun syncOneProject(
        deps: com.xiwei.sujian.app.di.SujianAppDependencies,
        projectId: String,
    ): ProjectSyncOutcome {
        val settingsRepository = deps.syncRepository
        // #592 六：一次只读取一份完整不可变快照（generation + config + secrets）。
        val snapshotResult =
            try {
                SyncProfileGate.snapshotExclusive { settingsRepository.snapshotSyncProfile(projectId) }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync profile snapshot for project $projectId", e)
                return ProjectSyncOutcome.TransientFailure
            }
        val snapshot =
            when (snapshotResult) {
                is SyncProfileReadResult.Found -> snapshotResult.snapshot
                is SyncProfileReadResult.NotConfigured -> snapshotResult.snapshot
                is SyncProfileReadResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "Sync profile snapshot failed for project $projectId: ${snapshotResult.message}",
                    )
                    // #595 四：临时故障交给 WorkManager 退避重试；确定性失败跳过该作品。
                    return if (snapshotResult.kind.isTransientReadFailure()) {
                        ProjectSyncOutcome.TransientFailure
                    } else {
                        ProjectSyncOutcome.Skipped
                    }
                }
            }
        if (!AutoSyncScheduler.shouldSync(snapshot.config, snapshot.secrets)) return ProjectSyncOutcome.Skipped

        if (!shouldSyncNow(settingsRepository, projectId, snapshot)) return ProjectSyncOutcome.Skipped

        val outcome = deps.syncCoordinator.runSync(SyncTrigger.Auto, projectId, snapshot)
        return when (outcome) {
            is SyncOutcome.Completed -> {
                DiagnosticsEvents.syncEvent("autosync", "completed")
                ProjectSyncOutcome.Success
            }
            is SyncOutcome.Unconfigured,
            is SyncOutcome.Disabled,
            -> {
                DiagnosticsEvents.syncEvent("autosync", "unconfigured")
                ProjectSyncOutcome.Skipped
            }
            is SyncOutcome.Busy -> {
                DiagnosticsEvents.syncEvent("autosync", "busy")
                ProjectSyncOutcome.TransientFailure
            }
            is SyncOutcome.RetryableFailure -> {
                DiagnosticsEvents.syncEvent("autosync", "retryable_failure")
                ProjectSyncOutcome.TransientFailure
            }
            is SyncOutcome.TerminalFailure -> {
                DiagnosticsEvents.syncEvent("autosync", "terminal_failure")
                ProjectSyncOutcome.TerminalFailure
            }
        }
    }

    /** 判定该作品是否到同步时间点（interval/elapsed 检查）。 */
    private suspend fun shouldSyncNow(
        settingsRepository: SyncRepository,
        projectId: String,
        snapshot: ProjectSyncProfileSnapshot,
    ): Boolean {
        val state =
            try {
                settingsRepository.loadSyncState(projectId)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync state for project $projectId", e)
                return false
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
        return elapsed == null || elapsed >= interval
    }

    /**
     * #600 评论 #5：判定应用级同步是否到时间点（interval/elapsed 检查）。
     * 镜像 [shouldSyncNow] 但使用应用级 sync state（<app_data_root>/app-meta/sync/state.local.json）。
     */
    private suspend fun shouldAppSyncNow(
        settingsRepository: SyncRepository,
        snapshot: AppSyncProfileSnapshot,
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

    /** 单个作品同步结果分类。 */
    private enum class ProjectSyncOutcome {
        Success,
        Skipped,
        TransientFailure,
        TerminalFailure,
    }

    companion object {
        private const val TAG = "AutoSyncWorker"
        private const val DEFAULT_INTERVAL_SECONDS = 300L
    }
}
