package com.xiwei.writerapp.data

import android.content.Context
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.xiwei.writerapp.model.SyncStatus

class AutoSyncWorker(
    appContext: Context,
    workerParams: WorkerParameters
) : CoroutineWorker(appContext, workerParams) {
    override suspend fun doWork(): Result {
        val settingsRepository = SettingsRepository(applicationContext)
        val config = try {
            settingsRepository.loadSyncConfig()
        } catch (e: Exception) {
            Log.w(TAG, "Unable to load sync config", e)
            return Result.success()
        }
        val secrets = try {
            settingsRepository.loadSyncSecrets()
        } catch (e: Exception) {
            Log.w(TAG, "Unable to load sync secrets", e)
            return Result.success()
        }
        if (!AutoSyncScheduler.shouldSync(config, secrets)) return Result.success()

        val state = try {
            settingsRepository.loadSyncState()
        } catch (e: Exception) {
            Log.w(TAG, "Unable to load sync state", e)
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
        if (!SyncSession.lock.compareAndSet(false, true)) return Result.retry()

        val taskId = SyncSession.currentTaskId.incrementAndGet()
        return try {
            when (val result = settingsRepository.performSync(config)) {
                is BridgeResult.Error -> {
                    Log.w(TAG, "AutoSync failed: ${result.message}")
                    Result.retry()
                }
                BridgeResult.NotLoaded -> {
                    Log.w(TAG, "AutoSync skipped: native core not loaded")
                    Result.retry()
                }
                is BridgeResult.Success -> {
                    val status = result.data.status
                    if (SyncSession.currentTaskId.get() == taskId && isSuccessfulStatus(status)) {
                        SyncChangeBus.notifyChanged()
                    }
                    Result.success()
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "AutoSync exception", e)
            Result.retry()
        } finally {
            SyncSession.lock.set(false)
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
