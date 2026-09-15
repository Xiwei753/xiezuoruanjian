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
     *
     * #684 评论 5663862982 Bug2：增加 [nextOffsetMap] — 多笔 intent 合成一个屏幕事务时，
     *   startFrame 的 targetRange 是 T0 坐标，必须用整条 chain 的 composedOffsetMap
     *   （T0->Tn）映射。[nextReplaceBounds] 仅作回退（单笔或无 offset map 时）。
     *   [currentSuppressedRanges] 表示"上一帧此刻已经被系统正文隐藏的 ranges"，
     *   不是新事务刚算出的 hiddenRanges — 两者概念不能混。
     *
     * #684 评论 5672654866：删除 cursorProgress / cursorSnapshot —
     *   startFrame 只物化正文 slice / retained move，不再携带光标。
     *   光标本来就是独立的当前几何状态，由 overlay 内长生命周期 Animatable 持有，
     *   不应该塞进文字 startFrame 再反推。
     */
    data class MaterializeStartFrameParams(
        val transaction: ComposeVisualTransaction?,
        val textProgress: Float,
        val rebaseProgress: Float,
        val nextOffsetMap: List<VisualOffsetMapEntry>?,
        val nextReplaceBounds: VisualReplaceBounds?,
        val currentSuppressedRanges: List<TextRange>,
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
     * [hiddenRanges] 由调用方从 visual state 读出后传入，
     * 本函数不直接访问任何 mutable state。
     *
     * 如果没有旧事务或 text/rebase 两条 progress 都已到 1f，返回 null。
     *
     * #684 评论 5672654866：不再物化光标 — materializeCursorRect 已删除，
     *   返回 frame 不再携带 cursorRect / cursorAlpha。光标由 overlay 内长生命周期
     *   Animatable 持有，不塞进文字 startFrame 再反推。
     */
    fun materializeStartFrame(params: MaterializeStartFrameParams): ComposeVisualFrame? {
        val transaction = params.transaction
        val textProgress = params.textProgress
        val rebaseProgress = params.rebaseProgress
        val nextOffsetMap = params.nextOffsetMap
        val nextReplaceBounds = params.nextReplaceBounds
        val currentSuppressedRanges = params.currentSuppressedRanges
        val prev = transaction ?: return null
        // #641 评论 5459896691 第1项：两条当前实际存在的 timeline 都结束才算没有视觉帧。
        // #684 评论 5672654866：cursor timeline 不再由 startFrame 物化，只看 text/rebase。
        if (textProgress >= 1f && rebaseProgress >= 1f) return null

        val prevStartFrame = prev.startFrame
        // #641 评论 5460160958 问题3：先按当前 rebaseProgress 物化旧 startFrame slice。
        // #641 评论 5460233781 问题2：materializeRebasedSlice 可能返回 null，用 mapNotNull 过滤。
        // 注意：materializedOlder（prev.startFrame 的 slice）与 prev 自己的 textAnimationActive 无关 —
        // prev.startFrame 是更早事务留下的帧，必须照常物化。
        val materializedOlder =
            prevStartFrame?.slices?.mapNotNull {
                materializeRebasedSlice(it, prev.newLayout, rebaseProgress)
            } ?: emptyList()

        // #684 评论 5666730754 问题1：currentSlices 和 retainedSlices 只在 prev.textAnimationActive==true
        // 时才物化。当 prev 是 SYSTEM_SUPPRESSED（textAnimationActive=false）时，prev 的正文从未被
        // overlay 动画过，屏幕已经在 prev 的最终正文，不应把 prev 当"半途动画"物化出来。
        // materializedOlder（prev.startFrame 的 slice）仍然照常物化 — 那是更早事务留下的帧。
        val currentSlices =
            if (prev.textAnimationActive) {
                collectCurrentSlicesAsRebased(prev, textProgress)
            } else {
                emptyList()
            }
        val retainedSlices =
            if (prev.textAnimationActive) {
                collectRetainedMoveSlicesAsRebased(prev, textProgress)
            } else {
                emptyList()
            }

        // #641 评论 5460160958 问题2+问题4：统一用 nextOffsetMap/nextReplaceBounds 映射 surviving targetRange。
        // #641 评论 5460373035 问题2：聚合所有 split 的 ownedOldRanges 计入返回 frame。
        //
        // #684 评论 5663862982 Bug2：优先用 nextOffsetMap（整条 chain 的 T0->Tn 映射）切 slice；
        //   回退到 nextReplaceBounds（单笔 replace bounds）；都没有则原样保留。
        val allSlices = materializedOlder + currentSlices + retainedSlices
        val mappedSlices = mutableListOf<RebasedTextSlice>()
        val ownedOldRanges = mutableListOf<TextRange>()
        when {
            // #684 评论 5664636035 Bug3：空 map 也必须走 splitRebasedSliceThroughOffsetMap（零存活映射，
            // 所有 slice 都 fading）。只有 nextOffsetMap == null 才回退到 nextReplaceBounds。
            nextOffsetMap != null -> {
                for (slice in allSlices) {
                    if (slice.targetRange == null) {
                        mappedSlices.add(slice)
                    } else {
                        val split = splitRebasedSliceThroughOffsetMap(slice, nextOffsetMap)
                        mappedSlices.addAll(split.slices)
                        ownedOldRanges.addAll(split.ownedOldRanges)
                    }
                }
            }
            nextReplaceBounds != null -> {
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
            else -> {
                mappedSlices.addAll(allSlices)
            }
        }

        return ComposeVisualFrame(
            slices = mappedSlices,
            suppressedCurrentRanges = currentSuppressedRanges,
            ownedOldRanges = ownedOldRanges,
        )
    }

    /**
     * #684 评论 5670182711 问题2：unit-wise 动画的局部 progress 纯函数。
     *
     * N 个 unit 共享一个 master progress，unit i 的局部 progress 按阶梯式分配：
     * `((master.coerceIn(0,1) * count) - index).coerceIn(0, 1)`。
     *
     * 这是视觉层纯计算 — [ComposeTextAnimationOverlay] 的 drawUnitWiseAppear/
     * drawUnitWiseDisappear 与本文件的 [collectCurrentSlicesAsRebased] 都调用它，
     * 保证 overlay 绘制与 startFrame 物化使用同一套 alpha 公式。
     */
    fun unitLocalProgress(master: Float, index: Int, count: Int): Float {
        if (count <= 0) return 0f
        return ((master.coerceIn(0f, 1f) * count) - index).coerceIn(0f, 1f)
    }

    /**
     * #641 评论 5459896691 第2项 + 评论 5460070064 第3项：
     * 按最后一个 intent 的 textKind 物化当前屏幕仍可见的 slice 为 [RebasedTextSlice]。
     *
     * #684 评论 5670182711 问题2：按事务真实 unit 逐个物化，与 overlay 的
     * drawUnitWiseAppear/drawUnitWiseDisappear 使用同一套 alpha 公式（[unitLocalProgress]）。
     * units 为空时回退到整段 oldRanges/newRanges 行为。new unit 即使当前 alpha=0 也不能丢掉，
     * 它仍属于被 BasicTextField 隐藏、后续 startFrame 需要继续从 0→1 的 surviving slice。
     */
    fun collectCurrentSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> {
        // #684 评论 5664636035 Bug1：用屏幕事务的 textKind（按最终净变化决定），
        // 不再从最后一笔 intent 的 textKind 读。
        val textKind = prev.textKind
        return when (textKind) {
            TextVisualKind.Delete -> collectDeleteSlicesAsRebased(prev, textProgress)
            TextVisualKind.Move ->
                collectDeleteSlicesAsRebased(prev, textProgress) +
                    collectInsertSlicesAsRebased(prev, textProgress)
            TextVisualKind.Insert -> collectInsertSlicesAsRebased(prev, textProgress)
            TextVisualKind.None -> emptyList()
        }
    }

    /**
     * #684 评论 5670182711 问题2：Insert/Move 的 new units 逐个物化为 surviving slice。
     * alpha = [unitLocalProgress]；targetRange = sourceRange（surviving）。
     * new unit 即使当前 alpha=0 也不丢掉 — 它仍属于被 BasicTextField 隐藏、后续
     * startFrame 需要继续从 0→1 的 surviving slice，否则下一事务期间会一直空着，
     * 到结束突然跳出来。units 为空时回退到整段 [survivingRebasedSlices] 行为。
     */
    private fun collectInsertSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> {
        val units = prev.newAnimationUnits
        if (units.isEmpty()) {
            return survivingRebasedSlices(prev.newRanges, prev.newLayout, textProgress)
        }
        val layout = prev.newLayout ?: return emptyList()
        val n = units.size
        val textLen = layout.result.layoutInput.text.length
        val slices = mutableListOf<RebasedTextSlice>()
        for ((i, range) in units.withIndex()) {
            if (range.start >= range.end || range.end > textLen) continue
            slices.add(
                RebasedTextSlice(
                    sourceLayout = layout,
                    sourceRange = range,
                    sourceTranslate = Offset.Zero,
                    sourceAlpha = unitLocalProgress(textProgress, i, n),
                    targetRange = range,
                ),
            )
        }
        return slices
    }

    /**
     * #684 评论 5670182711 问题2：Delete/Move 的 old units 逐个物化为 fading slice。
     * alpha = 1f - [unitLocalProgress]；targetRange = null（只属于旧画面，rebase 期间淡出）。
     * units 为空时回退到整段 [rebasedSlices] 行为。
     */
    private fun collectDeleteSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> {
        val units = prev.oldAnimationUnits
        if (units.isEmpty()) {
            return rebasedSlices(prev.oldRanges, prev.oldLayout, 1f - textProgress, targetRange = null)
        }
        val layout = prev.oldLayout ?: return emptyList()
        val n = units.size
        val textLen = layout.result.layoutInput.text.length
        val slices = mutableListOf<RebasedTextSlice>()
        for ((i, range) in units.withIndex()) {
            if (range.start >= range.end || range.end > textLen) continue
            slices.add(
                RebasedTextSlice(
                    sourceLayout = layout,
                    sourceRange = range,
                    sourceTranslate = Offset.Zero,
                    sourceAlpha = 1f - unitLocalProgress(textProgress, i, n),
                    targetRange = null,
                ),
            )
        }
        return slices
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
     * #684 评论 5664636035 Bug1：从 T0→Tn composed offset map 的补集算屏幕事务的 old/new changed ranges。
     *
     * 屏幕事务的 old/new changed ranges 不能用 chain.flatMap { it.oldRanges }，因为 chain 里的
     * 第 2、3 笔 range 属于 T1/T2 中间正文，不属于屏幕事务的 T0 oldLayout / Tn newLayout。
     * 正确做法：oldRanges = [0,oldLength) 中没有被 composed map old 区间覆盖的部分；
     * newRanges = [0,newLength) 中没有被 composed map new 区间覆盖的部分。
     *
     * @param map 整条 chain 合成后的 T0->Tn offset map（null 时返回空列表）。
     * @param oldLength T0 旧正文长度。
     * @param newLength Tn 新正文长度。
     * @return [FrameChangedRanges] — 屏幕坐标的 old/new changed ranges。
     */
    fun changedRangesFromComposedMap(
        map: List<VisualOffsetMapEntry>?,
        oldLength: Int,
        newLength: Int,
    ): FrameChangedRanges {
        if (map == null) return FrameChangedRanges(emptyList(), emptyList())
        val oldRanges = complementRanges(map.map { TextRange(it.oldStart, it.oldStart + it.length) }, oldLength)
        val newRanges = complementRanges(map.map { TextRange(it.newStart, it.newStart + it.length) }, newLength)
        return FrameChangedRanges(oldRanges, newRanges)
    }

    /**
     * 计算 [0, totalLength) 中没有被 [covered] 区间覆盖的部分 — 补集。
     */
    private fun complementRanges(
        covered: List<TextRange>,
        totalLength: Int,
    ): List<TextRange> {
        if (totalLength <= 0) return emptyList()
        val sorted = covered.filter { it.start < it.end }.sortedBy { it.start }
        val result = mutableListOf<TextRange>()
        var pos = 0
        for (range in sorted) {
            if (range.start > pos) {
                result.add(TextRange(pos, minOf(range.start, totalLength)))
            }
            pos = maxOf(pos, range.end)
            if (pos >= totalLength) break
        }
        if (pos < totalLength) {
            result.add(TextRange(pos, totalLength))
        }
        return result
    }

    /**
     * #684 评论 5664636035 Bug1：屏幕事务的 old/new changed ranges — 从 composed offset map 补集算出。
     */
    data class FrameChangedRanges(
        val oldRanges: List<TextRange>,
        val newRanges: List<TextRange>,
    )

    /**
     * #684 评论 5663862982 Bug2：按 composed offset map 切 surviving slice。
     *
     * 多笔 intent（T0->T1->...->Tn）合成一个屏幕事务时，slice.targetRange 是 T0 坐标，
     * [offsetMap] 是整条 chain 合成后的 T0->Tn 映射。对 targetRange 的每个部分：
     * - 与 offsetMap entry 的 old range 有交集 → 映射到 entry 的 new range（surviving）；
     * - 不在任何 entry 里 → fading slice（targetRange=null）+ ownedOldRange。
     *
     * 前提：surviving slice 表示同一逻辑文本，sourceRange 长度应等于 oldTarget 长度。
     * 若长度不等（不应发生），不静默复制整段——结束该 surviving 映射，按旧画面离场处理：
     * 返回 SplitRebasedResult(listOf(slice.copy(targetRange = null)), emptyList())。
     *
     * @param slice 待切分的 surviving slice（targetRange 非 null）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map 条目列表。
     */
    fun splitRebasedSliceThroughOffsetMap(
        slice: RebasedTextSlice,
        offsetMap: List<VisualOffsetMapEntry>,
    ): SplitRebasedResult {
        val oldTarget = slice.targetRange ?: return SplitRebasedResult(listOf(slice), emptyList())
        val sourceRange = slice.sourceRange
        // 前提：sourceRange 长度应等于 oldTarget 长度（同一逻辑文本）
        if ((sourceRange.end - sourceRange.start) != (oldTarget.end - oldTarget.start)) {
            return SplitRebasedResult(listOf(slice.copy(targetRange = null)), emptyList())
        }
        val outSlices = mutableListOf<RebasedTextSlice>()
        val ownedOldRanges = mutableListOf<TextRange>()
        val sortedEntries = offsetMap.sortedBy { it.oldStart }
        var pos = oldTarget.start
        for (entry in sortedEntries) {
            val entryOldEnd = entry.oldStart + entry.length
            if (entryOldEnd <= pos) continue
            if (entry.oldStart >= oldTarget.end) break
            // gap 部分 [pos, entry.oldStart) → fading（不在任何 entry 里，不存活）
            val gapEnd = minOf(entry.oldStart, oldTarget.end)
            if (pos < gapEnd) {
                val sourceOffset = pos - oldTarget.start
                val len = gapEnd - pos
                outSlices.add(
                    slice.copy(
                        sourceRange = TextRange(
                            sourceRange.start + sourceOffset,
                            sourceRange.start + sourceOffset + len,
                        ),
                        targetRange = null,
                    ),
                )
                ownedOldRanges.add(TextRange(pos, gapEnd))
            }
            // overlap 部分 → surviving，映射到 new range
            val overlapStart = maxOf(pos, entry.oldStart)
            val overlapEnd = minOf(entryOldEnd, oldTarget.end)
            if (overlapStart < overlapEnd) {
                val sourceOffset = overlapStart - oldTarget.start
                val len = overlapEnd - overlapStart
                val newStart = entry.newStart + (overlapStart - entry.oldStart)
                outSlices.add(
                    slice.copy(
                        sourceRange = TextRange(
                            sourceRange.start + sourceOffset,
                            sourceRange.start + sourceOffset + len,
                        ),
                        targetRange = TextRange(newStart, newStart + len),
                    ),
                )
            }
            pos = maxOf(pos, entryOldEnd)
        }
        // 尾部 gap [pos, oldTarget.end) → fading
        if (pos < oldTarget.end) {
            val sourceOffset = pos - oldTarget.start
            val len = oldTarget.end - pos
            outSlices.add(
                slice.copy(
                    sourceRange = TextRange(
                        sourceRange.start + sourceOffset,
                        sourceRange.start + sourceOffset + len,
                    ),
                    targetRange = null,
                ),
            )
            ownedOldRanges.add(TextRange(pos, oldTarget.end))
        }
        return SplitRebasedResult(outSlices, ownedOldRanges)
    }

    /**
     * #684 评论 5663862982 Bug2：把 suppressed ranges（T0 坐标）按 composed offset map
     * 映射到 Tn 坐标，只返回 surviving 的 new ranges。
     *
     * 多笔 intent 合成一个屏幕事务时，上一帧的 suppressedCurrentRanges 是 T0 坐标，
     * 必须用整条 chain 的 composedOffsetMap（T0->Tn）映射到当前 new text 坐标，
     * 而不是最后一笔 replaceBounds（T(n-1)->Tn 坐标）。
     *
     * 不在任何 entry 里的部分不存活（被编辑/删除），丢弃。
     *
     * @param ranges 上一帧的 suppressed ranges（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map，null 或空时返回空列表。
     */
    fun mapSuppressedRangesThroughOffsetMap(
        ranges: List<TextRange>,
        offsetMap: List<VisualOffsetMapEntry>?,
    ): List<TextRange> {
        if (offsetMap == null || offsetMap.isEmpty()) return emptyList()
        val sortedEntries = offsetMap.sortedBy { it.oldStart }
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            if (range.start >= range.end) continue
            for (entry in sortedEntries) {
                val entryOldEnd = entry.oldStart + entry.length
                val overlapStart = maxOf(range.start, entry.oldStart)
                val overlapEnd = minOf(range.end, entryOldEnd)
                if (overlapStart < overlapEnd) {
                    val newStart = entry.newStart + (overlapStart - entry.oldStart)
                    result.add(TextRange(newStart, newStart + (overlapEnd - overlapStart)))
                }
            }
        }
        return result
    }

    /**
     * #684 评论 5672654866：materializeCursorRect 已删除 —
     * startFrame 不再物化光标。光标由 overlay 内长生命周期 Animatable 持有。
     * 保留 lerpFloat 等纯函数供其他场景使用。
     */

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

    /**
     * #684 评论 5672654866：interpolateCursorRect 已删除 —
     * coordinator 不再用 _masterProgress 反算屏幕光标位置。
     * 当前 cursor rect 始终留在 overlay 的长生命周期 Animatable 里，
     * 新事务只改 target/path 不重置当前 rect。
     */

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
     *
     * #644 评论 #684：布局验证已移至 [ComposeVisualFrameCoordinator]，
     * 本方法只检查 offset 范围合法性。
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
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
        // old/new cursor offset 必须属于各自 layout 的合法 UTF-16 范围。
        if (oldSelectionEnd < 0 || oldSelectionEnd > oldText.length) return null
        if (newSelectionEnd < 0 || newSelectionEnd > newText.length) return null
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
     * #644 评论 #684：按 offset map chain 合并整条事务链的 retained moves。
     *
     * chain 中每一笔 [EditorVisualIntent] 的 [VisualOffsetMap] 顺序合成（[composeOffsetMapChain]），
     * 将最初屏幕 old UTF-16 range 映射到最终屏幕 new UTF-16 range。
     * 然后只比较 old/new [TextLayoutResult] 的真实几何，位置没变就不画，位置变化才生成 [RetainedMove]。
     *
     * 这样输入导致软换行、Enter 导致硬换行、删除换行导致两段合并、
     * 快速连续 Backspace 导致多次回流，全部走同一个 retained reflow，
     * 不再通过 `\n`、长度、previous/current 猜，也不再只用最后一笔的 replaceBounds
     * 去对应整帧最开始的 old layout（第二/三笔快速删除的坐标是中间文本）。
     *
     * 当 chain 中任一 intent 缺失 offset map 时，回退到旧式 suffix 线性平移算法
     * （取最后一笔 replaceBounds / 所有 ranges 摊平），保证无 offset map 的降级路径仍然可用。
     *
     * @param oldLayout 旧布局快照（最初屏幕 old 文本）。
     * @param newLayout 新布局快照（最终屏幕 new 文本）。
     * @param chain Core intent 链 — 按到达顺序排列。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot?,
        newLayout: ComposeLayoutSnapshot?,
        chain: List<EditorVisualIntent>,
    ): List<RetainedMove> {
        val prev = oldLayout ?: return emptyList()
        val curr = newLayout ?: return emptyList()
        if (chain.isEmpty()) return emptyList()

        // 当整条链都有 offset map 时，按合成后的 map 找存活 range，再比较真实几何。
        val composed = composeOffsetMapChain(chain)
        if (composed != null) {
            return computeRetainedMovesFromComposedMap(prev, curr, composed)
        }

        // 回退：旧式 suffix 线性平移（取最后一笔 replaceBounds / 所有 ranges 摊平）。
        return computeRetainedMovesLegacy(prev, curr, chain)
    }

    /**
     * #644 评论 #684 + 评论 5662132136 第1项 + 评论 5663032418 断点2：
     * 用合成后的 offset map 计算 retained moves。
     *
     * IDENTITY 与 SHIFTED 都表示"这段旧文字在新正文里仍然存在（内容相同）"，
     * 只是 IDENTITY 的 offset 没变、SHIFTED 的 offset 变了（被前后增删平移）。
     * 内容真正变化/被编辑/删除的区域 Core 根本不生成映射条目，所以不在 composed map 里。
     *
     * 因此两种 entry 都进入 oldRange -> newRange 的真实几何比较；
     * 只有 old/new [TextLayoutResult] 的真实 rect 真变了才生成 [RetainedMove]。
     * kind 只说明逻辑 offset 是否平移，不决定"画不画 move"。
     *
     * #684 评论 5663032418 断点2：不能再先合并相邻 entry 再整段算一个 dx/dy —
     * 软换行场景下同一段后缀里前半段仍在原行只横向移动、中间一段被挤到下一行、
     * 更后面的视觉行只纵向移动，不可能共用一个 bounding box 和一个位移向量。
     *
     * 新策略：对每个合成后的 mapped entry 再按 old/new 两边真实视觉行边界切片，
     * 每个 chunk 单独比较 old/new rect，只有 dx/dy 一致且 old/new 都连续的 chunk
     * 才合并成 RetainedMove。切点避开 surrogate pair / code point 中间。
     */
    private fun computeRetainedMovesFromComposedMap(
        prev: ComposeLayoutSnapshot,
        curr: ComposeLayoutSnapshot,
        composed: List<VisualOffsetMapEntry>,
    ): List<RetainedMove> {
        val moves = mutableListOf<RetainedMove>()
        for (entry in composed) {
            val oldStart = entry.oldStart
            val newStart = entry.newStart
            val length = entry.length
            if (length <= 0) continue
            if (oldStart + length > prev.result.layoutInput.text.length) continue
            if (newStart + length > curr.result.layoutInput.text.length) continue

            // 按两边视觉行边界切片 — 每个 chunk 单独算 dx/dy。
            val chunks = splitEntryByVisualLines(prev.result, curr.result, oldStart, newStart, length)
            // 只有 dx/dy 一致且 old/new 都连续的 chunk 才合并。
            mergeChunksIntoMoves(chunks, moves)
        }
        return moves
    }

    /**
     * #684 评论 5663032418 断点2：把一个合成 entry 按 old/new 两边真实视觉行边界切片。
     *
     * 切点是 old/new 两边各自视觉行结束 offset 的并集（映射回 entry 内部偏移），
     * 取两边更早的边界切成 chunk。切点避开 surrogate pair / code point 中间。
     *
     * 返回每个 chunk 的 (oldRange, newRange, oldBounds, newBounds)；
     * bounds 为 null 的 chunk 会被保留为 null（调用方据此跳过合并）。
     */
    private fun splitEntryByVisualLines(
        prevResult: TextLayoutResult,
        currResult: TextLayoutResult,
        oldStart: Int,
        newStart: Int,
        length: Int,
    ): List<RetainedMoveChunk> {
        val oldText = prevResult.layoutInput.text
        val newText = currResult.layoutInput.text
        // 收集所有切点（相对 entry 起始的偏移 0..length）。
        val cutOffsets = sortedSetOf(0, length)
        // old 侧视觉行边界
        var scan = 0
        while (scan < length) {
            val oldOffset = oldStart + scan
            if (oldOffset >= oldText.length) break
            val oldLine = prevResult.getLineForOffset(oldOffset)
            val oldLineEnd = prevResult.getLineEnd(oldLine)
            val nextCut = oldLineEnd - oldStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(oldText, oldStart, nextCut, length))
            }
            scan = oldLineEnd - oldStart
            if (scan <= 0) scan = 1 // 防御：避免死循环
        }
        // new 侧视觉行边界
        scan = 0
        while (scan < length) {
            val newOffset = newStart + scan
            if (newOffset >= newText.length) break
            val newLine = currResult.getLineForOffset(newOffset)
            val newLineEnd = currResult.getLineEnd(newLine)
            val nextCut = newLineEnd - newStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(newText, newStart, nextCut, length))
            }
            scan = newLineEnd - newStart
            if (scan <= 0) scan = 1
        }

        // 按切点生成 chunk。
        val chunks = mutableListOf<RetainedMoveChunk>()
        val sortedCuts = cutOffsets.toList()
        for (i in 0 until sortedCuts.size - 1) {
            val chunkStart = sortedCuts[i]
            val chunkEnd = sortedCuts[i + 1]
            if (chunkEnd <= chunkStart) continue
            val oldRange = TextRange(oldStart + chunkStart, oldStart + chunkEnd)
            val newRange = TextRange(newStart + chunkStart, newStart + chunkEnd)
            val oldBounds = safePathBounds(prevResult, oldRange)
            val newBounds = safePathBounds(currResult, newRange)
            if (oldBounds == null || newBounds == null) continue
            val dx = newBounds.left - oldBounds.left
            val dy = newBounds.top - oldBounds.top
            chunks.add(
                RetainedMoveChunk(
                    oldRange = oldRange,
                    newRange = newRange,
                    dx = dx,
                    dy = dy,
                ),
            )
        }
        return chunks
    }

    /**
     * 调整切点以避开 surrogate pair / code point 中间。
     * 如果 cutOffset 处正好切在一个 surrogate pair 中间，向前退一位。
     */
    private fun avoidSurrogateCut(
        text: AnnotatedString,
        base: Int,
        cutOffset: Int,
        maxOffset: Int,
    ): Int {
        if (cutOffset <= 0 || cutOffset >= maxOffset) return cutOffset.coerceIn(0, maxOffset)
        val absCut = base + cutOffset
        if (absCut in 1 until text.length &&
            text[absCut - 1].isHighSurrogate() &&
            text[absCut].isLowSurrogate()
        ) {
            return (cutOffset - 1).coerceIn(0, maxOffset)
        }
        return cutOffset
    }

    /**
     * #684 评论 5663032418 断点2：单个 chunk — 一段 old/new range + 算好的 dx/dy。
     */
    private data class RetainedMoveChunk(
        val oldRange: TextRange,
        val newRange: TextRange,
        val dx: Float,
        val dy: Float,
    )

    /**
     * #684 评论 5663032418 断点2：把 chunk 合并成 RetainedMove —
     * 只有 dx/dy 一致（容差 1f）且 old/new 都连续的 chunk 才合并。
     */
    private fun mergeChunksIntoMoves(
        chunks: List<RetainedMoveChunk>,
        out: MutableList<RetainedMove>,
    ) {
        if (chunks.isEmpty()) return
        var mergeStartIdx = 0
        for (i in 1..chunks.size) {
            val prevChunk = chunks[i - 1]
            val canContinue =
                i < chunks.size &&
                    kotlin.math.abs(chunks[i].dx - prevChunk.dx) <= 1f &&
                    kotlin.math.abs(chunks[i].dy - prevChunk.dy) <= 1f &&
                    chunks[i].oldRange.start == prevChunk.oldRange.end &&
                    chunks[i].newRange.start == prevChunk.newRange.end
            if (!canContinue) {
                // 把 [mergeStartIdx, i) 这段合并成一个 RetainedMove（如果位移真变了）。
                val first = chunks[mergeStartIdx]
                val last = chunks[i - 1]
                val dx = first.dx
                val dy = first.dy
                if (kotlin.math.abs(dx) > 1f || kotlin.math.abs(dy) > 1f) {
                    out.add(
                        RetainedMove(
                            oldRange = TextRange(first.oldRange.start, last.oldRange.end),
                            newRange = TextRange(first.newRange.start, last.newRange.end),
                        ),
                    )
                }
                mergeStartIdx = i
            }
        }
    }

    /**
     * #644 评论 #684：回退路径 — 取最后一个 replaceBounds / 所有 ranges 摊平做线性平移。
     */
    private fun computeRetainedMovesLegacy(
        prev: ComposeLayoutSnapshot,
        curr: ComposeLayoutSnapshot,
        chain: List<EditorVisualIntent>,
    ): List<RetainedMove> {
        val lastWithBounds = chain.lastOrNull { it.replaceBounds != null }
        val replaceBounds = lastWithBounds?.replaceBounds

        val effectiveOldRanges = chain.flatMap { it.oldRanges }.filter { it.start < it.end }
        val effectiveNewRanges = chain.flatMap { it.newRanges }.filter { it.start < it.end }

        val oldSuffixStart =
            replaceBounds?.oldEnd
                ?: (effectiveOldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart =
            replaceBounds?.newEnd
                ?: (effectiveNewRanges.maxOfOrNull { it.end } ?: 0)

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
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 intent 的 newAnimationUnits
     * 合成到最终 Tn 坐标。
     *
     * 第 i 笔的 newAnimationUnits 在 T(i+1) 坐标。顺着后续 intent 的 offsetMap
     * 一路映射到最终 Tn：每一步用该笔 offsetMap.entries 的 old→new 映射，unit range
     * 与 entry 的 old range 求交，交集映射到 new range；不在任何 entry 里的部分
     * 丢弃（被删除/改掉）。如果某笔没有 offsetMap（null），无法跨笔映射，保留原坐标。
     *
     * 保留 chain 的先后顺序和 Core 原来的 unit 边界；只去掉完全重复的 range，
     * 不把相邻 unit 再 merge 成一块。单笔 chain 时等价于直接返回该笔的 newAnimationUnits。
     */
    fun composeNewAnimationUnitsToFinal(
        chain: List<EditorVisualIntent>,
    ): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in chain.indices) {
            var units: List<TextRange> = chain[i].newAnimationUnits
            // 从第 i+1 笔开始，用每笔的 offsetMap 把 unit 从 T(i+1) 映射到 Tn
            mapForwardLoop@ for (j in (i + 1) until chain.size) {
                // #684 评论 5670182711 问题1：循环标签只能用 break/continue，不能用 return。
                // offsetMap == null 的 Core 契约是"纯 selection/cursor，无正文变化"，
                // 该阶段坐标不变，直接跳过该阶段继续映射后续正文事务。
                val entries = chain[j].offsetMap?.entries
                if (entries == null) continue@mapForwardLoop
                if (entries.isEmpty()) {
                    // 空 entries 表示整段删除/替换，无存活映射，所有 unit 丢失
                    units = emptyList()
                    break@mapForwardLoop
                }
                units = mapRangesForwardThroughOffsetMap(units, entries)
            }
            result.addAll(units)
        }
        return deduplicateRanges(result)
    }

    /**
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 intent 的 oldAnimationUnits
     * 合成回最初 T0 坐标。
     *
     * 第 i 笔的 oldAnimationUnits 在 Ti 坐标。把前面 intent 的 offsetMap 的
     * entry 反向使用（new→old），从 Ti 一路映射回 T0：每一步用该笔 offsetMap.entries
     * 的 new→old 逆映射，unit range 与 entry 的 new range 求交，交集映射到 old range；
     * 不在任何 entry 里的部分丢弃（在 T0 中没有前身）。如果某笔没有 offsetMap（null），
     * 无法跨笔映射，保留原坐标。
     *
     * 保留 chain 的先后顺序和 Core 原来的 unit 边界；只去掉完全重复的 range，
     * 不把相邻 unit 再 merge 成一块。单笔 chain 时等价于直接返回该笔的 oldAnimationUnits。
     */
    fun composeOldAnimationUnitsToBase(
        chain: List<EditorVisualIntent>,
    ): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in chain.indices) {
            var units: List<TextRange> = chain[i].oldAnimationUnits
            // 从第 i-1 笔开始反向，用每笔的 offsetMap 的逆映射把 unit 从 Ti 映射回 T0
            mapBackwardLoop@ for (j in (i - 1) downTo 0) {
                // #684 评论 5670182711 问题1：循环标签只能用 break/continue，不能用 return。
                // offsetMap == null 的 Core 契约是"纯 selection/cursor，无正文变化"，
                // 该阶段坐标不变，直接跳过该阶段继续映射前面的正文事务。
                val entries = chain[j].offsetMap?.entries
                if (entries == null) continue@mapBackwardLoop
                if (entries.isEmpty()) {
                    units = emptyList()
                    break@mapBackwardLoop
                }
                units = mapRangesBackwardThroughOffsetMap(units, entries)
            }
            result.addAll(units)
        }
        return deduplicateRanges(result)
    }

    /**
     * #684 评论 5673811415：把某笔 intent 的 cursor offset 沿后续 offset maps 映射到最终 Tn 坐标。
     *
     * 从 `intentIndex + 1` 开始，用每笔 intent 的 caret 边界语义把 offset 从 T(intentIndex+1)
     * 映射到 Tn。每一步调用 [mapCaretThroughIntent] — 优先用 `replaceBounds` 处理编辑边界，
     * 没有 `replaceBounds` 时回退到 `offsetMap`，对 entry 端点按 caret 边界处理。
     *
     * - 如果某笔没有 offsetMap（null）且没有 replaceBounds，跳过该笔（坐标不变）。
     * - 如果某笔的 offsetMap entries 为空，表示整段删除/替换，无法映射，返回 null。
     * - 如果 offset 在被替换掉的正文内部（oldStart < offset < oldEnd），返回 null（不猜）。
     * - 如果成功映射到最终 Tn，返回最终 offset。
     *
     * 与旧实现的区别：旧实现用半开区间 `[oldStart, oldEnd)` 查找字符 range 所属的 entry，
     * 但 caret 是边界点（合法范围 `0..textLength`），经常落在 changed range 的边界上。
     * 快速 Backspace 时中间 cursor point 落在 surviving prefix 的右边界（== oldEnd），
     * 半开区间找不到包含该 offset 的 entry，返回 null，中间点被丢掉。
     * 新实现用 caret 边界语义：`offset == oldEnd` 映射到 `newEnd`（删除/替换后的右边界），
     * `offset == oldStart` 映射到 `newStart`，纯插入时 `offset == oldStart == oldEnd` 映射到 `newStart`。
     */
    fun mapCursorOffsetThroughChain(
        chain: List<EditorVisualIntent>,
        intentIndex: Int,
        offset: Int,
    ): Int? {
        var currentOffset = offset
        for (j in (intentIndex + 1) until chain.size) {
            val intent = chain[j]
            val mapped = mapCaretThroughIntent(intent, currentOffset)
            if (mapped == null) {
                return null
            }
            currentOffset = mapped
        }
        return currentOffset
    }

    /**
     * #684 评论 5673811415：用 caret 边界语义把 offset 穿过单笔 intent 映射。
     *
     * 优先用 [EditorVisualIntent.replaceBounds] 处理编辑边界；没有 `replaceBounds` 时
     * 回退到 [VisualOffsetMap] entries，对 entry 端点也按 caret 边界处理。
     *
     * caret 边界语义（与字符 range 的半开区间语义不同）：
     * - `offset < oldStart` → 前缀，位置不变
     * - `offset == oldStart` → newStart
     * - `oldStart < offset < oldEnd` → null（点落在被替换掉的正文内部，不猜）
     * - `offset == oldEnd`:
     *    - `oldStart < oldEnd` → newEnd（删除/替换后的右边界）
     *    - `oldStart == oldEnd` → newStart（纯插入：历史 cursor 应留在新文字左边）
     * - `offset > oldEnd` → suffix 平移
     */
    private fun mapCaretThroughIntent(
        intent: EditorVisualIntent,
        offset: Int,
    ): Int? {
        // 优先用 replaceBounds 处理编辑边界
        val replaceBounds = intent.replaceBounds
        if (replaceBounds != null) {
            return mapCaretThroughReplaceBounds(replaceBounds, offset)
        }

        // 回退到 offsetMap，对 entry 端点按 caret 边界处理
        val entries = intent.offsetMap?.entries
        // offsetMap == null 的 Core 契约是"纯 selection/cursor，无正文变化"，
        // 该阶段坐标不变，直接跳过继续映射后续正文事务。
        if (entries == null) return offset
        if (entries.isEmpty()) {
            // 空 entries 表示整段删除/替换，无存活映射，该 offset 无法映射到 Tn
            return null
        }
        return mapCaretThroughOffsetMapEntries(entries, offset)
    }

    /**
     * #684 评论 5673811415：用 [VisualReplaceBounds] 的 caret 边界语义映射 offset。
     */
    private fun mapCaretThroughReplaceBounds(
        rb: VisualReplaceBounds,
        offset: Int,
    ): Int? {
        return when {
            offset < rb.oldStart -> offset
            offset == rb.oldStart -> rb.newStart
            offset < rb.oldEnd -> null
            offset == rb.oldEnd -> {
                if (rb.oldStart < rb.oldEnd) rb.newEnd else rb.newStart
            }
            else -> offset + (rb.newEnd - rb.oldEnd)
        }
    }

    /**
     * #684 评论 5673811415：用 [VisualOffsetMapEntry] 列表的 caret 边界语义映射 offset。
     *
     * 对每个 entry 的端点按 caret 边界处理（闭区间 `[oldStart, oldEnd]`）：
     * - `offset == entry.oldStart` → `entry.newStart`
     * - `oldStart < offset < oldEnd` → `entry.newStart + (offset - oldStart)`（存活正文内部）
     * - `offset == entry.oldEnd` → `entry.newStart + entry.length`（右边界）
     *
     * 不在任何 entry 的闭区间里的 offset：
     * - 在所有 entry 之前 → 前缀，位置不变
     * - 在所有 entry 之后 → suffix 平移（按最后一个 entry 的 newEnd-oldEnd delta）
     * - 在两个 entry 之间的 gap 里 → null（被删除/编辑的区域，不猜）
     */
    private fun mapCaretThroughOffsetMapEntries(
        entries: List<VisualOffsetMapEntry>,
        offset: Int,
    ): Int? {
        val sorted = entries.sortedBy { it.oldStart }

        // 遍历 entries，找包含 offset 的 entry（闭区间 [oldStart, oldEnd]）
        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (offset in entry.oldStart..oldEnd) {
                return when {
                    offset == entry.oldStart -> entry.newStart
                    offset == oldEnd -> entry.newStart + entry.length
                    else -> entry.newStart + (offset - entry.oldStart)
                }
            }
        }

        // offset 不在任何 entry 的闭区间里
        val firstOldStart = sorted.first().oldStart
        if (offset < firstOldStart) {
            // 前缀，位置不变
            return offset
        }

        val lastEntry = sorted.last()
        val lastOldEnd = lastEntry.oldStart + lastEntry.length
        if (offset > lastOldEnd) {
            // suffix 平移
            val delta = (lastEntry.newStart + lastEntry.length) - lastOldEnd
            return offset + delta
        }

        // offset 在两个 entry 之间的 gap 里（被删除/编辑的区域），不猜
        return null
    }

    /**
     * 把 ranges 沿 offsetMap entries 的 old→new 方向映射。
     * 每个 range 与每个 entry 的 old range [oldStart, oldStart+length) 求交，
     * 交集映射到 new range。不在任何 entry 里的部分丢弃。
     */
    private fun mapRangesForwardThroughOffsetMap(
        ranges: List<TextRange>,
        entries: List<VisualOffsetMapEntry>,
    ): List<TextRange> {
        val result = mutableListOf<TextRange>()
        for (unit in ranges) {
            if (unit.start >= unit.end) continue
            for (entry in entries) {
                val oldStart = entry.oldStart
                val oldEnd = entry.oldStart + entry.length
                val overlapStart = maxOf(unit.start, oldStart)
                val overlapEnd = minOf(unit.end, oldEnd)
                if (overlapStart >= overlapEnd) continue
                val newStart = entry.newStart + (overlapStart - oldStart)
                val newEnd = entry.newStart + (overlapEnd - oldStart)
                result.add(TextRange(newStart, newEnd))
            }
        }
        return result
    }

    /**
     * 把 ranges 沿 offsetMap entries 的 new→old 方向（逆映射）映射。
     * 每个 range 与每个 entry 的 new range [newStart, newStart+length) 求交，
     * 交集映射到 old range。不在任何 entry 里的部分丢弃。
     */
    private fun mapRangesBackwardThroughOffsetMap(
        ranges: List<TextRange>,
        entries: List<VisualOffsetMapEntry>,
    ): List<TextRange> {
        val result = mutableListOf<TextRange>()
        for (unit in ranges) {
            if (unit.start >= unit.end) continue
            for (entry in entries) {
                val newStart = entry.newStart
                val newEnd = entry.newStart + entry.length
                val overlapStart = maxOf(unit.start, newStart)
                val overlapEnd = minOf(unit.end, newEnd)
                if (overlapStart >= overlapEnd) continue
                val oldStart = entry.oldStart + (overlapStart - newStart)
                val oldEnd = entry.oldStart + (overlapEnd - newStart)
                result.add(TextRange(oldStart, oldEnd))
            }
        }
        return result
    }

    /**
     * 去掉完全重复的 range（start 和 end 都相同），不合并相邻 range。
     * 保留首次出现的顺序。
     */
    private fun deduplicateRanges(ranges: List<TextRange>): List<TextRange> {
        val seen = LinkedHashSet<Pair<Int, Int>>()
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            val key = range.start to range.end
            if (seen.add(key)) {
                result.add(range)
            }
        }
        return result
    }

    /**
     * #644 评论 #684：合成整条 offset map chain —
     * 把每笔 intent 的 [VisualOffsetMap] 顺序合成，得到最初屏幕 old UTF-16 range
     * → 最终屏幕 new UTF-16 range 的 map。
     *
     * 合成方式：把累积 map 维护成「初始 old 文本坐标 → 当前 frontier 文本坐标」的线段表；
     * 对每一阶段（intent[i] 的 old→new map），把当前 frontier 与这一阶段 map 求交，
     * 交集映射回初始 old 坐标并映射到下一阶段 new 坐标。逐笔做完后，acc 的坐标已经是最初 old → 最终 new。
     *
     * 仅当 chain 中每一笔都带非空 offset map 才返回非 null；否则返回 null，
     * 调用方回退到旧式 suffix 算法。
     */
    fun composeOffsetMapChain(
        chain: List<EditorVisualIntent>,
    ): List<VisualOffsetMapEntry>? {
        if (chain.isEmpty()) return null
        // #684 评论 5664636035 Bug3：空 offsetMap.entries 是合法"零存活映射"（整段删除/整段替换），
        // 不能当成没有 map。只有 offsetMap == null 才表示该笔没有 offset map。
        if (chain.any { it.offsetMap == null }) return null

        // acc：初始 old 文本坐标 → 当前 frontier 文本坐标。
        val initialOldLen = chain.first().expectedOldText.length
        var acc: List<AccSegment> =
            listOf(
                AccSegment(
                    oldStart = 0,
                    newStart = 0,
                    length = initialOldLen,
                    kind = VisualOffsetMapKind.IDENTITY,
                ),
            )

        for (intent in chain) {
            val entries = intent.offsetMap?.entries ?: return null
            val stage = buildStageSegments(entries)
            acc = composeStage(acc, stage)
        }

        return acc.map {
            VisualOffsetMapEntry(
                oldStart = it.oldStart,
                newStart = it.newStart,
                length = it.length,
                kind = it.kind,
            )
        }
    }

    /**
     * #644 评论 #684 + 评论 5662132136 第1项：把单阶段 offset map entries 铺成线段表，
     * **只由 Core 给出的 entries 构成**。
     *
     * 绝对不要补 identity gap：Core 故意不给中间编辑区生成映射，"无 entry" 就是
     * "这段旧文字没有对应的新文字"（被编辑/删除/替换）。把这些区间补成 IDENTITY 会把
     * 已删除文字当成存活文字参与 chain 合成，正好打坏快速删除和换行回流。
     *
     * entry 之间的空洞直接没有 segment；[composeStage] 只对真实映射段求交。
     */
    private fun buildStageSegments(
        entries: List<VisualOffsetMapEntry>,
    ): List<StageSegment> {
        return entries
            .sortedBy { it.oldStart }
            .map { e ->
                StageSegment(
                    oldStart = e.oldStart,
                    newStart = e.newStart,
                    length = e.length,
                    kind = e.kind,
                )
            }
    }

    /**
     * #644 评论 #684：把累积 acc（initial old → frontier）与单阶段 stage（frontier → next frontier）
     * 求交合成，返回新的 acc（initial old → next frontier）。
     */
    private fun composeStage(
        acc: List<AccSegment>,
        stage: List<StageSegment>,
    ): List<AccSegment> {
        val result = mutableListOf<AccSegment>()
        for (a in acc) {
            val aNewStart = a.newStart
            val aEnd = a.newStart + a.length
            for (s in stage) {
                val overlapStart = maxOf(aNewStart, s.oldStart)
                val overlapEnd = minOf(aEnd, s.oldStart + s.length)
                if (overlapStart >= overlapEnd) continue
                val offsetInAcc = overlapStart - aNewStart
                val oldStartInitial = a.oldStart + offsetInAcc
                val newStartFrontier = s.newStart + (overlapStart - s.oldStart)
                val kind =
                    if (a.kind == VisualOffsetMapKind.SHIFTED ||
                        s.kind == VisualOffsetMapKind.SHIFTED
                    ) {
                        VisualOffsetMapKind.SHIFTED
                    } else {
                        VisualOffsetMapKind.IDENTITY
                    }
                result.add(
                    AccSegment(
                        oldStart = oldStartInitial,
                        newStart = newStartFrontier,
                        length = overlapEnd - overlapStart,
                        kind = kind,
                    ),
                )
            }
        }
        return result
    }

    /** #644 评论 #684：累积线段 — initial old 坐标 → 当前 frontier 坐标。 */
    private data class AccSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )

    /** #644 评论 #684：单阶段线段 — frontier_old 坐标 → frontier_new 坐标。 */
    private data class StageSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )

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
