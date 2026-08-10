package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.RebaseContinuation
import com.xiwei.sujian.feature.editor.visual.RebaseReason
import com.xiwei.sujian.feature.editor.visual.RebaseSliceMapping
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceRoleAndByteRange
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot

class RebasePlanner {
    /**
     * #606: 计算旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
     *
     * 与 Core `compute_rebase_slice_mappings` 逻辑完全一致 — 平台无关的
     * 唯一事实来源。Android 不再使用 compatibleRebaseRoles/findRebaseIndexByClusterByteRange/
     * findRebaseIndexByLineAndRole/findRebaseIndexClosestByPosition 做启发式匹配，
     * 只按 byte range 精确匹配 + 角色兼容判定。
     *
     * 匹配规则（按优先级）：
     * 1. 旧/新 slice 的 byte range 完全相同 + 角色兼容 → SameByteRange + Continue
     * 2. 其余旧 slice 不生成映射（平台端按 End 处理）
     *
     * 每个新 slice 至多被一个旧 slice 匹配（usedNew 去重），
     * 避免多旧 slice 接续同一新 slice 造成 progress 抢占。
     */
    fun computeRebaseSliceMappings(
        oldSlices: List<SliceRoleAndByteRange>,
        newSlices: List<SliceRoleAndByteRange>,
    ): List<RebaseSliceMapping> {
        val mappings = mutableListOf<RebaseSliceMapping>()
        val usedNew = mutableSetOf<Int>()
        for ((oldIdx, oldSlice) in oldSlices.withIndex()) {
            for ((newIdx, newSlice) in newSlices.withIndex()) {
                if (newIdx in usedNew) continue
                if (
                    compatibleRebaseRolesInternal(newSlice.role, oldSlice.role) &&
                    oldSlice.byteStart == newSlice.byteStart &&
                    oldSlice.byteEndExclusive == newSlice.byteEndExclusive
                ) {
                    mappings.add(
                        RebaseSliceMapping(
                            oldSliceIndex = oldIdx,
                            newSliceIndex = newIdx,
                            continuation = RebaseContinuation.Continue,
                            reason = RebaseReason.SameByteRange,
                        ),
                    )
                    usedNew.add(newIdx)
                    break
                }
            }
        }
        return mappings
    }

    /**
     * #606: 角色兼容判定 — 与 Core `compatible_rebase_roles` 完全一致。
     *
     * Move/Insert/CrossfadeNew 互相兼容（都是"新出现的文字"动画）；
     * Delete/CrossfadeOld 互相兼容（都是"消失的文字"动画）；
     * 其余组合不兼容（Insert 与 Delete 不能接续，Move 与 CrossfadeOld 不能接续）。
     * Static 不参与 rebase。
     */
    private fun compatibleRebaseRolesInternal(
        newRole: SliceRole,
        oldRole: SliceRole,
    ): Boolean {
        return when (Pair(newRole, oldRole)) {
            Pair(SliceRole.Move, SliceRole.Move),
            Pair(SliceRole.Move, SliceRole.Insert),
            Pair(SliceRole.Move, SliceRole.CrossfadeNew),
            Pair(SliceRole.Insert, SliceRole.Move),
            Pair(SliceRole.Insert, SliceRole.Insert),
            Pair(SliceRole.Insert, SliceRole.CrossfadeNew),
            Pair(SliceRole.CrossfadeNew, SliceRole.CrossfadeNew),
            Pair(SliceRole.CrossfadeNew, SliceRole.Move),
            Pair(SliceRole.CrossfadeNew, SliceRole.Insert),
            Pair(SliceRole.Delete, SliceRole.Delete),
            Pair(SliceRole.Delete, SliceRole.CrossfadeOld),
            Pair(SliceRole.CrossfadeOld, SliceRole.CrossfadeOld),
            Pair(SliceRole.CrossfadeOld, SliceRole.Delete),
            -> true
            else -> false
        }
    }

    /**
     * #606: 将 rebase snapshot 的旧帧视觉状态应用到新事务的 animated slices。
     *
     * 旧→新 slice 的逻辑对应关系由 [computeRebaseSliceMappings] 唯一决定，
     * 平台端不再使用启发式匹配（compatibleRebaseRoles public 版本 /
     * findRebaseIndexByClusterByteRange / findRebaseIndexByLineAndRole /
     * findRebaseIndexClosestByPosition 已删除）。
     *
     * 签名保持不变 — 调用方无需修改。
     */
    fun applyRebaseToSlices(
        newSlices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        if (rebaseSnapshot.sliceVisualStates.isEmpty()) return newSlices

        // #606: 用 computeRebaseSliceMappings 计算旧→新对应关系，
        // 替代旧的 findRebaseIndexByClusterByteRange/findRebaseIndexByLineAndRole/
        // findRebaseIndexClosestByPosition 启发式匹配。
        val oldSlices =
            rebaseSnapshot.sliceVisualStates.map { state ->
                SliceRoleAndByteRange(
                    role = state.role,
                    byteStart = state.clusterByteStart,
                    byteEndExclusive = state.clusterByteEndExclusive,
                )
            }
        val newSlicesForMapping =
            newSlices.map { slice ->
                SliceRoleAndByteRange(
                    role = slice.role,
                    byteStart = slice.clusterByteStart,
                    byteEndExclusive = slice.clusterByteEndExclusive,
                )
            }
        val mappings = computeRebaseSliceMappings(oldSlices, newSlicesForMapping)

        val usedRebaseIndices = mutableSetOf<Int>()
        val result = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()

        for ((newIdx, slice) in newSlices.withIndex()) {
            val mapping = mappings.firstOrNull { it.newSliceIndex == newIdx }
            val rebaseIdx = mapping?.oldSliceIndex
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
        return when (slice.role) {
            SliceRole.Move -> {
                slice.copy(
                    snapshot = snapshot,
                    fromDestinationRect = fromRect,
                    startAlpha = rebaseState.currentAlpha,
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
                    )
                } else {
                    slice.copy(snapshot = snapshot, startAlpha = rebaseState.currentAlpha, revealSpec = updatedSpec)
                }
            }
            SliceRole.Delete -> {
                val updatedSpec = slice.revealSpec?.copy(initialFraction = rebaseState.revealFraction ?: 0f)
                slice.copy(
                    snapshot = snapshot,
                    startAlpha = rebaseState.currentAlpha,
                    endAlpha = 0f,
                    revealSpec = updatedSpec,
                )
            }
            SliceRole.CrossfadeOld -> {
                slice.copy(snapshot = snapshot, startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
            }
            SliceRole.CrossfadeNew -> {
                if (rebaseState.role == SliceRole.Move) {
                    slice.copy(
                        snapshot = snapshot,
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromRect,
                    )
                } else {
                    slice.copy(snapshot = snapshot, startAlpha = rebaseState.currentAlpha)
                }
            }
            SliceRole.Static -> slice
        }
    }
}
