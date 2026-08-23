package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
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
                // #637 评论 5386066978 项2：continuation 窗口 — 旧帧已走 state.localProgress，
                // 新事务只播放剩余部分。
                val continuedWindow = VisualProgressWindow.Full.continued(state.localProgress)
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
                    val continuedWindow = VisualProgressWindow.Full.continued(state.localProgress)
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
                val continuedWindow = VisualProgressWindow.Full.continued(state.localProgress)
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
        // #637 评论 5386066978 项2：rebase continuation 窗口 — 旧帧已走
        // rebaseState.localProgress，新事务只播放剩余 1 - localProgress 部分，
        // 已走部分不重新计时。Full.continued 在 localProgress 为 0 或 1 时返回 Full。
        val continuedWindow = VisualProgressWindow.Full.continued(rebaseState.localProgress)
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
                val updatedSpec = slice.revealSpec?.copy(initialFraction = rebaseState.revealFraction ?: 0f)
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
                val updatedSpec = slice.revealSpec?.copy(initialFraction = rebaseState.revealFraction ?: 0f)
                slice.copy(
                    snapshot = snapshot,
                    startAlpha = rebaseState.currentAlpha,
                    endAlpha = 0f,
                    revealSpec = updatedSpec,
                    progressWindow = continuedWindow,
                )
            }
            SliceRole.CrossfadeOld -> {
                slice.copy(
                    snapshot = snapshot,
                    startAlpha = rebaseState.currentAlpha,
                    endAlpha = 0f,
                    progressWindow = continuedWindow,
                )
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
}
