package com.xiwei.sujian.core.diagnostics

import android.content.Context
import android.util.Log
import com.xiwei.sujian.BuildConfig
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * 统一日志入口（Issue #612 重做）。
 *
 * 每条 d/i/w/e 固定走同一条链：
 *   调用方 → redact() → android.util.Log（实时进入 logd / adb logcat）
 *         → PersistentLogWriter.enqueue()（应用自己持久保存）
 *
 * 本对象只负责级别、tag、脱敏、把同一条脱敏日志同时送到 Android Log 和持久 writer。
 * 不再做阈值刷盘、不持有内存 buffer、不直接做文件 I/O —— 所有持久化由
 * [PersistentLogWriter] 的常驻后台线程独占完成。
 */
object DiagnosticsLogger {
    private const val TAG = "SujianDiag"

    private val enabled = AtomicBoolean(false)
    private val verbose = AtomicBoolean(false)
    private val contextRef = AtomicReference<Context>(null)
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

    fun redact(message: String): String {
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

    fun init(
        context: Context,
        isEnabled: Boolean,
        isVerbose: Boolean,
    ) {
        contextRef.set(context.applicationContext)
        enabled.set(isEnabled)
        verbose.set(isVerbose)
        PersistentLogWriter.init(context.applicationContext)
        PersistentLogWriter.setEnabled(isEnabled)
    }

    fun setEnabled(isEnabled: Boolean) {
        enabled.set(isEnabled)
        PersistentLogWriter.setEnabled(isEnabled)
    }

    fun setVerbose(isVerbose: Boolean) {
        verbose.set(isVerbose)
    }

    fun isEnabled(): Boolean = enabled.get()

    fun isVerbose(): Boolean = verbose.get()

    fun d(
        tag: String,
        message: String,
    ) {
        if (!BuildConfig.DEBUG) return
        if (!enabled.get() || !verbose.get()) return
        val redacted = redact(message)
        Log.d(tag, redacted)
        enqueue("DEBUG", tag, redacted)
    }

    fun i(
        tag: String,
        message: String,
    ) {
        val redacted = redact(message)
        Log.i(tag, redacted)
        if (enabled.get()) {
            enqueue("INFO", tag, redacted)
        }
    }

    fun w(
        tag: String,
        message: String,
        throwable: Throwable? = null,
    ) {
        val redacted = redact(message)
        if (throwable != null) {
            val redactedTrace = redactStackTrace(throwable)
            Log.w(tag, "$redacted\n$redactedTrace")
            if (enabled.get()) {
                enqueue("WARN", tag, "$redacted\n$redactedTrace")
            }
        } else {
            Log.w(tag, redacted)
            if (enabled.get()) {
                enqueue("WARN", tag, redacted)
            }
        }
    }

    fun e(
        tag: String,
        message: String,
        throwable: Throwable? = null,
    ) {
        val redacted = redact(message)
        if (throwable != null) {
            val redactedTrace = redactStackTrace(throwable)
            Log.e(tag, "$redacted\n$redactedTrace")
            if (enabled.get()) {
                enqueue("ERROR", tag, "$redacted\n$redactedTrace")
            }
        } else {
            Log.e(tag, redacted)
            if (enabled.get()) {
                enqueue("ERROR", tag, redacted)
            }
        }
    }

    private fun enqueue(
        level: String,
        tag: String,
        message: String,
    ) {
        PersistentLogWriter.enqueue(
            LogRequest(
                level = level,
                tag = tag,
                message = message,
                timestampMs = System.currentTimeMillis(),
                threadName = Thread.currentThread().name,
            ),
        )
    }

    /**
     * 阻塞直到调用前所有已入队的日志都被 writer 线程写完落盘。
     *
     * @return 落盘在超时前完成返回 true；writer 死亡超时或调用线程中断返回 false
     * （Issue #612 评论 3.4：导出等关键路径必须把失败传导给用户，不得假装成功）。
     */
    fun flushBlocking(): Boolean = PersistentLogWriter.flushBlocking()

    fun getLogFiles(): List<File> = PersistentLogWriter.getLogFiles()

    /**
     * 清空持久日志。
     *
     * @return 清空在超时前完成返回 true；超时/中断返回 false（此时不删除回退位置
     * 的 crash 文件，也不得在 UI 上假装已清空，见 Issue #612 评论 3.4）。
     */
    fun clearLogs(): Boolean {
        val ok = PersistentLogWriter.clearLogs()
        if (!ok) return false
        // PersistentLogWriter.clearLogs 已删 logsDir 下所有文件（含 last_crash.txt）。
        // 另外清理 filesDir/diagnostics/ 回退位置的 last_crash.txt。
        val ctx = contextRef.get()
        if (ctx != null) {
            val fallbackCrash = File(File(ctx.filesDir, "diagnostics"), "last_crash.txt")
            if (fallbackCrash.exists()) fallbackCrash.delete()
        }
        return true
    }

    /**
     * 返回 last_crash.txt。优先返回外部 logsDir 下的，回退到 filesDir/diagnostics/。
     */
    fun getCrashFile(): File? {
        val primary = File(AndroidDataRoot.logsDir(), "last_crash.txt")
        if (primary.exists()) return primary
        val ctx = contextRef.get() ?: return null
        val fallback = File(File(ctx.filesDir, "diagnostics"), "last_crash.txt")
        return if (fallback.exists()) fallback else null
    }

    /**
     * 仅在外部 logsDir 与 filesDir/diagnostics/ 两处都存在 last_crash.txt 时返回
     * 回退位置的那份（导出时两处都收集，见 Issue #612 评论二.4）。
     * 只有回退位置有文件时返回 null —— 那份由 [getCrashFile] 以主文件名导出。
     */
    fun getFallbackCrashFile(): File? {
        val ctx = contextRef.get() ?: return null
        val primary = File(AndroidDataRoot.logsDir(), "last_crash.txt")
        if (!primary.exists()) return null
        val fallback = File(File(ctx.filesDir, "diagnostics"), "last_crash.txt")
        return if (fallback.exists()) fallback else null
    }
}
