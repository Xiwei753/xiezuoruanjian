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
 * 预输入状态管理器（issue #516 资源所有权修正）
 *
 * 所有权规则：
 * - Manager 只持有 revision 元数据和快照所有权。
 * - 创建新事务时通过 take 转移旧 revision 快照，不能复制引用后立即 release。
 * - 事务完成或取消后统一释放其持有的 old/new snapshots。
 * - clear() 只能释放仍由 manager 持有、尚未转移给事务的资源。
 */
class AndroidCompositionManager {
    private var currentRevision: AndroidCompositionVisualRevision? = null
    private var previousRevision: AndroidCompositionVisualRevision? = null
    private var currentTransferred: Boolean = false
    private var previousTransferred: Boolean = false

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldPrevious = previousRevision
        val oldPreviousTransferred = previousTransferred

        previousRevision = currentRevision
        previousTransferred = currentTransferred

        if (oldPrevious != null && !oldPreviousTransferred) {
            oldPrevious.release()
        }

        currentRevision = revision
        currentTransferred = false
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentRevision

    fun getPrevious(): AndroidCompositionVisualRevision? = previousRevision

    fun takeCurrentForTransaction(): AndroidCompositionVisualRevision? {
        val rev = currentRevision
        if (rev != null) {
            currentTransferred = true
        }
        return rev
    }

    fun takePreviousForTransaction(): AndroidCompositionVisualRevision? {
        val rev = previousRevision
        if (rev != null) {
            previousTransferred = true
        }
        return rev
    }

    fun clear() {
        if (currentRevision != null && !currentTransferred) {
            currentRevision!!.release()
        }
        if (previousRevision != null && !previousTransferred) {
            previousRevision!!.release()
        }
        currentRevision = null
        previousRevision = null
        currentTransferred = false
        previousTransferred = false
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
