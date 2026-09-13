package com.xiwei.sujian.app

import android.app.Application
import android.os.Build
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
import java.util.Locale
import uniffi.writer_core.DiagnosticFieldDto
import uniffi.writer_core.DiagnosticLevelDto
import uniffi.writer_core.DiagnosticOriginDto
import uniffi.writer_core.DiagnosticsInitDto
import uniffi.writer_core.initDiagnostics as nativeInitDiagnostics

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
        // 但诊断后端必须先初始化，否则 crash handler 记录的事件会被丢弃。
        initDiagnostics()
        installCrashHandler()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
    }

    /**
     * 初始化诊断后端 — Issue #670 评论 5651816143 修改 1。
     *
     * 在任何日志产生之前调用 UniFFI `initDiagnostics`，把诊断后端真正接上。
     * 这是进程级基础设施，不要把 diagnostics 生命周期绑在 WriterAppService 上。
     *
     * - log_dir 从 AndroidPrivateDataRoot.logs(context) 获取
     * - platform = "android"
     * - device_id 从 Build 获取
     * - app_version 从 BuildConfig 获取
     * - build_key 从 DiagnosticsInterop.buildIdentity() 获取
     * - locale / timezone 从系统获取
     *
     * `LocalSettings` 仍是 enabled/verbose 唯一持久事实来源，设置加载后
     * 只调用 `setDiagnosticsConfig()` 更新运行时状态。本方法只同步 Kotlin 侧
     * 默认状态（enabled=true, verbose=true），等 Core 初始化后
     * SettingsRepository 会调用 [DiagnosticsInterop.setConfig] 更新实际值。
     */
    private fun initDiagnostics() {
        // 1. 调用 UniFFI initDiagnostics 把 Rust 诊断后端真正接上。
        try {
            val identity = DiagnosticsInterop.buildIdentity()
            val logDir = AndroidPrivateDataRoot.logs(this).absolutePath
            val deviceId =
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    Build.getSerial()
                } else {
                    @Suppress("DEPRECATION")
                    Build.SERIAL
                }
            val appVersion = BuildConfig.VERSION_NAME
            val locale = Locale.getDefault().toLanguageTag()
            val timezone = java.util.TimeZone.getDefault().id
            nativeInitDiagnostics(
                DiagnosticsInitDto(
                    logDir = logDir,
                    platform = "android",
                    deviceId = deviceId ?: "unknown",
                    appVersion = appVersion,
                    buildKey = identity.buildKey,
                    locale = locale,
                    timezone = timezone,
                ),
            )
        } catch (e: UnsatisfiedLinkError) {
            // 原生库未加载时静默跳过 — 后续 Core 初始化时会再次尝试。
        } catch (e: Exception) {
            android.util.Log.w("SujianApp", "initDiagnostics failed: ${e.message}")
        }
        // 2. 同步 Kotlin 侧状态。
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
                // 修改 5：crash 事件传 ERROR level。
                DiagnosticsInterop.recordEvent(
                    DiagnosticLevelDto.ERROR,
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
