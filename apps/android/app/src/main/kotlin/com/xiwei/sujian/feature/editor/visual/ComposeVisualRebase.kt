package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
import com.xiwei.sujian.feature.editor.layout.cursorRect
import com.xiwei.sujian.feature.editor.layout.rawLineEndForRawOffset

/**
 * #644 评论 5467821839 第5节剩余子项：visual rebase 纯计算 —
 * 从 [ComposeEditorVisualState] 抽出的无副作用几何/区间函数。
 *
 * #689 评论 5674631257 步骤6：删除整套旧事务物化代码。
 * 保留纯计算：
 * - [composeOffsetMapChain]
 * - [changedRangesFromComposedMap]
 * - [computeRetainedMoves]
 * - [mapCursorOffsetThroughChain]
 * - range 映射/切分工具
 *
 * 删除：
 * - MaterializeStartFrameParams / materializeStartFrame
 * - collectCurrentSlicesAsRebased / collectInsertSlicesAsRebased / collectDeleteSlicesAsRebased
 * - collectRetainedMoveSlicesAsRebased
 * - 所有只服务 ComposeVisualFrame/RebasedTextSlice 的 split/materialize 方法
 * - unitLocalProgress（旧 overlay 已删除，无调用）
 *
 * 新增 [mapRangeForwardThroughOffsetMapPublic] — 把仍存活的 targetRange 通过 composed offset map
 * 映射到下一份正文；timeline 用它把旧 unit 接到新 layout。
 */
@Suppress("LargeClass", "TooManyFunctions")
internal object ComposeVisualRebase {
    /**
     * #689 评论 5674631257 步骤6：把仍存活的 targetRange 通过 composed offset map 映射到下一份正文。
     *
     * timeline 用它把旧 unit 接到新 layout。映射失败（返回 null）就说明这段文字已被本次编辑
     * 覆盖/删除，转成 ghost fade-out，不要猜坐标。
     *
     * @param range 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map。
     * @param newTextLength 新正文长度 — 映射结果越界时返回 null。
     * @return 映射后的新正文 UTF-16 range；不在任何 entry 里（被编辑/删除）返回 null。
     */
    fun mapRangeForwardThroughOffsetMapPublic(
        range: TextRange,
        offsetMap: List<VisualOffsetMapEntry>,
        newTextLength: Int,
    ): TextRange? {
        if (range.start >= range.end) return null
        val sorted = offsetMap.sortedBy { it.oldStart }
        // 找完全包含 range 的 entry（存活正文内部）
        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (range.start >= entry.oldStart && range.end <= oldEnd) {
                val newStart = entry.newStart + (range.start - entry.oldStart)
                val newEnd = entry.newStart + (range.end - entry.oldStart)
                if (newEnd <= newTextLength) {
                    return TextRange(newStart, newEnd)
                }
                return null
            }
        }
        // 不在任何 entry 里 → 被编辑/删除，转 ghost
        return null
    }

    /**
     * #689 评论 5675270164 缺陷5：把旧 unit 的 targetRange 按 offset-map entry 切成多个片段。
     *
     * 旧实现 [mapRangeForwardThroughOffsetMapPublic] 对整个 range 调一次映射，返回不了完整
     * 新 range 就整块判死。但一个 unit 跨过删除洞时（例如 [0,4)="ab\nc" 删除中间换行 [2,3)），
     * "ab" 和 "c" 实际都还活着，整块转 ghost 会把没删除的上一行一起重新接管/重画，制造抽动。
     *
     * 本函数按 offset-map entry 的边界把 [range] 切成子区间：
     * - 仍存活的 slice（被某 entry 完全包含）→ [MappedRangeSlice.newSubRange] 映到新正文
     * - 被删除/覆盖的 slice（不在任何 entry 里）→ [MappedRangeSlice.newSubRange] = null，转 ghost
     *
     * @param range 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map。
     * @return 切片列表；空 offsetMap 时整个 range 作为单个 GHOST slice 返回。
     */
    fun splitMappedRangeForward(
        range: TextRange,
        offsetMap: List<VisualOffsetMapEntry>,
    ): List<MappedRangeSlice> {
        if (range.start >= range.end) return emptyList()
        if (offsetMap.isEmpty()) {
            return listOf(MappedRangeSlice(range, null, MappedRangeSliceKind.GHOST))
        }
        val sorted = offsetMap.sortedBy { it.oldStart }
        val sortedCuts = collectSliceCutPoints(range, sorted)
        return buildSlicesFromCuts(sortedCuts, sorted)
    }

    private fun collectSliceCutPoints(
        range: TextRange,
        sorted: List<VisualOffsetMapEntry>,
    ): List<Int> {
        val cutPoints = sortedSetOf(range.start, range.end)
        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (entry.oldStart > range.start && entry.oldStart < range.end) {
                cutPoints.add(entry.oldStart)
            }
            if (oldEnd > range.start && oldEnd < range.end) {
                cutPoints.add(oldEnd)
            }
        }
        return cutPoints.toList()
    }

    private fun buildSlicesFromCuts(
        sortedCuts: List<Int>,
        sorted: List<VisualOffsetMapEntry>,
    ): List<MappedRangeSlice> {
        val slices = mutableListOf<MappedRangeSlice>()
        for (i in 0 until sortedCuts.size - 1) {
            val subStart = sortedCuts[i]
            val subEnd = sortedCuts[i + 1]
            if (subEnd <= subStart) continue
            val survivingEntry =
                sorted.firstOrNull { entry ->
                    val oldEnd = entry.oldStart + entry.length
                    subStart >= entry.oldStart && subEnd <= oldEnd
                }
            slices.add(buildSlice(subStart, subEnd, survivingEntry))
        }
        return slices
    }

    private fun buildSlice(
        subStart: Int,
        subEnd: Int,
        survivingEntry: VisualOffsetMapEntry?,
    ): MappedRangeSlice {
        if (survivingEntry != null) {
            val newStart = survivingEntry.newStart + (subStart - survivingEntry.oldStart)
            val newEnd = survivingEntry.newStart + (subEnd - survivingEntry.oldStart)
            return MappedRangeSlice(
                oldSubRange = TextRange(subStart, subEnd),
                newSubRange = TextRange(newStart, newEnd),
                kind = MappedRangeSliceKind.SURVIVING,
            )
        }
        return MappedRangeSlice(
            oldSubRange = TextRange(subStart, subEnd),
            newSubRange = null,
            kind = MappedRangeSliceKind.GHOST,
        )
    }

    /**
     * #708 评论 5725706551：把旧 unit 的 targetRange 切成 SURVIVING/GHOST slice —
     * [ComposeVisualTimeline.mapSurvivingUnits] 和 [ComposeLocalHandoffRebase.rebase] 共用的切片逻辑。
     *
     * 优先用 [offsetMap]；没有则从 [intent] 用 [entriesForIntent] 生成 fallback survival map；
     * 都没有则检查 target 是否仍在新正文范围内（[target.end] <= [newTextLength]）：
     * - 在范围内 → 整段 SURVIVING（range 不变）；
     * - 超出范围 → 整段 GHOST。
     *
     * @param target 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0→Tn offset map；null 表示没有。
     * @param newTextLength 新正文长度 — fallback 判断 target 是否仍存活。
     * @param editFact 编辑事实 — offsetMap==null 时用 replaceBounds 生成 fallback。
     * @return 切片列表。
     */
    internal fun computeSlices(
        target: TextRange,
        offsetMap: List<VisualOffsetMapEntry>?,
        newTextLength: Int,
        editFact: EditorEditFact? = null,
    ): List<MappedRangeSlice> {
        val effectiveMap =
            offsetMap ?: editFact?.let { entriesForFact(it) }
        if (effectiveMap != null) {
            return splitMappedRangeForward(target, effectiveMap)
        }
        // 无 offset map 且无 intent 信息：若 target 仍在新正文范围内，保留；否则转 ghost
        return if (target.end <= newTextLength) {
            listOf(
                MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = target,
                    kind = MappedRangeSliceKind.SURVIVING,
                ),
            )
        } else {
            listOf(
                MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = null,
                    kind = MappedRangeSliceKind.GHOST,
                ),
            )
        }
    }

    /**
     * #689 评论 5675270164 缺陷5：offset-map 切片结果。
     *
     * @param oldSubRange 旧正文中的子区间。
     * @param newSubRange 新正文中的映射区间；null 表示被删除/覆盖，应转 ghost。
     * @param kind SURVIVING=存活，GHOST=被删除/覆盖。
     */
    data class MappedRangeSlice(
        val oldSubRange: TextRange,
        val newSubRange: TextRange?,
        val kind: MappedRangeSliceKind,
    )

    /**
     * #689 评论 5675270164 缺陷5：切片类型。
     */
    enum class MappedRangeSliceKind {
        /** 仍存活，映到新正文。 */
        SURVIVING,

        /** 被删除/覆盖，转 ghost。 */
        GHOST,
    }

    /**
     * #684 评论 5664636035 Bug1：从 T0→Tn composed offset map 的补集算屏幕事务的 old/new changed ranges。
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
     * #684 评论 5663862982 Bug2：把 suppressed ranges（T0 坐标）按 composed offset map
     * 映射到 Tn 坐标，只返回 surviving 的 new ranges。
     *
     * #689：保留供 [ComposeVisualFrameCoordinator] 在构建 patch 时使用（虽然新 timeline
     * 不再继承 suppressed ranges，但 offset map 映射工具仍有用）。
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
     * #641 评论 5459531909 第2项：从 [candidates] 中减去 [blockers] 覆盖的部分。
     */
    fun subtractRanges(
        candidates: List<TextRange>,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidates.isEmpty() || blockers.isEmpty()) return candidates
        return candidates.flatMap { candidate -> subtractCandidate(candidate, blockers) }
    }

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
     * timeline（[ComposeVisualTimeline]）和 handoff（[ComposeLocalHandoffRebase]）都用此 helper
     * 判定是否释放 surviving unit。
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
     * timeline 的 [ComposeVisualTimeline.toGhost] 和 handoff 的
     * [ComposeLocalHandoffRebase.toHandoffGhost] 共用此 helper，
     * 避免两套算法不一致导致 handoff 首帧旧字跳位。
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

    /**
     * #641 评论 问题3 + 评论 5457777142 问题2 + 评论 5458283021 问题2b：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * Issue #735 评论 5771063665：不再接收 [EditorVisualIntent]，改为接收纯数据。
     */
    fun computeRetainedMoves(
        textKind: TextVisualKind,
        replaceBounds: VisualReplaceBounds?,
        oldRanges: List<TextRange>,
        newRanges: List<TextRange>,
        previousSnapshot: ComposeLayoutSnapshot?,
        currentSnapshot: ComposeLayoutSnapshot?,
    ): List<RetainedMove> {
        if (textKind == TextVisualKind.None) return emptyList()
        val prev = previousSnapshot ?: return emptyList()
        val curr = currentSnapshot ?: return emptyList()

        val oldSuffixStart =
            replaceBounds?.oldEnd ?: (oldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart =
            replaceBounds?.newEnd ?: (newRanges.maxOfOrNull { it.end } ?: 0)

        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        // 类型从 AnnotatedString 变 String，String 也是 CharSequence。
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
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
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot?,
        newLayout: ComposeLayoutSnapshot?,
        chain: List<EditorEditFact>,
    ): List<RetainedMove> {
        val prev = oldLayout ?: return emptyList()
        val curr = newLayout ?: return emptyList()
        if (chain.isEmpty()) return emptyList()

        val composed = composeOffsetMapChain(chain)
        if (composed != null) {
            return computeRetainedMovesFromComposedMap(prev, curr, composed)
        }

        return computeRetainedMovesLegacy(prev, curr, chain)
    }

    /**
     * #644 评论 #684 + 评论 5662132136 第1项 + 评论 5663032418 断点2：
     * 用合成后的 offset map 计算 retained moves。
     *
     * #694 评论第 4 步：internal 可见性 — 供 [ComposeLocalVisualRebase] 复用，
     * 本地输入不再绕 [EditorVisualIntent] 才能调用。
     */
    internal fun computeRetainedMovesFromComposedMap(
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
            // Issue #717 评论 5742904417 修复1：边界检查用 rawText 长度。
            if (oldStart + length > prev.result.layoutInput.text.text.length) continue
            if (newStart + length > curr.result.layoutInput.text.text.length) continue

            val chunks = splitEntryByVisualLines(prev, curr, oldStart, newStart, length)
            mergeChunksIntoMoves(chunks, moves)
        }
        return moves
    }

    /**
     * #684 评论 5663032418 断点2：把一个合成 entry 按 old/new 两边真实视觉行边界切片。
     *
     * Issue #717 评论 5742273757 修复3：改为接收 [ComposeLayoutSnapshot]，
     * 内部通过 projection 做 raw→display 映射再调 TextLayoutResult。
     */
    private fun splitEntryByVisualLines(
        prevSnapshot: ComposeLayoutSnapshot,
        currSnapshot: ComposeLayoutSnapshot,
        oldStart: Int,
        newStart: Int,
        length: Int,
    ): List<RetainedMoveChunk> {
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        // avoidSurrogateCut 接收 CharSequence，String 兼容。
        val oldText = prevSnapshot.result.layoutInput.text.text
        val newText = currSnapshot.result.layoutInput.text.text
        val cutOffsets = sortedSetOf(0, length)
        var scan = 0
        while (scan < length) {
            val oldOffset = oldStart + scan
            if (oldOffset >= oldText.length) break
            // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
            // getLineEnd 返回 display offset（含 U+200B），retained move 切片需要 raw offset。
            // rawLineEndForRawOffset 内部会调 lineForRawOffset 查行。
            val oldLineEnd = prevSnapshot.rawLineEndForRawOffset(oldOffset)
            val nextCut = oldLineEnd - oldStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(oldText, oldStart, nextCut, length))
            }
            scan = oldLineEnd - oldStart
            if (scan <= 0) scan = 1
        }
        scan = 0
        while (scan < length) {
            val newOffset = newStart + scan
            if (newOffset >= newText.length) break
            // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
            val newLineEnd = currSnapshot.rawLineEndForRawOffset(newOffset)
            val nextCut = newLineEnd - newStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(newText, newStart, nextCut, length))
            }
            scan = newLineEnd - newStart
            if (scan <= 0) scan = 1
        }

        val chunks = mutableListOf<RetainedMoveChunk>()
        val sortedCuts = cutOffsets.toList()
        for (i in 0 until sortedCuts.size - 1) {
            val chunkStart = sortedCuts[i]
            val chunkEnd = sortedCuts[i + 1]
            if (chunkEnd <= chunkStart) continue
            val oldRange = TextRange(oldStart + chunkStart, oldStart + chunkEnd)
            val newRange = TextRange(newStart + chunkStart, newStart + chunkEnd)
            val oldBounds = safePathBounds(prevSnapshot, oldRange)
            val newBounds = safePathBounds(currSnapshot, newRange)
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
     * Issue #717 评论 5742904417 修复1：签名改成接收 [CharSequence]，
     * 这样 String（rawText）和 AnnotatedString 都能传入。
     */
    private fun avoidSurrogateCut(
        text: CharSequence,
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

    private data class RetainedMoveChunk(
        val oldRange: TextRange,
        val newRange: TextRange,
        val dx: Float,
        val dy: Float,
    )

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
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    private fun computeRetainedMovesLegacy(
        prev: ComposeLayoutSnapshot,
        curr: ComposeLayoutSnapshot,
        chain: List<EditorEditFact>,
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

        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
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
     * #684 评论 5669048233 Bug2 修复 + #694 评论 5691696678 问题3：通用 stage-map 版本 —
     * 把每个 stage 的 newUnits 沿后续 stage offset map 映射到最终 Tn 坐标。
     *
     * 不依赖 [EditorVisualIntent]，接收纯 [List]<[TextRange]> 和 [List]<[VisualOffsetMapEntry]?>，
     * 供 [ComposeVisualPatchBatch.compose] 合成本地输入 patch 的 insertedUnits（保留吐字顺序）。
     *
     * 算法和 [composeNewAnimationUnitsToFinal] 一致：每笔的 units 沿后续 stage offset map
     * 映射到最终 Tn（用 [mapRangesForwardThroughOffsetMap]），null offset map 跳过该 stage 映射
     * （和 `entries == null -> continue` 同语义），空 entries 清空 units。最后 [deduplicateRanges]。
     *
     * @param perStageNewUnits 每个 stage 的新动画 units（T_i 坐标）。
     * @param perStageOffsetMaps 每个 stage 的 offset map（T_i→T_{i+1}）；null 表示该 stage 无 offset map。
     * @return 合成到最终 Tn 坐标的 units 列表（去重保序）。
     */
    fun composeNewUnitsToFinalStages(
        perStageNewUnits: List<List<TextRange>>,
        perStageOffsetMaps: List<List<VisualOffsetMapEntry>?>,
    ): List<TextRange> {
        val n = perStageNewUnits.size
        if (n == 0) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in 0 until n) {
            var units: List<TextRange> = perStageNewUnits[i]
            mapForwardLoop@ for (j in (i + 1) until n) {
                val entries = perStageOffsetMaps[j]
                if (entries == null) continue@mapForwardLoop
                if (entries.isEmpty()) {
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
     * #684 评论 5669048233 Bug2 修复 + #694 评论 5691696678 问题3：通用 stage-map 版本 —
     * 把每个 stage 的 oldUnits 沿前面 stage offset map 映射回最初 T0 坐标。
     *
     * 不依赖 [EditorVisualIntent]，供 [ComposeVisualPatchBatch.compose] 合成 deletedUnits。
     *
     * 算法和 [composeOldAnimationUnitsToBase] 一致：每笔的 units 沿前面 stage offset map
     * 映射回最初 T0（用 [mapRangesBackwardThroughOffsetMap]），null offset map 跳过该 stage 映射，
     * 空 entries 清空 units。最后 [deduplicateRanges]。
     *
     * @param perStageOldUnits 每个 stage 的旧动画 units（T_{i+1} 坐标）。
     * @param perStageOffsetMaps 每个 stage 的 offset map（T_i→T_{i+1}）；null 表示该 stage 无 offset map。
     * @return 合成回最初 T0 坐标的 units 列表（去重保序）。
     */
    fun composeOldUnitsToBaseStages(
        perStageOldUnits: List<List<TextRange>>,
        perStageOffsetMaps: List<List<VisualOffsetMapEntry>?>,
    ): List<TextRange> {
        val n = perStageOldUnits.size
        if (n == 0) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in 0 until n) {
            var units: List<TextRange> = perStageOldUnits[i]
            mapBackwardLoop@ for (j in (i - 1) downTo 0) {
                val entries = perStageOffsetMaps[j]
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
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 fact 的 newAnimationUnits
     * 合成到最终 Tn 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeNewAnimationUnitsToFinal(chain: List<EditorEditFact>): List<TextRange> =
        composeNewUnitsToFinalStages(
            perStageNewUnits = chain.map { it.newAnimationUnits },
            perStageOffsetMaps = chain.map { it.offsetMap?.entries },
        )

    /**
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 fact 的 oldAnimationUnits
     * 合成回最初 T0 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeOldAnimationUnitsToBase(chain: List<EditorEditFact>): List<TextRange> =
        composeOldUnitsToBaseStages(
            perStageOldUnits = chain.map { it.oldAnimationUnits },
            perStageOffsetMaps = chain.map { it.offsetMap?.entries },
        )

    /**
     * #684 评论 5673811415：把某笔 fact 的 cursor offset 沿后续 offset maps 映射到最终 Tn 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun mapCursorOffsetThroughChain(
        chain: List<EditorEditFact>,
        factIndex: Int,
        offset: Int,
    ): Int? {
        var currentOffset = offset
        for (j in (factIndex + 1) until chain.size) {
            val fact = chain[j]
            val mapped = mapCaretThroughFact(fact, currentOffset)
            if (mapped == null) {
                return null
            }
            currentOffset = mapped
        }
        return currentOffset
    }

    private fun mapCaretThroughFact(
        fact: EditorEditFact,
        offset: Int,
    ): Int? {
        val replaceBounds = fact.replaceBounds
        if (replaceBounds != null) {
            return mapCaretThroughReplaceBounds(replaceBounds, offset)
        }

        val entries = fact.offsetMap?.entries
        if (entries == null) return offset
        if (entries.isEmpty()) {
            return null
        }
        return mapCaretThroughOffsetMapEntries(entries, offset)
    }

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

    private fun mapCaretThroughOffsetMapEntries(
        entries: List<VisualOffsetMapEntry>,
        offset: Int,
    ): Int? {
        val sorted = entries.sortedBy { it.oldStart }

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

        val firstOldStart = sorted.first().oldStart
        if (offset < firstOldStart) {
            return offset
        }

        val lastEntry = sorted.last()
        val lastOldEnd = lastEntry.oldStart + lastEntry.length
        if (offset > lastOldEnd) {
            val delta = (lastEntry.newStart + lastEntry.length) - lastOldEnd
            return offset + delta
        }

        return null
    }

    /**
     * 把 ranges 沿 offsetMap entries 的 old→new 方向映射。
     *
     * #694 评论 5691696678 问题3：internal 可见性 — 供 [composeNewUnitsToFinalStages] 调用。
     */
    internal fun mapRangesForwardThroughOffsetMap(
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
     *
     * #694 评论 5691696678 问题3：internal 可见性 — 供 [composeOldUnitsToBaseStages] 调用。
     */
    internal fun mapRangesBackwardThroughOffsetMap(
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
     * #689 评论 5676120929 问题3：获取 fact 的 entries — 优先用 offsetMap，没有则根据
     * replaceBounds + expectedOldText/expectedNewText 生成 fallback survival map。
     *
     * Issue #735 评论 5771063665：参数从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun entriesForFact(fact: EditorEditFact): List<VisualOffsetMapEntry> {
        fact.offsetMap?.entries?.let { return it }
        return buildFallbackEntriesFromReplaceBounds(
            fact.replaceBounds,
            fact.expectedOldText.length,
            fact.expectedNewText.length,
        )
    }

    /**
     * 根据 replaceBounds + oldLen + newLen
     * 生成 fallback survival map：
     * - replace 前面的前缀：old [0, oldStart) -> new [0, newStart)
     * - replace 后面的后缀：old [oldEnd, oldLen) -> new [newEnd, newLen)
     * - 被替换的中间区域没有 entry（被编辑/删除，不存活）。
     *
     * Issue #735 评论 5771063665：不再接收 [EditorVisualIntent]，改为接收纯数据。
     */
    private fun buildFallbackEntriesFromReplaceBounds(
        replaceBounds: VisualReplaceBounds?,
        oldLen: Int,
        newLen: Int,
    ): List<VisualOffsetMapEntry> {
        if (replaceBounds == null || oldLen == 0 || newLen == 0) return emptyList()

        val entries = mutableListOf<VisualOffsetMapEntry>()

        // 前缀：old [0, oldStart) -> new [0, newStart)
        if (replaceBounds.oldStart > 0 && replaceBounds.newStart > 0) {
            val prefixLen = minOf(replaceBounds.oldStart, replaceBounds.newStart)
            if (prefixLen > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = prefixLen,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }

        // 后缀：old [oldEnd, oldLen) -> new [newEnd, newLen)
        val oldSuffixStart = replaceBounds.oldEnd
        val newSuffixStart = replaceBounds.newEnd
        if (oldSuffixStart < oldLen && newSuffixStart < newLen) {
            val suffixLen = minOf(oldLen - oldSuffixStart, newLen - newSuffixStart)
            if (suffixLen > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = oldSuffixStart,
                        newStart = newSuffixStart,
                        length = suffixLen,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }

        return entries
    }

    /**
     * #644 评论 #684：合成整条 offset map chain。
     *
     * #689 评论 5676120929 问题3：不再因某一笔 offsetMap == null 就返回 null，
     * 而是每笔都用 [entriesForFact] 拿 entries（优先 offsetMap，没有则从 replaceBounds 生成 fallback），
     * 保证等长替换时被替换区域不会被错当成存活。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeOffsetMapChain(chain: List<EditorEditFact>): List<VisualOffsetMapEntry>? {
        if (chain.isEmpty()) return null

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

        for (fact in chain) {
            val entries = entriesForFact(fact)
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

    private fun buildStageSegments(entries: List<VisualOffsetMapEntry>): List<StageSegment> {
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

    private data class AccSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )

    private data class StageSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )

    /**
     * Retained moves 计算上下文 — 封装循环中不变的参数。
     *
     * Issue #717 评论 5742904417 修复1：oldText/newText 类型从 AnnotatedString 改成 CharSequence，
     * 因为现在传的是 String（rawText），但 ctx.oldText[segEnd-1].isHighSurrogate() 需要 CharSequence 索引，
     * String 和 AnnotatedString 都支持。
     */
    data class RetainedMovesContext(
        val prev: ComposeLayoutSnapshot,
        val curr: ComposeLayoutSnapshot,
        val oldText: CharSequence,
        val newText: CharSequence,
        val oldTextLen: Int,
        val newTextLen: Int,
        val oldSuffixStart: Int,
        val newSuffixStart: Int,
    )

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

    class MergeState {
        var mergedOldStart: Int = -1
        var mergedNewStart: Int = -1
        var mergedDx: Float = 0f
        var mergedDy: Float = 0f
        var merging: Boolean = false
    }

    data class RetainedMoveSegmentResult(
        val segEnd: Int,
        val oldBounds: Rect?,
        val newBounds: Rect?,
    )

    fun processRetainedMoveSegment(
        ctx: RetainedMovesContext,
        oldPos: Int,
    ): RetainedMoveSegmentResult {
        // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
        // getLineEnd 返回 display offset（含 U+200B），retained move 切片需要 raw offset。
        // ctx.oldTextLen 现在是 rawText.length，oldLineEnd 也是 raw offset，正确。
        val oldLineEnd = ctx.prev.rawLineEndForRawOffset(oldPos)
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
            val oldBounds = safePathBounds(ctx.prev, oldRange)
            val newBounds = safePathBounds(ctx.curr, newRange)
            RetainedMoveSegmentResult(segEnd, oldBounds, newBounds)
        }
    }

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
