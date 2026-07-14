package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

/**
 * 预输入临时视觉正文版本（issue #515 第四节）。
 *
 * 预输入文字必须真实推动后续正文、换行和 reflow，不能覆盖绘制在正文上。
 *
 * virtualText 仅用于排版和渲染，不写入正文、Undo、保存、同步和 Core 正文状态。
 *
 * 每次 setComposingText：
 * previous CompositionVisualRevision 或 committed revision
 * → new CompositionVisualRevision
 * → 重新布局受影响段落
 * → 使用相同 StaticLinePatch + AnimatedSlice 分类
 * → 创建 CompositionUpdate PlatformVisualTransaction
 */
data class AndroidCompositionVisualRevision(
    val committedText: String,
    val compositionReplaceRange: IntRange,
    val preeditText: String,
    val virtualText: String,
    val affectedParagraphRange: IntRange,
    val lineSnapshots: List<AndroidLineSnapshot>,
    val cursorRect: RectF,
    val decorationRanges: List<IntRange>
) {
    fun release() {
        lineSnapshots.forEach { it.release() }
    }
}

/**
 * 预输入装饰切片 — 下划线、分段颜色和 IME cursor。
 * 使用同一 Timeline。
 */
data class AndroidDecorationSlice(
    val rangeUtf16: IntRange,
    val kind: DecorationKind
)

enum class DecorationKind {
    Underline, ComposingCursor, SegmentColor
}

/**
 * 预输入状态管理器
 */
class AndroidCompositionManager {
    private var currentRevision: AndroidCompositionVisualRevision? = null
    private var previousRevision: AndroidCompositionVisualRevision? = null

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        previousRevision = currentRevision
        currentRevision?.release()
        currentRevision = revision
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentRevision

    fun getPrevious(): AndroidCompositionVisualRevision? = previousRevision

    fun clear() {
        currentRevision?.release()
        previousRevision?.release()
        currentRevision = null
        previousRevision = null
    }

    /**
     * 构建 virtualText：将 preeditText 插入 committedText 的 compositionReplaceRange 位置。
     * virtualText 用于排版和渲染，不修改正文 buffer。
     */
    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
