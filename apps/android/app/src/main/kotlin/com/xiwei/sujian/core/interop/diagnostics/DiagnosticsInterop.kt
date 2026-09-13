package com.xiwei.sujian.core.interop.diagnostics

import android.content.Context
import android.util.Log
import com.xiwei.sujian.BuildConfig
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import uniffi.writer_core.DiagnosticFieldDto
import uniffi.writer_core.DiagnosticOriginDto
import uniffi.writer_core.clearDiagnostics
import uniffi.writer_core.exportDiagnostics
import uniffi.writer_core.flushDiagnostics
import uniffi.writer_core.recordDiagnosticEvent
import uniffi.writer_core.setDiagnosticsConfig

/**
 * 统一诊断 interop — Issue #670 评论 5651060802。
 *
 * Android 端通用日志后端已删除（PersistentLogWriter / DiagnosticsLogger.kt 等），
 * 本对象是 Kotlin ↔ Rust 统一诊断后端的薄 interop：
 *
 * - `i/w/e/d` 同时输出到 `android.util.Log`（实时进入 logd / adb logcat）
 *   和 Rust `recordDiagnosticEvent`（统一脱敏 + JSONL 持久化 + 轮转 + flush barrier）。
 * - `flush/clear/export/setConfig` 直接转发到 UniFFI 接口。
 * - `redact` 保留 Kotlin 端实现，仅供平台采集器（LogcatSnapshotCollector 等）
 *   对附件做脱敏；普通日志事件由 Rust `writer_diagnostics::redact` 统一脱敏，
 *   不走本方法。
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

    // ── 脱敏规则 ──────────────────────────────────────────────────
    //
    // 保留 Kotlin 端 redact 仅供平台采集器对附件（logcat / processExit /
    // threadDump）做脱敏。普通日志事件由 Rust `writer_diagnostics::redact`
    // 统一脱敏，不走本方法。

    private val REDACT_RULES: List<Pair<Regex, (MatchResult) -> String>> =
        listOf(
            Pair(
                Regex("""(?i)ssh_private_key\s*[:=]\s*[\s\S]*?-----END[^\n]*PRIVATE KEY-----"""),
                { _ -> "ssh_private_key=[REDACTED]" },
            ),
            Pair(
                Regex("""-----BEGIN[^\n]*PRIVATE KEY-----[\s\S]*?-----END[^\n]*PRIVATE KEY-----"""),
                { _ -> "[REDACTED_PEM]" },
            ),
            Pair(
                Regex("""(?i)\b(authorization)\s*[:=]\s*Bearer\s+\S+"""),
                { m ->
                    val key = m.groupValues[1]
                    val rest = m.value.substringAfter(key)
                    val sep = rest.takeWhile { it in setOf(' ', ':', '=', '\t') }
                    "${key}${sep}Bearer [REDACTED]"
                },
            ),
            Pair(
                Regex(
                    """(?i)\b(token|access_token|refresh_token|authorization|password|passwd|secret|private_key)""" +
                        """\s*[:=]\s*(?:"[^"]*"|\S+)""",
                ),
                { m ->
                    val key = m.groupValues[1]
                    val rest = m.value.substringAfter(key)
                    val sep = rest.takeWhile { it in setOf(' ', ':', '=', '\t') }
                    "$key$sep[REDACTED]"
                },
            ),
            Pair(
                Regex(
                    """(?i)\b(content|text|body|chapter|chapter_content|chapterContent)""" +
                        """\s*[:=]\s*(?:"[^"]*"|[^,}\]\n]+)""",
                ),
                { m ->
                    val key = m.groupValues[1]
                    val rest = m.value.substringAfter(key)
                    val sep = rest.takeWhile { it in setOf(' ', ':', '=', '\t') }
                    "$key$sep[REDACTED]"
                },
            ),
            Pair(
                Regex("""(?i)["'](authorization)["']\s*:\s*["']Bearer\s+[^"\\]*(?:\\.[^"\\]*)*["']"""),
                { m -> "\"${m.groupValues[1]}\": \"Bearer [REDACTED]\"" },
            ),
            Pair(
                Regex(
                    """(?i)["'](token|access_token|refresh_token|authorization|password|passwd|secret""" +
                        """|private_key|ssh_private_key)["']""" +
                        """\s*:\s*["'][^"\\]*(?:\\.[^"\\]*)*["']""",
                ),
                { m -> "\"${m.groupValues[1]}\": \"[REDACTED]\"" },
            ),
            Pair(
                Regex(
                    """(?i)["'](content|text|body|chapter|chapter_content|chapterContent)["']""" +
                        """\s*:\s*["'][^"\\]*(?:\\.[^"\\]*)*["']""",
                ),
                { m -> "\"${m.groupValues[1]}\": \"[REDACTED]\"" },
            ),
            Pair(
                Regex("""(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*"""),
                { _ -> "Bearer [REDACTED]" },
            ),
            Pair(
                Regex("""ghp_[A-Za-z0-9]{36}"""),
                { _ -> "[REDACTED]" },
            ),
            Pair(
                Regex("""gho_[A-Za-z0-9]{36}"""),
                { _ -> "[REDACTED]" },
            ),
            Pair(
                Regex("""github_pat_[A-Za-z0-9_]{82}"""),
                { _ -> "[REDACTED]" },
            ),
        )

    /**
     * 敏感字段名标记（大小写不敏感）— 与 REDACT_RULES 中的 key 保持一致。
     */
    private val SENSITIVE_MARKERS_CI =
        listOf(
            "token", "password", "passwd", "secret", "authorization",
            "private_key", "ssh_private_key", "access_token", "refresh_token",
            "Bearer", "ghp_", "gho_", "github_pat_",
            "content", "chapter", "text", "body",
        )

    /** PEM 私钥结束标记（大小写敏感）。 */
    private const val PEM_END_MARKER = "PRIVATE KEY-----"

    /**
     * 廉价判断消息是否可能包含需要脱敏的敏感字段。
     * 只做子串/字符检查，不跑 Regex；结构事件快速返回 false。
     */
    private fun mayContainSensitiveData(message: String): Boolean =
        SENSITIVE_MARKERS_CI.any { message.contains(it, ignoreCase = true) } ||
            message.contains(PEM_END_MARKER)

    /**
     * 对附件内容做脱敏。仅供平台采集器（logcat / processExit / threadDump）使用。
     * 普通日志事件由 Rust `writer_diagnostics::redact` 统一脱敏，不走本方法。
     */
    fun redact(message: String): String {
        if (!mayContainSensitiveData(message)) return message
        var result = message
        for ((pattern, replacement) in REDACT_RULES) {
            result = result.replace(pattern, replacement)
        }
        return result
    }

    fun redactStackTrace(throwable: Throwable): String {
        val raw = Log.getStackTraceString(throwable)
        return redact(raw)
    }

    // ── 初始化 / 配置 ─────────────────────────────────────────────

    /**
     * 初始化诊断后端。
     *
     * Rust `writer_diagnostics::init` 在 `WriterAppService` 构造时由 Core 从
     * PlatformInit 调用，本方法只同步 Kotlin 侧的 enabled/verbose 状态和
     * context 引用（供 crash 文件解析）。
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

    fun d(
        tag: String,
        message: String,
    ) {
        if (!BuildConfig.DEBUG) return
        if (!enabled.get() || !verbose.get()) return
        Log.d(tag, message)
        recordEvent(DiagnosticOriginDto.APP, "log.debug", tag, message, emptyList())
    }

    fun i(
        tag: String,
        message: String,
    ) {
        Log.i(tag, message)
        if (enabled.get()) {
            recordEvent(DiagnosticOriginDto.APP, "log.info", tag, message, emptyList())
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
                recordEvent(DiagnosticOriginDto.APP, "log.warn", tag, message, emptyList())
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
                recordEvent(DiagnosticOriginDto.APP, "log.error", tag, message, emptyList())
            }
        }
    }

    // ── 结构化事件 ────────────────────────────────────────────────

    /**
     * 记录结构化诊断事件 — 转发到 Rust `recordDiagnosticEvent`。
     *
     * `origin` 区分用户操作（User）/ 系统回调（System）/ 应用自身（App）。
     * Rust 后端负责脱敏、序列化、入队 writer。
     */
    fun recordEvent(
        origin: DiagnosticOriginDto,
        event: String,
        target: String,
        message: String?,
        fields: List<DiagnosticFieldDto>,
    ) {
        try {
            recordDiagnosticEvent(origin, event, target, message, fields)
        } catch (e: UnsatisfiedLinkError) {
            // 原生库未加载时静默跳过。
        } catch (e: Exception) {
            Log.w(TAG, "recordDiagnosticEvent failed: ${e.message}")
        }
    }

    /**
     * 便捷重载 — 接受 vararg Pair 字段，自动转成 DiagnosticFieldDto 列表。
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
        recordEvent(origin, event, target, null, dtos)
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

    /**
     * 导出诊断包到 [outputDir]，返回生成的 zip 文件路径。
     * Rust 后端负责复制日志、生成 manifest、打 zip。
     */
    fun exportDiagnostics(outputDir: String): String? =
        try {
            exportDiagnostics(outputDir)
        } catch (e: UnsatisfiedLinkError) {
            null
        } catch (e: Exception) {
            Log.w(TAG, "exportDiagnostics failed: ${e.message}")
            null
        }

    // ── crash 文件操作 ────────────────────────────────────────────
    //
    // crash handler 把 crash 元数据通过 recordEvent 交给 Rust 后端，
    // 但也保留 last_crash.txt 单文件语义供导出和"上次崩溃"提示。
    // Rust 后端从 PlatformInit 获取构建身份用于日志文件名，crash 文件头
    // 仍从 BuildConfig 取（crash handler 安装在 Core init 之前）。

    /**
     * 返回 last_crash.txt。优先返回应用私有 logsDir 下的，回退到 filesDir/diagnostics/。
     * 未初始化（contextRef 为 null）时返回 null。
     */
    fun getCrashFile(): File? {
        val ctx = contextRef.get() ?: return null
        val primary = File(AndroidPrivateDataRoot.logs(ctx), "last_crash.txt")
        if (primary.exists()) return primary
        val fallback = File(File(ctx.filesDir, "diagnostics"), "last_crash.txt")
        return if (fallback.exists()) fallback else null
    }

    /**
     * 仅在应用私有 logsDir 与 filesDir/diagnostics/ 两处都存在 last_crash.txt 时返回
     * 回退位置的那份；只有回退位置有文件时返回 null。
     */
    fun getFallbackCrashFile(): File? {
        val ctx = contextRef.get() ?: return null
        val primary = File(AndroidPrivateDataRoot.logs(ctx), "last_crash.txt")
        if (!primary.exists()) return null
        val fallback = File(File(ctx.filesDir, "diagnostics"), "last_crash.txt")
        return if (fallback.exists()) fallback else null
    }

    /**
     * 把 crash 头部 + 脱敏栈写入 [file]，返回是否成功。
     */
    fun writeCrashFile(
        file: File,
        header: String,
        redactedTrace: String,
    ): Boolean =
        try {
            file.parentFile?.mkdirs()
            java.io.PrintWriter(java.io.FileWriter(file, false)).use { writer ->
                writer.print(header)
                writer.println(redactedTrace)
                writer.flush()
            }
            true
        } catch (_: Exception) {
            false
        }

    // ── 构建身份 ──────────────────────────────────────────────────
    //
    // 保留 Kotlin 端构建身份供 crash handler 写 crash 文件头和导出 manifest。
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
