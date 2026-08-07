package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot

class BlockShiftPlanner {
    fun applyRebaseToBlockShifts(
        newBlockShifts: List<PreparedVisualTransaction.BlockShift>,
        rebaseSnapshot: VisualFrameSnapshot,
        offsetMapper: ((Int) -> Int?)? = null,
        reverseMapper: ((Int) -> Int?)? = null,
    ): List<PreparedVisualTransaction.BlockShift> {
        if (rebaseSnapshot.blockShiftStates.isEmpty() || newBlockShifts.isEmpty()) return newBlockShifts
        val usedRebaseIndices = mutableSetOf<Int>()
        return newBlockShifts.map { shift ->
            if (shift.startUtf8 < 0) {
                val exactMatchIdx =
                    rebaseSnapshot.blockShiftStates.indices.firstOrNull { i ->
                        i !in usedRebaseIndices &&
                            rebaseSnapshot.blockShiftStates[i].startLineIndex == shift.startLineIndex &&
                            rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive == shift.endLineIndexExclusive
                    }
                if (exactMatchIdx != null) {
                    usedRebaseIndices.add(exactMatchIdx)
                    shift.copy(deltaY = shift.deltaY - rebaseSnapshot.blockShiftStates[exactMatchIdx].currentTranslateY)
                } else {
                    val matchIdx =
                        findBlockShiftRebaseByLineIndex(
                            shift,
                            rebaseSnapshot,
                            usedRebaseIndices,
                        )
                    if (matchIdx != null) {
                        usedRebaseIndices.add(matchIdx)
                        shift.copy(deltaY = shift.deltaY - rebaseSnapshot.blockShiftStates[matchIdx].currentTranslateY)
                    } else {
                        shift
                    }
                }
            } else {
                val matchIdx =
                    findBlockShiftRebaseMatch(
                        shift,
                        rebaseSnapshot,
                        usedRebaseIndices,
                        offsetMapper,
                        reverseMapper,
                    )
                if (matchIdx != null) {
                    usedRebaseIndices.add(matchIdx)
                    shift.copy(deltaY = shift.deltaY - rebaseSnapshot.blockShiftStates[matchIdx].currentTranslateY)
                } else {
                    shift
                }
            }
        }
    }

    private fun findBlockShiftRebaseMatch(
        shift: PreparedVisualTransaction.BlockShift,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: MutableSet<Int>,
        offsetMapper: ((Int) -> Int?)?,
        reverseMapper: ((Int) -> Int?)?,
    ): Int? {
        val candidates =
            rebaseSnapshot.blockShiftStates.indices.filter { i ->
                i !in usedRebaseIndices && rebaseSnapshot.blockShiftStates[i].startUtf8 >= 0
            }
        if (candidates.isEmpty()) return null

        offsetMapper?.let { mapper ->
            for (i in candidates) {
                val state = rebaseSnapshot.blockShiftStates[i]
                if (mapper(state.startUtf8) == shift.startUtf8) {
                    val endValidated =
                        if (shift.endUtf8Exclusive >= 0 && state.endUtf8Exclusive >= 0) {
                            val mappedEnd = mapper(state.endUtf8Exclusive)
                            mappedEnd != null && mappedEnd == shift.endUtf8Exclusive
                        } else {
                            true
                        }
                    if (endValidated) return i
                }
            }
            for (i in candidates) {
                val state = rebaseSnapshot.blockShiftStates[i]
                if (mapper(state.startUtf8) == shift.startUtf8) return i
            }
        }

        reverseMapper?.let { rMapper ->
            val mappedOldStart = rMapper(shift.startUtf8)
            if (mappedOldStart != null) {
                for (i in candidates) {
                    if (rebaseSnapshot.blockShiftStates[i].startUtf8 == mappedOldStart) return i
                }
            }
        }

        offsetMapper?.let { mapper ->
            val nearIdx =
                candidates.minByOrNull { i ->
                    val mapped = mapper(rebaseSnapshot.blockShiftStates[i].startUtf8)
                    if (mapped != null) {
                        kotlin.math.abs(mapped - shift.startUtf8)
                    } else {
                        Int.MAX_VALUE
                    }
                }
            if (nearIdx != null) {
                val mapped = mapper(rebaseSnapshot.blockShiftStates[nearIdx].startUtf8)
                val dist = if (mapped != null) kotlin.math.abs(mapped - shift.startUtf8) else Int.MAX_VALUE
                if (dist < 100) return nearIdx
            }
        }

        if (offsetMapper == null) {
            for (i in candidates) {
                if (rebaseSnapshot.blockShiftStates[i].startUtf8 == shift.startUtf8) return i
            }
        }

        val overlappingIndices =
            candidates.filter { i ->
                val state = rebaseSnapshot.blockShiftStates[i]
                state.startLineIndex < shift.endLineIndexExclusive &&
                    state.endLineIndexExclusive > shift.startLineIndex
            }
        if (overlappingIndices.isNotEmpty()) {
            return overlappingIndices.maxByOrNull { i ->
                val overlapStart = maxOf(rebaseSnapshot.blockShiftStates[i].startLineIndex, shift.startLineIndex)
                val overlapEnd =
                    minOf(rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive, shift.endLineIndexExclusive)
                overlapEnd - overlapStart
            }
        }

        return candidates.minByOrNull { i ->
            val state = rebaseSnapshot.blockShiftStates[i]
            val gap =
                if (state.endLineIndexExclusive <= shift.startLineIndex) {
                    shift.startLineIndex - state.endLineIndexExclusive
                } else {
                    state.startLineIndex - shift.endLineIndexExclusive
                }
            kotlin.math.abs(gap)
        }
    }

    private fun findBlockShiftRebaseByLineIndex(
        shift: PreparedVisualTransaction.BlockShift,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: MutableSet<Int>,
    ): Int? {
        val overlappingIndices =
            rebaseSnapshot.blockShiftStates.indices.filter { i ->
                i !in usedRebaseIndices &&
                    rebaseSnapshot.blockShiftStates[i].startLineIndex < shift.endLineIndexExclusive &&
                    rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive > shift.startLineIndex
            }
        if (overlappingIndices.isNotEmpty()) {
            return overlappingIndices.maxByOrNull { i ->
                val overlapStart = maxOf(rebaseSnapshot.blockShiftStates[i].startLineIndex, shift.startLineIndex)
                val overlapEnd =
                    minOf(rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive, shift.endLineIndexExclusive)
                overlapEnd - overlapStart
            }
        }
        return rebaseSnapshot.blockShiftStates.indices
            .filter { i -> i !in usedRebaseIndices }
            .minByOrNull { i ->
                val state = rebaseSnapshot.blockShiftStates[i]
                val gap =
                    if (state.endLineIndexExclusive <= shift.startLineIndex) {
                        shift.startLineIndex - state.endLineIndexExclusive
                    } else {
                        state.startLineIndex - shift.endLineIndexExclusive
                    }
                kotlin.math.abs(gap)
            }
    }
}
