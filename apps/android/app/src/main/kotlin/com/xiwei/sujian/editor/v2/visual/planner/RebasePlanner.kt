package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot
import com.xiwei.sujian.editor.v2.visual.SliceVisualState
import com.xiwei.sujian.editor.v2.visual.SliceRole
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

class RebasePlanner {

    fun applyRebaseToSlices(
        newSlices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot>
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        if (rebaseSnapshot.sliceVisualStates.isEmpty()) return newSlices

        val usedRebaseIndices = mutableSetOf<Int>()
        val result = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()

        for (slice in newSlices) {
            var rebaseIdx = findRebaseIndexByClusterByteRange(slice, rebaseSnapshot, usedRebaseIndices)
            if (rebaseIdx == null) {
                rebaseIdx = findRebaseIndexByLineAndRole(slice, rebaseSnapshot, usedRebaseIndices)
            }
            if (rebaseIdx == null) {
                rebaseIdx = findRebaseIndexClosestByPosition(slice, rebaseSnapshot, usedRebaseIndices)
            }

            if (rebaseIdx != null) {
                usedRebaseIndices.add(rebaseIdx)
                val rebaseState = rebaseSnapshot.sliceVisualStates[rebaseIdx]
                result.add(applyRebaseState(slice, rebaseState, snapshotLookup))
            } else {
                result.add(slice)
            }
        }

        for ((idx, state) in rebaseSnapshot.sliceVisualStates.withIndex()) {
            if (idx in usedRebaseIndices) continue
            val snapshot = snapshotLookup[state.snapshotId]
            if (snapshot != null) {
                result.add(PreparedVisualTransaction.AnimatedSlice(
                    role = state.role,
                    snapshot = snapshot,
                    sourceRect = snapshot.sourceRect,
                    destinationRect = android.graphics.RectF(
                        state.destinationLeft, state.destinationTop,
                        state.destinationRight, state.destinationBottom
                    ),
                    startAlpha = state.currentAlpha,
                    endAlpha = if (state.role == SliceRole.Delete || state.role == SliceRole.CrossfadeOld) 0f else 1f,
                    fromDestinationRect = android.graphics.RectF(
                        state.currentLeft, state.currentTop,
                        state.currentRight, state.currentBottom
                    ),
                    clusterByteStart = state.clusterByteStart,
                    clusterByteEndExclusive = state.clusterByteEndExclusive
                ))
            }
        }

        return result
    }

    private fun findRebaseIndexByClusterByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int>
    ): Int? {
        val sliceStart = slice.clusterByteStart
        val sliceEnd = slice.clusterByteEndExclusive
        if (sliceStart < 0 || sliceEnd < 0) return null

        return rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices && compatibleRebaseRoles(
                slice.role, rebaseSnapshot.sliceVisualStates[i].role
            ) && rebaseSnapshot.sliceVisualStates[i].clusterByteStart == sliceStart &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteEndExclusive == sliceEnd
        }
    }

    private fun compatibleRebaseRoles(newRole: SliceRole, rebaseRole: SliceRole): Boolean {
        val appearing = setOf(SliceRole.Insert, SliceRole.CrossfadeNew, SliceRole.Move)
        val disappearing = setOf(SliceRole.Delete, SliceRole.CrossfadeOld)
        return when (newRole) {
            in appearing -> rebaseRole in appearing
            in disappearing -> rebaseRole in disappearing
            else -> newRole == rebaseRole
        }
    }

    private fun findRebaseIndexByLineAndRole(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int>
    ): Int? {
        val sliceLine = slice.snapshot?.lineIndex ?: return null
        val sliceStart = slice.clusterByteStart

        return rebaseSnapshot.sliceVisualStates.indices
            .filter { i ->
                i !in usedRebaseIndices &&
                    compatibleRebaseRoles(slice.role, rebaseSnapshot.sliceVisualStates[i].role) &&
                    rebaseSnapshot.sliceVisualStates[i].lineIndex == sliceLine
            }
            .minByOrNull { i ->
                val rebaseStart = rebaseSnapshot.sliceVisualStates[i].clusterByteStart
                if (sliceStart >= 0 && rebaseStart >= 0) kotlin.math.abs(rebaseStart - sliceStart) else Int.MAX_VALUE
            }
    }

    private fun findRebaseIndexClosestByPosition(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int>
    ): Int? {
        val destRect = slice.destinationRect
        return rebaseSnapshot.sliceVisualStates.indices
            .filter { i ->
                i !in usedRebaseIndices &&
                    compatibleRebaseRoles(slice.role, rebaseSnapshot.sliceVisualStates[i].role)
            }
            .minByOrNull { i ->
                val state = rebaseSnapshot.sliceVisualStates[i]
                val dx = kotlin.math.abs(state.destinationLeft - destRect.left)
                val dy = kotlin.math.abs(state.destinationTop - destRect.top)
                dx + dy
            }
    }

    private fun applyRebaseState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot>
    ): PreparedVisualTransaction.AnimatedSlice {
        val snapshot = slice.snapshot ?: snapshotLookup[rebaseState.snapshotId]
        return slice.copy(
            snapshot = snapshot,
            fromDestinationRect = android.graphics.RectF(
                rebaseState.currentLeft, rebaseState.currentTop,
                rebaseState.currentRight, rebaseState.currentBottom
            ),
            startAlpha = rebaseState.currentAlpha
        )
    }
}
