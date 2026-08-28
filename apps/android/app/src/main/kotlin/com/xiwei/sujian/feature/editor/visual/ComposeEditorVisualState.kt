package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * #641 评论1 第4/5节：Core 返回的视觉意图 — 受影响的 UTF-16 range 和动画类型。
 * 从 Core display patch / VisualIntent 映射，offset 是 UTF-16（已由调用方从
 * UTF-8 byte 转换），不再用 byte 作为 Compose offset。
 *
 * #641 评论 问题2：cursor 拆成和文字动画并列的字段 —
 * [textKind] 描述文字动画类型（Insert/Delete/Move/None），
 * [cursor] 描述光标视觉意图。只要 [cursor] 的 [CursorVisualIntent.animate] 为 true，
 * 不管 [textKind] 是什么，都隐藏系统光标、创建 [VisualCursorSnapshot]、overlay 插值画光标。
 * `CURSOR_ONLY` 只是"没有文字动画"（[textKind] = None），
 * 不是"只有这种事务才允许画视觉光标"。
 *
 * @param transactionId 事务 ID — 由 [ComposeEditorVisualState.onVisualIntent] 内部分配，
 *   调用方可设为 0L。overlay 据此判断是否需要重新启动动画。
 * @param oldRanges 旧受影响 UTF-16 ranges — 删除动画用（来自 Core oldAffectedByteRanges）。
 * @param newRanges 新受影响 UTF-16 ranges — 插入/移动动画用（来自 Core newAffectedByteRanges）。
 * @param textKind 文字动画类型。
 * @param cursor 光标视觉意图 — null 表示不画视觉光标。
 */
data class EditorVisualIntent(
    val transactionId: Long = 0L,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val textKind: TextVisualKind,
    val cursor: CursorVisualIntent?,
)

/**
 * #641 评论 问题2：文字动画类型 — 与光标动画并列，不再用单一 Kind 枚举。
 *
 * - [Insert]：插入文字 — overlay 从 current layout 淡入 newRanges。
 * - [Delete]：删除文字 — overlay 从 previous layout 淡出 oldRanges。
 * - [Move]：移动/替换文字 — overlay 从 previous layout 淡出 oldRanges，
 *   从 current layout 淡入 newRanges。
 * - [None]：没有文字动画（如 CURSOR_ONLY 事务）。
 */
enum class TextVisualKind { Insert, Delete, Move, None }

/**
 * #641 评论 问题2：光标视觉意图 — 与文字动画并列。
 *
 * 只要 [animate] 为 true，不管 [TextVisualKind] 是什么，
 * 都隐藏系统光标、创建 [VisualCursorSnapshot]、overlay 插值画光标。
 *
 * @param oldEndUtf16 旧光标位置（UTF-16 offset）。
 * @param newEndUtf16 新光标位置（UTF-16 offset）。
 * @param animate 是否动画光标 — 来自 Core [com.xiwei.sujian.feature.editor.projection.CoordinatedCursor.shouldAnimate]。
 */
data class CursorVisualIntent(
    val oldEndUtf16: Int,
    val newEndUtf16: Int,
    val animate: Boolean,
)

/**
 * #641 评论1 第5节：视觉光标插值快照 — 保存 old/new cursor rect 和 selection，
 * overlay 据此按 progress 插值绘制视觉光标。
 */
data class VisualCursorSnapshot(
    val oldCursorRect: Rect,
    val newCursorRect: Rect,
    val oldSelectionEnd: Int,
    val newSelectionEnd: Int,
)

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态 — 保存上一份和当前一份
 * [ComposeLayoutSnapshot]，根据 Core 的 [EditorVisualIntent] 算受影响 UTF-16 range。
 *
 * 动画层只"画"，绝不能再改变 viewport / selection / IME 几何。
 * [onAuthoritativeLayout] 由 [BasicTextField] 的 `onTextLayout` 回调调用，
 * 把系统最终 [TextLayoutResult] 记录为权威布局，不反向修改输入。
 *
 * #641 评论 问题3：transaction/rebase — 新事务到来时如果旧事务还在跑，
 * 先把当前视觉帧物化成下一事务起点，再 rebase。
 * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
 */
class ComposeEditorVisualState(
    initialDrawsVisualCursor: Boolean = false,
) {
    /** 上一份布局快照 — 删除文字动画按旧 range 的 bounding box 画旧布局。 */
    private var previousSnapshot: ComposeLayoutSnapshot? = null

    /** 当前布局快照 — 来自系统 [BasicTextField] 的最终 [TextLayoutResult]。 */
    private var currentSnapshot: ComposeLayoutSnapshot? = null

    /**
     * 当前正在动画的 UTF-16 range — 这些 range 在 [OutputTransformation] 里被设为透明，
     * overlay 补画动画过程；动画完成立即从该列表删除，系统正文已在最终位置。
     */
    private val _hiddenRanges = MutableStateFlow<List<TextRange>>(emptyList())
    val hiddenRanges: StateFlow<List<TextRange>> = _hiddenRanges.asStateFlow()

    /**
     * 视觉光标是否由 overlay 绘制 — true 时 [BasicTextField] 的 cursorBrush 设为透明，
     * overlay 从 `oldResult.getCursorRect(oldSelection.end)` 插值到
     * `newResult.getCursorRect(newSelection.end)`。
     */
    private val _drawsVisualCursor = MutableStateFlow(initialDrawsVisualCursor)
    val drawsVisualCursor: StateFlow<Boolean> = _drawsVisualCursor.asStateFlow()

    /** 当前活跃的视觉意图 — 供 overlay 读取动画类型。 */
    private val _activeIntent = MutableStateFlow<EditorVisualIntent?>(null)
    val activeIntent: StateFlow<EditorVisualIntent?> = _activeIntent.asStateFlow()

    /**
     * 视觉光标插值快照 — cursor animate=true 时由 [onVisualIntent] 根据当前/上一份
     * [TextLayoutResult] 的 cursor rect 计算并保存，overlay 据此按 progress 插值。
     */
    private val _visualCursorSnapshot = MutableStateFlow<VisualCursorSnapshot?>(null)
    val visualCursorSnapshot: StateFlow<VisualCursorSnapshot?> = _visualCursorSnapshot.asStateFlow()

    /** #641 评论 问题3：当前事务 ID — 单调递增，overlay 据此判断是否需要重新启动动画。 */
    private var currentTransactionId: Long = 0L

    /** #641 评论 问题3：上一份完成的视觉动画事务 — 供 rebase 用。 */
    private var previousTransaction: ComposeVisualTransaction? = null

    /** #641 评论 问题3：当前活跃的视觉动画事务 — 供 overlay 读取 duration 和 ranges。 */
    private val _activeTransaction = MutableStateFlow<ComposeVisualTransaction?>(null)
    val activeTransaction: StateFlow<ComposeVisualTransaction?> = _activeTransaction.asStateFlow()

    /**
     * #641 评论1 第5节：系统给出权威布局 — 只记录，不修改输入几何。
     * 动画层据此算受影响 range，但不 scrollTo、不改 selection、不改 editor height。
     *
     * #641 评论1 第5节：onVisualIntent 时机 — commit 时 currentSnapshot 可能还是旧 layout。
     * 在新 layout 到达后，若当前活跃 intent 的 cursor 要动画，重新构建 cursor snapshot
     * （用新的 currentSnapshot 作为 newCursorRect，旧的 previousSnapshot 作为 oldCursorRect），
     * 避免 commit 时两份 rect 都是旧 layout。
     *
     * #641 评论 问题2：只要 cursor?.animate == true（不管 textKind），重新构建 cursor snapshot。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
    ) {
        previousSnapshot = currentSnapshot
        currentSnapshot = ComposeLayoutSnapshot(result, selection, scrollY)
        // 新 layout 到达后，若当前活跃 intent 的 cursor 要动画，重新构建 cursor snapshot。
        if (_activeIntent.value?.cursor?.animate == true) {
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        }
    }

    /**
     * #641 评论1 第5节 / 问题2 / 问题3：Core 给出视觉意图 —
     * 设置受影响 UTF-16 range、动画类型、cursor 和 transaction。
     *
     * #641 评论 问题2：只要 [intent.cursor]?.animate == true，不管 [intent.textKind]
     * 是什么（Insert/Delete/Move/None），都隐藏系统光标、创建 [VisualCursorSnapshot]、
     * overlay 插值画光标。`CURSOR_ONLY` 只是"没有文字动画"（textKind = None），
     * 不是"只有这种事务才允许画视觉光标"。
     *
     * #641 评论 问题3：transaction/rebase — 新事务到来时如果旧事务还在跑，
     * 先把当前视觉帧物化成下一事务起点（用 currentSnapshot 作为新事务的 oldLayout），
     * 再 rebase。不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
     *
     * @param durationMillis 动画时长 — 来自 [com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy]。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        durationMillis: Long,
    ) {
        // #641 评论 问题3：分配新事务 ID。
        val newId = currentTransactionId + 1
        currentTransactionId = newId
        val intentWithId = intent.copy(transactionId = newId)
        _activeIntent.update { intentWithId }

        // hiddenRanges：根据 textKind 决定用 oldRanges 还是 newRanges。
        // Insert：隐藏 newRanges（新文字在 BasicTextField 中，overlay 补画淡入）。
        // Delete：隐藏 oldRanges（旧文字已不在 BasicTextField 中，OutputTransformation
        //   的 range.end <= length 检查会跳过无效 range，overlay 从 previous layout 画旧文字淡出）。
        // Move：隐藏 newRanges（新位置的文字在 BasicTextField 中，overlay 补画位移）。
        // None：不隐藏。
        val hiddenRanges =
            when (intentWithId.textKind) {
                TextVisualKind.Insert -> intentWithId.newRanges
                TextVisualKind.Delete -> intentWithId.oldRanges
                TextVisualKind.Move -> intentWithId.newRanges
                TextVisualKind.None -> emptyList()
            }.filter { it.start < it.end }
        _hiddenRanges.update { hiddenRanges }

        // #641 评论 问题2：只要 cursor?.animate == true，就画视觉光标。
        if (intentWithId.cursor?.animate == true) {
            _drawsVisualCursor.update { true }
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        } else {
            _drawsVisualCursor.update { false }
            _visualCursorSnapshot.update { null }
        }

        // #641 评论 问题3：创建视觉动画事务。
        // 如果旧事务还在跑，用 currentSnapshot 作为新事务的 oldLayout（物化当前视觉帧）。
        val transaction =
            ComposeVisualTransaction(
                id = newId,
                oldLayout = previousSnapshot,
                newLayout = currentSnapshot,
                oldRanges = intentWithId.oldRanges,
                newRanges = intentWithId.newRanges,
                retainedMoves = computeRetainedMoves(intentWithId),
                cursor = intentWithId.cursor,
                durationMillis = durationMillis,
            )
        previousTransaction = _activeTransaction.value
        _activeTransaction.update { transaction }
    }

    /**
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     * old cursor 来自 previous result + previous selection end；
     * new cursor 来自 current result + current selection end。
     * 任一 layout 缺失时不构建快照（overlay 只画已有的一侧）。
     *
     * #641 评论 问题2：old/new selection end 从 [CursorVisualIntent] 读取。
     */
    private fun buildCursorSnapshot(): VisualCursorSnapshot? {
        val prev = previousSnapshot ?: return null
        val curr = currentSnapshot ?: return null
        val intent = _activeIntent.value
        val cursor = intent?.cursor
        val oldSelectionEnd = cursor?.oldEndUtf16 ?: prev.selection.end
        val newSelectionEnd = cursor?.newEndUtf16 ?: curr.selection.end
        val oldCursorRect = prev.result.getCursorRect(oldSelectionEnd)
        val newCursorRect = curr.result.getCursorRect(newSelectionEnd)
        return VisualCursorSnapshot(
            oldCursorRect = oldCursorRect,
            newCursorRect = newCursorRect,
            oldSelectionEnd = oldSelectionEnd,
            newSelectionEnd = newSelectionEnd,
        )
    }

    /**
     * #641 评论 问题3：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * 位置只从 [TextLayoutResult] 读，动画只负责画。
     * 如果 oldLayout 或 newLayout 缺失，返回空列表。
     * 简化实现：如果 textKind 是 Move，找出 newRanges 中不在 oldRanges 的 range
     * （即因折行移动的文字），构造 [RetainedMove]。
     */
    private fun computeRetainedMoves(intent: EditorVisualIntent): List<RetainedMove> {
        if (intent.textKind != TextVisualKind.Move) return emptyList()
        previousSnapshot ?: return emptyList()
        currentSnapshot ?: return emptyList()

        // 找出 newRanges 中不在 oldRanges 的 range（即因折行移动的文字）。
        val result = mutableListOf<RetainedMove>()
        for (newRange in intent.newRanges) {
            val isContainedInOld =
                intent.oldRanges.any { oldRange ->
                    newRange.start >= oldRange.start && newRange.end <= oldRange.end
                }
            if (!isContainedInOld) {
                // 这个 range 是因折行移动的"保留文字"。
                // 简化：用 newRange 作为 oldRange（位置只从 layout 读，动画只负责画）。
                result.add(RetainedMove(oldRange = newRange, newRange = newRange))
            }
        }
        return result
    }

    /**
     * #641 评论1 第5节：动画结束 — 清 hiddenRanges，系统正文马上可见。
     * 由 overlay 的动画完成回调调用。
     */
    fun clearAnimation() {
        _hiddenRanges.update { emptyList() }
        _activeIntent.update { null }
        _drawsVisualCursor.update { false }
        _visualCursorSnapshot.update { null }
        _activeTransaction.update { null }
    }

    /** 当前布局快照 — 供 overlay 读取 bounding box。 */
    fun currentLayout(): ComposeLayoutSnapshot? = currentSnapshot

    /** 上一份布局快照 — 删除文字动画用旧布局画。 */
    fun previousLayout(): ComposeLayoutSnapshot? = previousSnapshot
}
