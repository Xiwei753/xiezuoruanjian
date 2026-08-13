package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole

class SnapshotPlanner {
    fun createSnapshotFromRevision(
        revision: LayoutRevisionSource,
        lineIndex: Int,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        isNewRevision: Boolean = false,
    ): AndroidLineSnapshot? {
        return if (isNewRevision) {
            preCapturedNewSnapshots[lineIndex]
        } else {
            preCapturedOldSnapshots[lineIndex]
        }
    }

    fun buildSelectionDecoration(newRev: LayoutRevisionSource): PreparedVisualTransaction.SelectionDecoration? {
        if (newRev.selectionStartUtf16 < 0 || newRev.selectionEndUtf16 < 0) return null
        if (newRev.selectionStartUtf16 == newRev.selectionEndUtf16) return null
        return PreparedVisualTransaction.SelectionDecoration(
            startUtf16 = newRev.selectionStartUtf16,
            endUtf16 = newRev.selectionEndUtf16,
        )
    }

    fun buildPreeditDecoration(newRev: LayoutRevisionSource): PreparedVisualTransaction.PreeditDecoration? {
        if (newRev.compositionStartUtf16 < 0 || newRev.compositionEndUtf16 < 0) return null
        if (newRev.compositionStartUtf16 == newRev.compositionEndUtf16) return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = newRev.compositionStartUtf16,
            endUtf16 = newRev.compositionEndUtf16,
            underlineColor = 0xFF000000.toInt(),
        )
    }

    fun collectExcludedNewByteRanges(
        animatedSlices: List<PreparedVisualTransaction.AnimatedSlice>,
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
        animatedSlices: List<PreparedVisualTransaction.AnimatedSlice>,
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
