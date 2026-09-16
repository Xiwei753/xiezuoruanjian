package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #694 评论第 7 步：同一 VSync 的多笔 patch 合成器 —
 * 把同一帧内 pending 的多笔 [ComposeVisualPatch] 合成一个屏幕 transition，
 * 避免 [ComposeEditorVisualState.drainPendingPatchesAtFrame] 逐笔 applyPatch
 * 在同一个屏幕帧里把 retained text/cursor 连续重定向几次。
 *
 * 合成规则（Issue #694 评论第 7 步）：
 * - [oldLayout] = batch.first().oldLayout（第一份旧 layout）
 * - [newLayout] = batch.last().newLayout（最后一份新 layout）
 * - offset map 组合成 T0 -> Tn
 * - insert/delete 按最终净变化算，不把同一帧内"刚输入又删掉"的中间字拿出来播放
 * - retainedMoves 只按第一份旧 layout 和最后一份新 layout 算一次
 * - cursor 最终几何 target 只取最后 layout；多字符吐字顺序放在 unit/path 的时间分段里，
 *   不创建同 timestamp 的多条 position track
 */
internal object ComposeVisualPatchBatch {
    /**
     * 把同一 VSync 的多笔 patch 合成一个屏幕 transition。
     *
     * @param batch 同一帧待消费的 patch 列表（按入队顺序）。非空。
     * @return 合成后的单笔 [ComposeVisualPatch]；batch 为空返回 null。
     */
    @Suppress("LongMethod", "CognitiveComplexMethod")
    fun compose(batch: List<ComposeVisualPatch>): ComposeVisualPatch? {
        if (batch.isEmpty()) return null
        if (batch.size == 1) return batch.first()

        val first = batch.first()
        val last = batch.last()
        val oldLayout = first.oldLayout
        val newLayout = last.newLayout
        val oldText = oldLayout.result.layoutInput.text.text
        val newText = newLayout.result.layoutInput.text.text
        val oldLength = oldText.length
        val newLength = newText.length

        // offset map 组合成 T0 -> Tn
        val composedOffsetMap = composeBatchOffsetMap(batch, oldLength)

        // insert/delete 按最终净变化算（从 composed offset map 补集）
        val changedRanges =
            ComposeVisualRebase.changedRangesFromComposedMap(composedOffsetMap, oldLength, newLength)
        val transactionTextKind =
            when {
                changedRanges.oldRanges.isEmpty() && changedRanges.newRanges.isEmpty() ->
                    TextVisualKind.None
                changedRanges.oldRanges.isEmpty() -> TextVisualKind.Insert
                changedRanges.newRanges.isEmpty() -> TextVisualKind.Delete
                else -> TextVisualKind.Move
            }

        val motionPolicy = last.motionPolicy
        val effectivePolicy = motionPolicy.effective()
        val screenSuppressed = batch.any { it.animationMode == uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED }
        val customTextAnimationEnabled =
            effectivePolicy.textEnabled && !screenSuppressed && transactionTextKind != TextVisualKind.None

        // #694 评论 5691696678 问题3：insertedUnits/deletedUnits 用通用 stage-map 版本合成，
        // 保留多字符吐字顺序（a/b/c 三个 unit 而非单个 [0,3)）。
        // 每笔 patch 的 insertedUnits/deletedUnits 沿后续 stage offset map 映射到最终 Tn / 最初 T0。
        val perStageNewUnits = batch.map { it.insertedUnits }
        val perStageOldUnits = batch.map { it.deletedUnits }
        val perStageOffsetMaps = batch.map { it.offsetMap }
        val composedInserted =
            ComposeVisualRebase.composeNewUnitsToFinalStages(perStageNewUnits, perStageOffsetMaps)
        val composedDeleted =
            ComposeVisualRebase.composeOldUnitsToBaseStages(perStageOldUnits, perStageOffsetMaps)

        val insertedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Insert, TextVisualKind.Move -> {
                        // 优先用合成的 ordered units 保留吐字顺序；
                        // 若所有 stage 都没 insertedUnits 但净变化有插入，回退到净变化 newRanges。
                        if (composedInserted.isNotEmpty()) composedInserted else changedRanges.newRanges
                    }
                    TextVisualKind.Delete, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        val deletedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Delete, TextVisualKind.Move -> {
                        if (composedDeleted.isNotEmpty()) composedDeleted else changedRanges.oldRanges
                    }
                    TextVisualKind.Insert, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        // retainedMoves 只按第一份旧 layout 和最后一份新 layout 算一次
        val retainedMoves =
            if (composedOffsetMap.isNotEmpty()) {
                ComposeVisualRebase.computeRetainedMovesFromComposedMap(
                    oldLayout,
                    newLayout,
                    composedOffsetMap,
                )
            } else {
                emptyList()
            }

        // #694 评论 5694645209 问题2：cursor path 用专门的 batch cursor path 合成 —
        // 按 batch 入队顺序取每笔 patch.cursorMotionPath?.points，保留真实 stage caret 顺序，
        // 不再对 batch 纯删除重新走旧 buildCursorPath()（旧逻辑只按 insertedUnits 建点，
        // 纯删除时 insertedUnits 为空、回退到最终单点，丢失中间阶段 caret）。
        //
        // #694 评论 5695660885 问题2：batch cursor path 也必须遵守"同一 VSync 只表现最终屏幕差异"。
        // 同一帧 "" -> "a" -> ""，最终 transactionTextKind == None，insertedUnits/deletedUnits 都空，
        // 但旧 composeBatchCursorPath 仍收集各笔 stage cursor point 得到 0->1->0 路径，
        // computeCursorParamsForPatch 把它当 CURSOR_ONLY 用 cursorDurationMillis(80ms) 播放，
        // 导致光标在文字没变的情况下抽动。
        val finalTextVisualChanged =
            transactionTextKind != TextVisualKind.None || retainedMoves.isNotEmpty()
        val finalSelectionChanged = oldLayout.selection != newLayout.selection
        val cursorMotionPath: CursorMotionPath? =
            when {
                // !finalTextVisualChanged && !finalSelectionChanged：
                // cursorMotionPath = null，让上层 0ms snap 到最终 cursor，不播放任何 stage path。
                !finalTextVisualChanged && !finalSelectionChanged -> null
                // !finalTextVisualChanged && finalSelectionChanged：
                // 只生成 oldLayout cursor -> newLayout cursor 的最终直达路径，不保留同帧中间 stage。
                !finalTextVisualChanged && finalSelectionChanged -> {
                    composeCursorOnlySelectionChangedPath(oldLayout, newLayout)
                }
                // 只有最终仍存在真实文字视觉变化时，才用现在的 composeBatchCursorPath()
                // 保留删除/插入阶段 caret。
                else -> {
                    composeBatchCursorPath(
                        batch = batch,
                        insertedUnits = insertedUnits,
                        deletedUnits = deletedUnits,
                    ) ?: last.cursorMotionPath
                }
            }

        // coreTransactionIds 合并所有笔
        val coreTransactionIds = batch.flatMap { it.coreTransactionIds }

        // 无 overlay 工作时不按 durationMs 假装 active
        val hasCursorMotion = cursorMotionPath != null
        val effectiveDurationMs =
            if (!customTextAnimationEnabled && !hasCursorMotion) {
                0L
            } else {
                last.durationMs
            }

        val animationMode =
            if (screenSuppressed) {
                uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
            } else {
                last.animationMode
            }

        return ComposeVisualPatch(
            id = last.id,
            coreTransactionIds = coreTransactionIds,
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = composedOffsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            cursorMotionPath = cursorMotionPath,
            durationMs = effectiveDurationMs,
            animationMode = animationMode,
            motionPolicy = motionPolicy,
            intent = last.intent,
        )
    }

    /**
     * #694 评论 5695660885 问题2：净文本变化为 0 但 selection 变了 —
     * 只生成 oldLayout cursor -> newLayout cursor 的最终直达路径，不保留同帧中间 stage。
     *
     * @return [CursorMotionPath]；取不到 cursor rect 时返回 null。
     */
    private fun composeCursorOnlySelectionChangedPath(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
    ): CursorMotionPath? {
        val oldCursorRect = safeCursorRectFromBatch(oldLayout, oldLayout.selection.end)
        val newCursorRect = safeCursorRectFromBatch(newLayout, newLayout.selection.end)
        return when {
            oldCursorRect == null && newCursorRect == null -> null
            oldCursorRect == null -> CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect!!, endFraction = 1f)),
            )
            newCursorRect == null -> CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = oldCursorRect, endFraction = 1f)),
            )
            oldCursorRect == newCursorRect -> CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )
            else -> CursorMotionPath(
                points = listOf(
                    CursorMotionPoint(rect = oldCursorRect, endFraction = 0f),
                    CursorMotionPoint(rect = newCursorRect, endFraction = 1f),
                ),
            )
        }
    }

    /**
     * #694 评论 5694645209 问题2：batch cursor path 合成 —
     * 按 [batch] 入队顺序取每笔 `patch.cursorMotionPath?.points`，保留真实 stage caret 顺序，
     * 不再对 batch 纯删除重新走旧 [ComposeLocalVisualRebase.buildCursorPath]（旧逻辑只按 insertedUnits 建点，
     * 纯删除时 insertedUnits 为空、回退到最终单点，丢失中间阶段 caret）。
     *
     * 规则：
     * 1. 按 [batch] 入队顺序取每笔 `patch.cursorMotionPath?.points`；
     * 2. 保留真实 stage caret 顺序；
     * 3. 相邻相同 rect 去重；
     * 4. 最后一个点必须收敛到 `batch.last().newLayout.selection` 的最终 cursor rect；
     * 5. 最后统一重新分配 `endFraction = (i + 1) / n`，与同一有界窗口里的文字 stage 对齐；
     * 6. 只有取不到任何 stage path 时，再回退现有 [ComposeLocalVisualRebase.buildCursorPath]。
     *
     * @return 合成后的 [CursorMotionPath]；batch 为空或取不到任何 stage path 且 buildCursorPath 也失败时返回 null。
     */
    @Suppress("CognitiveComplexMethod")
    private fun composeBatchCursorPath(
        batch: List<ComposeVisualPatch>,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): CursorMotionPath? {
        if (batch.isEmpty()) return null
        val first = batch.first()
        val last = batch.last()
        val oldLayout = first.oldLayout
        val newLayout = last.newLayout

        // 第一步：按 batch 入队顺序取每笔 patch.cursorMotionPath?.points，保留真实 stage caret 顺序。
        val stagePoints = mutableListOf<CursorMotionPoint>()
        for (patch in batch) {
            val path = patch.cursorMotionPath ?: continue
            for (point in path.points) {
                stagePoints.add(point)
            }
        }

        // 第二步：相邻相同 rect 去重。
        val dedupedPoints = mutableListOf<CursorMotionPoint>()
        for (point in stagePoints) {
            if (dedupedPoints.isEmpty() || dedupedPoints.last().rect != point.rect) {
                dedupedPoints.add(point)
            }
        }

        // 第三步：最后一个点必须收敛到 batch.last().newLayout.selection 的最终 cursor rect。
        val finalCursorRect = safeCursorRectFromBatch(newLayout, last.newLayout.selection.end)
        if (finalCursorRect != null) {
            if (dedupedPoints.isEmpty()) {
                dedupedPoints.add(CursorMotionPoint(rect = finalCursorRect, endFraction = 1f))
            } else if (dedupedPoints.last().rect != finalCursorRect) {
                dedupedPoints.add(CursorMotionPoint(rect = finalCursorRect, endFraction = 1f))
            }
        }

        // 第四步：只有取不到任何 stage path 时，再回退现有 buildCursorPath()。
        if (dedupedPoints.isEmpty()) {
            return ComposeLocalVisualRebase.buildCursorPath(
                oldLayout = oldLayout,
                newLayout = newLayout,
                oldSelection = first.oldLayout.selection,
                newSelection = last.newLayout.selection,
                insertedUnits = insertedUnits,
                deletedUnits = deletedUnits,
            )
        }

        // 第五步：最后统一重新分配 endFraction = (i + 1) / n，
        // 与同一有界窗口里的文字 stage 对齐。
        val n = dedupedPoints.size
        val normalizedPoints =
            if (n <= 1) {
                dedupedPoints.map { it.copy(endFraction = 1f) }
            } else {
                dedupedPoints.mapIndexed { i, point ->
                    point.copy(endFraction = (i + 1f) / n)
                }
            }
        return CursorMotionPath(normalizedPoints)
    }

    /**
     * 安全取 cursor rect — offset 越界或 layout 抛异常时返回 null。
     */
    private fun safeCursorRectFromBatch(
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
     * 组合 batch 中各 patch 的 offset map（T0→T1→...→Tn）成 T0→Tn。
     */
    private fun composeBatchOffsetMap(
        batch: List<ComposeVisualPatch>,
        oldLength: Int,
    ): List<VisualOffsetMapEntry> {
        var acc: List<VisualOffsetMapEntry> =
            if (oldLength > 0) {
                listOf(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = oldLength,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            } else {
                emptyList()
            }
        for (patch in batch) {
            val stage = patchOffsetMapOrFallback(patch)
            acc = composeTwoMaps(acc, stage)
            if (acc.isEmpty()) break
        }
        return acc
    }

    /**
     * 取 patch 的 offsetMap；为 null 时从 oldText/newText 算 common prefix/suffix fallback。
     */
    private fun patchOffsetMapOrFallback(patch: ComposeVisualPatch): List<VisualOffsetMapEntry> {
        patch.offsetMap?.let { if (it.isNotEmpty()) return it }
        val oldText = patch.oldLayout.result.layoutInput.text.text
        val newText = patch.newLayout.result.layoutInput.text.text
        return buildFallbackOffsetMap(oldText, newText)
    }

    /**
     * 从 oldText/newText 算 common prefix/suffix 存活段 —
     * 与 [ComposeVisualRebase.buildFallbackEntriesFromReplaceBounds] 同语义。
     */
    private fun buildFallbackOffsetMap(
        oldText: String,
        newText: String,
    ): List<VisualOffsetMapEntry> {
        val oldLen = oldText.length
        val newLen = newText.length
        if (oldLen == 0 || newLen == 0) return emptyList()
        var prefix = 0
        while (prefix < oldLen && prefix < newLen && oldText[prefix] == newText[prefix]) prefix++
        var suffix = 0
        while (
            suffix < oldLen - prefix && suffix < newLen - prefix &&
            oldText[oldLen - 1 - suffix] == newText[newLen - 1 - suffix]
        ) {
            suffix++
        }
        val entries = mutableListOf<VisualOffsetMapEntry>()
        if (prefix > 0) {
            entries.add(
                VisualOffsetMapEntry(
                    oldStart = 0,
                    newStart = 0,
                    length = prefix,
                    kind = VisualOffsetMapKind.IDENTITY,
                ),
            )
        }
        if (suffix > 0) {
            entries.add(
                VisualOffsetMapEntry(
                    oldStart = oldLen - suffix,
                    newStart = newLen - suffix,
                    length = suffix,
                    kind = VisualOffsetMapKind.IDENTITY,
                ),
            )
        }
        return entries
    }

    /**
     * 组合两段 offset map（acc: T0→T_i, stage: T_i→T_{i+1}）成 T0→T_{i+1}。
     * 与 [ComposeVisualRebase.composeStage] 同算法。
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
}
