package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
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
            val isFadingOut = state.role == SliceRole.Delete || state.role == SliceRole.CrossfadeOld
            val shouldContinue =
                if (state.revealFraction != null) {
                    state.revealFraction < 0.99f
                } else {
                    state.currentAlpha > 0.01f
                }
            val snapshot = snapshotLookup[state.snapshotId]
            val matchedCluster =
                if (snapshot != null) {
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
            if (isFadingOut && shouldContinue) {
                // #605 评论3: 正在 SWALLOW 中的未匹配 Delete slice 必须继续用 clip swallow 吞完，
                // 不能退回 alpha 淡出（那正是 #605 要消除的视觉语义）。只有当无法从 snapshot
                // 重建 cluster caret 几何时才回退 alpha（向后兼容无 cluster 数据的旧快照）。
                val continueRevealSpec =
                    if (state.role == SliceRole.Delete && state.revealFraction != null && matchedCluster != null) {
                        TextRevealSpec(
                            mode = TextRevealMode.SWALLOW,
                            anchorX = matchedCluster.caretStartX,
                            boundaryFromX = matchedCluster.caretEndX,
                            boundaryToX = matchedCluster.caretStartX,
                            progressStart = 0f,
                            progressEnd = 1f,
                            initialFraction = state.revealFraction,
                        )
                    } else {
                        null
                    }
                // #637 评论 5386573878：continuation 窗口 — 直接消费旧帧保存的
                // remainingFraction（当前帧之后还剩多少基准时长），不再从 localProgress
                // 重新推，连续 rebase 不会反复减速。
                val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
                result.add(
                    PreparedVisualTransaction.AnimatedSlice(
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
                        progressWindow = continuedWindow,
                    ),
                )
            } else if (state.role == SliceRole.Move) {
                // #639 评论 5419182722：跨视觉行的保留字符已不再生成 SliceRole.Move
                // （MoveCrossfadePlanner.appendRetainedTransition 跨行落成
                // CrossfadeOld+CrossfadeNew）。此分支只处理同线 Move 的 rebase 续播，
                // 不会把上一帧跨行 Move 映射成下一帧新跨行 Move。跨行字符的 continuation
                // 走 CrossfadeOld/CrossfadeNew 分支，只续 alpha，位置钉死在布局坐标。
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
                    result.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.Move,
                            snapshot = snapshot,
                            sourceRect = sourceRect,
                            destinationRect = destRect,
                            startAlpha = state.currentAlpha,
                            endAlpha = 1f,
                            fromDestinationRect = currentRect,
                            clusterByteStart = state.clusterByteStart,
                            clusterByteEndExclusive = state.clusterByteEndExclusive,
                            progressWindow = continuedWindow,
                        ),
                    )
                }
            } else if (!isFadingOut && state.currentAlpha < 0.99f) {
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
                result.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = state.role,
                        snapshot = snapshot,
                        sourceRect = sourceRect,
                        destinationRect = originalDestRect,
                        startAlpha = state.currentAlpha,
                        endAlpha = endAlpha,
                        fromDestinationRect = currentRect,
                        clusterByteStart = state.clusterByteStart,
                        clusterByteEndExclusive = state.clusterByteEndExclusive,
                        progressWindow = continuedWindow,
                    ),
                )
            }
        }

        return result
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
        return when (slice.role) {
            SliceRole.Move -> {
                slice.copy(
                    snapshot = snapshot,
                    fromDestinationRect = fromRect,
                    startAlpha = rebaseState.currentAlpha,
                    progressWindow = continuedWindow,
                )
            }
            SliceRole.Insert -> {
                // #637 评论 5386573878：映射成功的 Insert continuation 重建 spec 为
                // progressStart=0/progressEnd=1/initialFraction=当前 revealFraction。
                // 多 cluster/run 的 reveal 本来可能有非 [0,1] 子窗口；rebase 后外层
                // progress 从 0 重新开始，继续沿用旧 progressStart 会先停一段再继续。
                // 剩余时长交给外层 VisualProgressWindow 控制，与未映射 Delete 统一。
                val updatedSpec =
                    slice.revealSpec?.let { spec ->
                        spec.copy(
                            progressStart = 0f,
                            progressEnd = 1f,
                            initialFraction = rebaseState.revealFraction ?: 0f,
                        )
                    }
                if (rebaseState.role == SliceRole.Move) {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromRect,
                        revealSpec = updatedSpec,
                        progressWindow = continuedWindow,
                    )
                } else {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        revealSpec = updatedSpec,
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
            SliceRole.CrossfadeNew -> {
                if (rebaseState.role == SliceRole.Move) {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromRect,
                        progressWindow = continuedWindow,
                    )
                } else {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        progressWindow = continuedWindow,
                    )
                }
            }
            SliceRole.Static -> slice
        }
    }

    /**
     * #639 评论 5421085782 问题2：为 CrossfadeOld 算 rebase 冻结的 document-space clip rect。
     *
     * 旧 Insert 只 reveal 到一半时，rebase 成 CrossfadeOld 不能把半个字突然变成完整字
     * 再淡出。用旧 snapshot 匹配 cluster 的 caretStartX/caretEndX + rebaseState.revealFraction
     * 经 [TextRevealGeometry.computeRevealClipRect] 算出冻结的 clip rect。
     *
     * - 旧 snapshot 优先从 [snapshotLookup] 按 [rebaseState.snapshotId] 取，fallback
     *   用传入的 [fallbackSnapshot]（即新 slice 自己的 snapshot）。
     * - cluster 用 [rebaseState.clusterByteStart]/[rebaseState.clusterByteEndExclusive]
     *   在旧 snapshot 的 clusters 里匹配。
     * - destination 用旧 Insert 的 currentRect（[rebaseState.currentLeft/Top/Right/Bottom]），
     *   即当前屏幕真实位置。
     * - mode = REVEAL，anchorX = cluster.caretStartX，boundaryFromX = cluster.caretStartX，
     *   boundaryToX = cluster.caretEndX — 与 CaretRevealPlanner Insert REVEAL spec 一致。
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
        val oldSnapshot = snapshotLookup[rebaseState.snapshotId] ?: fallbackSnapshot ?: return null
        val cluster =
            oldSnapshot.clusters.firstOrNull {
                it.documentByteStart == rebaseState.clusterByteStart &&
                    it.documentByteEndExclusive == rebaseState.clusterByteEndExclusive
            } ?: return null
        val destination =
            android.graphics.RectF(
                rebaseState.currentLeft,
                rebaseState.currentTop,
                rebaseState.currentRight,
                rebaseState.currentBottom,
            )
        return TextRevealGeometry.computeRevealClipRect(
            destination,
            TextRevealMode.REVEAL,
            cluster.caretStartX,
            cluster.caretStartX,
            cluster.caretEndX,
            fraction,
        )
    }
}
