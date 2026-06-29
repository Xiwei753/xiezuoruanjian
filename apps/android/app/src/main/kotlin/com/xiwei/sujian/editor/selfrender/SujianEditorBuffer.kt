package com.xiwei.sujian.editor.selfrender

import com.xiwei.sujian.model.EditorAnimationEventData
import com.xiwei.sujian.model.EditorAnimationKindData

/**
 * 文本变更原因，与 Core EditorTransactionCause 对齐。
 */
enum class SujianEditCause {
    Typing,           // 普通单字输入
    Delete,           // 删除
    TypingCommit,     // 中文/日文 IME commit 多字
    Paste,            // 粘贴
    Load,             // 加载章节
    Format,           // 格式化/设置变化
    ImeComposition,   // IME composing（不进正文 buffer）
    Programmatic      // 程序化变更
}

/**
 * 光标/选区状态
 */
data class SujianSelection(
    val anchor: Int,  // UTF-16 offset
    val head: Int     // UTF-16 offset (caret position)
) {
    val isCollapsed: Boolean get() = anchor == head
    val start: Int get() = minOf(anchor, head)
    val end: Int get() = maxOf(anchor, head)

    companion object {
        fun collapsed(offset: Int) = SujianSelection(offset, offset)
    }
}

/**
 * 文本变更结果，包含新文本、新选区、以及 Core 返回的动画事件
 */
data class SujianEditResult(
    val newText: String,
    val newSelection: SujianSelection,
    val cause: SujianEditCause,
    val animationEvents: List<EditorAnimationEventData>
)

/**
 * 动画事件提供者接口。
 * Android 端通过此接口调用 Core EditorEngine 的 animation_events 方法。
 */
interface SujianAnimationEventProvider {
    fun provide(
        oldText: String,
        newText: String,
        oldCursorUtf8: Int,
        newCursorUtf8: Int,
        cause: String,
        maxAnimatedChars: Int,
        animationDurationMs: Long
    ): List<EditorAnimationEventData>
}

/**
 * SujianEditorBuffer — 自研写作区文本缓冲区
 *
 * 管理正文纯文本、光标/选区、composing 状态。
 * 所有文本变更通过 edit() 方法统一处理，生成 EditorTransaction 语义。
 *
 * ## 架构原则
 * - Core 仍唯一业务语义来源
 * - Android 只负责文本存储和选区管理
 * - 文本变化必须生成 EditorTransaction 语义
 * - composing text 不进 undo/保存/正文动画
 */
class SujianEditorBuffer {

    // ── 文本状态 ──
    var text: String = ""
        private set

    var selection: SujianSelection = SujianSelection.collapsed(0)
        private set

    // ── Composing 状态 ──
    var composingStart: Int = -1
        private set
    var composingEnd: Int = -1
        private set
    val hasComposing: Boolean get() = composingStart >= 0 && composingEnd >= 0 && composingStart != composingEnd

    // ── 动画事件提供者 ──
    private var animationProvider: SujianAnimationEventProvider? = null

    // ── 配置 ──
    var maxAnimatedChars: Int = 8
    var animationDurationMs: Long = 160L

    // ── 监听器 ──
    var onTextChanged: ((SujianEditResult) -> Unit)? = null

    fun setAnimationEventProvider(provider: SujianAnimationEventProvider) {
        animationProvider = provider
    }

    // ── 公共 API ──

    /**
     * 加载文本（章节加载），不触发动画
     */
    fun loadText(newText: String) {
        val oldText = text
        text = newText
        selection = SujianSelection.collapsed(0)
        clearComposing()

        val events = queryAnimationEvents(oldText, newText, 0, 0, "Load")
        onTextChanged?.invoke(SujianEditResult(newText, selection, SujianEditCause.Load, events))
    }

    /**
     * 插入文本（commitText），触发 TypingCommit 或 Typing
     */
    fun commitText(inserted: String, cause: SujianEditCause = SujianEditCause.TypingCommit): SujianEditResult {
        if (inserted.isEmpty()) return SujianEditResult(text, selection, cause, emptyList())

        val oldText = text
        val oldCursorUtf8 = selectionUtf8()

        // 如果有选区，先删除选中内容
        val effectiveText = if (!selection.isCollapsed) {
            deleteSelection()
        } else {
            text
        }

        val insertPos = selection.head
        text = effectiveText.substring(0, insertPos) + inserted + effectiveText.substring(insertPos)
        val newCursorPos = insertPos + inserted.length
        selection = SujianSelection.collapsed(newCursorPos)
        clearComposing()

        val newCursorUtf8 = selectionUtf8()
        val causeStr = when (cause) {
            SujianEditCause.Typing -> "Typing"
            SujianEditCause.TypingCommit -> "TypingCommit"
            SujianEditCause.Paste -> "Paste"
            else -> "Programmatic"
        }
        val events = queryAnimationEvents(oldText, text, oldCursorUtf8, newCursorUtf8, causeStr)
        val result = SujianEditResult(text, selection, cause, events)
        onTextChanged?.invoke(result)
        return result
    }

    /**
     * 删除光标周围的文本
     */
    fun deleteSurrounding(beforeLength: Int, afterLength: Int): SujianEditResult {
        val oldText = text
        val oldCursorUtf8 = selectionUtf8()

        val cursorPos = selection.head
        val deleteStart = (cursorPos - beforeLength).coerceAtLeast(0)
        val deleteEnd = (cursorPos + afterLength).coerceAtMost(text.length)

        if (deleteStart >= deleteEnd) return SujianEditResult(text, selection, SujianEditCause.Delete, emptyList())

        text = text.substring(0, deleteStart) + text.substring(deleteEnd)
        selection = SujianSelection.collapsed(deleteStart)
        clearComposing()

        val newCursorUtf8 = selectionUtf8()
        val events = queryAnimationEvents(oldText, text, oldCursorUtf8, newCursorUtf8, "Delete")
        val result = SujianEditResult(text, selection, SujianEditCause.Delete, events)
        onTextChanged?.invoke(result)
        return result
    }

    /**
     * 删除选区内容
     */
    fun deleteSelection(): String {
        if (selection.isCollapsed) return ""
        val deleted = text.substring(selection.start, selection.end)
        text = text.substring(0, selection.start) + text.substring(selection.end)
        selection = SujianSelection.collapsed(selection.start)
        clearComposing()
        return deleted
    }

    /**
     * 设置 composing 区域（仅更新 preedit 层，不触发正文变更/动画）
     */
    fun setComposingRegion(start: Int, end: Int) {
        composingStart = start.coerceIn(0, text.length)
        composingEnd = end.coerceIn(0, text.length)
    }

    /**
     * 设置 composing 文本（替换当前 composing 区域）
     * 只更新 composing 状态，不修改正文 buffer
     */
    fun setComposingText(composing: String?, newCursorPos: Int) {
        if (composing.isNullOrEmpty()) {
            clearComposing()
            return
        }
        // composing text 不进正文 buffer，只记录 composing 状态
        // 实际的 preedit 显示由 SujianEditorView 的 preedit 层处理
        composingStart = selection.head
        composingEnd = selection.head + composing.length
    }

    /**
     * 结束 composing（finishComposingText）
     */
    fun finishComposing() {
        clearComposing()
    }

    /**
     * 设置选区
     */
    fun setSelection(start: Int, end: Int) {
        selection = SujianSelection(
            anchor = start.coerceIn(0, text.length),
            head = end.coerceIn(0, text.length)
        )
    }

    /**
     * 获取光标前的文本
     */
    fun getTextBeforeCursor(maxLen: Int): String {
        val start = (selection.head - maxLen).coerceAtLeast(0)
        return text.substring(start, selection.head)
    }

    /**
     * 获取光标后的文本
     */
    fun getTextAfterCursor(maxLen: Int): String {
        val end = (selection.head + maxLen).coerceAtMost(text.length)
        return text.substring(selection.head, end)
    }

    /**
     * 获取选中的文本
     */
    fun getSelectedText(): String? {
        if (selection.isCollapsed) return null
        return text.substring(selection.start, selection.end)
    }

    // ── UTF-8 转换辅助 ──

    /**
     * UTF-16 offset → UTF-8 byte offset（基于当前 buffer.text）
     * 正确处理 surrogate pair：高/低代理各算 3 字节（BMP supplementary 按 4 字节算 code point）。
     */
    fun utf16ToUtf8(utf16Offset: Int): Int {
        return utf16ToUtf8(text, utf16Offset)
    }

    /**
     * UTF-8 byte offset → UTF-16 offset（基于当前 buffer.text）
     */
    fun utf8ToUtf16(utf8Offset: Int): Int {
        return utf8ToUtf16(text, utf8Offset)
    }

    companion object {
        /**
         * UTF-16 offset → UTF-8 byte offset（静态方法，接受 text 参数）
         *
         * 按 code point 遍历 text，累加每个 code point 的 UTF-8 字节数。
         * 正确处理 surrogate pair：高代理+低代理作为一个 code point 计算。
         */
        @JvmStatic
        fun utf16ToUtf8(text: String, utf16Offset: Int): Int {
            var byteOffset = 0
            var charIdx = 0
            val safeOffset = utf16Offset.coerceIn(0, text.length)
            while (charIdx < safeOffset) {
                val codePoint = text.codePointAt(charIdx)
                byteOffset += when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
                charIdx += Character.charCount(codePoint)
            }
            return byteOffset
        }

        /**
         * UTF-8 byte offset → UTF-16 offset（静态方法，接受 text 参数）
         *
         * 按 code point 遍历 text，累加 UTF-8 字节数，直到达到目标 byte offset。
         * 返回对应的 UTF-16 offset（code unit 索引）。
         * 如果 utf8Offset 落在某个 code point 的 UTF-8 序列中间，停在当前 code point 之前。
         */
        @JvmStatic
        fun utf8ToUtf16(text: String, utf8Offset: Int): Int {
            var byteCount = 0
            var charIdx = 0
            while (charIdx < text.length) {
                if (byteCount >= utf8Offset) break
                val codePoint = text.codePointAt(charIdx)
                val utf8Len = when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
                if (byteCount + utf8Len > utf8Offset) break // 不拆分 code point
                byteCount += utf8Len
                charIdx += Character.charCount(codePoint)
            }
            return charIdx
        }

        /**
         * 将 offset 钳位到合法的字符边界。
         *
         * 如果 offset 落在 surrogate pair 中间（低代理位置），
         * 回退到高代理位置，确保 offset 对齐到 code point 边界。
         */
        @JvmStatic
        fun clampToCharBoundary(text: String, offset: Int): Int {
            if (offset <= 0) return 0
            if (offset >= text.length) return text.length
            // 如果 offset 指向低代理，回退到高代理
            if (Character.isLowSurrogate(text[offset]) && offset > 0 && Character.isHighSurrogate(text[offset - 1])) {
                return offset - 1
            }
            return offset
        }
    }

    // ── 内部方法 ──

    private fun selectionUtf8(): Int = utf16ToUtf8(selection.head)

    private fun clearComposing() {
        composingStart = -1
        composingEnd = -1
    }

    private fun queryAnimationEvents(
        oldText: String,
        newText: String,
        oldCursorUtf8: Int,
        newCursorUtf8: Int,
        cause: String
    ): List<EditorAnimationEventData> {
        val provider = animationProvider ?: return emptyList()
        return try {
            provider.provide(
                oldText = oldText,
                newText = newText,
                oldCursorUtf8 = oldCursorUtf8,
                newCursorUtf8 = newCursorUtf8,
                cause = cause,
                maxAnimatedChars = maxAnimatedChars,
                animationDurationMs = animationDurationMs
            )
        } catch (e: Exception) {
            // Core 调用失败时跳过动画，不打断编辑
            emptyList()
        }
    }
}
