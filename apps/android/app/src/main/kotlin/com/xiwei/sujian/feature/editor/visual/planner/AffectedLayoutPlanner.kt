package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction

class AffectedLayoutPlanner {
    private companion object {
        const val STABLE_SUFFIX_GEOMETRY_TOLERANCE = 1.0f
        const val BLOCK_SHIFT_DELTA_Y_EPSILON = 0.5f
    }

    data class AffectedLinesResult(
        val lineIndices: Set<Int>,
        val oldLineIndices: Set<Int>,
        val newLineIndices: Set<Int>,
        val blockShifts: List<PreparedVisualTransaction.BlockShift>,
    )

    internal data class ParagraphRange(
        val paragraphId: Int,
        val startUtf8: Int,
        val endUtf8Exclusive: Int,
        val top: Float,
    )

    fun computeAffectedLineIndices(
        visualIntent: VisualIntent,
        revision: AndroidLayoutRevision?,
        useNewRanges: Boolean = false,
    ): Set<Int> {
        if (revision == null) return emptySet()
        val affectedLines = mutableSetOf<Int>()
        val primaryRanges = if (useNewRanges) visualIntent.newAffectedByteRanges else visualIntent.oldAffectedByteRanges
        val fallbackRanges =
            if (useNewRanges) visualIntent.oldAffectedByteRanges else visualIntent.newAffectedByteRanges
        for ((start, end) in primaryRanges) {
            for (i in revision.lineRanges.indices) {
                val lineRange = revision.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }
        val editByteStart =
            primaryRanges.firstOrNull()?.first
                ?: fallbackRanges.firstOrNull()?.first
        if (editByteStart != null) {
            val editLine = findLineForUtf8(revision, editByteStart)
            val editParagraphId = revision.lineRanges.getOrNull(editLine)?.paragraphId ?: 0
            for (i in editLine downTo 0) {
                if (revision.lineRanges.getOrNull(i)?.paragraphId != editParagraphId) break
                affectedLines.add(i)
            }
            for (i in editLine until revision.lineRanges.size) {
                if (revision.lineRanges.getOrNull(i)?.paragraphId != editParagraphId) break
                affectedLines.add(i)
            }
            if (visualIntent.isDeleteOrReplaceRenderRole()) {
                val lastEditLine = affectedLines.maxOrNull() ?: editLine
                val nextParaStartLine = findNextParagraphStartLine(revision, lastEditLine)
                if (nextParaStartLine != null) {
                    val nextParaId = revision.lineRanges.getOrNull(nextParaStartLine)?.paragraphId ?: -1
                    for (i in nextParaStartLine until revision.lineRanges.size) {
                        if (revision.lineRanges.getOrNull(i)?.paragraphId != nextParaId) break
                        affectedLines.add(i)
                    }
                }
            }
        }
        return affectedLines
    }

    internal fun findNextParagraphStartLine(
        revision: AndroidLayoutRevision,
        afterLine: Int,
    ): Int? {
        val currentParaId = revision.lineRanges.getOrNull(afterLine)?.paragraphId ?: return null
        for (i in (afterLine + 1) until revision.lineRanges.size) {
            if (revision.lineRanges[i].paragraphId != currentParaId) return i
        }
        return null
    }

    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
    ): AffectedLinesResult {
        if (oldRevision == null || newRevision == null) {
            val revision =
                newRevision ?: oldRevision
                    ?: return AffectedLinesResult(emptySet(), emptySet(), emptySet(), emptyList())
            val useNewRanges = newRevision != null
            val indices = computeAffectedLineIndices(visualIntent, revision, useNewRanges = useNewRanges)
            if (newRevision == null) {
                val structuralIndices = computeStructurallyAffectedOldLineIndices(visualIntent, oldRevision!!)
                val combined = indices + structuralIndices
                return AffectedLinesResult(
                    lineIndices = emptySet(),
                    oldLineIndices = combined,
                    newLineIndices = emptySet(),
                    blockShifts = emptyList(),
                )
            }
            return AffectedLinesResult(
                lineIndices = emptySet(),
                oldLineIndices = emptySet(),
                newLineIndices = indices,
                blockShifts = emptyList(),
            )
        }
        return computeAffectedLines(visualIntent, oldRevision, newRevision)
    }

    fun computeStructurallyAffectedOldLineIndices(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision,
    ): Set<Int> {
        val affectedLines = mutableSetOf<Int>()
        val affectedParaIds = mutableSetOf<Int>()

        for ((start, end) in visualIntent.oldAffectedByteRanges) {
            for (i in oldRevision.lineRanges.indices) {
                val lineRange = oldRevision.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedParaIds.add(lineRange.paragraphId)
                }
            }
        }

        if (visualIntent.isDeleteOrReplaceRenderRole()) {
            val extraParaIds = mutableSetOf<Int>()
            for (pid in affectedParaIds) {
                val firstLineOfPara =
                    oldRevision.lineRanges.withIndex()
                        .firstOrNull { it.value.paragraphId == pid }?.index ?: continue
                val lastLineOfPara =
                    oldRevision.lineRanges.withIndex()
                        .filter { it.value.paragraphId == pid }
                        .lastOrNull()?.index ?: continue
                if (firstLineOfPara > 0) {
                    val prevParaId = oldRevision.lineRanges[firstLineOfPara - 1].paragraphId
                    if (prevParaId != pid) {
                        extraParaIds.add(prevParaId)
                    }
                }
                for (i in (lastLineOfPara + 1) until oldRevision.lineRanges.size) {
                    val nextParaId = oldRevision.lineRanges[i].paragraphId
                    if (nextParaId != pid) {
                        extraParaIds.add(nextParaId)
                        break
                    }
                }
            }
            affectedParaIds.addAll(extraParaIds)
        }

        val reverseMapper = buildStandaloneReverseOffsetMapper(visualIntent)
        for ((start, end) in visualIntent.newAffectedByteRanges) {
            val mappedStart = reverseMapper(start)
            val mappedEnd = reverseMapper(end)
            if (mappedStart != null || mappedEnd != null) {
                val effectiveStart = mappedStart ?: start
                val effectiveEnd = mappedEnd ?: end
                for (i in oldRevision.lineRanges.indices) {
                    val lineRange = oldRevision.lineRanges[i]
                    if (effectiveStart < lineRange.endUtf8 && effectiveEnd > lineRange.startUtf8) {
                        affectedParaIds.add(lineRange.paragraphId)
                    }
                }
            } else {
                val oldRanges = visualIntent.oldAffectedByteRanges
                if (oldRanges.isNotEmpty()) {
                    val oldAffectedStart = oldRanges.first().first
                    val oldAffectedEnd = oldRanges.last().second
                    for (i in oldRevision.lineRanges.indices) {
                        val lineRange = oldRevision.lineRanges[i]
                        if (oldAffectedStart < lineRange.endUtf8 && oldAffectedEnd > lineRange.startUtf8) {
                            affectedParaIds.add(lineRange.paragraphId)
                        }
                    }
                } else {
                    for (i in oldRevision.lineRanges.indices) {
                        val lineRange = oldRevision.lineRanges[i]
                        if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                            affectedParaIds.add(lineRange.paragraphId)
                        }
                    }
                }
            }
        }

        for (pid in affectedParaIds) {
            for (entry in oldRevision.lineRanges.withIndex()) {
                if (entry.value.paragraphId == pid) {
                    affectedLines.add(entry.index)
                }
            }
        }

        return affectedLines
    }

    fun computeAffectedLines(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
    ): AffectedLinesResult {
        val affectedOldLines = mutableSetOf<Int>()
        val affectedNewLines = mutableSetOf<Int>()
        val blockShifts = mutableListOf<PreparedVisualTransaction.BlockShift>()

        for ((start, end) in visualIntent.oldAffectedByteRanges) {
            for (i in oldRev.lineRanges.indices) {
                val lineRange = oldRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedOldLines.add(i)
                }
            }
        }

        for ((start, end) in visualIntent.newAffectedByteRanges) {
            for (i in newRev.lineRanges.indices) {
                val lineRange = newRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedNewLines.add(i)
                }
            }
        }

        val editByteStart =
            visualIntent.oldAffectedByteRanges.firstOrNull()?.first
                ?: visualIntent.newAffectedByteRanges.firstOrNull()?.first
        if (editByteStart != null) {
            val oldEditLine = findLineForUtf8(oldRev, editByteStart)
            val newEditLine = findLineForUtf8(newRev, editByteStart)

            val editParagraphId = oldRev.lineRanges.getOrNull(oldEditLine)?.paragraphId ?: 0

            val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
            val reverseMapper = buildReverseOffsetMapper(visualIntent, oldRev, newRev)

            val oldParagraphs = buildParagraphRanges(oldRev)
            val newParagraphs = buildParagraphRanges(newRev)

            val editOldPara = oldParagraphs.firstOrNull { it.paragraphId == editParagraphId }
            val editNewParaId = newRev.lineRanges.getOrNull(newEditLine)?.paragraphId ?: 0
            val editNewPara = newParagraphs.firstOrNull { it.paragraphId == editNewParaId }
            val structurallyAffectedOldParaIds = mutableSetOf<Int>()
            val structurallyAffectedNewParaIds = mutableSetOf<Int>()
            structurallyAffectedOldParaIds.add(editParagraphId)
            structurallyAffectedNewParaIds.add(editNewParaId)

            for (oldPara in oldParagraphs) {
                if (oldPara.paragraphId == editParagraphId) continue
                val mappedEnd = offsetMapper(oldPara.endUtf8Exclusive)
                if (mappedEnd == null) {
                    structurallyAffectedOldParaIds.add(oldPara.paragraphId)
                    continue
                }
                for (newPara in newParagraphs) {
                    if (newPara.endUtf8Exclusive == mappedEnd) {
                        if (newPara.startUtf8 != offsetMapper(oldPara.startUtf8)) {
                            structurallyAffectedOldParaIds.add(oldPara.paragraphId)
                            structurallyAffectedNewParaIds.add(newPara.paragraphId)
                        }
                        break
                    }
                }
            }
            for (newPara in newParagraphs) {
                if (newPara.paragraphId in structurallyAffectedNewParaIds) continue
                val reverseMappedStart = reverseMapper(newPara.startUtf8)
                if (reverseMappedStart == null) {
                    structurallyAffectedNewParaIds.add(newPara.paragraphId)
                    for (oldPara in oldParagraphs) {
                        val ms = offsetMapper(oldPara.startUtf8)
                        if (ms != null && ms < newPara.endUtf8Exclusive &&
                            newPara.startUtf8 < oldPara.endUtf8Exclusive +
                            (newPara.endUtf8Exclusive - newPara.startUtf8)
                        ) {
                            structurallyAffectedOldParaIds.add(oldPara.paragraphId)
                        }
                    }
                } else if (editOldPara != null) {
                    if (reverseMappedStart >= editOldPara.startUtf8 &&
                        reverseMappedStart < editOldPara.endUtf8Exclusive
                    ) {
                        structurallyAffectedNewParaIds.add(newPara.paragraphId)
                    }
                }
            }
            if (editNewPara != null) {
                for (oldPara in oldParagraphs) {
                    if (oldPara.paragraphId in structurallyAffectedOldParaIds) continue
                    val mappedStart = offsetMapper(oldPara.startUtf8)
                    if (mappedStart != null && mappedStart >= editNewPara.startUtf8 &&
                        mappedStart < editNewPara.endUtf8Exclusive
                    ) {
                        structurallyAffectedOldParaIds.add(oldPara.paragraphId)
                    }
                }
            }

            for (pid in structurallyAffectedOldParaIds) {
                for (entry in oldRev.lineRanges.withIndex()) {
                    if (entry.value.paragraphId == pid) affectedOldLines.add(entry.index)
                }
            }
            for (pid in structurallyAffectedNewParaIds) {
                for (entry in newRev.lineRanges.withIndex()) {
                    if (entry.value.paragraphId == pid) affectedNewLines.add(entry.index)
                }
            }

            val matchedNewParagraphs = mutableSetOf<Int>()
            val rawBlockShifts = mutableListOf<PreparedVisualTransaction.BlockShift>()

            for ((oldParaIdx, oldPara) in oldParagraphs.withIndex()) {
                if (oldPara.paragraphId in structurallyAffectedOldParaIds) continue

                var bestNewParaIdx: Int? = null
                val mappedStart = offsetMapper(oldPara.startUtf8)
                if (mappedStart != null) {
                    for ((newParaIdx, newPara) in newParagraphs.withIndex()) {
                        if (newParaIdx in matchedNewParagraphs) continue
                        if (newPara.startUtf8 == mappedStart) {
                            bestNewParaIdx = newParaIdx
                            break
                        }
                    }
                }
                if (bestNewParaIdx == null) {
                    val mappedEnd = offsetMapper(oldPara.endUtf8Exclusive)
                    if (mappedEnd != null) {
                        for ((newParaIdx, newPara) in newParagraphs.withIndex()) {
                            if (newParaIdx in matchedNewParagraphs) continue
                            if (newPara.endUtf8Exclusive == mappedEnd) {
                                bestNewParaIdx = newParaIdx
                                break
                            }
                        }
                    }
                }
                if (bestNewParaIdx == null) {
                    for ((newParaIdx, newPara) in newParagraphs.withIndex()) {
                        if (newParaIdx in matchedNewParagraphs) continue
                        if (newPara.paragraphId == oldPara.paragraphId) {
                            bestNewParaIdx = newParaIdx
                            break
                        }
                    }
                }
                if (bestNewParaIdx == null) continue
                matchedNewParagraphs.add(bestNewParaIdx)

                val newPara = newParagraphs[bestNewParaIdx]
                val oldTop = oldPara.top
                val newTop = newPara.top
                val deltaY = newTop - oldTop
                if (kotlin.math.abs(deltaY) > STABLE_SUFFIX_GEOMETRY_TOLERANCE) {
                    val newParaLines =
                        newRev.lineRanges.withIndex()
                            .filter { it.value.paragraphId == newPara.paragraphId }
                    if (newParaLines.isNotEmpty()) {
                        val firstLine = newParaLines.first()
                        val lastLine = newParaLines.last()
                        rawBlockShifts.add(
                            PreparedVisualTransaction.BlockShift(
                                startLineIndex = firstLine.index,
                                endLineIndexExclusive = lastLine.index + 1,
                                top = firstLine.value.top,
                                bottom = lastLine.value.bottom,
                                left = newParaLines.map { it.value.left }.minOrNull() ?: 0f,
                                right = newParaLines.map { it.value.right }.maxOrNull() ?: 0f,
                                deltaY = deltaY,
                                startUtf8 = newPara.startUtf8,
                                endUtf8Exclusive = newPara.endUtf8Exclusive,
                            ),
                        )
                    }
                }
            }

            blockShifts.addAll(mergeAdjacentBlockShifts(rawBlockShifts))
        }

        return AffectedLinesResult(
            lineIndices = emptySet(),
            oldLineIndices = affectedOldLines,
            newLineIndices = affectedNewLines,
            blockShifts = blockShifts,
        )
    }

    /**
     * #606: Build an old→new offset mapper.
     *
     * If [VisualIntent.offsetMap] is non-null (Core provided an explicit offset map),
     * consume it directly — single source of truth for offset translation semantics.
     * Each [OffsetMapEntry] maps old offsets [oldByteOffset, oldByteOffset+length) to
     * new offsets [newByteOffset, newByteOffset+length) linearly. Offsets not covered
     * by any entry are in the changed region and return null.
     *
     * If [VisualIntent.offsetMap] is null (cursor-only operations, or Core did not
     * provide a map), fall back to identity mapping.
     */
    internal fun buildOffsetMapper(
        visualIntent: VisualIntent,
        @Suppress("UNUSED_PARAMETER") oldRev: AndroidLayoutRevision,
        @Suppress("UNUSED_PARAMETER") newRev: AndroidLayoutRevision,
    ): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { offset -> offset }
        }
        return { offset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    offset >= e.oldByteOffset && offset < e.oldByteOffset + e.length
                }
            entry?.let { it.newByteOffset + (offset - it.oldByteOffset) }
        }
    }

    /**
     * #606: Build a new→old (reverse) offset mapper.
     *
     * If [VisualIntent.offsetMap] is non-null, consume it directly — traverse entries
     * to find the one containing the new byte offset and map back to old coordinates.
     * If [VisualIntent.offsetMap] is null, fall back to identity mapping.
     */
    internal fun buildReverseOffsetMapper(
        visualIntent: VisualIntent,
        @Suppress("UNUSED_PARAMETER") oldRev: AndroidLayoutRevision,
        @Suppress("UNUSED_PARAMETER") newRev: AndroidLayoutRevision,
    ): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { newOffset -> newOffset }
        }
        return { newOffset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    newOffset >= e.newByteOffset && newOffset < e.newByteOffset + e.length
                }
            entry?.let { it.oldByteOffset + (newOffset - it.newByteOffset) }
        }
    }

    /**
     * #606: Build a standalone new→old (reverse) offset mapper.
     *
     * Same semantics as [buildReverseOffsetMapper] but does not require layout revisions.
     * Used by [computeStructurallyAffectedOldLineIndices] which only has the old revision.
     */
    internal fun buildStandaloneReverseOffsetMapper(visualIntent: VisualIntent): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { newOffset -> newOffset }
        }
        return { newOffset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    newOffset >= e.newByteOffset && newOffset < e.newByteOffset + e.length
                }
            entry?.let { it.oldByteOffset + (newOffset - it.newByteOffset) }
        }
    }

    fun mergeAdjacentBlockShifts(
        shifts: List<PreparedVisualTransaction.BlockShift>,
    ): List<PreparedVisualTransaction.BlockShift> {
        if (shifts.size <= 1) return shifts
        val sorted = shifts.sortedBy { it.startLineIndex }
        val merged = mutableListOf<PreparedVisualTransaction.BlockShift>()
        var current = sorted[0]
        for (i in 1 until sorted.size) {
            val next = sorted[i]
            val deltaYClose = kotlin.math.abs(next.deltaY - current.deltaY) < BLOCK_SHIFT_DELTA_Y_EPSILON
            if (next.startLineIndex == current.endLineIndexExclusive && deltaYClose) {
                current =
                    current.copy(
                        endLineIndexExclusive = next.endLineIndexExclusive,
                        bottom = next.bottom,
                        left = minOf(current.left, next.left),
                        right = maxOf(current.right, next.right),
                        startUtf8 = if (current.startUtf8 >= 0) current.startUtf8 else next.startUtf8,
                        endUtf8Exclusive =
                            if (next.endUtf8Exclusive >= 0) next.endUtf8Exclusive else current.endUtf8Exclusive,
                    )
            } else {
                merged.add(current)
                current = next
            }
        }
        merged.add(current)
        return merged
    }

    internal fun buildParagraphRanges(rev: AndroidLayoutRevision): List<ParagraphRange> {
        val paragraphs = mutableListOf<ParagraphRange>()
        val linesByParagraph = rev.lineRanges.withIndex().groupBy { it.value.paragraphId }
        for ((pid, lines) in linesByParagraph.toSortedMap()) {
            val startUtf8 = lines.first().value.startUtf8
            val endUtf8Exclusive = lines.last().value.endUtf8
            val top = lines.first().value.top
            paragraphs.add(ParagraphRange(pid, startUtf8, endUtf8Exclusive, top))
        }
        return paragraphs
    }

    internal fun findLineForUtf8(
        rev: AndroidLayoutRevision,
        byteOffset: Int,
    ): Int {
        for (i in rev.lineRanges.indices) {
            val range = rev.lineRanges[i]
            if (byteOffset <= range.endUtf8) return i
        }
        return rev.lineRanges.lastIndex.coerceAtLeast(0)
    }
}
