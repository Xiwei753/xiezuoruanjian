package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot
import com.xiwei.sujian.editor.v2.visual.BlockShiftVisualState

class BlockShiftPlanner {

    fun applyRebaseToBlockShifts(
        blockShifts: List<PreparedVisualTransaction.BlockShift>,
        rebaseSnapshot: VisualFrameSnapshot,
        offsetMapper: ((Int) -> Int?)?,
        reverseMapper: ((Int) -> Int?)?
    ): List<PreparedVisualTransaction.BlockShift> {
        if (rebaseSnapshot.blockShiftStates.isEmpty()) return blockShifts
        return blockShifts.map { shift ->
            val match = findBlockShiftRebaseMatch(shift, rebaseSnapshot, offsetMapper, reverseMapper)
            if (match != null) {
                val adjustedDeltaY = shift.deltaY - match.currentTranslateY
                shift.copy(deltaY = adjustedDeltaY)
            } else {
                shift
            }
        }
    }

    private fun findBlockShiftRebaseMatch(
        shift: PreparedVisualTransaction.BlockShift,
        rebaseSnapshot: VisualFrameSnapshot,
        offsetMapper: ((Int) -> Int?)?,
        reverseMapper: ((Int) -> Int?)?
    ): BlockShiftVisualState? {
        if (offsetMapper != null && shift.startUtf8 >= 0) {
            val mappedStart = offsetMapper(shift.startUtf8)
            if (mappedStart != null) {
                val match = rebaseSnapshot.blockShiftStates.firstOrNull { bs ->
                    bs.startUtf8 == mappedStart
                }
                if (match != null) return match
            }
        }
        if (reverseMapper != null) {
            for (bs in rebaseSnapshot.blockShiftStates) {
                val reverseMapped = reverseMapper(bs.startUtf8)
                if (reverseMapped != null && reverseMapped == shift.startUtf8) {
                    return bs
                }
            }
        }
        for (bs in rebaseSnapshot.blockShiftStates) {
            if (bs.startUtf8 == shift.startUtf8 && bs.startUtf8 >= 0) {
                return bs
            }
        }
        if (shift.startUtf8 < 0) {
            return findBlockShiftRebaseByLineIndex(shift, rebaseSnapshot)
        }
        return null
    }

    private fun findBlockShiftRebaseByLineIndex(
        shift: PreparedVisualTransaction.BlockShift,
        rebaseSnapshot: VisualFrameSnapshot
    ): BlockShiftVisualState? {
        return rebaseSnapshot.blockShiftStates.firstOrNull { bs ->
            bs.startLineIndex == shift.startLineIndex
        }
    }
}
