package com.xiwei.sujian.app

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.sujian.app.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.app.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.core.interop.app.BridgeProvider
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.sync.AutoSyncScheduler
import com.xiwei.sujian.core.platform.AndroidDataRoot
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter

class SujianApplication : Application(), DefaultLifecycleObserver, com.xiwei.sujian.app.SujianAppDependenciesProvider {
    private var autoSyncScheduler: AutoSyncScheduler? = null

    /**
     * 进程级唯一依赖容器：默认线程安全 lazy 保证 UI 线程与 WorkManager
     * 后台线程首次并发访问也只构造一个实例，避免出现两份
     * SyncStatusRepository StateFlow / SyncCoordinator 互相覆盖。
     */
    private val appContainerDelegate: Lazy<com.xiwei.sujian.app.AppServiceContainer> =
        lazy { com.xiwei.sujian.app.DefaultAppServiceContainer(this) }
    val appContainer: com.xiwei.sujian.app.AppServiceContainer
        get() = appContainerDelegate.value

    override val dependencies: com.xiwei.sujian.app.SujianAppDependencies
        get() = com.xiwei.sujian.app.DefaultSujianAppDependencies(appContainer)

    override fun onCreate() {
        super<Application>.onCreate()
        // 崩溃处理器放在第一项：无论是否持有存储权限，crash 都要落到日志目录。
        installCrashHandler()
        initDiagnostics()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
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
                val crashFile = File(AndroidDataRoot.logsDir(), "last_crash.txt")
                crashFile.parentFile?.mkdirs()
                val writer = PrintWriter(FileWriter(crashFile, false))
                writer.println(
                    "Crash at ${java.text.SimpleDateFormat(
                        "yyyy-MM-dd HH:mm:ss",
                        java.util.Locale.US,
                    ).format(java.util.Date())}",
                )
                writer.println("Thread: ${thread.name}")
                writer.println()
                val redactedTrace = DiagnosticsLogger.redactStackTrace(throwable)
                writer.println(redactedTrace)
                writer.flush()
                writer.close()
                DiagnosticsLogger.e("SujianApp", "Uncaught exception", throwable)
                DiagnosticsLogger.flush()
            } catch (_: Exception) {
            }
            if (defaultHandler != null) {
                defaultHandler.uncaughtException(thread, throwable)
            } else {
                android.os.Process.killProcess(android.os.Process.myPid())
                kotlin.system.exitProcess(10)
            }
        }
    }

    override fun onStart(owner: LifecycleOwner) {
        // 没有外部存储权限时（例如首次启动跳转授权页）不得触碰
        // appContainer / BridgeProvider / WriterAppServiceHolder，
        // 避免在授权前提前初始化 Rust Core（Issue #600）。
        if (!AndroidDataRoot.hasStorageAccess()) return
        com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.appLifecycle("start")
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this, appContainer.settingsRepository)
        }
        autoSyncScheduler?.start()
    }

    override fun onStop(owner: LifecycleOwner) {
        // 同 onStart：授权前进入后台（跳转系统授权页）不能拉起 Core。
        if (!AndroidDataRoot.hasStorageAccess()) return
        com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.appLifecycle("stop")
        autoSyncScheduler?.stop()
        val result = BridgeProvider.getStarmapBridge(this).flushAllStarmapStores()
        when (result) {
            is BridgeResult.Error ->
                DiagnosticsLogger.e(
                    "SujianApp",
                    "flushAllStarmapStores failed: ${result.fullEnvelope}",
                )
            BridgeResult.NotLoaded ->
                DiagnosticsLogger.w(
                    "SujianApp",
                    "flushAllStarmapStores skipped: native library not loaded",
                )
            is BridgeResult.Success -> {}
        }
    }
}
