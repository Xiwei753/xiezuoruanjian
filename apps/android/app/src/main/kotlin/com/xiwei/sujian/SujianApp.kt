package com.xiwei.sujian

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.sujian.data.AutoSyncScheduler
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.diagnostics.EditorEventRingBuffer
import java.io.File
import java.io.PrintWriter
import java.io.FileWriter

class SujianApp : Application(), DefaultLifecycleObserver {

    private var autoSyncScheduler: AutoSyncScheduler? = null

    override fun onCreate() {
        super<Application>.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
        initDiagnostics()
        installCrashHandler()
    }

    private fun initDiagnostics() {
        val repo = SettingsRepository(this)
        val settings = repo.getLocalSettings()
        DiagnosticsLogger.init(this, settings.diagnosticsEnabled, settings.diagnosticsVerbose)
        EditorEventRingBuffer.setEnabled(settings.diagnosticsEnabled)
    }

    private fun installCrashHandler() {
        val defaultHandler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            try {
                val crashFile = File(filesDir, "last_crash.txt")
                val writer = PrintWriter(FileWriter(crashFile, false))
                writer.println("Crash at ${java.text.SimpleDateFormat("yyyy-MM-dd HH:mm:ss", java.util.Locale.US).format(java.util.Date())}")
                writer.println("Thread: ${thread.name}")
                writer.println()
                val redactedTrace = DiagnosticsLogger.redactStackTrace(throwable)
                writer.println(redactedTrace)
                writer.flush()
                writer.close()
                DiagnosticsLogger.e("SujianApp", "Uncaught exception", throwable)
                DiagnosticsLogger.flush()
            } catch (_: Exception) {}
            if (defaultHandler != null) {
                defaultHandler.uncaughtException(thread, throwable)
            } else {
                android.os.Process.killProcess(android.os.Process.myPid())
                kotlin.system.exitProcess(10)
            }
        }
    }

    override fun onStart(owner: LifecycleOwner) {
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this)
        }
        autoSyncScheduler?.start()
    }

    override fun onStop(owner: LifecycleOwner) {
        autoSyncScheduler?.stop()
        val result = BridgeProvider.getStarmapBridge(this).repository.flushAllStarmapStores()
        if (result is BridgeResult.Error) {
            DiagnosticsLogger.e("SujianApp", "flushAllStarmapStores failed: ${result.message}")
        }
    }
}
