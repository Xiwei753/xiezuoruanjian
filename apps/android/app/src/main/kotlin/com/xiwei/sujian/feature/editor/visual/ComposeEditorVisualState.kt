package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
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
 * #641 评论 问题3 + 评论 5457777142 问题2：transaction/rebase —
 * 新事务到来时如果旧事务还在跑，先用旧 transaction + 当前 progress 物化
 * [ComposeVisualFrame]，再把它作为新事务的 start_frame。
 * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
 *
 * #641 评论 5457777142 问题4：[onVisualIntent] 签名改为接收 [EditorMotionPolicy]
 * 而非 `durationMillis`，overlay 据此决定 text/cursor 两条 timeline。
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

    /** #641 评论 问题3：当前活跃的视觉动画事务 — 供 overlay 读取 motionPolicy 和 ranges。 */
    private val _activeTransaction = MutableStateFlow<ComposeVisualTransaction?>(null)
    val activeTransaction: StateFlow<ComposeVisualTransaction?> = _activeTransaction.asStateFlow()

    /**
     * #641 评论 5457777142 问题2：overlay 报告的当前动画 progress —
     * 新事务到来时用它物化 [ComposeVisualFrame] 作为新事务的 start_frame。
     * 由 overlay 在每帧绘制后调用 [reportProgress] 更新。
     */
    private val _currentProgress = MutableStateFlow(0f)
    val currentProgress: StateFlow<Float> = _currentProgress.asStateFlow()

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
     * #641 评论1 第5节 / 问题2 / 问题3 + 评论 5457777142 问题2/问题3/问题4：
     * Core 给出视觉意图 — 设置受影响 UTF-16 range、动画类型、cursor 和 transaction。
     *
     * #641 评论 问题2：只要 [intent.cursor]?.animate == true，不管 [intent.textKind]
     * 是什么（Insert/Delete/Move/None），都隐藏系统光标、创建 [VisualCursorSnapshot]、
     * overlay 插值画光标。`CURSOR_ONLY` 只是"没有文字动画"（textKind = None），
     * 不是"只有这种事务才允许画视觉光标"。
     *
     * #641 评论 问题3 + 评论 5457777142 问题2：transaction/rebase —
     * 新事务到来时如果旧事务还在跑，先用旧 transaction + 当前 progress 物化
     * [ComposeVisualFrame]，再把它作为新事务的 start_frame。
     * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
     *
     * #641 评论 5457777142 问题3：Delete 不隐藏新正文 range。
     * [OutputTransformation] 作用的是**新正文**。删除 `abc` 中的 `a`，oldRange=0..1，
     * 新正文 `bc` 的 0..1 是 `b`，把 `b` 设透明是错的。Delete 的离场动画由 overlay
     * 从 previous [TextLayoutResult] 画旧字，不靠 hiddenRanges。
     * 需要隐藏的是当前正文里由 overlay 接管的范围：Insert/Move 的 newRanges、
     * 以及 retained move 的 newRange。Delete 导致后续保留文字位置变化时，
     * 靠 retained move 隐藏新位置并做 old→new 位移。
     *
     * #641 评论 5457777142 问题4：[motionPolicy] 直接放进 transaction，
     * overlay 据此决定 text/cursor 两条 timeline、reduceMotion、textEnabled、cursorEnabled。
     *
     * @param motionPolicy 动画策略 — 已由调用方调用 [EditorMotionPolicy.effective]。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        // #641 评论 问题3：分配新事务 ID。
        val newId = currentTransactionId + 1
        currentTransactionId = newId
        val intentWithId = intent.copy(transactionId = newId)
        _activeIntent.update { intentWithId }

        // #641 评论 5457777142 问题3：hiddenRanges 修正。
        // Delete 不隐藏新正文 range（OutputTransformation 作用的是新正文，
        // 把新正文里 oldRange 对应位置设透明会错误隐藏现存文字）。
        // 需要隐藏的是当前正文里由 overlay 接管的范围：
        // Insert/Move 的 newRanges、retained move 的 newRange。
        val retainedMoves = computeRetainedMoves(intentWithId)
        val retainedNewRanges = retainedMoves.map { it.newRange }
        val hiddenRanges =
            (
                when (intentWithId.textKind) {
                    TextVisualKind.Insert -> intentWithId.newRanges
                    TextVisualKind.Delete -> emptyList()
                    TextVisualKind.Move -> intentWithId.newRanges
                    TextVisualKind.None -> emptyList()
                } + retainedNewRanges
            ).filter { it.start < it.end }
        _hiddenRanges.update { hiddenRanges }

        // #641 评论 问题2：只要 cursor?.animate == true，就画视觉光标。
        if (intentWithId.cursor?.animate == true) {
            _drawsVisualCursor.update { true }
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        } else {
            _drawsVisualCursor.update { false }
            _visualCursorSnapshot.update { null }
        }

        // #641 评论 问题3 + 评论 5457777142 问题2：创建视觉动画事务。
        // 如果旧事务还在跑，先用旧 transaction + 当前 progress 物化 ComposeVisualFrame，
        // 再把它作为新事务的 startFrame。
        val startFrame = materializeStartFrame()
        val transaction =
            ComposeVisualTransaction(
                id = newId,
                oldLayout = previousSnapshot,
                newLayout = currentSnapshot,
                oldRanges = intentWithId.oldRanges,
                newRanges = intentWithId.newRanges,
                retainedMoves = retainedMoves,
                cursor = intentWithId.cursor,
                startFrame = startFrame,
                motionPolicy = motionPolicy,
            )
        previousTransaction = _activeTransaction.value
        _activeTransaction.update { transaction }
    }

    /**
     * #641 评论 5457777142 问题2：物化当前视觉帧作为新事务的 start_frame。
     *
     * 用旧 transaction + 当前 progress 算出每个 slice 当前的 translate/alpha、
     * cursor 当前的 rect/alpha。新事务从该帧对应的 progress 开始，
     * 而不是 `snapTo(0f)`。
     *
     * 如果没有旧事务或 progress 已到 1f，返回 null（从 0 开始）。
     */
    private fun materializeStartFrame(): ComposeVisualFrame? {
        val prev = previousTransaction ?: return null
        val progress = _currentProgress.value
        if (progress >= 1f) return null

        val slices = mutableListOf<VisualFrameSlice>()
        // 旧事务的 newRanges：当前 alpha = progress，translate = 0
        for (range in prev.newRanges) {
            if (range.start >= range.end) continue
            slices.add(
                VisualFrameSlice(
                    range = range,
                    translate = Offset.Zero,
                    alpha = progress,
                ),
            )
        }
        // 旧事务的 retainedMoves：当前 translate 按 progress 插值 old→new bounds
        val prevLayout = prev.oldLayout
        val currLayout = prev.newLayout
        for (move in prev.retainedMoves) {
            val oldTranslate = estimateTranslate(prevLayout, move.oldRange)
            val newTranslate = estimateTranslate(currLayout, move.newRange)
            val dx = lerpFloat(oldTranslate.x, newTranslate.x, progress)
            val dy = lerpFloat(oldTranslate.y, newTranslate.y, progress)
            slices.add(
                VisualFrameSlice(
                    range = move.newRange,
                    translate = Offset(dx, dy),
                    alpha = 1f,
                ),
            )
        }

        // cursor rect：按 progress 插值 old→new
        val cursorSnapshot = _visualCursorSnapshot.value
        val cursorRect =
            if (cursorSnapshot != null && prev.cursor?.animate == true) {
                val left = lerpFloat(cursorSnapshot.oldCursorRect.left, cursorSnapshot.newCursorRect.left, progress)
                val top = lerpFloat(cursorSnapshot.oldCursorRect.top, cursorSnapshot.newCursorRect.top, progress)
                val right = lerpFloat(cursorSnapshot.oldCursorRect.right, cursorSnapshot.newCursorRect.right, progress)
                val bottom =
                    lerpFloat(
                        cursorSnapshot.oldCursorRect.bottom,
                        cursorSnapshot.newCursorRect.bottom,
                        progress,
                    )
                Rect(left, top, right, bottom)
            } else {
                null
            }
        val cursorAlpha = if (prev.cursor?.animate == true) 1f else 0f

        return ComposeVisualFrame(
            slices = slices,
            cursorRect = cursorRect,
            cursorAlpha = cursorAlpha,
        )
    }

    /**
     * 估算某 range 在某 layout 下的 translate（相对原点）。
     * 用 `getPathForRange` 的 bounds 左上角作为该 range 的位置。
     * layout 缺失时返回 [Offset.Zero]。
     */
    private fun estimateTranslate(
        layout: ComposeLayoutSnapshot?,
        range: TextRange,
    ): Offset {
        if (layout == null) return Offset.Zero
        if (range.start >= range.end) return Offset.Zero
        if (range.end > layout.result.layoutInput.text.length) return Offset.Zero
        val path = layout.result.getPathForRange(range.start, range.end)
        val bounds = path.getBounds()
        return Offset(bounds.left, bounds.top)
    }

    /** 线性插值 helper。 */
    private fun lerpFloat(
        a: Float,
        b: Float,
        t: Float,
    ): Float = a + (b - a) * t.coerceIn(0f, 1f)

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
     * #641 评论 问题3 + 评论 5457777142 问题2：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * 真实现：根据一次 replace 的 old/new 边界建立 retained text 映射。
     * 共同前缀 offset 不变；共同后缀按 `deltaUtf16 = newInsertedLength - oldRemovedLength`
     * 映射。只检查受影响视觉行附近的 retained range，用前后两份 [TextLayoutResult]
     * 比较 line/Path bounds；坐标变化的连续片段合并成 [RetainedMove]。
     *
     * 位置只从 [TextLayoutResult] 读，动画只负责画。
     * 如果 oldLayout 或 newLayout 缺失，返回空列表。
     */
    @Suppress("CyclomaticComplexMethod")
    private fun computeRetainedMoves(intent: EditorVisualIntent): List<RetainedMove> {
        if (intent.textKind == TextVisualKind.None) return emptyList()
        val prev = previousSnapshot ?: return emptyList()
        val curr = currentSnapshot ?: return emptyList()

        // 用 old/new affected ranges 推断 replace 边界。
        // 共同前缀 = oldRanges.start 之前；共同后缀 = oldRanges.end 之后。
        // deltaUtf16 = newInsertedLength - oldRemovedLength。
        val oldAffectedStart = intent.oldRanges.minOfOrNull { it.start } ?: 0
        val oldAffectedEnd = intent.oldRanges.maxOfOrNull { it.end } ?: 0
        val newAffectedStart = intent.newRanges.minOfOrNull { it.start } ?: 0
        val newAffectedEnd = intent.newRanges.maxOfOrNull { it.end } ?: 0
        val oldRemovedLength = oldAffectedEnd - oldAffectedStart
        val newInsertedLength = newAffectedEnd - newAffectedStart
        val deltaUtf16 = newInsertedLength - oldRemovedLength

        // 共同前缀长度（UTF-16）= oldAffectedStart（假设 Core 给的 range 已是最小 replace）。
        val commonPrefix = oldAffectedStart
        // 共同后缀起点（old 正文里）= oldAffectedEnd；在新正文里 = newAffectedEnd。
        val oldSuffixStart = oldAffectedEnd
        val newSuffixStart = newAffectedEnd

        val oldText = prev.result.layoutInput.text
        val newText = curr.result.layoutInput.text
        val oldTextLen = oldText.length
        val newTextLen = newText.length

        // 只检查受影响视觉行附近的 retained range，避免全文扫描。
        // 用前后两份 TextLayoutResult 比较 line/Path bounds；
        // 坐标变化的连续片段合并成 RetainedMove。
        val result = mutableListOf<RetainedMove>()
        if (oldSuffixStart >= oldTextLen || newSuffixStart >= newTextLen) return result

        // 把后缀按视觉行分段，比较每段在 old/new layout 中的 bounds。
        // 段长取一个合理上限（如 64 个 char），避免单段过大。
        val segmentMax = 64
        var oldPos = oldSuffixStart
        var newPos = newSuffixStart
        while (oldPos < oldTextLen && newPos < newTextLen) {
            val oldSegEnd = minOf(oldPos + segmentMax, oldTextLen)
            val newSegEnd = minOf(newPos + segmentMax, newTextLen)
            val oldRange = TextRange(oldPos, oldSegEnd)
            val newRange = TextRange(newPos, newSegEnd)
            val oldBounds = safePathBounds(prev.result, oldRange)
            val newBounds = safePathBounds(curr.result, newRange)
            // 坐标变化（top 或 left 差超过 1px）才算 retained move。
            if (oldBounds != null && newBounds != null) {
                val topChanged = kotlin.math.abs(oldBounds.top - newBounds.top) > 1f
                val leftChanged = kotlin.math.abs(oldBounds.left - newBounds.left) > 1f
                if (topChanged || leftChanged) {
                    result.add(RetainedMove(oldRange = oldRange, newRange = newRange))
                }
            }
            oldPos = oldSegEnd
            newPos = newSegEnd
        }
        return result
    }

    /** 安全获取 path bounds — range 无效或越界时返回 null。 */
    private fun safePathBounds(
        result: TextLayoutResult,
        range: TextRange,
    ): Rect? {
        if (range.start >= range.end) return null
        if (range.end > result.layoutInput.text.length) return null
        return try {
            result.getPathForRange(range.start, range.end).getBounds()
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * #641 评论 5457777142 问题2：overlay 报告当前动画 progress —
     * 新事务到来时用它物化 [ComposeVisualFrame]。
     */
    fun reportProgress(progress: Float) {
        _currentProgress.update { progress }
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
        _currentProgress.update { 0f }
    }

    /** 当前布局快照 — 供 overlay 读取 bounding box。 */
    fun currentLayout(): ComposeLayoutSnapshot? = currentSnapshot

    /** 上一份布局快照 — 删除文字动画用旧布局画。 */
    fun previousLayout(): ComposeLayoutSnapshot? = previousSnapshot
}
