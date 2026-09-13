package com.xiwei.sujian.app

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.app.di.SujianAppDependenciesProvider
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsEventsInterop
import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.feature.sync.work.AutoSyncScheduler
import com.xiwei.sujian.storage.recovery.LegacyStorageMigrationGate
import java.io.File
import uniffi.writer_core.DiagnosticFieldDto
import uniffi.writer_core.DiagnosticOriginDto

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
        // 崩溃处理器放在第一项：crash 都要落到日志目录（应用私有 filesDir，无需权限）。
        installCrashHandler()
        initDiagnostics()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
    }

    /**
     * 初始化诊断后端。
     *
     * Issue #670 评论 5651060802：不再使用独立的 `sujian_diagnostics` SharedPreferences，
     * diagnostics_enabled / diagnostics_verbose 只认 Core `LocalSettings`。
     * Core 在 WriterAppService 构造时从 PlatformInit 初始化 Rust diagnostics 后端；
     * 本方法只同步 Kotlin 侧默认状态（enabled=true, verbose=true），等 Core 初始化后
     * SettingsRepository 会调用 [DiagnosticsInterop.setConfig] 更新实际值。
     */
    private fun initDiagnostics() {
        DiagnosticsInterop.init(this, isEnabled = true, isVerbose = true)
        EditorEventRingBuffer.setEnabled(true)
    }

    /**
     * JVM uncaught exception handler — Android 平台采集器。
     *
     * Issue #670 评论 5651060802：不再自己定义 last_crash.txt 格式和 writer，
     * 把 crash 元数据 / 脱敏栈交给 Rust 统一诊断后端并 flush。
     * 仍保留 last_crash.txt 单文件语义供导出和"上次崩溃"提示。
     */
    private fun installCrashHandler() {
        val defaultHandler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            try {
                val redactedTrace = DiagnosticsInterop.redactStackTrace(throwable)
                val identity = DiagnosticsInterop.buildIdentity()
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
                    DiagnosticsInterop.writeCrashFile(
                        File(AndroidPrivateDataRoot.logs(this), "last_crash.txt"),
                        header,
                        redactedTrace,
                    )
                if (!externalWritten) {
                    val fallbackDir = File(filesDir, "diagnostics")
                    fallbackDir.mkdirs()
                    DiagnosticsInterop.writeCrashFile(
                        File(fallbackDir, "last_crash.txt"),
                        header,
                        redactedTrace,
                    )
                }
                // 把 crash 元数据交给 Rust 统一诊断后端。
                DiagnosticsInterop.recordEvent(
                    DiagnosticOriginDto.SYSTEM,
                    "app.crash",
                    "SujianApp",
                    "Uncaught exception in thread ${thread.name}",
                    listOf(
                        DiagnosticFieldDto("thread", thread.name),
                        DiagnosticFieldDto("buildKey", identity.buildKey),
                        DiagnosticFieldDto("versionCode", identity.versionCode.toString()),
                        DiagnosticFieldDto("flavor", identity.flavor),
                        DiagnosticFieldDto("buildType", identity.buildType),
                    ),
                )
                DiagnosticsInterop.flushBlocking()
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
        // 数据根目录已改为应用私有 filesDir，不再需要共享存储权限检查。
        DiagnosticsEventsInterop.appLifecycle("start")
        // 旧工作区仍待迁移时 Core 尚未打开；此时不能初始化依赖容器或自动同步，
        // 否则会提前打开新数据根目录。
        if (LegacyStorageMigrationGate.legacyGitWorkspaceExists(this)) {
            DiagnosticsInterop.w("SujianApp", "Legacy storage pending migration; skip appContainer init on start")
            return
        }
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this)
        }
        autoSyncScheduler?.start()
    }

    override fun onStop(owner: LifecycleOwner) {
        // 同 onStart：私有存储无需权限检查。
        DiagnosticsEventsInterop.appLifecycle("stop")
        // 旧结构仍待迁移时跳过 syncRepository/starMapBridge 调用，
        // 它们会触发 Core 初始化。autoSyncScheduler 此时也必为 null，无需 stop。
        if (LegacyStorageMigrationGate.legacyGitWorkspaceExists(this)) {
            DiagnosticsInterop.w("SujianApp", "Legacy storage pending migration; skip appContainer touch on stop")
            DiagnosticsInterop.flushBlocking()
            return
        }
        autoSyncScheduler?.stop()
        val result = AppServiceProvider.getAppServiceBridge(this).starMapBridge.flushAllStarmapStores()
        when (result) {
            is BridgeResult.Error ->
                DiagnosticsInterop.e(
                    "SujianApp",
                    "flushAllStarmapStores failed: ${result.fullEnvelope}",
                )
            BridgeResult.NotLoaded ->
                DiagnosticsInterop.w(
                    "SujianApp",
                    "flushAllStarmapStores skipped: native library not loaded",
                )
            is BridgeResult.Success -> {}
        }
        // 生命周期收尾：把已入队的应用日志落盘。正常日志本来就应该由 writer 持续写，
        // 这里只是收尾，不依赖它解决日志缺失。
        DiagnosticsInterop.flushBlocking()
    }
}
