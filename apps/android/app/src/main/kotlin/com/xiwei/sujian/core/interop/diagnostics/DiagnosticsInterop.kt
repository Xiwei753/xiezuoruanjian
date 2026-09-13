package com.xiwei.sujian.core.interop.diagnostics

import android.content.Context
import android.util.Log
import com.xiwei.sujian.BuildConfig
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import uniffi.writer_core.DiagnosticFieldDto
import uniffi.writer_core.DiagnosticLevelDto
import uniffi.writer_core.DiagnosticOriginDto
import uniffi.writer_core.clearDiagnostics
import uniffi.writer_core.flushDiagnostics
import uniffi.writer_core.recordDiagnosticEvent
import uniffi.writer_core.redactDiagnosticText
import uniffi.writer_core.setDiagnosticsConfig

/**
 * 统一诊断 interop — Issue #670 评论 5651060802 / 5651816143。
 *
 * Android 端通用日志后端已删除（PersistentLogWriter / DiagnosticsLogger.kt 等），
 * 本对象是 Kotlin ↔ Rust 统一诊断后端的薄 interop：
 *
 * - `i/w/e/d` 同时输出到 `android.util.Log`（实时进入 logd / adb logcat）
 *   和 Rust `recordDiagnosticEvent`（统一脱敏 + JSONL 持久化 + 轮转 + flush barrier）。
 * - `flush/clear/export/setConfig` 直接转发到 UniFFI 接口。
 * - `redact` / `redactStackTrace` 调用 UniFFI `redactDiagnosticText`，脱敏只有一份
 *   事实来源（Rust `writer_diagnostics::redact`），不再在 Kotlin 端复制一套规则
 *   （Issue #670 评论 5651816143 修改 4）。
 * - `DiagnosticsBuildIdentity` 保留为内部 data class，供 crash handler 和导出
 *   manifest 使用构建身份；Rust 后端从 PlatformInit 获取构建身份用于日志文件名。
 *
 * 不再持有 PersistentLogWriter、内存 buffer 或自己定义日志格式/轮转/落盘。
 */
object DiagnosticsInterop {
    private const val TAG = "SujianDiag"

    private val enabled = AtomicBoolean(false)
    private val verbose = AtomicBoolean(false)
    private val contextRef = AtomicReference<Context>(null)

    // ── 脱敏 ──────────────────────────────────────────────────
    //
    // Issue #670 评论 5651816143 修改 4：删除 REDACT_RULES / SENSITIVE_MARKERS_CI /
    // mayContainSensitiveData() 这套复制实现。redact() 和 redactStackTrace() 改为
    // 调用 UniFFI redactDiagnosticText()，脱敏只有一份事实来源（Rust redact）。

    /**
     * 对附件内容做脱敏 — 调用 UniFFI `redactDiagnosticText`。
     *
     * Issue #670 评论 5651816143 修改 4：脱敏只有一份事实来源（Rust
     * `writer_diagnostics::redact`），不再在 Kotlin 端复制一套 REDACT_RULES。
     * 仅供平台采集器（logcat / processExit / threadDump）使用；普通日志事件由
     * Rust `writer_diagnostics::redact` 统一脱敏，不走本方法。
     */
    fun redact(message: String): String =
        try {
            redactDiagnosticText(message)
        } catch (e: UnsatisfiedLinkError) {
            // 原生库未加载时返回原文，避免阻塞采集。
            message
        } catch (e: Exception) {
            Log.w(TAG, "redactDiagnosticText failed: ${e.message}")
            message
        }

    fun redactStackTrace(throwable: Throwable): String {
        val raw = Log.getStackTraceString(throwable)
        return redact(raw)
    }

    // ── 初始化 / 配置 ─────────────────────────────────────────────

    /**
     * 初始化诊断后端。
     *
     * Rust `writer_diagnostics::init` 由 `SujianApplication.initDiagnostics()` 在
     * 进程启动最早期通过 UniFFI `initDiagnostics` 调用（Issue #670 评论 5651816143 修改 1）。
     * 本方法只同步 Kotlin 侧的 enabled/verbose 状态和 context 引用（供 crash 文件解析）。
     */
    fun init(
        context: Context,
        isEnabled: Boolean,
        isVerbose: Boolean,
    ) {
        contextRef.set(context.applicationContext)
        enabled.set(isEnabled)
        verbose.set(isVerbose)
    }

    fun setEnabled(isEnabled: Boolean) {
        enabled.set(isEnabled)
        trySetConfig()
    }

    fun setVerbose(isVerbose: Boolean) {
        verbose.set(isVerbose)
        trySetConfig()
    }

    fun isEnabled(): Boolean = enabled.get()

    fun isVerbose(): Boolean = verbose.get()

    /**
     * 更新 Rust 诊断配置（enabled / verbose）。失败只记 Android Log，不抛异常。
     */
    private fun trySetConfig() {
        try {
            setDiagnosticsConfig(enabled.get(), verbose.get())
        } catch (e: UnsatisfiedLinkError) {
            // 原生库未加载时静默跳过。
        } catch (e: Exception) {
            Log.w(TAG, "setDiagnosticsConfig failed: ${e.message}")
        }
    }

    // ── 日志方法 ──────────────────────────────────────────────────
    //
    // 同时输出到 android.util.Log（实时进入 logd / adb logcat）和 Rust
    // recordDiagnosticEvent（统一脱敏 + JSONL 持久化）。不再走 PersistentLogWriter。
    //
    // Issue #670 评论 5651816143 修改 5：d/i/w/e 分别传 DEBUG/INFO/WARN/ERROR level，
    // 不再固定 INFO。

    fun d(
        tag: String,
        message: String,
    ) {
        if (!BuildConfig.DEBUG) return
        if (!enabled.get() || !verbose.get()) return
        Log.d(tag, message)
        recordEvent(
            DiagnosticLevelDto.DEBUG,
            DiagnosticOriginDto.APP,
            "log.debug",
            tag,
            message,
            emptyList(),
        )
    }

    fun i(
        tag: String,
        message: String,
    ) {
        Log.i(tag, message)
        if (enabled.get()) {
            recordEvent(
                DiagnosticLevelDto.INFO,
                DiagnosticOriginDto.APP,
                "log.info",
                tag,
                message,
                emptyList(),
            )
        }
    }

    fun w(
        tag: String,
        message: String,
        throwable: Throwable? = null,
    ) {
        if (throwable != null) {
            val trace = Log.getStackTraceString(throwable)
            Log.w(tag, "$message\n$trace")
            if (enabled.get()) {
                recordEvent(
                    DiagnosticLevelDto.WARN,
                    DiagnosticOriginDto.APP,
                    "log.warn",
                    tag,
                    "$message\n$trace",
                    emptyList(),
                )
            }
        } else {
            Log.w(tag, message)
            if (enabled.get()) {
                recordEvent(
                    DiagnosticLevelDto.WARN,
                    DiagnosticOriginDto.APP,
                    "log.warn",
                    tag,
                    message,
                    emptyList(),
                )
            }
        }
    }

    fun e(
        tag: String,
        message: String,
        throwable: Throwable? = null,
    ) {
        if (throwable != null) {
            val trace = Log.getStackTraceString(throwable)
            Log.e(tag, "$message\n$trace")
            if (enabled.get()) {
                recordEvent(
                    DiagnosticLevelDto.ERROR,
                    DiagnosticOriginDto.APP,
                    "log.error",
                    tag,
                    "$message\n$trace",
                    emptyList(),
                )
            }
        } else {
            Log.e(tag, message)
            if (enabled.get()) {
                recordEvent(
                    DiagnosticLevelDto.ERROR,
                    DiagnosticOriginDto.APP,
                    "log.error",
                    tag,
                    message,
                    emptyList(),
                )
            }
        }
    }

    // ── 结构化事件 ────────────────────────────────────────────────

    /**
     * 记录结构化诊断事件 — 转发到 Rust `recordDiagnosticEvent`。
     *
     * `level` 区分日志级别（Error/Warn/Info/Debug/Trace）— Issue #670 评论 5651816143 修改 5。
     * `origin` 区分用户操作（User）/ 系统回调（System）/ 应用自身（App）。
     * Rust 后端负责脱敏、序列化、入队 writer。
     */
    fun recordEvent(
        level: DiagnosticLevelDto,
        origin: DiagnosticOriginDto,
        event: String,
        target: String,
        message: String?,
        fields: List<DiagnosticFieldDto>,
    ) {
        try {
            recordDiagnosticEvent(level, origin, event, target, message, fields)
        } catch (e: UnsatisfiedLinkError) {
            // 原生库未加载时静默跳过。
        } catch (e: Exception) {
            Log.w(TAG, "recordDiagnosticEvent failed: ${e.message}")
        }
    }

    /**
     * 便捷重载 — 接受 vararg Pair 字段，自动转成 DiagnosticFieldDto 列表。
     * 默认 level=INFO，适用于普通结构化业务事件。
     */
    fun recordEvent(
        origin: DiagnosticOriginDto,
        event: String,
        target: String,
        vararg fields: Pair<String, Any?>,
    ) {
        val dtos =
            fields.mapNotNull { (k, v) ->
                if (v == null) null else DiagnosticFieldDto(k, v.toString())
            }
        recordEvent(DiagnosticLevelDto.INFO, origin, event, target, null, dtos)
    }

    // ── flush / clear / export ────────────────────────────────────

    /**
     * flush barrier — 阻塞直到前序日志落盘。
     * 返回 true 表示落盘成功；writer 死亡超时或写盘失败返回 false。
     */
    fun flushBlocking(): Boolean =
        try {
            flushDiagnostics()
        } catch (e: UnsatisfiedLinkError) {
            false
        } catch (e: Exception) {
            Log.w(TAG, "flushDiagnostics failed: ${e.message}")
            false
        }

    /**
     * 清空日志文件。
     * 返回 true 表示删除成功；超时/中断/删除失败返回 false。
     */
    fun clearLogs(): Boolean =
        try {
            clearDiagnostics()
        } catch (e: UnsatisfiedLinkError) {
            false
        } catch (e: Exception) {
            Log.w(TAG, "clearDiagnostics failed: ${e.message}")
            false
        }

    // 注意：exportDiagnostics 不在本对象暴露 — 附件收集 + 导出由
    // DiagnosticsExporterInterop 统一负责，调用 UniFFI exportDiagnostics 时
    // 已用 alias `nativeExportDiagnostics` 避免同名自调用
    // （Issue #670 评论 5651816143 修改 7）。

    // ── 构建身份 ──────────────────────────────────────────────────
    //
    // 保留 Kotlin 端构建身份供 crash handler 写 crash 事件字段和导出 manifest。
    // Rust 后端从 PlatformInit 获取构建身份用于日志文件名，不再需要 Kotlin 传入。

    /**
     * 不可变构建身份 — 标识当前 APK 的版本、commit、flavor 和 buildType。
     */
    data class BuildIdentity(
        val versionName: String,
        val versionCode: Int,
        val gitCommitSha: String,
        val flavor: String,
        val buildType: String,
        val applicationId: String,
    ) {
        val buildKey: String
            get() = "v$versionCode-$gitCommitSha-$flavor-$buildType"

        fun toManifestFields(): Map<String, Any?> =
            mapOf(
                "appVersion" to versionName,
                "versionCode" to versionCode,
                "gitCommitSha" to gitCommitSha,
                "buildType" to buildType,
                "flavor" to flavor,
                "buildKey" to buildKey,
            )
    }

    fun buildIdentity(): BuildIdentity =
        BuildIdentity(
            versionName = BuildConfig.VERSION_NAME,
            versionCode = BuildConfig.VERSION_CODE,
            gitCommitSha = BuildConfig.GIT_COMMIT_SHA,
            flavor = BuildConfig.FLAVOR,
            buildType = BuildConfig.BUILD_TYPE,
            applicationId = BuildConfig.APPLICATION_ID,
        )
}
