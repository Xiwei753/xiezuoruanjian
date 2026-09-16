package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.LocalVisualPlanDto
import uniffi.writer_core.LocalVisualSliceDto
import uniffi.writer_core.classifyLocalVisualPlan

/**
 * #694 评论第 4 步：本地输入视觉 rebase —
 * 把"其实只需要 old/new ranges + old/new layout"的计算从 Core [EditorVisualIntent] 解耦出来。
 *
 * 本地键盘/退格的动画不再绕 [EditorVisualIntent] / Core 回声才能调用 [ComposeVisualRebase]。
 * 这里复用 [ComposeVisualRebase] 现有的 range 切片/映射函数，只是入口换成
 * [LocalInputChange] + old/new [ComposeLayoutSnapshot]。
 *
 * 特别是删除换行：[buildOffsetMap] 必须保住换行两侧仍然存在的文字。
 * 删除掉的只生成 ghost；上一行/下一行仍存活的文字只在 oldRect != newRect 时生成 retained move。
 * 这样删除到上一行时不会把整段幸存文字重新接管一遍。
 */
@Suppress("TooManyFunctions")
internal object ComposeLocalVisualRebase {
    /**
     * #694 评论 5691696678 问题1：把连续本地输入 chain 中每笔 [LocalInputVisualEdit] 的
     * changes 逐 stage 合成成 T0→Tn 的 unchanged offset map。
     *
     * 不同坐标系的 changes 不能直接摊平，必须通过 stage map 逐级合成：
     * - stage 0: T0→T1（首笔 edit.changes）
     * - stage 1: T1→T2
     * - ...
     * - stage n-1: Tn-1→Tn
     * - 合成: T0→Tn
     *
     * 算法参考 [ComposeVisualPatchBatch.composeBatchOffsetMap] / [composeTwoMaps]，
     * 这里实现一份同样的合成算法（[ComposeVisualPatchBatch.composeTwoMaps] 是 private，
     * 不便跨 object 复用；保持本 object 自洽）。
     *
     * @param chain 连续本地输入链（按入队顺序，chain[i+1].oldText == chain[i].newText）。
     * @return 合成后的 T0→Tn offset map entries；空 chain 返回空列表。
     */
    fun composeLocalChainOffsetMap(chain: List<LocalInputVisualEdit>): List<VisualOffsetMapEntry> {
        if (chain.isEmpty()) return emptyList()
        val first = chain.first()
        var acc: List<VisualOffsetMapEntry> =
            if (first.oldText.isNotEmpty()) {
                listOf(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = first.oldText.length,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            } else {
                emptyList()
            }
        for (edit in chain) {
            val stage = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            acc = composeTwoMaps(acc, stage)
            if (acc.isEmpty()) break
        }
        return acc
    }

    /**
     * 组合两段 offset map（acc: T0→T_i, stage: T_i→T_{i+1}）成 T0→T_{i+1}。
     * 与 [ComposeVisualPatchBatch.composeTwoMaps] / [ComposeVisualRebase.composeStage] 同算法。
     */
    private fun composeTwoMaps(
        acc: List<VisualOffsetMapEntry>,
        stage: List<VisualOffsetMapEntry>,
    ): List<VisualOffsetMapEntry> {
        if (acc.isEmpty() || stage.isEmpty()) return emptyList()
        val result = mutableListOf<VisualOffsetMapEntry>()
        for (a in acc) {
            val aNewEnd = a.newStart + a.length
            for (s in stage) {
                val sOldEnd = s.oldStart + s.length
                val overlapStart = maxOf(a.newStart, s.oldStart)
                val overlapEnd = minOf(aNewEnd, sOldEnd)
                if (overlapStart >= overlapEnd) continue
                val offsetInAcc = overlapStart - a.newStart
                val oldStartInitial = a.oldStart + offsetInAcc
                val newStartFrontier = s.newStart + (overlapStart - s.oldStart)
                val kind =
                    if (a.kind == VisualOffsetMapKind.SHIFTED || s.kind == VisualOffsetMapKind.SHIFTED) {
                        VisualOffsetMapKind.SHIFTED
                    } else {
                        VisualOffsetMapKind.IDENTITY
                    }
                result.add(
                    VisualOffsetMapEntry(
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

    /**
     * #694 评论第 4 步：从 [LocalInputChange.oldRange]/[LocalInputChange.newRange]
     * 构造 T0→Tn 的 unchanged offset map。
     *
     * 存活文本（不在任何 oldRange 内的部分）映射到 newText 中对应位置；
     * 被编辑/删除的区域没有 entry（补集即 changedRanges）。
     *
     * @param changes 本次本地输入的 change 列表（来自 TextFieldBuffer.forEachChange）。
     * @param oldLength 旧正文长度（UTF-16）。
     * @param newLength 新正文长度（UTF-16）。
     * @return offset map entries；空 changes 时若 oldLength==newLength 返回整段 identity。
     */
    fun buildOffsetMap(
        changes: List<LocalInputChange>,
        oldLength: Int,
        newLength: Int,
    ): List<VisualOffsetMapEntry> {
        if (changes.isEmpty()) {
            if (oldLength > 0 && newLength > 0 && oldLength == newLength) {
                return listOf(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = oldLength,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
            return emptyList()
        }
        val sorted = changes.sortedBy { it.oldRange.start }
        val entries = mutableListOf<VisualOffsetMapEntry>()
        var oldCursor = 0
        var newCursor = 0
        for (change in sorted) {
            val oldStart = change.oldRange.start
            val newStart = change.newRange.start
            // 存活段 [oldCursor, oldStart) → [newCursor, newStart)
            if (oldCursor < oldStart && newCursor < newStart) {
                val len = minOf(oldStart - oldCursor, newStart - newCursor)
                if (len > 0) {
                    entries.add(
                        VisualOffsetMapEntry(
                            oldStart = oldCursor,
                            newStart = newCursor,
                            length = len,
                            kind = VisualOffsetMapKind.IDENTITY,
                        ),
                    )
                }
            }
            oldCursor = change.oldRange.end
            newCursor = change.newRange.end
        }
        // 尾部存活段 [oldCursor, oldLength) → [newCursor, newLength)
        if (oldCursor < oldLength && newCursor < newLength) {
            val len = minOf(oldLength - oldCursor, newLength - newCursor)
            if (len > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = oldCursor,
                        newStart = newCursor,
                        length = len,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }
        return entries
    }

    /**
     * #694 评论第 4 步：从 offset map 的补集算 deletedUnits/insertedUnits。
     * 复用 [ComposeVisualRebase.changedRangesFromComposedMap]。
     */
    fun changedRangesFromOffsetMap(
        offsetMap: List<VisualOffsetMapEntry>,
        oldLength: Int,
        newLength: Int,
    ): ComposeVisualRebase.FrameChangedRanges =
        ComposeVisualRebase.changedRangesFromComposedMap(offsetMap, oldLength, newLength)

    /**
     * #694 评论 5691696678 问题3：把连续本地输入 chain 中每笔 edit 的 insertedUnits
     * 逐 stage 合成到最终 Tn 坐标，保留多字符吐字顺序。
     *
     * 每笔 edit 的 insertedUnits 从该笔 stage offset map 补集算（per-stage 净变化），
     * 然后用 [ComposeVisualRebase.composeNewUnitsToFinalStages] 沿后续 stage offset map
     * 映射到最终 Tn。这样 `"" -> "a" -> "ab" -> "abc"` 的 chain 会得到 3 个 unit
     * `[0,1), [1,2), [2,3)` 而非单个 `[0,3)`。
     *
     * @param chain 连续本地输入链（按入队顺序）。
     * @return 合成到最终 Tn 坐标的 insertedUnits 列表（去重保序）。
     */
    fun composeLocalChainInsertedUnits(chain: List<LocalInputVisualEdit>): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val perStageNewUnits =
            chain.map { edit ->
                val stageMap = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
                changedRangesFromOffsetMap(stageMap, edit.oldText.length, edit.newText.length).newRanges
            }
        val perStageOffsetMaps =
            chain.map { edit ->
                buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            }
        return ComposeVisualRebase.composeNewUnitsToFinalStages(perStageNewUnits, perStageOffsetMaps)
    }

    /**
     * #694 评论 5691696678 问题3：把连续本地输入 chain 中每笔 edit 的 deletedUnits
     * 逐 stage 合成回最初 T0 坐标，保留多字符吞字顺序。
     *
     * @param chain 连续本地输入链（按入队顺序）。
     * @return 合成回最初 T0 坐标的 deletedUnits 列表（去重保序）。
     */
    fun composeLocalChainDeletedUnits(chain: List<LocalInputVisualEdit>): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val perStageOldUnits =
            chain.map { edit ->
                val stageMap = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
                changedRangesFromOffsetMap(stageMap, edit.oldText.length, edit.newText.length).oldRanges
            }
        val perStageOffsetMaps =
            chain.map { edit ->
                buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            }
        return ComposeVisualRebase.composeOldUnitsToBaseStages(perStageOldUnits, perStageOffsetMaps)
    }

    /**
     * #694 评论 5692161955 问题1/2：调用 Core 纯计算 API [classifyLocalVisualPlan] 做视觉分类。
     *
     * 把 Android UTF-16 [TextRange] affected ranges 转成 UTF-8 byte ranges 调 Core，
     * 再把 Core 返回的 UTF-8 byte animation units 转回 UTF-16 [TextRange]。
     *
     * 返回 [LocalVisualPlanDto]（animationMode + old/new animation units，UTF-8 byte ranges），
     * 调用方按需用 [utf16TextRangeForUtf8] 转回 UTF-16。
     *
     * #694 评论 5693864609 问题3：删除 Kotlin fallback — Core API 现在是纯函数不再返回 Result，
     * 不会抛异常。测试环境（Robolectric）通过注入 [LocalVisualPlanClassifier] fake 绕过 Core。
     * 本方法保留为兼容入口，内部委托给 [CoreLocalVisualPlanClassifier]。
     *
     * @param oldText 旧正文（UTF-16）。
     * @param newText 新正文（UTF-16）。
     * @param oldAffectedRanges 旧正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param newAffectedRanges 新正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param animationEnabled 是否启用动画。
     */
    fun classifyLocalVisualPlanFromCore(
        oldText: String,
        newText: String,
        oldAffectedRanges: List<TextRange>,
        newAffectedRanges: List<TextRange>,
        animationEnabled: Boolean,
    ): LocalVisualPlanDto =
        CoreLocalVisualPlanClassifier.classify(
            oldText = oldText,
            newText = newText,
            oldAffectedRanges = oldAffectedRanges,
            newAffectedRanges = newAffectedRanges,
            animationEnabled = animationEnabled,
        )

    /**
     * #694 评论 5693077441：从 [text] 的 [range] 截出 affected substring 构造 [LocalVisualSliceDto]。
     *
     * [absoluteStart] 用 UTF-8 byte offset（与 Core 契约一致），[text] 是 UTF-16 substring。
     * range 越界或空区间时返回 null（由 mapNotNull 过滤）。
     *
     * #694 评论 5693864609 问题3：改成 internal 可见性以便 [CoreLocalVisualPlanClassifier] 访问。
     */
    internal fun buildLocalVisualSliceDto(
        text: String,
        range: TextRange,
    ): LocalVisualSliceDto? {
        if (range.start !in 0..text.length || range.end !in 0..text.length) return null
        if (range.start >= range.end) return null
        val segment = text.substring(range.start, range.end)
        val absoluteStart = TextOffsetUtils.utf8OffsetForCharIndex(text, range.start).toUInt()
        return LocalVisualSliceDto(absoluteStart = absoluteStart, text = segment)
    }

    /**
     * #694 评论 5692161955 回归修复：用 `java.text.BreakIterator` 拆分 grapheme cluster，
     * 再按 ZWJ（U+200D）合并被错误拆开的 emoji family。
     *
     * 算法：
     * 1. 用 BreakIterator.getCharacterInstance() 得到初步 cluster 边界（UTF-16 偏移）。
     * 2. 遍历初步 cluster，按 ZWJ 合并：**当前 segment 是 ZWJ，或上一 cluster 以 ZWJ 结尾时
     *    才合并下一段**。这样 `👨‍👩‍👧‍👦`（BreakIterator 拆成 7 个 cluster：
     *    man + ZWJ + woman + ZWJ + girl + ZWJ + boy）会被合并成 1 个 cluster，
     *    而 `👨‍👩‍👧‍👦a` 会合并成 [emoji family] + [a] = 2 个 cluster。
     *
     * #694 评论 5693077441 问题2：旧逻辑用"group 曾经包含过 ZWJ"（currentContainsZwj 一旦 true
     * 永远 true），会把 `👨‍👩‍👧‍👦a` 结尾的 a 合进 emoji family。新逻辑只看当前 group 的
     * **最后一个** cluster 是否以 ZWJ 结尾，合并完一段后状态重置为当前 segment 的 ZWJ 状态。
     *
     * 此方案不依赖 ICU4J 是否可用，在所有环境（Robolectric、真实设备）下都能正确处理
     * ZWJ emoji family。
     *
     * #694 评论 5693864609 问题3：改成 internal 可见性供测试 fake classifier 复用
     * （fake classifier 用 BreakIterator + ZWJ 合并模拟 Core 的 grapheme cluster 拆分）。
     * 生产环境优先用 Core API（unicode_segmentation 已正确）。
     *
     * @param text 要拆分的文本（UTF-16）。
     * @return cluster 在原文中的 UTF-16 [TextRange] 列表（按顺序，不重叠，覆盖所有字符）。
     */
    internal fun splitGraphemeClusterRangesWithZwjMerge(text: String): List<TextRange> {
        if (text.isEmpty()) return emptyList()
        val iterator = java.text.BreakIterator.getCharacterInstance()
        iterator.setText(text)
        // 1. 用 BreakIterator 得到初步 cluster 边界
        val rawRanges = mutableListOf<TextRange>()
        var start = iterator.first()
        var end = iterator.next()
        while (end != java.text.BreakIterator.DONE) {
            rawRanges.add(TextRange(start, end))
            start = end
            end = iterator.next()
        }
        if (rawRanges.isEmpty()) return emptyList()
        // 2. #694 评论 5693077441：ZWJ 合并改成"当前 segment 是 ZWJ，或上一 cluster 以 ZWJ 结尾
        //    时才合并下一段"。不再用"group 曾经包含过 ZWJ"（currentContainsZwj 一旦 true 永远 true），
        //    否则 👨‍👩‍👧‍👦a 会把结尾的 a 合进 emoji family。
        val zwj = '\u200D'
        val mergedRanges = mutableListOf<TextRange>()
        var currentStart = rawRanges[0].start
        var currentEnd = rawRanges[0].end
        // lastClusterContainsZwj：当前 group 的最后一个 cluster 是否包含 ZWJ。
        var lastClusterContainsZwj = text.substring(currentStart, currentEnd).contains(zwj)
        for (i in 1 until rawRanges.size) {
            val range = rawRanges[i]
            val segment = text.substring(range.start, range.end)
            val segmentContainsZwj = segment.contains(zwj)
            if (segmentContainsZwj || lastClusterContainsZwj) {
                // 合并到当前 group
                currentEnd = range.end
            } else {
                // 输出当前 group，开始新 group
                mergedRanges.add(TextRange(currentStart, currentEnd))
                currentStart = range.start
                currentEnd = range.end
            }
            // 更新为当前 segment 的 ZWJ 状态（无论合并与否，最后一个 cluster 都是当前 segment）
            lastClusterContainsZwj = segmentContainsZwj
        }
        mergedRanges.add(TextRange(currentStart, currentEnd))
        return mergedRanges
    }

    /**
     * 把 Core 返回的 UTF-8 byte animation units 转成 Android UTF-16 [TextRange]。
     */
    fun utf16AnimationUnitsFromPlan(
        text: String,
        byteUnits: List<EditorByteRangeDto>,
    ): List<TextRange> =
        byteUnits.map { byteRangeDto ->
            TextOffsetUtils.utf16TextRangeForUtf8(
                text,
                byteRangeDto.start.toInt(),
                byteRangeDto.endExclusive.toInt(),
            )
        }

    /**
     * #694 评论第 4 步：retained reflow 只比较 oldLayout -> newLayout 的真实几何。
     * 复用 [ComposeVisualRebase.computeRetainedMovesFromComposedMap]。
     *
     * retainedMoves 只按第一份旧 layout 和最后一份新 layout 算一次，
     * 不把同一帧内中间态的幸存文字重新接管一遍。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>,
    ): List<RetainedMove> = ComposeVisualRebase.computeRetainedMovesFromComposedMap(oldLayout, newLayout, offsetMap)

    /**
     * #694 评论第 4 步：cursor 从 oldSelection.end -> newSelection.end 构造路径。
     *
     * 多插入 unit 时按 unit 顺序生成路径点，endFraction 与 timeline 的 unit-wise 分段时序一致；
     * 单字符/删除/无 unit 时只保留最终目标点（endFraction = 1f）。
     * 不伪造中间位置，最终几何 target 只取最后 layout。
     *
     * @return 光标运动路径；取不到新光标 rect 时返回 null。
     */
    fun buildCursorPath(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        oldSelection: TextRange,
        newSelection: TextRange,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): CursorMotionPath? {
        val newCursorRect = safeCursorRectFromLayout(newLayout, newSelection.end) ?: return null
        // 无文字动画语义 → 单点路径 snap 到新光标位置。
        if (insertedUnits.isEmpty() && deletedUnits.isEmpty()) {
            return CursorMotionPath(listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))
        }
        // 多插入 unit：按 unit.end 顺序生成路径点（光标依次经过每个字/cluster 出现后的位置）。
        val points = mutableListOf<CursorMotionPoint>()
        for (unit in insertedUnits) {
            val rect = safeCursorRectFromLayout(newLayout, unit.end) ?: continue
            points.add(CursorMotionPoint(rect = rect, endFraction = 0f))
        }
        if (points.isEmpty()) {
            // 只有删除 unit 或插入 unit 都取不到 rect → 最终目标点。
            points.add(CursorMotionPoint(rect = newCursorRect, endFraction = 1f))
        } else {
            // 归一化 endFraction = (i + 1f) / n，与 timeline unit-wise 分段时序一致。
            val n = points.size
            for (i in points.indices) {
                points[i] = points[i].copy(endFraction = (i + 1f) / n)
            }
        }
        return CursorMotionPath(points)
    }

    /**
     * 安全获取光标 rect — offset 越界或 layout 抛异常时返回 null。
     */
    private fun safeCursorRectFromLayout(
        layout: ComposeLayoutSnapshot,
        offset: Int,
    ): Rect? {
        val textLen = layout.result.layoutInput.text.length
        if (offset < 0 || offset > textLen) return null
        return try {
            layout.result.getCursorRect(offset)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * 安全获取 path bounds — 复用 [ComposeVisualRebase.safePathBounds]。
     */
    fun safePathBounds(
        result: TextLayoutResult,
        range: TextRange,
    ): Rect? = ComposeVisualRebase.safePathBounds(result, range)

    /**
     * #694 评论 5693864609 问题1：把 Core plan 的细粒度 unit 按 local chain 的 stage range 重新排序。
     *
     * 规则：按 [orderedStageRanges] 顺序遍历，每个 stage range 内的 plan unit 保持 Core 自己的粒度顺序；最后去重。
     * 这样 `abc -> ab -> a` 会得到 `[c, b]`（按删除时间倒序），
     * 但一次删除一个多 grapheme 选区时仍然保留 Core 的 grapheme/run 切分。
     *
     * @param planUnits Core plan 返回的 animation units（UTF-16 [TextRange]，已按 Core 粒度切分）。
     * @param orderedStageRanges local chain 的时间顺序 stage ranges（每个 stage 一笔删除/插入的 affected range）。
     * @return 按 stage range 顺序重排后的 plan units；空 plan 或空 stage ranges 时原样返回 planUnits。
     */
    fun orderPlanUnitsByStageRanges(
        planUnits: List<TextRange>,
        orderedStageRanges: List<TextRange>,
    ): List<TextRange> {
        if (planUnits.isEmpty()) return emptyList()
        if (orderedStageRanges.isEmpty()) return planUnits
        val result = mutableListOf<TextRange>()
        val used = mutableSetOf<Int>() // 已使用的 planUnits 索引
        for (stageRange in orderedStageRanges) {
            // 找出落在当前 stage range 内的 plan unit（按 Core 粒度顺序）
            for ((i, planUnit) in planUnits.withIndex()) {
                if (i in used) continue
                // plan unit 与 stage range 有交集就归到这个 stage
                if (planUnit.start < stageRange.end && planUnit.end > stageRange.start) {
                    result.add(planUnit)
                    used.add(i)
                }
            }
        }
        // 没被任何 stage range 覆盖的 plan unit 追加到末尾（保持 Core 顺序）
        for ((i, planUnit) in planUnits.withIndex()) {
            if (i !in used) result.add(planUnit)
        }
        return result
    }

    /**
     * #694 评论 5693864609 问题1：本地 chain 版 cursor path — 对每一笔删除/插入，
     * 用该笔 [LocalInputVisualEdit.newSelection] 的 end 作为这一阶段的 caret；
     * 把它映射回 T0 后从 oldLayout 取 rect，按 chain 顺序组成路径；
     * 最后一个点必须是 newLayout 的最终真实 cursor rect。
     *
     * 与 [buildCursorPath] 的区别：[buildCursorPath] 只看最终 insertedUnits 的 end，
     * 快速连续删除时所有 unit 的 end 都在最终文本上，路径点会重叠。
     * 本方法用 chain 每笔的 newSelection.end，保留中间阶段的 caret 位置。
     *
     * @param chain 连续本地输入链（按入队顺序）。
     * @param oldLayout T0 时的 layout。
     * @param newLayout Tn 时的 layout。
     * @param insertedUnits 已合成的插入 unit（用于判断是否有文字动画语义）。
     * @param deletedUnits 已合成的删除 unit（用于判断是否有文字动画语义）。
     * @return 光标运动路径；chain 为空或取不到新光标 rect 时返回 null。
     */
    fun buildLocalChainCursorPath(
        chain: List<LocalInputVisualEdit>,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): CursorMotionPath? {
        val lastEdit = chain.lastOrNull() ?: return null
        val newCursorRect = safeCursorRectFromLayout(newLayout, lastEdit.newSelection.end) ?: return null
        // 无文字动画语义 → 单点路径 snap 到新光标位置
        if (insertedUnits.isEmpty() && deletedUnits.isEmpty()) {
            return CursorMotionPath(listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))
        }
        val points = mutableListOf<CursorMotionPoint>()
        // 对每一笔 edit，用该笔 newSelection.end 作为阶段 caret
        for (edit in chain) {
            val caretOffset = edit.newSelection.end
            // 优先从 newLayout 取 rect（如果 offset 在新正文范围内）
            val rect =
                safeCursorRectFromLayout(newLayout, caretOffset)
                    ?: safeCursorRectFromLayout(oldLayout, caretOffset)
                    ?: continue
            points.add(CursorMotionPoint(rect = rect, endFraction = 0f))
        }
        // 确保最后一个点是最终真实 cursor rect
        if (points.isEmpty() || points.last().rect != newCursorRect) {
            points.add(CursorMotionPoint(rect = newCursorRect, endFraction = 1f))
        }
        // 归一化 endFraction = (i + 1f) / n，与 timeline unit-wise 分段时序一致
        val n = points.size
        if (n > 1) {
            for (i in points.indices) {
                points[i] = points[i].copy(endFraction = (i + 1f) / n)
            }
        }
        return CursorMotionPath(points)
    }
}

/**
 * #694 评论 5693864609 问题3：本地视觉 plan 分类器接口 —
 * 把 Core `classifyLocalVisualPlan` 的调用抽象成可注入的接口，
 * 生产环境用 [CoreLocalVisualPlanClassifier] 直接调 Core，
 * 测试环境（Robolectric）注入 fake 绕过原生库加载。
 *
 * 设计为 `fun interface` 以便用 lambda 构造 fake。
 */
fun interface LocalVisualPlanClassifier {
    /**
     * 对一次本地输入的 affected ranges 做视觉分类，返回 [LocalVisualPlanDto]。
     *
     * @param oldText 旧正文（UTF-16）。
     * @param newText 新正文（UTF-16）。
     * @param oldAffectedRanges 旧正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param newAffectedRanges 新正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param animationEnabled 是否启用动画。
     */
    fun classify(
        oldText: String,
        newText: String,
        oldAffectedRanges: List<TextRange>,
        newAffectedRanges: List<TextRange>,
        animationEnabled: Boolean,
    ): LocalVisualPlanDto
}

/**
 * #694 评论 5693864609 问题3：main 默认实现 — 直接调 Core [classifyLocalVisualPlan]。
 *
 * Core API 现在是纯函数（不返回 Result、不抛异常），所以不需要 try-catch fallback。
 * 测试环境通过注入 fake [LocalVisualPlanClassifier] 绕过 Core 原生库加载。
 */
object CoreLocalVisualPlanClassifier : LocalVisualPlanClassifier {
    override fun classify(
        oldText: String,
        newText: String,
        oldAffectedRanges: List<TextRange>,
        newAffectedRanges: List<TextRange>,
        animationEnabled: Boolean,
    ): LocalVisualPlanDto {
        // #694 评论 5693077441：只从 changedRanges 截出 affected substring 构造 LocalVisualSliceDto，
        // 不再把整章 oldText/newText 都跨 UniFFI 复制给 Core。
        val oldSlices =
            oldAffectedRanges.mapNotNull { range -> ComposeLocalVisualRebase.buildLocalVisualSliceDto(oldText, range) }
        val newSlices =
            newAffectedRanges.mapNotNull { range -> ComposeLocalVisualRebase.buildLocalVisualSliceDto(newText, range) }
        return classifyLocalVisualPlan(
            oldSlices = oldSlices,
            newSlices = newSlices,
            animationEnabled = animationEnabled,
        )
    }
}
