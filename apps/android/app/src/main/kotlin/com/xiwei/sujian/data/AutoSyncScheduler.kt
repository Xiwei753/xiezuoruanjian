package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequest
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncSecrets
import java.util.concurrent.TimeUnit

class AutoSyncScheduler(context: Context) {
    private val appContext = context.applicationContext

    fun start() {
        scheduleFromSettings(appContext)
        enqueueForegroundCheck(appContext)
    }

    fun stop() {
        // Cancel the immediate foreground check when entering background to stop/downgrade sync.
        val workManager = WorkManager.getInstance(appContext)
        workManager.cancelUniqueWork(UNIQUE_FOREGROUND_WORK)
    }

    companion object {
        private const val TAG = "AutoSyncScheduler"
        private const val UNIQUE_PERIODIC_WORK = "writer_auto_sync_periodic"
        private const val UNIQUE_FOREGROUND_WORK = "writer_auto_sync_foreground"
        private const val DEFAULT_INTERVAL_SECONDS = 300L

        fun scheduleFromSettings(context: Context) {
            val appContext = context.applicationContext
            val settingsRepository = SettingsRepository(appContext)
            val config = try {
                settingsRepository.loadSyncConfig()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync config for scheduling", e)
                cancel(appContext)
                return
            }
            val secrets = try {
                settingsRepository.loadSyncSecrets()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Unable to load sync secrets for scheduling", e)
                cancel(appContext)
                return
            }

            if (!shouldSync(config, secrets)) {
                cancel(appContext)
                return
            }

            val intervalMinutes = ((config.syncIntervalSeconds ?: DEFAULT_INTERVAL_SECONDS).toLong())
                .coerceAtLeast(PeriodicWorkRequest.MIN_PERIODIC_INTERVAL_MILLIS / 1000)
                .let { TimeUnit.SECONDS.toMinutes(it).coerceAtLeast(15L) }
            val request = PeriodicWorkRequestBuilder<AutoSyncWorker>(intervalMinutes, TimeUnit.MINUTES)
                .setConstraints(networkConstraints())
                .build()

            WorkManager.getInstance(appContext).enqueueUniquePeriodicWork(
                UNIQUE_PERIODIC_WORK,
                ExistingPeriodicWorkPolicy.UPDATE,
                request
            )
        }

        fun enqueueForegroundCheck(context: Context) {
            val request = OneTimeWorkRequestBuilder<AutoSyncWorker>()
                .setInitialDelay(1500L, TimeUnit.MILLISECONDS)
                .setConstraints(networkConstraints())
                .build()

            WorkManager.getInstance(context.applicationContext).enqueueUniqueWork(
                UNIQUE_FOREGROUND_WORK,
                ExistingWorkPolicy.REPLACE,
                request
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

        internal fun shouldSync(config: SyncConfig, secrets: SyncSecrets): Boolean {
            if (config.enabled != true) return false
            if (config.autoSync != true) return false
            if (config.remoteUrl.isNullOrEmpty()) return false
            if (secrets.token.isNullOrEmpty()) return false
            return true
        }
    }
}
