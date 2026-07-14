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
    val preeditRangeInVirtualText: IntRange,
    val preeditText: String,
    val virtualText: String,
    val affectedParagraphRange: IntRange,
    val lineSnapshots: List<AndroidLineSnapshot>,
    val cursorRect: RectF,
    val decorationRanges: List<IntRange>,
    val revisionId: Long = 0,
    val sessionId: CompositionSessionId = CompositionSessionId(0)
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
enum class SnapshotOwner {
    OwnedBySession, OwnedByTransaction, Released
}

data class OwnedRevision(
    val revision: AndroidCompositionVisualRevision,
    var owner: SnapshotOwner = SnapshotOwner.OwnedBySession
) {
    fun release() {
        check(owner != SnapshotOwner.Released) { "Double release of revision ${revision.revisionId}" }
        owner = SnapshotOwner.Released
        revision.release()
    }
}

class AndroidCompositionManager {
    private val TAG = "CompositionManager"
    private var currentOwned: OwnedRevision? = null
    private var previousOwned: OwnedRevision? = null

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldPrevious = previousOwned

        previousOwned = currentOwned

        if (oldPrevious != null) {
            if (oldPrevious.owner == SnapshotOwner.OwnedBySession) {
                oldPrevious.release()
            }
        }

        currentOwned = if (revision != null) OwnedRevision(revision) else null
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentOwned?.revision

    fun getPrevious(): AndroidCompositionVisualRevision? = previousOwned?.revision

    fun takeCurrentForTransaction(): AndroidCompositionVisualRevision? {
        val owned = currentOwned ?: return null
        check(owned.owner == SnapshotOwner.OwnedBySession) {
            "takeCurrentForTransaction: current revision ${owned.revision.revisionId} owner is ${owned.owner}, expected OwnedBySession"
        }
        owned.owner = SnapshotOwner.OwnedByTransaction
        currentOwned = null
        return owned.revision
    }

    fun takePreviousForTransaction(): AndroidCompositionVisualRevision? {
        val owned = previousOwned ?: return null
        check(owned.owner == SnapshotOwner.OwnedBySession) {
            "takePreviousForTransaction: previous revision ${owned.revision.revisionId} owner is ${owned.owner}, expected OwnedBySession"
        }
        owned.owner = SnapshotOwner.OwnedByTransaction
        previousOwned = null
        return owned.revision
    }

    fun clear() {
        if (currentOwned != null && currentOwned!!.owner == SnapshotOwner.OwnedBySession) {
            currentOwned!!.release()
        }
        if (previousOwned != null && previousOwned!!.owner == SnapshotOwner.OwnedBySession) {
            previousOwned!!.release()
        }
        currentOwned = null
        previousOwned = null
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
