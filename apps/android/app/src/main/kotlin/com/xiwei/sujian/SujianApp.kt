package com.xiwei.sujian

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.sujian.data.AutoSyncScheduler
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.diagnostics.EditorEventRingBuffer
import java.io.File
import java.io.PrintWriter
import java.io.FileWriter

class SujianApp : Application(), DefaultLifecycleObserver, com.xiwei.sujian.runtime.SujianAppDependenciesProvider {

    private var autoSyncScheduler: AutoSyncScheduler? = null

    /**
     * 进程级唯一依赖容器：默认线程安全 lazy 保证 UI 线程与 WorkManager
     * 后台线程首次并发访问也只构造一个实例，避免出现两份
     * SyncStatusRepository StateFlow / SyncCoordinator 互相覆盖。
     */
    private val dependenciesDelegate: Lazy<com.xiwei.sujian.runtime.SujianAppDependencies> =
        lazy { com.xiwei.sujian.runtime.DefaultSujianAppDependencies(this) }
    override val dependencies: com.xiwei.sujian.runtime.SujianAppDependencies
        get() = dependenciesDelegate.value

    override fun onCreate() {
        super<Application>.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
        initDiagnostics()
        installCrashHandler()
    }

    private fun initDiagnostics() {
        val diagPrefs = getSharedPreferences("sujian_diagnostics", MODE_PRIVATE)
        val diagnosticsEnabled = diagPrefs.getBoolean("diagnostics_enabled", true)
        val diagnosticsVerbose = diagPrefs.getBoolean("diagnostics_verbose", true)
        DiagnosticsLogger.init(this, diagnosticsEnabled, diagnosticsVerbose)
        EditorEventRingBuffer.setEnabled(diagnosticsEnabled)
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
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.appLifecycle("start")
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this)
        }
        autoSyncScheduler?.start()
    }

    override fun onStop(owner: LifecycleOwner) {
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.appLifecycle("stop")
        autoSyncScheduler?.stop()
        val result = BridgeProvider.getStarmapBridge(this).flushAllStarmapStores()
        when (result) {
            is BridgeResult.Error -> DiagnosticsLogger.e("SujianApp", "flushAllStarmapStores failed: ${result.fullEnvelope}")
            BridgeResult.NotLoaded -> DiagnosticsLogger.w("SujianApp", "flushAllStarmapStores skipped: native library not loaded")
            is BridgeResult.Success -> {}
        }
    }
}
