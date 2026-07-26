package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.SliceRole

class SnapshotPlanner {

    fun createSnapshotFromRevision(
        revision: AndroidLayoutRevision,
        lineIndex: Int,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        isNewRevision: Boolean = false
    ): AndroidLineSnapshot? {
        return if (isNewRevision) {
            preCapturedNewSnapshots[lineIndex]
        } else {
            preCapturedOldSnapshots[lineIndex]
        }
    }

    fun buildSelectionDecoration(
        newRev: AndroidLayoutRevision
    ): PreparedVisualTransaction.SelectionDecoration? {
        if (newRev.selectionStartUtf16 < 0 || newRev.selectionEndUtf16 < 0) return null
        if (newRev.selectionStartUtf16 == newRev.selectionEndUtf16) return null
        return PreparedVisualTransaction.SelectionDecoration(
            startUtf16 = newRev.selectionStartUtf16,
            endUtf16 = newRev.selectionEndUtf16
        )
    }

    fun buildPreeditDecoration(
        newRev: AndroidLayoutRevision
    ): PreparedVisualTransaction.PreeditDecoration? {
        if (newRev.preeditStartUtf16 < 0 || newRev.preeditEndUtf16 < 0) return null
        if (newRev.preeditStartUtf16 == newRev.preeditEndUtf16) return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = newRev.preeditStartUtf16,
            endUtf16 = newRev.preeditEndUtf16,
            underlineColor = newRev.preeditUnderlineColor
        )
    }

    fun collectExcludedNewByteRanges(
        animatedSlices: List<PreparedVisualTransaction.AnimatedSlice>
    ): Set<Pair<Int, Int>> {
        return animatedSlices
            .filter { it.role == SliceRole.Insert || it.role == SliceRole.CrossfadeNew || it.role == SliceRole.Move }
            .mapNotNull { slice ->
                val start = slice.clusterByteStart
                val end = slice.clusterByteEndExclusive
                if (start >= 0 && end > start) Pair(start, end) else null
            }
            .toSet()
    }

    fun collectExcludedOldByteRanges(
        animatedSlices: List<PreparedVisualTransaction.AnimatedSlice>
    ): Set<Pair<Int, Int>> {
        return animatedSlices
            .filter { it.role == SliceRole.Delete || it.role == SliceRole.CrossfadeOld }
            .mapNotNull { slice ->
                val start = slice.clusterByteStart
                val end = slice.clusterByteEndExclusive
                if (start >= 0 && end > start) Pair(start, end) else null
            }
            .toSet()
    }
}
