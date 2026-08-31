package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #644 评论 5467821839 第5节剩余子项：visual rebase 纯计算 —
 * 从 [ComposeEditorVisualState] 抽出的无副作用几何/区间函数。
 *
 * [ComposeEditorVisualState] 只负责保存 previous/current layout、pending intent、
 * active transaction、progress，并调用这里的纯函数。viewport 恢复、session attach、
 * 正文写回不进 visual 层。
 *
 * 全部函数无副作用、不读 Compose mutable state、不修改输入几何。
 * 位置只从 [TextLayoutResult] 读，动画只负责画。
 *
 * #641 评论 5459754425 / 5459896691 / 5460160958 / 5460233781 / 5460373035：
 * rebase 物化、split、subtract、retained moves 的完整算法搬自原
 * [ComposeEditorVisualState]，逻辑不变，只把对 mutable state 的依赖改成显式参数。
 */
@Suppress("LargeClass", "TooManyFunctions")
internal object ComposeVisualRebase {
    /**
     * 物化 start_frame 的参数 — 提取以降低 [materializeStartFrame] 参数列表长度。
     */
    data class MaterializeStartFrameParams(
        val transaction: ComposeVisualTransaction?,
        val textProgress: Float,
        val cursorProgress: Float,
        val rebaseProgress: Float,
        val nextReplaceBounds: VisualReplaceBounds?,
        val hiddenRanges: List<TextRange>,
        val cursorSnapshot: VisualCursorSnapshot?,
    )

    /**
     * #641 评论 5459754425 + 评论 5459896691：物化当前视觉帧作为新事务的 start_frame。
     *
     * 每次物化都把当前屏幕正在显示的所有内容 flatten 成一层新的扁平 [ComposeVisualFrame]，
     * 每个 [RebasedTextSlice] 携带自己的 sourceLayout。不再形成 startFrame 套 startFrame 的链。
     *
     * #641 评论 5458283021 问题1a：直接接收 [transaction]（当前正在跑的事务）。
     * #641 评论 5458283021 问题1c：分别接收 [textProgress] / [cursorProgress]。
     * #641 评论 5459896691 第1项：增加 [rebaseProgress] — 三条 timeline 都结束才算无视觉帧。
     *
     * #641 评论 5460160958 问题2：增加 [nextReplaceBounds] — frozenStartFrame 马上要交给本事务（C）
     *   绘制，surviving targetRange 必须是 C 的 new text 坐标，因此用 incoming 的 replaceBounds
     *   映射，而不是上一事务（B）自己的 replaceBounds。
     * #641 评论 5460160958 问题3：flatten 旧 startFrame 前先按当前 [rebaseProgress] 物化每个 slice
     *   到"这一帧真实状态"，避免从 A 当初冻结的 sourceAlpha/sourceTranslate 重新起跑。
     * #641 评论 5460160958 问题4：targetRange 切成 prefix/suffix 时 sourceRange 成对切分。
     * #641 评论 5460373035 问题2：splitRebasedSliceThroughReplace 返回 [SplitRebasedResult]，
     *   overlap 部分生成 fading slice + ownedOldRange。本方法聚合所有 split 的 ownedOldRanges
     *   计入返回 frame 的 [ComposeVisualFrame.ownedOldRanges]。
     *
     * [hiddenRanges] / [cursorSnapshot] 由调用方从 visual state 读出后传入，
     * 本函数不直接访问任何 mutable state。
     *
     * 如果没有旧事务或 text/cursor/rebase 三条 progress 都已到 1f，返回 null。
     */
    fun materializeStartFrame(params: MaterializeStartFrameParams): ComposeVisualFrame? {
        val transaction = params.transaction
        val textProgress = params.textProgress
        val cursorProgress = params.cursorProgress
        val rebaseProgress = params.rebaseProgress
        val nextReplaceBounds = params.nextReplaceBounds
        val hiddenRanges = params.hiddenRanges
        val cursorSnapshot = params.cursorSnapshot
        val prev = transaction ?: return null
        // #641 评论 5459896691 第1项：三条当前实际存在的 timeline 都结束才算没有视觉帧。
        if (textProgress >= 1f && cursorProgress >= 1f && rebaseProgress >= 1f) return null

        val prevStartFrame = prev.startFrame
        // #641 评论 5460160958 问题3：先按当前 rebaseProgress 物化旧 startFrame slice。
        // #641 评论 5460233781 问题2：materializeRebasedSlice 可能返回 null，用 mapNotNull 过滤。
        val materializedOlder =
            prevStartFrame?.slices?.mapNotNull {
                materializeRebasedSlice(it, prev.newLayout, rebaseProgress)
            } ?: emptyList()

        val currentSlices = collectCurrentSlicesAsRebased(prev, textProgress)
        val retainedSlices = collectRetainedMoveSlicesAsRebased(prev, textProgress)

        // #641 评论 5460160958 问题2+问题4：统一用 nextReplaceBounds 映射 surviving targetRange。
        // #641 评论 5460373035 问题2：聚合所有 split 的 ownedOldRanges 计入返回 frame。
        val allSlices = materializedOlder + currentSlices + retainedSlices
        val mappedSlices = mutableListOf<RebasedTextSlice>()
        val ownedOldRanges = mutableListOf<TextRange>()
        if (nextReplaceBounds == null) {
            mappedSlices.addAll(allSlices)
        } else {
            for (slice in allSlices) {
                if (slice.targetRange == null) {
                    mappedSlices.add(slice)
                } else {
                    val split = splitRebasedSliceThroughReplace(slice, nextReplaceBounds)
                    mappedSlices.addAll(split.slices)
                    ownedOldRanges.addAll(split.ownedOldRanges)
                }
            }
        }

        val cursorRect = materializeCursorRect(prev, cursorProgress, cursorSnapshot)
        val cursorAlpha = if (prev.cursor?.animate == true) 1f else 0f

        return ComposeVisualFrame(
            slices = mappedSlices,
            cursorRect = cursorRect,
            cursorAlpha = cursorAlpha,
            suppressedCurrentRanges = hiddenRanges,
            ownedOldRanges = ownedOldRanges,
        )
    }

    /**
     * #641 评论 5459896691 第2项 + 评论 5460070064 第3项：
     * 按 [prev.textKind] 物化当前屏幕仍可见的 slice 为 [RebasedTextSlice]。
     */
    fun collectCurrentSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> =
        when (prev.textKind) {
            TextVisualKind.Delete ->
                rebasedSlices(prev.oldRanges, prev.oldLayout, 1f - textProgress, targetRange = null)
            TextVisualKind.Move ->
                rebasedSlices(prev.oldRanges, prev.oldLayout, 1f - textProgress, targetRange = null) +
                    survivingRebasedSlices(prev.newRanges, prev.newLayout, textProgress)
            TextVisualKind.Insert ->
                survivingRebasedSlices(prev.newRanges, prev.newLayout, textProgress)
            TextVisualKind.None -> emptyList()
        }

    /**
     * 把 [ranges] 里有效段物化成 [RebasedTextSlice]，alpha = [alphaRaw].coerceIn(0,1)。
     * [layout] 为 null 时跳过。[targetRange] = null 表示只属于旧画面（rebase 期间淡出）。
     */
    fun rebasedSlices(
        ranges: List<TextRange>,
        layout: ComposeLayoutSnapshot?,
        alphaRaw: Float,
        targetRange: TextRange?,
    ): List<RebasedTextSlice> {
        if (layout == null) return emptyList()
        val alpha = alphaRaw.coerceIn(0f, 1f)
        return ranges
            .filter { it.start < it.end && it.end <= layout.result.layoutInput.text.length }
            .map { range ->
                RebasedTextSlice(
                    sourceLayout = layout,
                    sourceRange = range,
                    sourceTranslate = Offset.Zero,
                    sourceAlpha = alpha,
                    targetRange = targetRange,
                )
            }
    }

    /**
     * #641 评论 5460070064 第3项：surviving slice — targetRange = range 自身。
     */
    fun survivingRebasedSlices(
        ranges: List<TextRange>,
        layout: ComposeLayoutSnapshot?,
        alphaRaw: Float,
    ): List<RebasedTextSlice> =
        rebasedSlices(ranges, layout, alphaRaw, targetRange = null).map { slice ->
            slice.copy(targetRange = slice.sourceRange)
        }

    /**
     * 旧事务的 retainedMoves → [RebasedTextSlice]。
     * translate 按 [textProgress] 插值 old→new bounds，alpha=1。
     *
     * #641 评论 5459531909 第4项：translate = delta（相对 source layout 原位置的偏移）。
     * #641 评论 5460070064 第3项：retained 文字在当前 new text 里仍存在 → surviving。
     */
    fun collectRetainedMoveSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> {
        val prevLayout = prev.oldLayout
        val currLayout = prev.newLayout
        if (currLayout == null) return emptyList()
        val slices = mutableListOf<RebasedTextSlice>()
        for (move in prev.retainedMoves) {
            val oldBounds = prevLayout?.let { safePathBounds(it.result, move.oldRange) }
            val newBounds = safePathBounds(currLayout.result, move.newRange)
            if (oldBounds == null || newBounds == null) continue
            val currentX = lerpFloat(oldBounds.left, newBounds.left, textProgress)
            val currentY = lerpFloat(oldBounds.top, newBounds.top, textProgress)
            val translate =
                Offset(
                    currentX - newBounds.left,
                    currentY - newBounds.top,
                )
            slices.add(
                RebasedTextSlice(
                    sourceLayout = currLayout,
                    sourceRange = move.newRange,
                    sourceTranslate = translate,
                    sourceAlpha = 1f,
                    targetRange = move.newRange,
                ),
            )
        }
        return slices
    }

    /**
     * #641 评论 5460160958 问题3：按当前 [rebaseProgress] 物化单个 [RebasedTextSlice]。
     *
     * - [slice.targetRange] == null（fading）：alpha = lerp(sourceAlpha, 0f, rebaseProgress)。
     *   alpha <= 0 时返回 null 丢弃。
     * - [slice.targetRange] != null（surviving）：alpha = lerp(sourceAlpha, 1f, rebaseProgress)，
     *   位置从 source bounds + sourceTranslate 插值到 target bounds，重新锚定到 [currentLayout]。
     *   [currentLayout] 为 null 或 bounds 无效时只更新 alpha。
     *
     * 返回 null 表示该 slice 应被丢弃（fading alpha 已降到 0）。
     */
    fun materializeRebasedSlice(
        slice: RebasedTextSlice,
        currentLayout: ComposeLayoutSnapshot?,
        rebaseProgress: Float,
    ): RebasedTextSlice? {
        val sourceAlpha = slice.sourceAlpha
        val targetRange = slice.targetRange
        if (targetRange == null) {
            // fading：从原位置继续淡出，alpha 向 0 收敛。
            val currentAlpha = lerpFloat(sourceAlpha, 0f, rebaseProgress)
            if (currentAlpha <= 0f) return null
            return slice.copy(sourceAlpha = currentAlpha)
        }
        // surviving：alpha 向 1 收敛。
        val currentAlpha = lerpFloat(sourceAlpha, 1f, rebaseProgress)
        if (currentLayout == null) {
            return slice.copy(sourceAlpha = currentAlpha)
        }
        val sourceBounds = safePathBounds(slice.sourceLayout.result, slice.sourceRange)
        val targetBounds = safePathBounds(currentLayout.result, targetRange)
        if (sourceBounds == null || targetBounds == null) {
            return slice.copy(sourceAlpha = currentAlpha)
        }
        // #641 评论 5460233781 问题2：用 source bounds + sourceTranslate 与 target bounds 算当前 x/y。
        val currentX =
            lerpFloat(
                sourceBounds.left + slice.sourceTranslate.x,
                targetBounds.left,
                rebaseProgress,
            )
        val currentY =
            lerpFloat(
                sourceBounds.top + slice.sourceTranslate.y,
                targetBounds.top,
                rebaseProgress,
            )
        return RebasedTextSlice(
            sourceLayout = currentLayout,
            sourceRange = targetRange,
            sourceTranslate = Offset(currentX - targetBounds.left, currentY - targetBounds.top),
            sourceAlpha = currentAlpha,
            targetRange = targetRange,
        )
    }

    /**
     * #641 评论 5460373035 问题2：splitRebasedSliceThroughReplace 的返回 —
     * surviving prefix/suffix slices + 被 replace overlap 接管的 old-text ranges。
     */
    data class SplitRebasedResult(
        val slices: List<RebasedTextSlice>,
        val ownedOldRanges: List<TextRange>,
    )

    /**
     * #641 评论 5460160958 问题4：surviving slice 通过下一事务 replace 边界切分时，
     * sourceRange 和 targetRange 成对切分。
     *
     * 前提：surviving slice 表示同一逻辑文本，sourceRange 长度应等于 oldTarget 长度。
     * 若长度不等（不应发生），不静默复制整段——结束该 surviving 映射，按旧画面离场处理：
     * 返回 SplitRebasedResult(listOf(slice.copy(targetRange = null)), emptyList())。
     *
     * - prefix 部分（target 在 [0, b.oldStart) 里，位置不变）。
     * - suffix 部分（target 在 [b.oldEnd, ...) 里，平移 delta）。
     * - #641 评论 5460373035 问题2：overlap 部分不丢弃，生成 targetRange = null 的 fading slice，
     *   同时把 overlap 的 old-text range 计进 [SplitRebasedResult.ownedOldRanges]。
     */
    fun splitRebasedSliceThroughReplace(
        slice: RebasedTextSlice,
        b: VisualReplaceBounds,
    ): SplitRebasedResult {
        val oldTarget = slice.targetRange ?: return SplitRebasedResult(listOf(slice), emptyList())
        val sourceRange = slice.sourceRange
        if ((sourceRange.end - sourceRange.start) != (oldTarget.end - oldTarget.start)) {
            return SplitRebasedResult(listOf(slice.copy(targetRange = null)), emptyList())
        }
        val outSlices = mutableListOf<RebasedTextSlice>()
        val ownedOldRanges = mutableListOf<TextRange>()
        // prefix 部分（target 在 [0, b.oldStart) 里，位置不变）
        val prefixEnd = minOf(oldTarget.end, b.oldStart)
        if (oldTarget.start < prefixEnd) {
            val len = prefixEnd - oldTarget.start
            val newTarget = TextRange(oldTarget.start, prefixEnd)
            val newSource = TextRange(sourceRange.start, sourceRange.start + len)
            outSlices.add(slice.copy(sourceRange = newSource, targetRange = newTarget))
        }
        // overlap 部分（target 与 [b.oldStart, b.oldEnd) 重叠）不丢弃。
        val overlapStart = maxOf(oldTarget.start, b.oldStart)
        val overlapEnd = minOf(oldTarget.end, b.oldEnd)
        if (overlapStart < overlapEnd) {
            val sourceOffset = overlapStart - oldTarget.start
            val len = overlapEnd - overlapStart
            val overlapSource =
                TextRange(
                    sourceRange.start + sourceOffset,
                    sourceRange.start + sourceOffset + len,
                )
            outSlices.add(slice.copy(sourceRange = overlapSource, targetRange = null))
            ownedOldRanges.add(TextRange(overlapStart, overlapEnd))
        }
        // suffix 部分（target 在 [b.oldEnd, ...) 里，平移 delta）
        val suffixStart = maxOf(oldTarget.start, b.oldEnd)
        if (suffixStart < oldTarget.end) {
            val delta = b.newEnd - b.oldEnd
            val newTarget = TextRange(suffixStart + delta, oldTarget.end + delta)
            val len = oldTarget.end - suffixStart
            val newSource = TextRange(sourceRange.end - len, sourceRange.end)
            outSlices.add(slice.copy(sourceRange = newSource, targetRange = newTarget))
        }
        return SplitRebasedResult(outSlices, ownedOldRanges)
    }

    /**
     * cursor rect：按 [cursorProgress] 插值 old→new。无 cursor 动画时返回 null。
     */
    fun materializeCursorRect(
        prev: ComposeVisualTransaction,
        cursorProgress: Float,
        cursorSnapshot: VisualCursorSnapshot?,
    ): Rect? {
        if (cursorSnapshot == null || prev.cursor?.animate != true) return null
        val left =
            lerpFloat(
                cursorSnapshot.oldCursorRect.left,
                cursorSnapshot.newCursorRect.left,
                cursorProgress,
            )
        val top =
            lerpFloat(
                cursorSnapshot.oldCursorRect.top,
                cursorSnapshot.newCursorRect.top,
                cursorProgress,
            )
        val right =
            lerpFloat(
                cursorSnapshot.oldCursorRect.right,
                cursorSnapshot.newCursorRect.right,
                cursorProgress,
            )
        val bottom =
            lerpFloat(
                cursorSnapshot.oldCursorRect.bottom,
                cursorSnapshot.newCursorRect.bottom,
                cursorProgress,
            )
        return Rect(left, top, right, bottom)
    }

    /**
     * #641 评论 5459531909 第2项：把上一事务的 suppressedCurrentRanges 映射到本次 new text 坐标。
     *
     * 用 [replaceBounds] 的共同前缀/后缀映射 + 区间切分（prefix 保留、suffix 平移、
     * 跨越 replace 区域的部分不存活丢弃）。[replaceBounds] 为 null 时返回空列表。
     */
    fun mapSuppressedRangesThroughReplace(
        ranges: List<TextRange>,
        replaceBounds: VisualReplaceBounds?,
    ): List<TextRange> {
        if (replaceBounds == null) return emptyList()
        val delta = replaceBounds.newEnd - replaceBounds.oldEnd
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            if (range.start >= range.end) continue
            // prefix 部分：完全在共同前缀 [0, oldStart) 里 — 位置不变
            val prefixStart = range.start
            val prefixEnd = minOf(range.end, replaceBounds.oldStart)
            if (prefixStart < prefixEnd) {
                result.add(TextRange(prefixStart, prefixEnd))
            }
            // suffix 部分：完全在共同后缀 [oldEnd, ...) 里 — 平移
            val suffixStart = maxOf(range.start, replaceBounds.oldEnd)
            val suffixEnd = range.end
            if (suffixStart < suffixEnd) {
                result.add(
                    TextRange(
                        suffixStart + delta,
                        suffixEnd + delta,
                    ),
                )
            }
            // 跨越 replace 区域的部分不存活，丢弃
        }
        return result
    }

    /**
     * #641 评论 5459531909 第2项：从 [candidates] 中减去 [blockers] 覆盖的部分，
     * 避免双重隐藏。改成真正的区间 subtraction，不整段丢弃。
     */
    fun subtractRanges(
        candidates: List<TextRange>,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidates.isEmpty() || blockers.isEmpty()) return candidates
        return candidates.flatMap { candidate -> subtractCandidate(candidate, blockers) }
    }

    /**
     * 从单个 candidate 中减去 blockers 覆盖的部分 — 提取以降低 [subtractRanges] 认知复杂度。
     */
    private fun subtractCandidate(
        candidate: TextRange,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidate.start >= candidate.end) return emptyList()
        val relevantBlockers =
            blockers
                .filter { it.start < candidate.end && it.end > candidate.start }
                .sortedBy { it.start }
        if (relevantBlockers.isEmpty()) return listOf(candidate)
        val result = mutableListOf<TextRange>()
        var currentStart = candidate.start
        for (blocker in relevantBlockers) {
            if (blocker.start > currentStart) {
                result.add(TextRange(currentStart, minOf(blocker.start, candidate.end)))
            }
            currentStart = maxOf(currentStart, blocker.end)
            if (currentStart >= candidate.end) break
        }
        if (currentStart < candidate.end) {
            result.add(TextRange(currentStart, candidate.end))
        }
        return result
    }

    /** 线性插值 helper。 */
    fun lerpFloat(
        a: Float,
        b: Float,
        t: Float,
    ): Float = a + (b - a) * t.coerceIn(0f, 1f)

    /** 安全获取 path bounds — range 无效或越界时返回 null。 */
    fun safePathBounds(
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
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     * 任一 layout 缺失时不构建快照。
     *
     * #641 评论 问题2：old/new selection end 从 [CursorVisualIntent] 读取。
     */
    fun buildCursorSnapshot(
        previousSnapshot: ComposeLayoutSnapshot?,
        currentSnapshot: ComposeLayoutSnapshot?,
        intent: EditorVisualIntent?,
    ): VisualCursorSnapshot? {
        val prev = previousSnapshot ?: return null
        val curr = currentSnapshot ?: return null
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
     * #641 评论 问题3 + 评论 5457777142 问题2 + 评论 5458283021 问题2b：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * 共同前缀 offset 不变；共同后缀按 deltaUtf16 映射。按 old layout 视觉行切片，
     * 再把位移向量一致的连续 slice 合并；切点走 code-point 边界。
     *
     * [previousSnapshot] / [currentSnapshot] 由调用方传入，本函数不访问 mutable state。
     * 如果任一 layout 缺失，返回空列表。
     */
    fun computeRetainedMoves(
        intent: EditorVisualIntent,
        previousSnapshot: ComposeLayoutSnapshot?,
        currentSnapshot: ComposeLayoutSnapshot?,
    ): List<RetainedMove> {
        if (intent.textKind == TextVisualKind.None) return emptyList()
        val prev = previousSnapshot ?: return emptyList()
        val curr = currentSnapshot ?: return emptyList()

        val replaceBounds = intent.replaceBounds
        val oldSuffixStart =
            replaceBounds?.oldEnd ?: (intent.oldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart =
            replaceBounds?.newEnd ?: (intent.newRanges.maxOfOrNull { it.end } ?: 0)

        val oldText = prev.result.layoutInput.text
        val newText = curr.result.layoutInput.text
        val oldTextLen = oldText.length
        val newTextLen = newText.length

        if (oldSuffixStart >= oldTextLen || newSuffixStart >= newTextLen) return emptyList()

        val ctx =
            RetainedMovesContext(
                prev = prev,
                curr = curr,
                oldText = oldText,
                newText = newText,
                oldTextLen = oldTextLen,
                newTextLen = newTextLen,
                oldSuffixStart = oldSuffixStart,
                newSuffixStart = newSuffixStart,
            )
        return computeRetainedMovesLoop(ctx)
    }

    /**
     * Retained moves 计算上下文 — 封装循环中不变的参数，降低函数参数数量。
     */
    data class RetainedMovesContext(
        val prev: ComposeLayoutSnapshot,
        val curr: ComposeLayoutSnapshot,
        val oldText: AnnotatedString,
        val newText: AnnotatedString,
        val oldTextLen: Int,
        val newTextLen: Int,
        val oldSuffixStart: Int,
        val newSuffixStart: Int,
    )

    /**
     * 计算 retained moves 的主循环 — 提取以降低 [computeRetainedMoves] 长度。
     */
    fun computeRetainedMovesLoop(ctx: RetainedMovesContext): List<RetainedMove> {
        val result = mutableListOf<RetainedMove>()
        var oldPos = ctx.oldSuffixStart
        val mergeState = MergeState()

        while (oldPos < ctx.oldTextLen) {
            val moveResult = processRetainedMoveSegment(ctx, oldPos)
            val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
            oldPos = updateMoveResult(result, moveResult, mergeState, oldPos, newPos)
        }

        flushPendingMove(result, mergeState, oldPos, ctx)
        return result
    }

    /**
     * 合并状态 — 用于跟踪连续的 retained move 合并。
     */
    class MergeState {
        var mergedOldStart: Int = -1
        var mergedNewStart: Int = -1
        var mergedDx: Float = 0f
        var mergedDy: Float = 0f
        var merging: Boolean = false
    }

    /**
     * 处理单个 retained move 段 — 提取以降低 [computeRetainedMovesLoop] 复杂度。
     */
    data class RetainedMoveSegmentResult(
        val segEnd: Int,
        val oldBounds: Rect?,
        val newBounds: Rect?,
    )

    fun processRetainedMoveSegment(
        ctx: RetainedMovesContext,
        oldPos: Int,
    ): RetainedMoveSegmentResult {
        val oldLine = ctx.prev.result.getLineForOffset(oldPos)
        val oldLineEnd = ctx.prev.result.getLineEnd(oldLine)
        var segEnd = minOf(oldLineEnd, ctx.oldTextLen)
        if (segEnd in 1 until ctx.oldTextLen &&
            ctx.oldText[segEnd - 1].isHighSurrogate() &&
            ctx.oldText[segEnd].isLowSurrogate()
        ) {
            segEnd -= 1
        }
        if (segEnd <= oldPos) segEnd = oldPos + 1

        val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
        val newSegEnd = segEnd - ctx.oldSuffixStart + ctx.newSuffixStart

        return if (newSegEnd > ctx.newTextLen) {
            RetainedMoveSegmentResult(segEnd, null, null)
        } else {
            val oldRange = TextRange(oldPos, segEnd)
            val newRange = TextRange(newPos, newSegEnd)
            val oldBounds = safePathBounds(ctx.prev.result, oldRange)
            val newBounds = safePathBounds(ctx.curr.result, newRange)
            RetainedMoveSegmentResult(segEnd, oldBounds, newBounds)
        }
    }

    /**
     * 更新 move 结果并返回下一个 oldPos — 提取以降低 [computeRetainedMovesLoop] 复杂度。
     *
     * @param oldPos 当前段在 old text 中的起始位置，作为合并起点/终点边界。
     * @param newPos 当前段在 new text 中的起始位置，作为合并起点/终点边界。
     */
    fun updateMoveResult(
        result: MutableList<RetainedMove>,
        segmentResult: RetainedMoveSegmentResult,
        mergeState: MergeState,
        oldPos: Int,
        newPos: Int,
    ): Int {
        val segEnd = segmentResult.segEnd
        val oldBounds = segmentResult.oldBounds
        val newBounds = segmentResult.newBounds

        if (oldBounds != null && newBounds != null) {
            handleBoundsChanged(result, oldPos, oldBounds, newBounds, mergeState, newPos)
        } else {
            handleBoundsNull(result, oldPos, mergeState, newPos)
        }
        return segEnd
    }

    /** 处理 bounds 变化的情况 — 提取以降低 [updateMoveResult] 复杂度。 */
    private fun handleBoundsChanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        oldBounds: Rect,
        newBounds: Rect,
        mergeState: MergeState,
        newPos: Int,
    ) {
        val dx = newBounds.left - oldBounds.left
        val dy = newBounds.top - oldBounds.top
        val topChanged = kotlin.math.abs(dy) > 1f
        val leftChanged = kotlin.math.abs(dx) > 1f
        if (topChanged || leftChanged) {
            handlePositionChanged(result, oldPos, dx, dy, mergeState, newPos)
        } else {
            handlePositionUnchanged(result, oldPos, mergeState, newPos)
        }
    }

    /** 处理位置变化 — 提取以降低 [handleBoundsChanged] 复杂度。 */
    private fun handlePositionChanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        dx: Float,
        dy: Float,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging &&
            kotlin.math.abs(dx - mergeState.mergedDx) <= 1f &&
            kotlin.math.abs(dy - mergeState.mergedDy) <= 1f
        ) {
            // 位移向量一致，继续合并。
        } else if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            startNewMerge(oldPos, dx, dy, mergeState, newPos)
        } else {
            startNewMerge(oldPos, dx, dy, mergeState, newPos)
        }
    }

    /** 处理位置未变化 — 提取以降低 [handleBoundsChanged] 复杂度。 */
    private fun handlePositionUnchanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            mergeState.merging = false
        }
    }

    /** 处理 bounds 为 null 的情况 — 提取以降低 [updateMoveResult] 复杂度。 */
    private fun handleBoundsNull(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            mergeState.merging = false
        }
    }

    /** 完成当前合并 — 提取以降低 [updateMoveResult] 复杂度。 */
    private fun finishCurrentMerge(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        result.add(
            RetainedMove(
                oldRange = TextRange(mergeState.mergedOldStart, oldPos),
                newRange = TextRange(mergeState.mergedNewStart, newPos),
            ),
        )
    }

    /** 开始新合并 — 提取以降低 [updateMoveResult] 复杂度。 */
    private fun startNewMerge(
        oldPos: Int,
        dx: Float,
        dy: Float,
        mergeState: MergeState,
        newPos: Int,
    ) {
        mergeState.mergedOldStart = oldPos
        mergeState.mergedNewStart = newPos
        mergeState.mergedDx = dx
        mergeState.mergedDy = dy
        mergeState.merging = true
    }

    /**
     * 刷新 pending move — 提取以降低 [computeRetainedMovesLoop] 复杂度。
     */
    fun flushPendingMove(
        result: MutableList<RetainedMove>,
        mergeState: MergeState,
        oldPos: Int,
        ctx: RetainedMovesContext,
    ) {
        if (mergeState.merging) {
            val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
            if (newPos <= ctx.newTextLen) {
                result.add(
                    RetainedMove(
                        oldRange = TextRange(mergeState.mergedOldStart, oldPos),
                        newRange = TextRange(mergeState.mergedNewStart, newPos),
                    ),
                )
            }
        }
    }
}
