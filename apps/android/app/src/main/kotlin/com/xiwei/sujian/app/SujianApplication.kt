package com.xiwei.sujian.app

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.app.di.SujianAppDependenciesProvider
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter

class SujianApplication : Application(), DefaultLifecycleObserver, SujianAppDependenciesProvider {
    private var autoSyncScheduler: AutoSyncScheduler? = null

    /**
     * 进程级唯一依赖容器：默认线程安全 lazy 保证 UI 线程与 WorkManager
     * 后台线程首次并发访问也只构造一个实例，避免出现两份
     * SyncStatusRepository StateFlow / SyncCoordinator 互相覆盖。
     */
    private val appContainerDelegate: Lazy<com.xiwei.sujian.app.di.AppServiceContainer> =
        lazy { com.xiwei.sujian.app.di.DefaultAppServiceContainer(this) }
    val appContainer: com.xiwei.sujian.app.di.AppServiceContainer
        get() = appContainerDelegate.value

    override val dependencies: com.xiwei.sujian.app.di.SujianAppDependencies
        get() = com.xiwei.sujian.app.di.DefaultSujianAppDependencies(appContainer)

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
                val redactedTrace = DiagnosticsLogger.redactStackTrace(throwable)
                // #623 评论7：crash 头部必须带构建身份 — crash handler 安装在
                // initDiagnostics 之前，不能依赖 DiagnosticsLogger.init 完成，
                // 直接从 BuildConfig 取身份。last_crash.txt 保持"最近一次崩溃"
                // 单文件语义，但文件内可见它属于哪个 APK/commit/flavor。
                val identity = com.xiwei.sujian.core.diagnostics.DiagnosticsBuildIdentity.fromBuildConfig()
                val timestamp =
                    java.text.SimpleDateFormat(
                        "yyyy-MM-dd HH:mm:ss",
                        java.util.Locale.US,
                    ).format(java.util.Date())
                val header =
                    buildString {
                        appendLine("buildKey: ${identity.buildKey}")
                        appendLine("versionName: ${identity.versionName}")
                        appendLine("versionCode: ${identity.versionCode}")
                        appendLine("gitCommitSha: ${identity.gitCommitSha}")
                        appendLine("flavor: ${identity.flavor}")
                        appendLine("buildType: ${identity.buildType}")
                        appendLine("applicationId: ${identity.applicationId}")
                        append("Crash at $timestamp\nThread: ${thread.name}\n\n")
                    }
                val externalWritten =
                    writeCrashFile(File(AndroidDataRoot.logsDir(), "last_crash.txt"), header, redactedTrace)
                if (!externalWritten) {
                    val fallbackDir = File(filesDir, "diagnostics")
                    fallbackDir.mkdirs()
                    writeCrashFile(File(fallbackDir, "last_crash.txt"), header, redactedTrace)
                }
                DiagnosticsLogger.e("SujianApp", "Uncaught exception", throwable)
                DiagnosticsLogger.flushBlocking()
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

    /** 把 crash 头部 + 脱敏栈写入 [file]，返回是否成功。 */
    private fun writeCrashFile(
        file: File,
        header: String,
        redactedTrace: String,
    ): Boolean {
        return try {
            file.parentFile?.mkdirs()
            PrintWriter(FileWriter(file, false)).use { writer ->
                writer.print(header)
                writer.println(redactedTrace)
                writer.flush()
            }
            true
        } catch (_: Exception) {
            false
        }
    }

    override fun onStart(owner: LifecycleOwner) {
        // 没有外部存储权限时（例如首次启动跳转授权页）不得触碰
        // appContainer / AppServiceProvider / WriterAppServiceHolder，
        // 避免在授权前提前初始化 Rust Core（Issue #600）。
        if (!AndroidDataRoot.hasStorageAccess()) return
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.appLifecycle("start")
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this, appContainer.syncRepository)
        }
        autoSyncScheduler?.start()
    }

    override fun onStop(owner: LifecycleOwner) {
        // 同 onStart：授权前进入后台（跳转系统授权页）不能拉起 Core。
        if (!AndroidDataRoot.hasStorageAccess()) return
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.appLifecycle("stop")
        autoSyncScheduler?.stop()
        val result = AppServiceProvider.getAppServiceBridge(this).starMapBridge.flushAllStarmapStores()
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
        // 生命周期收尾：把已入队的应用日志落盘。正常日志本来就应该由 writer 持续写，
        // 这里只是收尾，不依赖它解决日志缺失。
        DiagnosticsLogger.flushBlocking()
    }
}
