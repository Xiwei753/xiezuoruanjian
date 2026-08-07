package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.SliceRole
import com.xiwei.sujian.editor.v2.visual.SliceVisualState
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot

class RebasePlanner {
    fun applyRebaseToSlices(
        newSlices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
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

        for ((stateIdx, state) in rebaseSnapshot.sliceVisualStates.withIndex()) {
            if (stateIdx in usedRebaseIndices) continue
            val isFadingOut = state.role == SliceRole.Delete || state.role == SliceRole.CrossfadeOld
            val snapshot = snapshotLookup[state.snapshotId]
            val sourceRect =
                if (snapshot != null) {
                    val cluster =
                        snapshot.clusters.firstOrNull {
                            it.documentByteStart == state.clusterByteStart &&
                                it.documentByteEndExclusive == state.clusterByteEndExclusive
                        }
                    cluster?.sourceRectInLineImage ?: snapshot.sourceRect
                } else {
                    android.graphics.Rect(0, 0, 0, 0)
                }
            if (isFadingOut && state.currentAlpha > 0.01f) {
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
                        startAlpha = state.currentAlpha,
                        endAlpha = 0f,
                        clusterByteStart = state.clusterByteStart,
                        clusterByteEndExclusive = state.clusterByteEndExclusive,
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

    private fun findRebaseIndexByClusterByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int>,
    ): Int? {
        val sliceStart = slice.clusterByteStart
        val sliceEnd = slice.clusterByteEndExclusive
        if (sliceStart < 0 || sliceEnd < 0) return null

        return rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices &&
                compatibleRebaseRoles(
                    slice.role, rebaseSnapshot.sliceVisualStates[i].role,
                ) && rebaseSnapshot.sliceVisualStates[i].clusterByteStart == sliceStart &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteEndExclusive == sliceEnd
        }
    }

    fun compatibleRebaseRoles(role: SliceRole): Set<SliceRole> {
        return when (role) {
            SliceRole.Move -> setOf(SliceRole.Move, SliceRole.Insert, SliceRole.CrossfadeNew)
            SliceRole.Insert -> setOf(SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew)
            SliceRole.CrossfadeNew -> setOf(SliceRole.CrossfadeNew, SliceRole.Move, SliceRole.Insert)
            SliceRole.Delete -> setOf(SliceRole.Delete, SliceRole.CrossfadeOld)
            SliceRole.CrossfadeOld -> setOf(SliceRole.CrossfadeOld, SliceRole.Delete)
            SliceRole.Static -> setOf(SliceRole.Static)
        }
    }

    private fun compatibleRebaseRoles(
        newRole: SliceRole,
        rebaseRole: SliceRole,
    ): Boolean {
        return rebaseRole in compatibleRebaseRoles(newRole)
    }

    private fun findRebaseIndexByLineAndRole(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int>,
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
        usedRebaseIndices: Set<Int>,
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
            SliceRole.Delete -> {
                slice.copy(snapshot = snapshot, startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
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
