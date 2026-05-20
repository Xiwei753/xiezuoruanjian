package com.xiwei.writerapp.data

import android.content.Context
import android.os.Handler
import android.os.Looper

class AutoSyncScheduler(context: Context) {
    private val settingsRepository = SettingsRepository(context)
    private val handler = Handler(Looper.getMainLooper())
    private var isRunning = false

    private val checkRunnable = object : Runnable {
        override fun run() {
            if (!isRunning) return
            try {
                performAutoSyncIfNeeded()
            } catch (_: Exception) {
            }
            if (isRunning) {
                handler.postDelayed(this, 30_000L)
            }
        }
    }

    fun start() {
        if (isRunning) return
        isRunning = true
        handler.post(checkRunnable)
    }

    fun stop() {
        isRunning = false
        handler.removeCallbacks(checkRunnable)
    }

    private fun performAutoSyncIfNeeded() {
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
                        is NativeResult.Error -> {
                            System.err.println("AutoSync failed: ${result.message}")
                        }
                        NativeResult.NotLoaded -> {
                            System.err.println("AutoSync: native core not loaded")
                        }
                        is NativeResult.Success -> {}
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
