package com.xiwei.sujian.core.diagnostics

import java.io.File

/**
 * 当前线程栈采集器（Issue #612 三、3.3）。
 *
 * 在导出诊断包时调用 [collect]，把 Thread.getAllStackTraces() 格式化写入
 * [destDir]/threads.txt。只在导出时抓一次，不常驻后台。
 *
 * - 每个线程输出：线程名、状态、栈帧。
 * - 所有文本经 [DiagnosticsLogger.redact] 脱敏后再落盘。
 * - 失败时写占位文件，不抛异常。
 */
internal object ThreadDumpCollector {
    private const val OUTPUT_NAME = "threads.txt"

    /**
     * 抓取所有线程的栈轨迹，格式化脱敏后写入 [destDir]/threads.txt。
     * 失败时写占位文件，不抛异常。
     */
    fun collect(destDir: File) {
        try {
            val sb = StringBuilder()
            val ts =
                java.text.SimpleDateFormat(
                    "yyyy-MM-dd HH:mm:ss",
                    java.util.Locale.US,
                ).format(java.util.Date())
            sb.append("Thread dump at $ts\n")
            sb.append("JVM: ${System.getProperty("java.vm.name", "unknown")}\n")
            sb.append("\n")

            val allTraces = Thread.getAllStackTraces()
            // 按线程名排序，保证输出稳定便于 diff。
            val sortedThreads = allTraces.keys.sortedBy { it.name }
            for (thread in sortedThreads) {
                val stack = allTraces[thread] ?: emptyArray()
                appendThread(sb, thread, stack)
            }
            val redacted = DiagnosticsLogger.redact(sb.toString())
            File(destDir, OUTPUT_NAME).writeText(redacted)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            File(destDir, OUTPUT_NAME).writeText("thread dump failed: $safeMsg\n")
        }
    }

    private fun appendThread(
        sb: StringBuilder,
        thread: Thread,
        stack: Array<StackTraceElement>,
    ) {
        sb.append("\"${thread.name}\"")
        sb.append(" #${thread.id}")
        sb.append(" prio=${thread.priority}")
        sb.append(" daemon=${thread.isDaemon}")
        sb.append(" state=${thread.state}")
        sb.append("\n")
        if (stack.isEmpty()) {
            sb.append("    (no stack)\n")
        } else {
            for (frame in stack) {
                sb.append("    at $frame\n")
            }
        }
        sb.append("\n")
    }
}
