package com.xiwei.sujian.core.diagnostics

import android.app.ActivityManager
import android.content.Context
import android.os.Build

/**
 * 进程状态摘要写入器（Issue #612 三、3.2）。
 *
 * 用 ActivityManager.setProcessStateSummary 把 ≤128 bytes 的脱敏摘要写入系统，
 * 让 dumpsys / bugreport / ApplicationExitInfo.processStateSummary 能看到本进程
 * 最近的关键状态（如 screen=Works;editor=0;sync=idle），便于实机问题定位。
 *
 * - 只在关键状态变化时调用（导航切换、编辑器开关、同步状态变化），不每帧调用。
 * - 摘要 ≤128 bytes，超出截断。
 * - API 31+ 才有 setProcessStateSummary；低于则 no-op。
 * - 失败不抛异常。
 */
internal object ProcessStateSummary {
    private const val MAX_BYTES = 128

    /**
     * 构造摘要 `screen=[screen];editor=[editor];sync=[sync]`，
     * 截断到 ≤[MAX_BYTES] bytes 后写入 ActivityManager.setProcessStateSummary。
     */
    fun update(
        context: Context,
        screen: String,
        editor: String,
        sync: String,
    ) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        try {
            val summary = buildSummary(screen, editor, sync)
            val truncated = truncateToBytes(summary, MAX_BYTES)
            val bytes = truncated.toByteArray(Charsets.UTF_8)
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager ?: return
            am.setProcessStateSummary(bytes)
        } catch (_: Exception) {
            // setProcessStateSummary 失败不影响业务流程。
        }
    }

    /**
     * 构造摘要字符串。提取为 internal 可见函数便于单测验证格式与截断。
     * 输入先经 [DiagnosticsLogger.redact] 脱敏，避免把作品标题等敏感内容写进系统摘要。
     */
    internal fun buildSummary(
        screen: String,
        editor: String,
        sync: String,
    ): String {
        val safeScreen = DiagnosticsLogger.redact(screen)
        val safeEditor = DiagnosticsLogger.redact(editor)
        val safeSync = DiagnosticsLogger.redact(sync)
        return "screen=$safeScreen;editor=$safeEditor;sync=$safeSync"
    }

    /**
     * 把 [text] 按 UTF-8 截断到 ≤[maxBytes] bytes。
     * 截断时不会产生半个 UTF-8 字符（按 char 单元回退直到字节数合规）。
     */
    internal fun truncateToBytes(
        text: String,
        maxBytes: Int,
    ): String {
        if (text.isEmpty()) return text
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (bytes.size <= maxBytes) return text
        // 按 char 长度从尾部回退，直到 UTF-8 编码字节数 ≤ maxBytes。
        var end = text.length
        while (end > 0) {
            val sub = text.substring(0, end)
            if (sub.toByteArray(Charsets.UTF_8).size <= maxBytes) return sub
            end--
        }
        return ""
    }
}
