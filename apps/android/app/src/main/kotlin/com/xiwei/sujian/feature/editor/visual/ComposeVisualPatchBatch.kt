package com.xiwei.sujian.feature.editor.visual

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

        val insertedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Insert, TextVisualKind.Move -> changedRanges.newRanges
                    TextVisualKind.Delete, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        val deletedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Delete, TextVisualKind.Move -> changedRanges.oldRanges
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

        // cursor 最终几何 target 只取最后 layout（不创建同 timestamp 的多条 position track）
        val cursorMotionPath = last.cursorMotionPath

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
