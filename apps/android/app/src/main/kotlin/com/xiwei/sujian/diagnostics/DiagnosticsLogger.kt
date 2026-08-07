package com.xiwei.sujian.diagnostics

import android.content.Context
import android.util.Log
import com.xiwei.sujian.BuildConfig
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

object DiagnosticsLogger {
    private const val TAG = "SujianDiag"
    private const val LOG_PREFIX = "sujian-current"
    private const val MAX_FILE_SIZE = 1024 * 1024L
    private const val MAX_LOG_FILES = 5
    private const val FLUSH_THRESHOLD = 50

    private val enabled = AtomicBoolean(false)
    private val verbose = AtomicBoolean(false)
    private val contextRef = AtomicReference<Context>(null)
    private val buffer = ConcurrentLinkedQueue<String>()
    private val bufferCount = AtomicInteger(0)
    private val lock = Any()

    private val timestampFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)

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
    }

    fun setEnabled(isEnabled: Boolean) {
        enabled.set(isEnabled)
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
        val ts = timestampFormat.format(Date())
        val line = "$ts $level/$tag: $message"
        buffer.add(line)
        if (bufferCount.incrementAndGet() >= FLUSH_THRESHOLD) {
            flush()
        }
    }

    fun flush() {
        synchronized(lock) {
            val ctx = contextRef.get() ?: return
            val logDir = File(ctx.filesDir, "logs")
            if (!logDir.exists()) logDir.mkdirs()
            val currentFile = File(logDir, "$LOG_PREFIX.log")
            try {
                rotateIfNeeded(currentFile)
                val writer = PrintWriter(FileWriter(currentFile, true))
                while (true) {
                    val line = buffer.poll() ?: break
                    writer.println(line)
                }
                writer.flush()
                writer.close()
                bufferCount.set(0)
            } catch (_: Exception) {
                while (buffer.poll() != null) {
                    bufferCount.decrementAndGet()
                }
            }
        }
    }

    private fun rotateIfNeeded(file: File) {
        if (!file.exists() || file.length() < MAX_FILE_SIZE) return
        val logDir = file.parentFile ?: return
        val rotated = File(logDir, "$LOG_PREFIX-${System.currentTimeMillis()}.log")
        file.renameTo(rotated)
        pruneOldLogs(logDir)
    }

    private fun pruneOldLogs(logDir: File) {
        val logs =
            logDir.listFiles { _, name -> name.startsWith(LOG_PREFIX) && name.endsWith(".log") }
                ?.sortedByDescending { it.lastModified() }
                ?: return
        for (i in (MAX_LOG_FILES - 1) until logs.size) {
            logs[i].delete()
        }
    }

    fun getLogFiles(): List<File> {
        val ctx = contextRef.get() ?: return emptyList()
        val logDir = File(ctx.filesDir, "logs")
        if (!logDir.exists()) return emptyList()
        return logDir.listFiles { _, name -> name.startsWith(LOG_PREFIX) && name.endsWith(".log") }
            ?.toList() ?: emptyList()
    }

    fun clearLogs() {
        synchronized(lock) {
            val ctx = contextRef.get() ?: return
            val logDir = File(ctx.filesDir, "logs")
            if (logDir.exists()) {
                logDir.listFiles()?.forEach { it.delete() }
            }
            buffer.clear()
            bufferCount.set(0)
            val crashFile = File(ctx.filesDir, "last_crash.txt")
            if (crashFile.exists()) crashFile.delete()
        }
    }

    fun getCrashFile(): File? {
        val ctx = contextRef.get() ?: return null
        val f = File(ctx.filesDir, "last_crash.txt")
        return if (f.exists()) f else null
    }
}
