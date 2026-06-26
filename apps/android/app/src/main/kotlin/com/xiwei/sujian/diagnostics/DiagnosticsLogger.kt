package com.xiwei.sujian.diagnostics

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean
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
    private val bufferCount = java.util.concurrent.atomic.AtomicInteger(0)
    private val lock = Any()

    private val timestampFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)

    private val REDACT_PATTERNS = listOf(
        Regex("""(?i)(token|access_token|refresh_token|authorization|bearer)\s*[:=]\s*\S+""", RegexOption.MULTILINE),
        Regex("""(?i)(ssh_private_key|private_key)\s*[:=]\s*[\s\S]*?-----END[^\n]*""", RegexOption.MULTILINE),
        Regex("""(?i)(password|passwd|secret)\s*[:=]\s*\S+""", RegexOption.MULTILINE),
        Regex("""-----BEGIN[^\n]*PRIVATE KEY-----[\s\S]*?-----END[^\n]*PRIVATE KEY-----""", RegexOption.MULTILINE),
        Regex("""ghp_[A-Za-z0-9]{36}"""),
        Regex("""gho_[A-Za-z0-9]{36}"""),
        Regex("""github_pat_[A-Za-z0-9_]{82}"""),
        Regex("""(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*"""),
        Regex("""(?i)""content"""\s*:\s*"[^"]{50,}""""),
        Regex("""(?i)""text"""\s*:\s*"[^"]{50,}""""),
        Regex("""(?i)""body"""\s*:\s*"[^"]{50,}""""),
        Regex("""(?i)""chapter"""\s*:\s*"[^"]{50,}"""")
    )

    fun redact(message: String): String {
        var result = message
        for (pattern in REDACT_PATTERNS) {
            result = result.replace(pattern) { match ->
                val keyPart = match.value.substringBefore("=").substringBefore(":").trim()
                if (keyPart.isNotEmpty()) "$keyPart=[REDACTED]" else "[REDACTED]"
            }
        }
        return result
    }

    fun init(context: Context, isEnabled: Boolean, isVerbose: Boolean) {
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

    fun d(tag: String, message: String) {
        val redacted = redact(message)
        Log.d(tag, redacted)
        if (enabled.get() && verbose.get()) {
            enqueue("DEBUG", tag, redacted)
        }
    }

    fun i(tag: String, message: String) {
        val redacted = redact(message)
        Log.i(tag, redacted)
        if (enabled.get()) {
            enqueue("INFO", tag, redacted)
        }
    }

    fun w(tag: String, message: String, throwable: Throwable? = null) {
        val redacted = redact(message)
        Log.w(tag, redacted, throwable)
        if (enabled.get()) {
            val combined = if (throwable != null) "$redacted\n${Log.getStackTraceString(throwable)}" else redacted
            enqueue("WARN", tag, combined)
        }
    }

    fun e(tag: String, message: String, throwable: Throwable? = null) {
        val redacted = redact(message)
        Log.e(tag, redacted, throwable)
        if (enabled.get()) {
            val combined = if (throwable != null) "$redacted\n${Log.getStackTraceString(throwable)}" else redacted
            enqueue("ERROR", tag, combined)
        }
    }

    private fun enqueue(level: String, tag: String, message: String) {
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
        val logs = logDir.listFiles { _, name -> name.startsWith(LOG_PREFIX) && name.endsWith(".log") }
            ?.sortedByDescending { it.lastModified() }
            ?: return
        for (i in MAX_LOG_FILES until logs.size) {
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
