package com.xiwei.sujian.core.diagnostics

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import com.google.gson.GsonBuilder
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * 进程退出原因采集器（Issue #612 三、3.2）。
 *
 * 在导出诊断包时调用 [collect]，通过
 * ActivityManager.getHistoricalProcessExitReasons 获取本进程最近的异常退出原因
 * （崩溃 / ANR / 系统杀进程等，API 31+），脱敏后写入 [destDir]/process_exits.json，
 * 每条记录的原始 trace（getTraceInputStream）原样保存到 [destDir]/exit_traces/。
 *
 * - 只在导出时按需调用，不常驻后台抓取。
 * - 限制最多 8 条退出记录（与 Issue 正文一致）。
 * - 文本字段经 [DiagnosticsLogger.redact] 脱敏后再落盘；trace 原样保存供 native 解析。
 * - 任何失败都写占位文件，不抛异常（导出不能因采集失败而中断）。
 */
internal object ProcessExitCollector {
    private const val MAX_REASONS = 8
    private const val OUTPUT_NAME = "process_exits.json"
    private const val TRACE_DIR_NAME = "exit_traces"
    private const val METADATA_NAME = "exit_metadata.json"

    /**
     * 抓取进程退出原因，脱敏后写入 [destDir]/process_exits.json；
     * 每条 ApplicationExitInfo 的原始 trace 保存到 [destDir]/exit_traces/。
     * API 30 以下或失败时写占位文件，不抛异常。
     */
    fun collect(
        context: Context,
        destDir: File,
    ) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            File(destDir, OUTPUT_NAME).writeText("{\"error\":\"process_exits requires API 31+\"}\n")
            return
        }
        try {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
            if (am == null) {
                writeError(destDir, "ActivityManager unavailable")
                return
            }
            val reasons = am.getHistoricalProcessExitReasons(context.packageName, 0, MAX_REASONS)
            if (reasons.isNullOrEmpty()) {
                File(destDir, OUTPUT_NAME).writeText("{\"entries\":[]}\n")
                return
            }
            val traceDir = File(destDir, TRACE_DIR_NAME)
            traceDir.mkdirs()
            val entries = mutableListOf<Map<String, Any?>>()
            val traceMetadata = mutableListOf<Map<String, Any?>>()
            for (reason in reasons) {
                val ts = reason.timestamp
                val reasonStr = reason.reason
                val entry = buildEntry(reason, ts, reasonStr)
                entries.add(entry)
                saveTraceIfPresent(reason, traceDir, ts, reasonStr, traceMetadata)
            }
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsLogger.redact(gson.toJson(mapOf("entries" to entries)))
            File(destDir, OUTPUT_NAME).writeText(json)
            if (traceMetadata.isNotEmpty()) {
                val metaJson = DiagnosticsLogger.redact(gson.toJson(mapOf("traces" to traceMetadata)))
                File(destDir, METADATA_NAME).writeText(metaJson)
            }
        } catch (e: Exception) {
            writeError(destDir, e.message ?: "unknown")
        }
    }

    private fun buildEntry(
        reason: ApplicationExitInfo,
        ts: Long,
        reasonStr: Int,
    ): Map<String, Any?> {
        val tsFormatted =
            SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(ts))
        return linkedMapOf<String, Any?>(
            "timestamp" to ts,
            "timestampFormatted" to tsFormatted,
            "reason" to reasonStr,
            "reasonName" to reasonName(reasonStr),
            "status" to reason.status,
            "importance" to reason.importance,
            "pss" to reason.pss,
            "rss" to reason.rss,
            "description" to DiagnosticsLogger.redact(reason.description ?: ""),
            "processName" to DiagnosticsLogger.redact(reason.processName ?: ""),
            "processStateSummary" to encodeProcessStateSummary(reason),
        )
    }

    /** ProcessStateSummary 是 ≤128 bytes 的 byte[]，转成 hex 字符串脱敏后记录。 */
    private fun encodeProcessStateSummary(reason: ApplicationExitInfo): String? {
        return try {
            val summary: ByteArray = reason.processStateSummary ?: return null
            if (summary.isEmpty()) return null
            val hex = summary.joinToString("") { byte -> "%02x".format(byte) }
            DiagnosticsLogger.redact(hex)
        } catch (_: Exception) {
            null
        }
    }

    private fun saveTraceIfPresent(
        reason: ApplicationExitInfo,
        traceDir: File,
        ts: Long,
        reasonStr: Int,
        traceMetadata: MutableList<Map<String, Any?>>,
    ) {
        try {
            val traceStream = reason.getTraceInputStream() ?: return
            val safeReason = sanitizeReasonForFileName(reasonStr)
            val traceFile = File(traceDir, "$ts-$safeReason.trace")
            traceStream.use { input ->
                FileOutputStream(traceFile).use { output ->
                    input.copyTo(output)
                }
            }
            traceMetadata.add(
                linkedMapOf<String, Any?>(
                    "timestamp" to ts,
                    "reason" to reasonStr,
                    "reasonName" to reasonName(reasonStr),
                    "file" to traceFile.name,
                    "size" to traceFile.length(),
                ),
            )
        } catch (_: Exception) {
            // 单条 trace 保存失败不影响其他记录。
        }
    }

    private fun sanitizeReasonForFileName(reason: Int): String =
        try {
            reasonName(reason).filter { it.isLetterOrDigit() || it == '_' }
        } catch (_: Exception) {
            "reason$reason"
        }

    private val reasonNames: Map<Int, String> by lazy {
        mapOf(
            ApplicationExitInfo.REASON_ANR to "ANR",
            ApplicationExitInfo.REASON_CRASH to "CRASH",
            ApplicationExitInfo.REASON_CRASH_NATIVE to "CRASH_NATIVE",
            ApplicationExitInfo.REASON_DEPENDENCY_DIED to "DEPENDENCY_DIED",
            ApplicationExitInfo.REASON_OTHER to "OTHER",
            ApplicationExitInfo.REASON_LOW_MEMORY to "LOW_MEMORY",
            ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE to "EXCESSIVE_RESOURCE_USAGE",
            ApplicationExitInfo.REASON_USER_REQUESTED to "USER_REQUESTED",
            ApplicationExitInfo.REASON_USER_STOPPED to "USER_STOPPED",
            ApplicationExitInfo.REASON_INITIALIZATION_FAILURE to "INITIALIZATION_FAILURE",
            ApplicationExitInfo.REASON_PERMISSION_CHANGE to "PERMISSION_CHANGE",
            ApplicationExitInfo.REASON_PACKAGE_STATE_CHANGE to "PACKAGE_STATE_CHANGE",
            ApplicationExitInfo.REASON_PACKAGE_UPDATED to "PACKAGE_UPDATED",
            ApplicationExitInfo.REASON_EXIT_SELF to "EXIT_SELF",
            ApplicationExitInfo.REASON_SIGNALED to "SIGNALED",
            ApplicationExitInfo.REASON_FREEZER to "FREEZER",
            ApplicationExitInfo.REASON_UNKNOWN to "UNKNOWN",
        )
    }

    private fun reasonName(reason: Int): String = reasonNames[reason] ?: "UNKNOWN_$reason"

    private fun writeError(
        destDir: File,
        message: String,
    ) {
        val safeMsg = DiagnosticsLogger.redact(message)
        val gson = GsonBuilder().create()
        File(destDir, OUTPUT_NAME).writeText(gson.toJson(mapOf("error" to safeMsg)))
    }
}
