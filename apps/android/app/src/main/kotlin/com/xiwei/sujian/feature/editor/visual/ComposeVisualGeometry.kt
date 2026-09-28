package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
import com.xiwei.sujian.feature.editor.layout.cursorRect

/**
 * 动画片段的屏幕几何：插值、路径包围盒、是否真的动了、切片落点、光标快照。
 *
 * 拆自 1372 行的 [ComposeVisualRebase]。这一层是纯几何计算，不依赖 offset map
 * 或 retained move 状态，retained moves 与合并状态机都复用 [safePathBounds]。
 */
internal object ComposeVisualGeometry {
    /** 线性插值 helper。 */
    fun lerpFloat(
        a: Float,
        b: Float,
        t: Float,
    ): Float = a + (b - a) * t.coerceIn(0f, 1f)

    /**
     * 安全获取 path bounds — range 无效或越界时返回 null。
     *
     * Issue #728 评论 5754045689：删除 EditorSoftBreakProjection 后，
     * range 直接是正文坐标，不再需要 raw→display 映射。
     */
    fun safePathBounds(
        snapshot: ComposeLayoutSnapshot,
        range: TextRange,
    ): Rect? = snapshot.boundsForRawRange(range)

    /**
     * Issue #720 评论 5746323050 / 评论 5747339452：统一的"自然几何是否变化"判定 —
     * 输入旧/新 [ComposeLayoutSnapshot] + old/new raw range，
     * 只通过 snapshot 的 projection-aware 几何入口 [ComposeLayoutSnapshot.boundsForRawRange] 比较，
     * 不直接碰 TextLayoutResult。
     *
     * 比较完整自然几何（left/top/right/bottom）— Issue #720 评论 5747339452 要求：
     * 凡是因为自动换行、硬换行删除或前文长度变化而改变自然位置/尺寸的幸存文字，
     * 都释放给 BasicTextField。不再只判跨行 top，同行水平位移也判定为几何变化。
     *
     * - 比较 boundsForRawRange() 的 left/top/right/bottom；
     * - 任一边变化超过 [epsilon]（默认 0.5f px）就视为自然几何变化；
     * - 旧/新任一侧取不到有效 bounds（null），也按"几何变化"返回 true
     *   （不能继续由 survivor overlay 持有，交给 BasicTextField）。
     *
     * Issue #737：旧 timeline（[ComposeVisualTimeline]）已删除，本 helper 仍供
     * [ComposeVisualFrameCoordinator] 算 retained moves 时判定自然几何变化使用；
     * handoff（[ComposeLocalHandoffRebase]）也用此 helper 判定是否释放 surviving unit。
     *
     * @param oldLayout 旧布局快照。
     * @param oldRange 旧正文中的 raw range。
     * @param newLayout 新布局快照。
     * @param newRange 新正文中的 raw range。
     * @param epsilon 浮点误差容限（px），默认 0.5f。
     * @return true 表示自然几何发生变化（位置/尺寸任一边变化），应释放给 BasicTextField；false 表示几何未变化。
     */
    fun naturalGeometryChanged(
        oldLayout: ComposeLayoutSnapshot,
        oldRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        newRange: TextRange,
        epsilon: Float = 0.5f,
    ): Boolean {
        val oldBounds = oldLayout.boundsForRawRange(oldRange)
        val newBounds = newLayout.boundsForRawRange(newRange)
        // 旧/新任一侧取不到有效 bounds → 不能继续由 survivor overlay 持有
        if (oldBounds == null || newBounds == null) return true
        // Issue #720 评论 5747339452：比较完整自然几何 left/top/right/bottom。
        // 任何自然位置/尺寸变化超过 epsilon，本地 survivor 都释放给 BasicTextField。
        return kotlin.math.abs(oldBounds.left - newBounds.left) > epsilon ||
            kotlin.math.abs(oldBounds.top - newBounds.top) > epsilon ||
            kotlin.math.abs(oldBounds.right - newBounds.right) > epsilon ||
            kotlin.math.abs(oldBounds.bottom - newBounds.bottom) > epsilon
    }

    /**
     * #708 评论 5726837636：子片段屏幕位置计算 —
     * 当一个 active unit 被切开只删一部分时，ghost 的屏幕位置不能直接用父 unit 左上角，
     * 要用"slice 自然位置 + 父 unit 当前位移"。
     *
     * Issue #737：旧 timeline 的 [ComposeVisualTimeline.toGhost] 已删除，本 helper 仍供
     * handoff 的 [ComposeLocalHandoffRebase.toHandoffGhost] 共用，避免两套算法不一致
     * 导致 handoff 首帧旧字跳位。
     *
     * @param layout 父 unit 的 layout snapshot。
     * @param parentRange 父 unit 的完整 range。
     * @param sliceRange 切片 range（ghost 的 range）。
     * @param parentScreenPosition 父 unit 当前屏幕位置（已含位移）。
     * @return slice 的屏幕位置；layout 取不到自然位置时返回 null。
     */
    fun sliceScreenPosition(
        layout: ComposeLayoutSnapshot,
        parentRange: TextRange,
        sliceRange: TextRange,
        parentScreenPosition: Offset,
    ): Offset? {
        if (sliceRange == parentRange) return parentScreenPosition
        val parentNatural = unitPositionFromLayout(layout, parentRange) ?: return null
        val parentDelta =
            Offset(
                parentScreenPosition.x - parentNatural.x,
                parentScreenPosition.y - parentNatural.y,
            )
        val sliceNatural = unitPositionFromLayout(layout, sliceRange) ?: return null
        return Offset(sliceNatural.x + parentDelta.x, sliceNatural.y + parentDelta.y)
    }

    /** 从 layout 取 range 的左上角位置（内部 helper）。 */
    private fun unitPositionFromLayout(
        layout: ComposeLayoutSnapshot,
        range: TextRange,
    ): Offset? {
        val bounds = safePathBounds(layout, range) ?: return null
        return Offset(bounds.left, bounds.top)
    }

    /**
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     *
     * Issue #735 评论 5771063665：不再接收 [EditorVisualIntent]，改为直接接收
     * old/new selection end（UTF-16）。
     */
    fun buildCursorSnapshot(
        previousSnapshot: ComposeLayoutSnapshot?,
        currentSnapshot: ComposeLayoutSnapshot?,
        oldSelectionEndUtf16: Int = -1,
        newSelectionEndUtf16: Int = -1,
    ): VisualCursorSnapshot? {
        val prev = previousSnapshot ?: return null
        val curr = currentSnapshot ?: return null
        val oldSelectionEnd = if (oldSelectionEndUtf16 >= 0) oldSelectionEndUtf16 else prev.selection.end
        val newSelectionEnd = if (newSelectionEndUtf16 >= 0) newSelectionEndUtf16 else curr.selection.end
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
        if (oldSelectionEnd < 0 || oldSelectionEnd > oldText.length) return null
        if (newSelectionEnd < 0 || newSelectionEnd > newText.length) return null
        val oldCursorRect = prev.cursorRect(oldSelectionEnd)
        val newCursorRect = curr.cursorRect(newSelectionEnd)
        return VisualCursorSnapshot(
            oldCursorRect = oldCursorRect,
            newCursorRect = newCursorRect,
            oldSelectionEnd = oldSelectionEnd,
            newSelectionEnd = newSelectionEnd,
        )
    }
}
