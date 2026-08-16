package com.xiwei.sujian.feature.sync.work
import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import java.util.concurrent.TimeUnit

/**
 * #630 评论 #1：AutoSyncScheduler 调度全量自动同步。
 *
 * 保持单一周期任务（AutoSyncWorker 内部读一份全局 profile 后调用一次 runFullSync），
 * scheduleFromSettings 始终以默认间隔（15 分钟，WorkManager 最小周期）调度周期任务，
 * Worker 内部按全局 config.syncIntervalSeconds 判定是否到点。
 */
class AutoSyncScheduler(context: Context, private val syncRepository: SyncRepository? = null) {
    private val appContext = context.applicationContext

    fun start() {
        DiagnosticsEvents.syncEvent("scheduler", "start")
        val repo = syncRepository
        if (repo != null) {
            kotlinx.coroutines.runBlocking { scheduleFromSettings(appContext, repo) }
        }
        enqueueForegroundCheck(appContext)
    }

    fun stop() {
        DiagnosticsEvents.syncEvent("scheduler", "stop")
        // Cancel the immediate foreground check when entering background to stop/downgrade sync.
        val workManager = WorkManager.getInstance(appContext)
        workManager.cancelUniqueWork(UNIQUE_FOREGROUND_WORK)
    }

    companion object {
        private const val TAG = "AutoSyncScheduler"
        private const val UNIQUE_PERIODIC_WORK = "writer_auto_sync_periodic"
        private const val UNIQUE_FOREGROUND_WORK = "writer_auto_sync_foreground"
        internal const val DEFAULT_INTERVAL_SECONDS = 300L

        /**
         * #630 评论 #1：调度单一周期任务，Worker 内部读一份全局 profile 后调用一次 runFullSync。
         *
         * 用默认间隔（15 分钟，WorkManager 最小周期）调度周期任务，保证 Worker 被定期唤醒。
         * 调用方（SyncRepository.commitSyncProfile）在 commitExclusive 释放后调用。
         */
        @Suppress("UNUSED_PARAMETER")
        fun scheduleFromSettings(
            context: Context,
            syncRepository: SyncRepository,
        ) {
            val appContext = context.applicationContext
            // 始终调度周期任务 — Worker 内部遍历作品，无配置作品时 Worker 直接 success。
            // syncRepository 参数保留以维持调用方契约（commitSyncProfile 传入应用容器仓库）。
            val intervalMinutes = 15L
            val request =
                PeriodicWorkRequestBuilder<AutoSyncWorker>(intervalMinutes, TimeUnit.MINUTES)
                    .setConstraints(networkConstraints())
                    .build()

            WorkManager.getInstance(appContext).enqueueUniquePeriodicWork(
                UNIQUE_PERIODIC_WORK,
                ExistingPeriodicWorkPolicy.UPDATE,
                request,
            )
        }

        fun enqueueForegroundCheck(context: Context) {
            val request =
                OneTimeWorkRequestBuilder<AutoSyncWorker>()
                    .setInitialDelay(1500L, TimeUnit.MILLISECONDS)
                    .setConstraints(networkConstraints())
                    .build()

            WorkManager.getInstance(context.applicationContext).enqueueUniqueWork(
                UNIQUE_FOREGROUND_WORK,
                ExistingWorkPolicy.REPLACE,
                request,
            )
        }

        fun cancel(context: Context) {
            val workManager = WorkManager.getInstance(context.applicationContext)
            workManager.cancelUniqueWork(UNIQUE_PERIODIC_WORK)
            workManager.cancelUniqueWork(UNIQUE_FOREGROUND_WORK)
        }

        private fun networkConstraints(): Constraints {
            return Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                .build()
        }

        internal fun shouldSync(
            config: SyncConfig,
            secrets: SyncSecrets,
        ): Boolean {
            if (config.enabled != true) return false
            if (config.autoSync != true) return false
            if (config.remoteUrl.isNullOrEmpty()) return false
            if (secrets.token.isNullOrEmpty()) return false
            return true
        }

        /**
         * #630 评论 #1：全量同步 interval/elapsed 纯函数判定。
         *
         * - intervalSeconds 为 null 或 <= 0 时使用 [DEFAULT_INTERVAL_SECONDS]；
         * - lastSyncTime 为 null 或 <= 0 时视为从未同步，返回 true；
         * - 否则返回 (now - lastSyncTime) >= interval。
         */
        internal fun shouldSyncByInterval(
            intervalSeconds: Long?,
            lastSyncTime: Long?,
            nowEpochSeconds: Long,
        ): Boolean {
            val interval =
                if (intervalSeconds != null && intervalSeconds > 0) {
                    intervalSeconds
                } else {
                    DEFAULT_INTERVAL_SECONDS
                }
            val elapsed =
                if (lastSyncTime != null && lastSyncTime > 0) {
                    nowEpochSeconds - lastSyncTime
                } else {
                    null
                }
            return elapsed == null || elapsed >= interval
        }
    }
}
