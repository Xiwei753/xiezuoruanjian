package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealGeometry
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import com.xiwei.sujian.feature.editor.visual.VisualProgressWindow
import uniffi.writer_core.RebaseSliceMappingDto

class RebasePlanner {
    /**
     * #639 评论 5424986783：rebase continuation 应按 [SliceVisualState.currentRect]
     * 与 [destinationRectOf] 的实际几何关系判断 slice 是否还有位置运动，而非按旧
     * [SliceRole] 判断。连续 rebase（Move → Insert/CrossfadeNew 后再次 rebase）时
     * 旧 role 已变成 Insert/CrossfadeNew，但 currentRect 仍在 from→destination 中间，
     * 必须继承 fromRect 才不跳变。这三个 helper 把几何判断收成一处，避免分散重复。
     */
    private fun currentRectOf(state: SliceVisualState): android.graphics.RectF =
        android.graphics.RectF(
            state.currentLeft,
            state.currentTop,
            state.currentRight,
            state.currentBottom,
        )

    private fun destinationRectOf(state: SliceVisualState): android.graphics.RectF =
        android.graphics.RectF(
            state.destinationLeft,
            state.destinationTop,
            state.destinationRight,
            state.destinationBottom,
        )

    /** currentRect != destinationRect → slice 仍有位置运动未走完。 */
    private fun hasRemainingPositionMotion(state: SliceVisualState): Boolean =
        currentRectOf(state) != destinationRectOf(state)

    /**
     * #606: 将 Core 计算的旧→新逻辑 slice 对应关系应用到新事务的 animated slices。
     *
     * 旧→新 slice 的逻辑对应关系由 Core（`compute_rebase_slice_mappings`）
     * 唯一计算并作为 [mappings] 传入（直接消费 `RebaseSliceMappingDto`，
     * 平台端不再维护本地副本）— 平台端不再使用任何本地匹配逻辑
     * （compatibleRebaseRoles / findRebaseIndexByClusterByteRange /
     * findRebaseIndexByLineAndRole / findRebaseIndexClosestByPosition 已删除）。
     * 本方法只负责：
     * 1. 对映射到的旧 slice，把旧帧当前 `RectF/alpha/revealFraction/Bitmap`
     *    填入新 slice 的 `fromDestinationRect/initialFraction/startAlpha`；
     * 2. 对 Core 无映射的旧 slice，按 Core 的继续/结束语义处理（仍在进行中的
     *    Delete/CrossfadeOld 继续吞完/淡出，Move/Insert/CrossfadeNew 继续原动画），
     *    这是旧帧动画状态的平台侧延续，不重新猜测逻辑对应。
     */
    fun applyRebaseToSlices(
        newSlices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
        mappings: List<RebaseSliceMappingDto> = emptyList(),
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        if (rebaseSnapshot.sliceVisualStates.isEmpty()) return newSlices

        val usedRebaseIndices = mutableSetOf<Int>()
        val result = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()

        for ((newIdx, slice) in newSlices.withIndex()) {
            val mapping = mappings.firstOrNull { it.newSliceIndex.toInt() == newIdx }
            val rebaseIdx = mapping?.oldSliceIndex?.toInt()
            if (rebaseIdx != null && rebaseIdx >= 0 && rebaseIdx < rebaseSnapshot.sliceVisualStates.size) {
                usedRebaseIndices.add(rebaseIdx)
                val rebaseState = rebaseSnapshot.sliceVisualStates[rebaseIdx]
                result.add(applyRebaseState(slice, rebaseState, snapshotLookup))
            } else {
                result.add(slice)
            }
        }

        for ((stateIdx, state) in rebaseSnapshot.sliceVisualStates.withIndex()) {
            if (stateIdx in usedRebaseIndices) continue
            handleUnmappedRebaseState(state, snapshotLookup, result)
        }

        return result
    }

    /**
     * #639 评论 5425871530 第四部分：处理未映射旧 slice 的 appearance continuation。
     *
     * 从 [applyRebaseToSlices] 提取，降低嵌套深度。按旧 slice 的 role 和当前外观状态
     * 决定是否继续动画，直接消费 [SliceVisualState.caretRevealGeometry] + revealFraction，
     * 不再反查 snapshot.clusters（synthetic run 也能继续）。
     */
    private fun handleUnmappedRebaseState(
        state: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot>,
        result: MutableList<PreparedVisualTransaction.AnimatedSlice>,
    ) {
        val isFadingOut = state.role == SliceRole.Delete || state.role == SliceRole.CrossfadeOld
        val shouldContinue =
            if (state.revealFraction != null) {
                state.revealFraction < 0.99f
            } else {
                state.currentAlpha > 0.01f
            }
        val snapshot = snapshotLookup[state.snapshotId]
        // matchedCluster 仅作向后兼容 fallback（旧快照没有 caretRevealGeometry 时从 snapshot 反查）。
        val matchedCluster =
            if (snapshot != null && state.caretRevealGeometry == null) {
                snapshot.clusters.firstOrNull {
                    it.documentByteStart == state.clusterByteStart &&
                        it.documentByteEndExclusive == state.clusterByteEndExclusive
                }
            } else {
                null
            }
        val sourceRect =
            matchedCluster?.sourceRectInLineImage
                ?: snapshot?.sourceRect
                ?: android.graphics.Rect(0, 0, 0, 0)
        val hasCaretGeometry = state.caretRevealGeometry != null || matchedCluster != null
        val shouldContinueInsertReveal =
            state.role == SliceRole.Insert &&
                state.revealFraction != null &&
                state.revealFraction < 0.99f &&
                hasCaretGeometry
        if (isFadingOut && shouldContinue) {
            result.add(buildFadingOutContinuation(state, snapshot, sourceRect, matchedCluster, hasCaretGeometry))
        } else if (state.role == SliceRole.Move) {
            buildMoveContinuation(state, snapshot, sourceRect)?.let { result.add(it) }
        } else if (shouldContinueInsertReveal) {
            result.add(buildInsertRevealContinuation(state, snapshot, sourceRect, matchedCluster))
        } else if (!isFadingOut && (state.currentAlpha < 0.99f || hasRemainingPositionMotion(state))) {
            result.add(buildAlphaOrPositionContinuation(state, snapshot, sourceRect))
        }
    }

    /** #605 评论3 + #639 评论 5425871530：未匹配 Delete/CrossfadeOld continuation。 */
    private fun buildFadingOutContinuation(
        state: SliceVisualState,
        snapshot: AndroidLineSnapshot?,
        sourceRect: android.graphics.Rect,
        matchedCluster: LineClusterSnapshot?,
        hasCaretGeometry: Boolean,
    ): PreparedVisualTransaction.AnimatedSlice {
        val continueRevealSpec =
            if (state.role == SliceRole.Delete && state.revealFraction != null && hasCaretGeometry) {
                val (caretStart, caretEnd) =
                    if (state.caretRevealGeometry != null) {
                        Pair(state.caretRevealGeometry.caretStartX, state.caretRevealGeometry.caretEndX)
                    } else {
                        Pair(matchedCluster!!.caretStartX, matchedCluster.caretEndX)
                    }
                TextRevealSpec(
                    mode = TextRevealMode.SWALLOW,
                    anchorX = caretStart,
                    boundaryFromX = caretEnd,
                    boundaryToX = caretStart,
                    progressStart = 0f,
                    progressEnd = 1f,
                    initialFraction = state.revealFraction,
                )
            } else {
                null
            }
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
        return PreparedVisualTransaction.AnimatedSlice(
            role = state.role,
            snapshot = snapshot,
            sourceRect = sourceRect,
            destinationRect =
                android.graphics.RectF(
                    state.currentLeft,
                    state.currentTop,
                    state.currentRight,
                    state.currentBottom,
                ),
            startAlpha = if (continueRevealSpec != null) 1f else state.currentAlpha,
            endAlpha = if (continueRevealSpec != null) 1f else 0f,
            clusterByteStart = state.clusterByteStart,
            clusterByteEndExclusive = state.clusterByteEndExclusive,
            revealSpec = continueRevealSpec,
            caretRevealGeometry = state.caretRevealGeometry,
            progressWindow = continuedWindow,
        )
    }

    /** #639 评论 5419182722：未匹配 Move continuation（同线 Move rebase 续播）。 */
    private fun buildMoveContinuation(
        state: SliceVisualState,
        snapshot: AndroidLineSnapshot?,
        sourceRect: android.graphics.Rect,
    ): PreparedVisualTransaction.AnimatedSlice? {
        val currentRect =
            android.graphics.RectF(
                state.currentLeft,
                state.currentTop,
                state.currentRight,
                state.currentBottom,
            )
        val destRect =
            android.graphics.RectF(
                state.destinationLeft,
                state.destinationTop,
                state.destinationRight,
                state.destinationBottom,
            )
        if (currentRect != destRect || state.currentAlpha < 0.99f) {
            val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
            return PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Move,
                snapshot = snapshot,
                sourceRect = sourceRect,
                destinationRect = destRect,
                startAlpha = state.currentAlpha,
                endAlpha = 1f,
                fromDestinationRect = currentRect,
                clusterByteStart = state.clusterByteStart,
                clusterByteEndExclusive = state.clusterByteEndExclusive,
                caretRevealGeometry = state.caretRevealGeometry,
                progressWindow = continuedWindow,
            )
        }
        return null
    }

    /** #639 评论 5422606865 + 5425871530：未匹配半截 Insert reveal continuation。 */
    private fun buildInsertRevealContinuation(
        state: SliceVisualState,
        snapshot: AndroidLineSnapshot?,
        sourceRect: android.graphics.Rect,
        matchedCluster: LineClusterSnapshot?,
    ): PreparedVisualTransaction.AnimatedSlice {
        val currentRect = currentRectOf(state)
        val destRect = destinationRectOf(state)
        // caret 几何优先用 state.caretRevealGeometry，fallback 用 matchedCluster。
        val visualRect: android.graphics.RectF
        val caretStartX: Float
        val caretEndX: Float
        val revealSourceRect: android.graphics.Rect
        if (state.caretRevealGeometry != null) {
            val g = state.caretRevealGeometry
            visualRect = g.visualRect
            caretStartX = g.caretStartX
            caretEndX = g.caretEndX
            revealSourceRect = sourceRect
        } else {
            visualRect = matchedCluster!!.visualRectInDocument
            caretStartX = matchedCluster.caretStartX
            caretEndX = matchedCluster.caretEndX
            revealSourceRect = matchedCluster.sourceRectInLineImage
        }
        val (shiftedAnchorX, shiftedBoundaryFromX, shiftedBoundaryToX) =
            TextRevealGeometry.shiftClusterCaretGeometry(
                visualRect,
                caretStartX,
                caretEndX,
                destRect,
                TextRevealMode.REVEAL,
            )
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Insert,
            snapshot = snapshot,
            sourceRect = revealSourceRect,
            destinationRect = destRect,
            startAlpha = 1f,
            endAlpha = 1f,
            fromDestinationRect = if (currentRect != destRect) currentRect else null,
            clusterByteStart = state.clusterByteStart,
            clusterByteEndExclusive = state.clusterByteEndExclusive,
            revealSpec =
                TextRevealSpec(
                    mode = TextRevealMode.REVEAL,
                    anchorX = shiftedAnchorX,
                    boundaryFromX = shiftedBoundaryFromX,
                    boundaryToX = shiftedBoundaryToX,
                    progressStart = 0f,
                    progressEnd = 1f,
                    initialFraction = state.revealFraction!!,
                ),
            caretRevealGeometry = state.caretRevealGeometry,
            progressWindow = continuedWindow,
        )
    }

    /** #639 评论 5424986783：未匹配 CrossfadeNew/其他非淡出 slice 的 alpha/位置续播。 */
    private fun buildAlphaOrPositionContinuation(
        state: SliceVisualState,
        snapshot: AndroidLineSnapshot?,
        sourceRect: android.graphics.Rect,
    ): PreparedVisualTransaction.AnimatedSlice {
        val currentRect =
            android.graphics.RectF(
                state.currentLeft,
                state.currentTop,
                state.currentRight,
                state.currentBottom,
            )
        val originalDestRect =
            android.graphics.RectF(
                state.destinationLeft,
                state.destinationTop,
                state.destinationRight,
                state.destinationBottom,
            )
        val endAlpha =
            when (state.role) {
                SliceRole.Insert, SliceRole.CrossfadeNew, SliceRole.Move -> 1f
                else -> state.currentAlpha
            }
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
        return PreparedVisualTransaction.AnimatedSlice(
            role = state.role,
            snapshot = snapshot,
            sourceRect = sourceRect,
            destinationRect = originalDestRect,
            startAlpha = state.currentAlpha,
            endAlpha = endAlpha,
            fromDestinationRect = if (currentRect != originalDestRect) currentRect else null,
            clusterByteStart = state.clusterByteStart,
            clusterByteEndExclusive = state.clusterByteEndExclusive,
            caretRevealGeometry = state.caretRevealGeometry,
            progressWindow = continuedWindow,
        )
    }

    fun applyRebaseState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
    ): PreparedVisualTransaction.AnimatedSlice {
        val snapshot = slice.snapshot ?: snapshotLookup[rebaseState.snapshotId]
        val fromRect =
            android.graphics.RectF(
                rebaseState.currentLeft,
                rebaseState.currentTop,
                rebaseState.currentRight,
                rebaseState.currentBottom,
            )
        // #637 评论 5386573878：rebase continuation 窗口 — 直接消费旧帧保存的
        // remainingFraction，不再从 localProgress 重新推，连续 rebase 不会反复减速。
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(rebaseState.remainingFraction)
        // #639 评论 5425871530 第四部分：统一做 appearance continuation，不再按 slice.role
        // 分支处理外观状态。逻辑按"旧屏幕当前外观"决定，而不是按旧 role：
        // - 旧 state 有 revealFraction：说明当前屏幕是裁到一半的字。无论新 role 是 Insert /
        //   Move / CrossfadeNew，都给新 slice 重建一个 REVEAL continuation，
        //   initialFraction = old revealFraction，geometry 用新 slice 自己的
        //   caretRevealGeometry，位置继续走 fromRect -> destinationRect。
        // - 旧 state 没有 revealFraction：说明当前不是 clip reveal。新 slice 不应凭空启动
        //   reveal。即使新 role 是 Insert，也要把 planner 原本的 revealSpec 清掉，继续
        //   startAlpha = old currentAlpha -> endAlpha = 1。
        return when (slice.role) {
            SliceRole.Static -> slice
            SliceRole.CrossfadeOld -> {
                // #639 评论 5421085782 问题2：CrossfadeOld 接到旧 Move/CrossfadeNew/Insert
                // 状态时，必须从当前屏幕真实位置退场。fromRect 就是
                // SliceVisualState.currentLeft/currentTop/currentRight/currentBottom，
                // 不退回上一笔事务的逻辑起点，也不用新事务的最终位置。
                // 旧 Insert 可能只 reveal 到一半，把当前裁剪状态用真实 caret reveal
                // 几何（cluster.caretStartX/caretEndX + revealFraction）算成
                // document-space clip rect 存入 fixedRevealClipRect，renderer 画
                // CrossfadeOld 时 clipRect(fixedRevealClipRect) 后画完整 bitmap，
                // 冻结当前可见部分，只让 alpha 变化。这与正常 Insert/Delete 的
                // computeRevealClipRect 共用同一份几何（TextRevealGeometry），
                // 字形 overhang 和 RTL 都自动正确，不再用 bitmap 宽度比例近似。
                if (rebaseState.role == SliceRole.Move ||
                    rebaseState.role == SliceRole.CrossfadeNew ||
                    rebaseState.role == SliceRole.Insert
                ) {
                    val fixedRevealClipRect =
                        computeFixedRevealClipRect(rebaseState, snapshotLookup, snapshot)
                    slice.copy(
                        snapshot = snapshot,
                        destinationRect = fromRect,
                        startAlpha = rebaseState.currentAlpha,
                        endAlpha = 0f,
                        fixedRevealClipRect = fixedRevealClipRect,
                        progressWindow = continuedWindow,
                    )
                } else {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        endAlpha = 0f,
                        progressWindow = continuedWindow,
                    )
                }
            }
            SliceRole.Delete -> {
                // #637 评论 5386573878：同 Insert，重建 spec 为 [0,1] 子窗口，
                // initialFraction=当前 revealFraction，剩余时长交给外层窗口控制。
                val updatedSpec =
                    slice.revealSpec?.let { spec ->
                        spec.copy(
                            progressStart = 0f,
                            progressEnd = 1f,
                            initialFraction = rebaseState.revealFraction ?: 0f,
                        )
                    }
                slice.copy(
                    snapshot = snapshot,
                    startAlpha = rebaseState.currentAlpha,
                    endAlpha = 0f,
                    revealSpec = updatedSpec,
                    progressWindow = continuedWindow,
                )
            }
            // #639 评论 5425871530 第四部分：Insert / Move / CrossfadeNew 统一做
            // appearance continuation，不再按新 role 分支处理。
            SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew -> {
                // #639 评论 5424986783：按几何判断而非旧 SliceRole 判断位置运动。
                // Move 保持原有行为：总是设置 fromDestinationRect = fromRect（即使
                // fromRect == destinationRect，visualDestinationRectAt 退化为常量
                // destinationRect，效果与 null 完全一致，但保持 RebaseSliceMappingTest
                // 的契约）。Insert/CrossfadeNew 用几何判断：有位置差时 = fromRect，
                // 否则 null（退化为纯 alpha 续播）。
                val fromDest =
                    if (slice.role == SliceRole.Move) {
                        fromRect
                    } else {
                        if (fromRect != slice.destinationRect) fromRect else null
                    }
                if (rebaseState.revealFraction != null) {
                    // 旧 state 有 revealFraction：重建 REVEAL continuation。
                    // geometry 优先用新 slice 自己的 caretRevealGeometry；fallback 用
                    // rebaseState.caretRevealGeometry（旧 state 携带的几何）；再 fallback
                    // 用 slice.destinationRect 构造简单几何（whole-line 或测试场景）。
                    val geometry = slice.caretRevealGeometry ?: rebaseState.caretRevealGeometry
                    val (visualRect, caretStartX, caretEndX) =
                        if (geometry != null) {
                            Triple(geometry.visualRect, geometry.caretStartX, geometry.caretEndX)
                        } else {
                            Triple(slice.destinationRect, slice.destinationRect.left, slice.destinationRect.right)
                        }
                    val (shiftedAnchorX, shiftedBoundaryFromX, shiftedBoundaryToX) =
                        TextRevealGeometry.shiftClusterCaretGeometry(
                            visualRect,
                            caretStartX,
                            caretEndX,
                            slice.destinationRect,
                            TextRevealMode.REVEAL,
                        )
                    val revealSpec =
                        TextRevealSpec(
                            mode = TextRevealMode.REVEAL,
                            anchorX = shiftedAnchorX,
                            boundaryFromX = shiftedBoundaryFromX,
                            boundaryToX = shiftedBoundaryToX,
                            progressStart = 0f,
                            progressEnd = 1f,
                            initialFraction = rebaseState.revealFraction,
                        )
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromDest,
                        revealSpec = revealSpec,
                        progressWindow = continuedWindow,
                    )
                } else {
                    // 旧 state 没有 revealFraction：新 slice 不应凭空启动 reveal。
                    // 即使新 role 是 Insert，也要把 planner 原本的 revealSpec 清掉，
                    // 继续 startAlpha = old currentAlpha -> endAlpha = 1。
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromDest,
                        revealSpec = null,
                        progressWindow = continuedWindow,
                    )
                }
            }
        }
    }

    /**
     * #639 评论 5421085782 问题2：为 CrossfadeOld 算 rebase 冻结的 document-space clip rect。
     *
     * 旧 Insert 只 reveal 到一半时，rebase 成 CrossfadeOld 不能把半个字突然变成完整字
     * 再淡出。用旧 snapshot 匹配 cluster 的 caretStartX/caretEndX + rebaseState.revealFraction
     * 经 [TextRevealGeometry.computeClusterRevealClipRect] 算出冻结的 clip rect。
     *
     * - 旧 snapshot 优先从 [snapshotLookup] 按 [rebaseState.snapshotId] 取，fallback
     *   用传入的 [fallbackSnapshot]（即新 slice 自己的 snapshot）。
     * - cluster 用 [rebaseState.clusterByteStart]/[rebaseState.clusterByteEndExclusive]
     *   在旧 snapshot 的 clusters 里匹配。
     * - destination 用旧 Insert 的 currentRect（[rebaseState.currentLeft/Top/Right/Bottom]），
     *   即当前屏幕真实位置。
     * - mode = REVEAL，与 CaretRevealPlanner Insert REVEAL spec 一致。
     *
     * #639 评论 5424613367 问题1：caret 几何来自旧 snapshot 的 visualRectInDocument，
     * 当前 destination 是 currentRect（旧 Insert 可能正在 from/destination 中间）。
     * 用 [TextRevealGeometry.computeClusterRevealClipRect] 统一平移，与未匹配 Insert
     * continuation 共用同一份平移公式。
     *
     * 返回 null 的条件：revealFraction 为 null（旧 slice 不是 reveal 动画）、
     * 找不到旧 snapshot、找不到匹配 cluster。此时 renderer 画完整 bitmap 不裁剪
     * （fallback 到旧 CrossfadeOld 行为）。
     */
    private fun computeFixedRevealClipRect(
        rebaseState: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot>,
        fallbackSnapshot: AndroidLineSnapshot?,
    ): android.graphics.RectF? {
        val fraction = rebaseState.revealFraction ?: return null
        val destination =
            android.graphics.RectF(
                rebaseState.currentLeft,
                rebaseState.currentTop,
                rebaseState.currentRight,
                rebaseState.currentBottom,
            )
        // #639 评论 5425871530 第四部分：caret 几何优先用 rebaseState.caretRevealGeometry
        // （slice 自带，不依赖 snapshot.clusters 反查 — synthetic run 也能继续），
        // fallback 用旧 snapshot 的 matchedCluster（向后兼容旧快照）。
        if (rebaseState.caretRevealGeometry != null) {
            val g = rebaseState.caretRevealGeometry
            return TextRevealGeometry.computeClusterRevealClipRect(
                g.visualRect,
                g.caretStartX,
                g.caretEndX,
                destination,
                TextRevealMode.REVEAL,
                fraction,
            )
        }
        val oldSnapshot = snapshotLookup[rebaseState.snapshotId] ?: fallbackSnapshot ?: return null
        val cluster =
            oldSnapshot.clusters.firstOrNull {
                it.documentByteStart == rebaseState.clusterByteStart &&
                    it.documentByteEndExclusive == rebaseState.clusterByteEndExclusive
            } ?: return null
        // #639 评论 5424613367 问题1：caret 几何来自旧 snapshot 的 visualRectInDocument，
        // 当前 destination 是 currentRect（旧 Insert 可能正在 from/destination 中间）。
        // 用 TextRevealGeometry.computeClusterRevealClipRect 统一平移，与未匹配 Insert
        // continuation 共用同一份平移公式。
        return TextRevealGeometry.computeClusterRevealClipRect(
            cluster.visualRectInDocument,
            cluster.caretStartX,
            cluster.caretEndX,
            destination,
            TextRevealMode.REVEAL,
            fraction,
        )
    }
}
