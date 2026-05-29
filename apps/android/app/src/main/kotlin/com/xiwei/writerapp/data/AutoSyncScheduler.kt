package com.xiwei.writerapp.data

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.xiwei.writerapp.model.SyncConfig
import com.xiwei.writerapp.model.SyncSecrets

/**
 * AutoSyncScheduler — 自动同步调度器
 *
 * 根据用户设置的同步间隔，自动触发后台同步任务。
 *
 * ## 架构定位
 * - WriterApp → AutoSyncScheduler → SettingsRepository → SyncBridge → Rust Core
 *
 * ## 职责边界
 * - **做**：定时检查同步配置、触发自动同步、网络状态检测
 * - **不做**：实际同步操作（由 Rust Core 负责）
 *
 * ## 使用场景
 * - 应用进入前台时启动调度
 * - 应用进入后台时停止调度
 * - 根据 sync_interval_seconds 定时触发同步
 */
class AutoSyncScheduler(context: Context) {
    private val settingsRepository = SettingsRepository(context)
    private val handler = Handler(Looper.getMainLooper())
    private var scheduled = false

    fun start() {
        if (scheduled) return
        scheduled = true
        handler.postDelayed({
            scheduled = false
            try {
                performAutoSyncIfNeeded("app_foreground")
            } catch (_: Exception) {
            }
        }, 1500L)
    }

    fun stop() {
        scheduled = false
        handler.removeCallbacksAndMessages(null)
    }

    private fun performAutoSyncIfNeeded(reason: String) {
        val config = try {
            settingsRepository.loadSyncConfig()
        } catch (_: Exception) {
            return
        }
        val secrets = try {
            settingsRepository.loadSyncSecrets()
        } catch (_: Exception) {
            return
        }

        if (!shouldSync(config, secrets)) return

        val state = try {
            settingsRepository.loadSyncState()
        } catch (_: Exception) {
            return
        }

        val elapsed = if (state.lastSyncTime != null && state.lastSyncTime > 0) {
            (System.currentTimeMillis() / 1000) - state.lastSyncTime
        } else {
            null
        }

        val interval = when {
            config.syncIntervalSeconds != null && config.syncIntervalSeconds > 0 -> config.syncIntervalSeconds.toLong()
            else -> 300L
        }

        if (elapsed != null && elapsed < interval) return

        if (!SyncSession.lock.compareAndSet(false, true)) return

        val taskId = SyncSession.currentTaskId.incrementAndGet()

        Thread {
            try {
                val result = settingsRepository.performSync(config)
                if (SyncSession.currentTaskId.get() == taskId) {
                    when (result) {
                        is BridgeResult.Error -> {
                            System.err.println("AutoSync failed: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            System.err.println("AutoSync: native core not loaded")
                        }
                        is BridgeResult.Success -> {
                            val syncResult = result.data
                            val ok = syncResult.status == com.xiwei.writerapp.model.SyncStatus.Success ||
                                syncResult.status == com.xiwei.writerapp.model.SyncStatus.NoChanges ||
                                syncResult.status == com.xiwei.writerapp.model.SyncStatus.LatestWinsApplied ||
                                syncResult.status == com.xiwei.writerapp.model.SyncStatus.BranchMissingRecovered
                            if (ok) {
                                SyncChangeBus.notifyChanged()
                                System.out.println("AutoSync success: reason=$reason")
                            }
                        }
                    }
                }
            } catch (e: Exception) {
                System.err.println("AutoSync exception: ${e.message}")
            } finally {
                SyncSession.lock.set(false)
            }
        }.start()
    }

    private fun shouldSync(config: SyncConfig, secrets: SyncSecrets): Boolean {
        if (config.enabled != true) return false
        if (config.autoSync != true) return false
        if (config.remoteUrl.isNullOrEmpty()) return false
        if (secrets.token.isNullOrEmpty()) return false
        return true
    }
}
