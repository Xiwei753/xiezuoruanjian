package com.xiwei.sujian.core.diagnostics

import java.io.File

/**
 * Logcat 快照采集器（Issue #612）。
 *
 * 在导出诊断包时调用 [collect]，执行 `logcat -d` 获取当前进程的 logcat
 * 快照（dump 一次后退出），截断到 2 MiB 并脱敏后写入 [destDir]/logcat.txt。
 *
 * - 只在导出时按需调用，不常驻后台抓取。
 * - 限制 2 MiB 避免日志过多撑爆诊断 zip。
 * - 通过 [DiagnosticsLogger.redact] 脱敏后再落盘。
 */
internal object LogcatSnapshotCollector {
    private const val MAX_SNAPSHOT_BYTES = 2 * 1024 * 1024L // 2 MiB
    private const val OUTPUT_NAME = "logcat.txt"

    /**
     * 执行 logcat -d 抓取快照，脱敏后写入 [destDir]/logcat.txt。
     * 失败时写一个包含错误信息的占位文件，不抛异常。
     */
    fun collect(destDir: File) {
        try {
            val process =
                ProcessBuilder("logcat", "-d", "-v", "threadtime")
                    .redirectErrorStream(true)
                    .start()
            val output =
                process.inputStream.bufferedReader().use { reader ->
                    reader.readText()
                }
            process.waitFor()
            val truncated = truncate(output)
            val redacted = DiagnosticsLogger.redact(truncated)
            File(destDir, OUTPUT_NAME).writeText(redacted)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            File(destDir, OUTPUT_NAME).writeText("logcat capture failed: $safeMsg")
        }
    }

    private fun truncate(text: String): String {
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (bytes.size <= MAX_SNAPSHOT_BYTES) return text
        return String(bytes, 0, MAX_SNAPSHOT_BYTES.toInt(), Charsets.UTF_8)
    }
}
