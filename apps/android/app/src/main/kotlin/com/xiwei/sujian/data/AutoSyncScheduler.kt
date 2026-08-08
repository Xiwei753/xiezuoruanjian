package com.xiwei.sujian.data

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import com.xiwei.sujian.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncSecrets
import java.util.concurrent.TimeUnit

/**
 * #600 评论 #3 问题三：AutoSyncScheduler 适配 per-project 自动同步。
 *
 * 保持单一周期任务（AutoSyncWorker 内部遍历所有作品），scheduleFromSettings
 * 不再读取某个作品的 snapshot 来决定调度 — 改为始终以默认间隔（15 分钟）
 * 调度周期任务，由 Worker 内部按各作品自己的 syncIntervalSeconds 判定是否到点。
 *
 * 这样避免了"读哪个作品的 snapshot 来决定全局调度间隔"的歧义，也让
 * 不同作品的不同间隔都能在 Worker 内被尊重（Worker 逐个作品检查 elapsed < interval）。
 */
class AutoSyncScheduler(context: Context, private val settingsRepository: SettingsRepository? = null) {
    private val appContext = context.applicationContext

    fun start() {
        DiagnosticsEvents.syncEvent("scheduler", "start")
        val repo = settingsRepository
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
         * #592 六 / #600 评论 #3 问题三：调度单一周期任务，Worker 内部遍历所有作品。
         *
         * 不再读取某个作品的 snapshot 来决定调度间隔 — 不同作品可有不同间隔，
         * Worker 逐个作品检查 elapsed < interval。这里用默认间隔（15 分钟，
         * WorkManager 最小周期）调度周期任务，保证 Worker 被定期唤醒。
         *
         * 调用方（SettingsRepository.commitSyncProfile）在 commitExclusive 释放后调用。
         */
        @Suppress("UNUSED_PARAMETER")
        suspend fun scheduleFromSettings(
            context: Context,
            settingsRepository: SettingsRepository,
        ) {
            val appContext = context.applicationContext
            // 始终调度周期任务 — Worker 内部遍历作品，无配置作品时 Worker 直接 success。
            // settingsRepository 参数保留以维持调用方契约（commitSyncProfile 传入应用容器仓库）。
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
         * #600 评论 #5：应用级同步 interval/elapsed 纯函数判定。
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
