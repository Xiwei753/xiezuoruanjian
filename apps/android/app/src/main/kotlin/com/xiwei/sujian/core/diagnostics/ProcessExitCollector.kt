package com.xiwei.sujian.core.diagnostics

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
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
 * （崩溃 / ANR / 系统杀进程等，API 30+），脱敏后写入 [destDir]/process_exits.json，
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
        // getHistoricalProcessExitReasons、ApplicationExitInfo 基础字段与 processStateSummary
        // 均自 API 30 起可用；minSdk=30 保证可直接调用，不再用 API 31 守卫跳过整段采集。
        try {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
            if (am == null) {
                writeError(destDir, "ActivityManager unavailable")
                return
            }
            // getHistoricalProcessExitReasons 是平台类型：SDK stub 标注 @NonNull，但
            // 运行时可返回 null；显式声明可空类型，让 isNullOrEmpty 空安全且不触发
            // detekt UselessCallOnNotNull。
            val reasons: List<ApplicationExitInfo>? =
                am.getHistoricalProcessExitReasons(context.packageName, 0, MAX_REASONS)
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

    /**
     * ProcessStateSummary 是 ≤128 bytes 的 byte[]，自 API 30 起可用（minSdk=30 保证
     * 可直接读取）。按 UTF-8 解码为可读文本后脱敏记录，便于诊断包直接阅读。
     */
    private fun encodeProcessStateSummary(reason: ApplicationExitInfo): String? {
        return try {
            val summary: ByteArray = reason.processStateSummary ?: return null
            decodeProcessStateSummary(summary)
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 把 processStateSummary 的 byte[] 按 UTF-8 解码为字符串并脱敏。
     * 提取为 internal 纯函数便于单测验证解码与脱敏逻辑，不需要 ApplicationExitInfo。
     * 空数组返回 null；非空则 UTF-8 解码后经 [DiagnosticsLogger.redact] 脱敏。
     */
    internal fun decodeProcessStateSummary(bytes: ByteArray): String? {
        if (bytes.isEmpty()) return null
        val text = String(bytes, Charsets.UTF_8)
        return DiagnosticsLogger.redact(text)
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
            // 同一毫秒内同一 reason 的多条退出记录（多进程包：主进程与 provider 进程
            // 同时被系统杀死）会撞文件名；后者不覆盖前者，追加 -1/-2 序号保留全部 trace。
            val traceFile = uniqueTraceFile(traceDir, "$ts-$safeReason")
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

    /**
     * 在 [traceDir] 内为 [baseName] 取一个不冲突的文件名：优先 `baseName.trace`，
     * 已存在则依次尝试 `baseName-1.trace`、`baseName-2.trace`……
     * 提取为 internal 纯函数便于单测验证冲突不覆盖（Issue #612 三、3.2）。
     */
    internal fun uniqueTraceFile(
        traceDir: File,
        baseName: String,
    ): File {
        var file = File(traceDir, "$baseName.trace")
        var index = 1
        while (file.exists()) {
            file = File(traceDir, "$baseName-$index.trace")
            index++
        }
        return file
    }

    private fun sanitizeReasonForFileName(reason: Int): String =
        try {
            reasonName(reason).filter { it.isLetterOrDigit() || it == '_' }
        } catch (_: Exception) {
            "reason$reason"
        }

    private val reasonNames: Map<Int, String> by lazy { buildReasonNames() }

    /**
     * REASON_* 全是 public static final int 编译期常量，Kotlin 编译时直接内联数值，
     * 运行期不读字段，API 30 设备不会抛 NoSuchFieldError。
     * 其中 FREEZER(API 33)、PACKAGE_STATE_CHANGE/PACKAGE_UPDATED(API 34) 高于 minSdk=30，
     * lint 的 InlinedApi 仍会标记，这里精确抑制（仅限内联常量场景，非宽放）。
     */
    @android.annotation.SuppressLint("InlinedApi")
    private fun buildReasonNames(): Map<Int, String> =
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
