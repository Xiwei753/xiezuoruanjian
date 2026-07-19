package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import uniffi.writer_core.AnimationModeDto

class AndroidVisualPlanner {

    private companion object {
        const val STABLE_SUFFIX_GEOMETRY_TOLERANCE = 1.0f
    }

    /**
     * Provisional capture lines for a single revision (before/after edit).
     * When the preferred ranges are empty (pure Insert old / pure Delete new),
     * fall back to the other side's edit point and expand forward.
     * Expansion stops at the first paragraph boundary after the edit point
     * (hard line break detected via byte-gap between consecutive visual lines),
     * or at the end of the document. No fixed line cap.
     */
    fun computeAffectedLineIndices(
        visualIntent: VisualIntent,
        revision: AndroidLayoutRevision?,
        useNewRanges: Boolean = false
    ): Set<Int> {
        if (revision == null) return emptySet()
        val affectedLines = mutableSetOf<Int>()
        val primaryRanges = if (useNewRanges) visualIntent.newAffectedByteRanges else visualIntent.oldAffectedByteRanges
        val fallbackRanges = if (useNewRanges) visualIntent.oldAffectedByteRanges else visualIntent.newAffectedByteRanges
        for ((start, end) in primaryRanges) {
            for (i in revision.lineRanges.indices) {
                val lineRange = revision.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }
        val editByteStart = primaryRanges.firstOrNull()?.first
            ?: fallbackRanges.firstOrNull()?.first
        if (editByteStart != null) {
            val editLine = findLineForUtf8(revision, editByteStart)
            for (i in editLine until revision.lineRanges.size) {
                affectedLines.add(i)
                if (i > editLine && isParagraphBoundary(revision, i)) {
                    break
                }
            }
        }
        return affectedLines
    }

    private fun isParagraphBoundary(revision: AndroidLayoutRevision, lineIndex: Int): Boolean {
        if (lineIndex <= 0 || lineIndex >= revision.lineRanges.size) return false
        val prev = revision.lineRanges[lineIndex - 1]
        val curr = revision.lineRanges[lineIndex]
        return curr.startUtf8 > prev.endUtf8
    }

    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?
    ): Set<Int> {
        if (oldRevision == null || newRevision == null) {
            return computeAffectedLineIndices(visualIntent, newRevision ?: oldRevision, useNewRanges = true)
        }
        return computeAffectedLines(visualIntent, oldRevision, newRevision)
    }

    fun prepare(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        rebaseSnapshot: VisualFrameSnapshot? = null,
        transactionKey: Long,
        ownedSnapshotIds: Set<Long>,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap()
    ): PreparedVisualTransaction {
        val durationMs = visualIntent.durationMs

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        val oldRev = oldRevision
        val newRev = newRevision

        if (oldRev != null && newRev != null) {
            val affectedLines = computeAffectedLines(visualIntent, oldRev, newRev)
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planClusterLevelAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                    addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        collectExcludedNewByteRanges(animatedSlices),
                        animatedSlices
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                    addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        collectExcludedNewByteRanges(animatedSlices),
                        animatedSlices
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.SnapshotAnimation -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
            }
        } else if (newRev != null) {
            planNoAnimation(newRev, staticPatches)
        }

        if (visualIntent.coordinatedCursor.shouldAnimate && newRev != null) {
            val fromX: Float
            val fromY: Float
            val fromHeight: Float
            if (rebaseSnapshot?.cursorRect != null) {
                val rc = rebaseSnapshot.cursorRect
                fromX = rc.left
                fromY = rc.top
                fromHeight = rc.bottom - rc.top
            } else if (oldRev != null) {
                fromX = oldRev.cursorX
                fromY = oldRev.cursorY
                fromHeight = oldRev.cursorHeight
            } else {
                fromX = newRev.cursorX
                fromY = newRev.cursorY
                fromHeight = newRev.cursorHeight
            }

            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = fromX,
                fromY = fromY,
                fromHeight = fromHeight,
                toX = newRev.cursorX,
                toY = newRev.cursorY,
                toHeight = newRev.cursorHeight,
                shouldAnimate = true
            )
        } else if (newRev != null) {
            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = newRev.cursorX,
                fromY = newRev.cursorY,
                fromHeight = newRev.cursorHeight,
                toX = newRev.cursorX,
                toY = newRev.cursorY,
                toHeight = newRev.cursorHeight,
                shouldAnimate = false
            )
        }

        val finalSlices = if (rebaseSnapshot != null && rebaseSnapshot.sliceVisualStates.isNotEmpty()) {
            applyRebaseToSlices(animatedSlices, rebaseSnapshot, snapshotLookup)
        } else {
            animatedSlices
        }

        return PreparedVisualTransaction(
            transactionId = transactionKey,
            oldRevision = oldRev,
            newRevision = newRev,
            staticPatches = staticPatches,
            animatedSlices = finalSlices,
            ownedSnapshotIds = ownedSnapshotIds,
            selectionDecoration = buildSelectionDecoration(newRev),
            preeditDecoration = buildPreeditDecoration(newRev),
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )
    }

    private fun planClusterLevelAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        // CompositionUpdate/Commit are replace-like: old preedit/range fades, new text appears, retained text Moves.
        val isReplace = visualIntent.isReplace()
            || visualIntent.isCompositionCommit()
            || visualIntent.isCompositionUpdate()

        if (isReplace) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
            )
            return
        }

        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (isInsert && newSnapshot != null && newLineRange != null) {
                val insertClusters = newSnapshot.clusters.filter { cluster ->
                    visualIntent.newAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
                    }
                }
                for (cluster in insertClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 0f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            } else if (isDelete && oldSnapshot != null && oldLineRange != null) {
                val deleteClusters = oldSnapshot.clusters.filter { cluster ->
                    visualIntent.oldAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
                    }
                }
                for (cluster in deleteClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 0f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            }
        }
    }

    private fun addMoveSlicesForShiftedClusters(
        oldSnapshot: AndroidLineSnapshot,
        newSnapshot: AndroidLineSnapshot,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        excludedNewByteRanges: Set<Pair<Int, Int>>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>
    ) {
        val matchedPairs = matchClustersByOffsetMap(oldSnapshot, newSnapshot, visualIntent, oldRev, newRev)
        for ((oldCluster, newCluster) in matchedPairs) {
            val isExcluded = excludedNewByteRanges.any { (start, end) ->
                newCluster.documentByteStart >= start && newCluster.documentByteEndExclusive <= end
            }
            if (isExcluded) continue
            val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
            if (!positionChanged) continue
            if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint) {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Move,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 1f,
                    fromDestinationRect = oldCluster.visualRectInDocument,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive
                ))
            } else {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.CrossfadeOld,
                    snapshot = oldSnapshot,
                    sourceRect = oldCluster.sourceRectInLineImage,
                    destinationRect = oldCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 0f,
                    clusterByteStart = oldCluster.documentByteStart,
                    clusterByteEndExclusive = oldCluster.documentByteEndExclusive
                ))
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.CrossfadeNew,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    startAlpha = 0f,
                    endAlpha = 1f,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive
                ))
            }
        }
    }

    private fun addMoveSlicesForShiftedClustersCrossLine(
        allOldSnapshots: Map<Int, AndroidLineSnapshot>,
        allNewSnapshots: Map<Int, AndroidLineSnapshot>,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        excludedNewByteRanges: Set<Pair<Int, Int>>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>
    ) {
        val allOldClusters = mutableListOf<Pair<LineClusterSnapshot, Pair<Int, AndroidLineSnapshot>>>()
        for ((lineIdx, snapshot) in allOldSnapshots) {
            for (cluster in snapshot.clusters) {
                allOldClusters.add(Pair(cluster, Pair(lineIdx, snapshot)))
            }
        }
        val allNewClusters = mutableListOf<Pair<LineClusterSnapshot, Pair<Int, AndroidLineSnapshot>>>()
        for ((lineIdx, snapshot) in allNewSnapshots) {
            for (cluster in snapshot.clusters) {
                allNewClusters.add(Pair(cluster, Pair(lineIdx, snapshot)))
            }
        }
        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
        val newUsed = mutableSetOf<Int>()
        for ((oldCluster, oldInfo) in allOldClusters) {
            val isDeleted = visualIntent.oldAffectedByteRanges.any { (start, end) ->
                oldCluster.documentByteStart >= start && oldCluster.documentByteEndExclusive <= end
            }
            if (isDeleted) continue
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchedNewIdx: Int? = null
            if (mappedStart != null) {
                matchedNewIdx = allNewClusters.indices.firstOrNull { i ->
                    i !in newUsed && allNewClusters[i].first.documentByteStart == mappedStart &&
                        (mappedEnd == null || allNewClusters[i].first.documentByteEndExclusive == mappedEnd)
                }
            }
            if (matchedNewIdx == null) {
                matchedNewIdx = allNewClusters.indices.firstOrNull { i ->
                    val candidate = allNewClusters[i].first
                    i !in newUsed &&
                        candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                        visualIntent.newAffectedByteRanges.none { (start, end) ->
                            candidate.documentByteStart >= start && candidate.documentByteEndExclusive <= end
                        }
                }
            }
            if (matchedNewIdx != null) {
                newUsed.add(matchedNewIdx)
                val (newCluster, newInfo) = allNewClusters[matchedNewIdx]
                val isExcluded = excludedNewByteRanges.any { (start, end) ->
                    newCluster.documentByteStart >= start && newCluster.documentByteEndExclusive <= end
                }
                if (isExcluded) continue
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                if (!positionChanged) continue
                val newSnapshot = newInfo.second
                val oldSnapshot = oldInfo.second
                if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Move,
                        snapshot = newSnapshot,
                        sourceRect = newCluster.sourceRectInLineImage,
                        destinationRect = newCluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        fromDestinationRect = oldCluster.visualRectInDocument,
                        clusterByteStart = newCluster.documentByteStart,
                        clusterByteEndExclusive = newCluster.documentByteEndExclusive
                    ))
                } else {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeOld,
                        snapshot = oldSnapshot,
                        sourceRect = oldCluster.sourceRectInLineImage,
                        destinationRect = oldCluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 0f,
                        clusterByteStart = oldCluster.documentByteStart,
                        clusterByteEndExclusive = oldCluster.documentByteEndExclusive
                    ))
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeNew,
                        snapshot = newSnapshot,
                        sourceRect = newCluster.sourceRectInLineImage,
                        destinationRect = newCluster.visualRectInDocument,
                        startAlpha = 0f,
                        endAlpha = 1f,
                        clusterByteStart = newCluster.documentByteStart,
                        clusterByteEndExclusive = newCluster.documentByteEndExclusive
                    ))
                }
            }
        }
    }

    private fun planClusterReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (oldSnapshot != null && oldLineRange != null) {
                if (oldSnapshot.clusters.isNotEmpty()) {
                    for (cluster in oldSnapshot.clusters) {
                        val inOldRange = visualIntent.oldAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
                        }
                        if (inOldRange) {
                            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                                role = SliceRole.Delete,
                                snapshot = oldSnapshot,
                                sourceRect = cluster.sourceRectInLineImage,
                                destinationRect = cluster.visualRectInDocument,
                                startAlpha = 1f,
                                endAlpha = 0f
                            ))
                        }
                    }
                } else {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            }

            if (newSnapshot != null && newLineRange != null) {
                if (newSnapshot.clusters.isNotEmpty()) {
                    for (cluster in newSnapshot.clusters) {
                        val inNewRange = visualIntent.newAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
                        }
                        if (inNewRange) {
                            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                                role = SliceRole.Insert,
                                snapshot = newSnapshot,
                                sourceRect = cluster.sourceRectInLineImage,
                                destinationRect = cluster.visualRectInDocument,
                                startAlpha = 0f,
                                endAlpha = 1f
                            ))
                        }
                    }
                } else {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                }
            }
        }
    }

    /**
     * Map an old-document byte offset into the new document after this edit.
     * Returns null for offsets inside a deleted/replaced range that has no retained identity.
     * Pure Insert/Delete are first-class: retained text after the edit point shifts by delta.
     */
    private fun buildOffsetMapper(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): (Int) -> Int? {
        val oldRanges = visualIntent.oldAffectedByteRanges
        val newRanges = visualIntent.newAffectedByteRanges

        // Pure Insert: old empty, new non-empty.
        if (oldRanges.isEmpty() && newRanges.isNotEmpty()) {
            val insertStart = newRanges.first().first
            val insertLen = newRanges.sumOf { (s, e) -> e - s }
            return { offset ->
                if (offset < insertStart) offset else offset + insertLen
            }
        }

        // Pure Delete: new empty, old non-empty.
        if (newRanges.isEmpty() && oldRanges.isNotEmpty()) {
            val deleteStart = oldRanges.first().first
            val deleteEnd = oldRanges.last().second
            val deleteLen = oldRanges.sumOf { (s, e) -> e - s }
            return { offset ->
                when {
                    offset < deleteStart -> offset
                    offset < deleteEnd -> null
                    else -> offset - deleteLen
                }
            }
        }

        if (oldRanges.isEmpty() || newRanges.isEmpty()) {
            return { offset -> offset }
        }

        val oldAffectedStart = oldRanges.first().first
        val oldAffectedEnd = oldRanges.last().second
        val newAffectedEnd = newRanges.last().second
        val shift = newAffectedEnd - oldAffectedEnd

        return { offset ->
            when {
                offset < oldAffectedStart -> offset
                offset >= oldAffectedEnd -> offset + shift
                else -> {
                    val mapped = mapThroughRanges(offset, oldRanges, newRanges)
                    if (mapped >= 0) mapped else null
                }
            }
        }
    }

    /** Sparse map used by same-line matchers; backed by [buildOffsetMapper]. */
    private fun buildOffsetMap(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Map<Int, Int> {
        val mapper = buildOffsetMapper(visualIntent, oldRev, newRev)
        val offsetMap = mutableMapOf<Int, Int>()
        val candidates = mutableSetOf<Int>()
        for (line in oldRev.lineRanges) {
            candidates.add(line.startUtf8)
            candidates.add(line.endUtf8)
        }
        for (line in newRev.lineRanges) {
            candidates.add(line.startUtf8)
            candidates.add(line.endUtf8)
        }
        for (offset in candidates) {
            val mapped = mapper(offset) ?: continue
            offsetMap[offset] = mapped
        }
        return offsetMap
    }

    private fun mapThroughRanges(offset: Int, oldRanges: List<Pair<Int, Int>>, newRanges: List<Pair<Int, Int>>): Int {
        for (i in oldRanges.indices) {
            val (oldStart, oldEnd) = oldRanges[i]
            if (offset in oldStart..oldEnd) {
                val newRange = newRanges.getOrNull(i) ?: continue
                val ratio = if (oldEnd == oldStart) 0f else (offset - oldStart).toFloat() / (oldEnd - oldStart)
                return newRange.first + (ratio * (newRange.second - newRange.first)).toInt()
            }
        }
        return -1
    }

    private fun planRunAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        val isReplace = visualIntent.isReplace()
            || visualIntent.isCompositionCommit()
            || visualIntent.isCompositionUpdate()

        if (isReplace) {
            planRunReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
            )
            return
        }

        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (isInsert && newSnapshot != null && newLineRange != null) {
                val insertClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in insertClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 0f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            } else if (isDelete && oldSnapshot != null && oldLineRange != null) {
                val deleteClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in deleteClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 0f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            }
        }
    }

    private fun planRunReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            // Only animate clusters inside the affected ranges. Never fall back to whole-line
            // CrossfadeOld when oldAffected is empty (ordinary Insert / composition insert path);
            // retained text is handled by cross-line Move after planRunAnimation returns.
            if (oldSnapshot != null && oldLineRange != null && visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 0f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            }

            if (newSnapshot != null && newLineRange != null && visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 0f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive
                    ))
                }
            }
        }
    }

    private fun groupClustersIntoRuns(
        clusters: List<LineClusterSnapshot>,
        affectedRanges: List<Pair<Int, Int>>
    ): List<LineClusterSnapshot> {
        if (clusters.isEmpty()) return emptyList()
        val affected = clusters.filter { cluster ->
            affectedRanges.any { (start, end) ->
                cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
            }
        }
        return affected
    }

    private fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots)
                if (newSnapshot != null && oldSnapshot != null) {
                    val matchedPairs = matchClustersByOffsetMap(
                        oldSnapshot, newSnapshot, visualIntent, oldRev, newRev
                    )
                    if (matchedPairs.isNotEmpty()) {
                        for ((oldCluster, newCluster) in matchedPairs) {
                            if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint) {
                                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                                    role = SliceRole.Move,
                                    snapshot = newSnapshot,
                                    sourceRect = newCluster.sourceRectInLineImage,
                                    destinationRect = newCluster.visualRectInDocument,
                                    startAlpha = 1f,
                                    endAlpha = 1f,
                                    fromDestinationRect = oldCluster.visualRectInDocument
                                ))
                            } else {
                                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                                    role = SliceRole.CrossfadeOld,
                                    snapshot = oldSnapshot,
                                    sourceRect = oldCluster.sourceRectInLineImage,
                                    destinationRect = oldCluster.visualRectInDocument,
                                    startAlpha = 1f,
                                    endAlpha = 0f
                                ))
                                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                                    role = SliceRole.CrossfadeNew,
                                    snapshot = newSnapshot,
                                    sourceRect = newCluster.sourceRectInLineImage,
                                    destinationRect = newCluster.visualRectInDocument,
                                    startAlpha = 0f,
                                    endAlpha = 1f
                                ))
                            }
                        }
                    } else {
                        animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.Move,
                            snapshot = newSnapshot,
                            sourceRect = newSnapshot.sourceRect,
                            destinationRect = android.graphics.RectF(
                                newLineRange.left, newLineRange.top,
                                newLineRange.right, newLineRange.bottom
                            ),
                            startAlpha = 1f,
                            endAlpha = 1f,
                            fromDestinationRect = android.graphics.RectF(
                                oldLineRange.left, oldLineRange.top,
                                oldLineRange.right, oldLineRange.bottom
                            )
                        ))
                    }
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                }
            } else if (oldLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots)
                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            }
        }
    }

    private fun matchClustersByOffsetMap(
        oldSnapshot: AndroidLineSnapshot,
        newSnapshot: AndroidLineSnapshot,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): List<Pair<LineClusterSnapshot, LineClusterSnapshot>> {
        if (oldSnapshot.clusters.isEmpty() || newSnapshot.clusters.isEmpty()) return emptyList()
        val mapper = buildOffsetMapper(visualIntent, oldRev, newRev)
        val pairs = mutableListOf<Pair<LineClusterSnapshot, LineClusterSnapshot>>()
        val newUsed = mutableSetOf<Int>()
        for (oldCluster in oldSnapshot.clusters) {
            val isDeleted = visualIntent.oldAffectedByteRanges.any { (start, end) ->
                oldCluster.documentByteStart >= start && oldCluster.documentByteEndExclusive <= end
            }
            if (isDeleted) continue
            val mappedStart = mapper(oldCluster.documentByteStart) ?: continue
            val mappedEnd = mapper(oldCluster.documentByteEndExclusive)
            val newIdx = newSnapshot.clusters.indices.firstOrNull { i ->
                i !in newUsed && newSnapshot.clusters[i].documentByteStart == mappedStart &&
                    (mappedEnd == null || newSnapshot.clusters[i].documentByteEndExclusive == mappedEnd)
            }
            if (newIdx != null) {
                newUsed.add(newIdx)
                pairs.add(Pair(oldCluster, newSnapshot.clusters[newIdx]))
            }
        }
        if (pairs.isNotEmpty()) return pairs
        return matchClustersByFingerprint(oldSnapshot, newSnapshot, visualIntent)
    }

    private fun matchClustersByFingerprint(
        oldSnapshot: AndroidLineSnapshot,
        newSnapshot: AndroidLineSnapshot,
        visualIntent: VisualIntent
    ): List<Pair<LineClusterSnapshot, LineClusterSnapshot>> {
        val pairs = mutableListOf<Pair<LineClusterSnapshot, LineClusterSnapshot>>()
        val insertByteRanges = visualIntent.newAffectedByteRanges.toSet()
        val deleteByteRanges = visualIntent.oldAffectedByteRanges.toSet()
        val newByFp = mutableMapOf<String, MutableList<Int>>()
        for (i in newSnapshot.clusters.indices) {
            val cluster = newSnapshot.clusters[i]
            val isInserted = insertByteRanges.any { (start, end) ->
                cluster.documentByteStart >= start && cluster.documentByteEndExclusive <= end
            }
            if (isInserted) continue
            newByFp.getOrPut(cluster.shapingFingerprint) { mutableListOf() }.add(i)
        }
        for (oldCluster in oldSnapshot.clusters) {
            val isDeleted = deleteByteRanges.any { (start, end) ->
                oldCluster.documentByteStart >= start && oldCluster.documentByteEndExclusive <= end
            }
            if (isDeleted) continue
            val candidates = newByFp[oldCluster.shapingFingerprint]
            if (candidates != null && candidates.isNotEmpty()) {
                val newIdx = candidates.removeAt(0)
                pairs.add(Pair(oldCluster, newSnapshot.clusters[newIdx]))
            }
        }
        return pairs
    }

    private fun planCrossfadeAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots)
                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeOld,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                    // addStaticPatchForAnimatedLine removed
                }
            }
        }
    }

    private fun planNoAnimation(
        newRev: AndroidLayoutRevision,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
    }

    private fun computeAffectedLines(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Set<Int> {
        val affectedLines = mutableSetOf<Int>()

        for ((start, end) in visualIntent.oldAffectedByteRanges) {
            for (i in oldRev.lineRanges.indices) {
                val lineRange = oldRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }

        for ((start, end) in visualIntent.newAffectedByteRanges) {
            for (i in newRev.lineRanges.indices) {
                val lineRange = newRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }

        val editByteStart = visualIntent.oldAffectedByteRanges.firstOrNull()?.first
            ?: visualIntent.newAffectedByteRanges.firstOrNull()?.first
        if (editByteStart != null) {
            val oldEditLine = findLineForUtf8(oldRev, editByteStart)
            val newEditLine = findLineForUtf8(newRev, editByteStart)
            val scanStart = minOf(oldEditLine, newEditLine)
            val minCommonLines = minOf(oldRev.lineRanges.size, newRev.lineRanges.size)

            // Walk forward from the edit line using lightweight geometry metadata only.
            // Stop at the first stable suffix line; no fixed 3/10/30 line cap.
            var reachedStableSuffix = false
            for (i in scanStart until minCommonLines) {
                val oldLine = oldRev.lineRanges[i]
                val newLine = newRev.lineRanges[i]
                val geometryChanged = kotlin.math.abs(oldLine.top - newLine.top) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                    kotlin.math.abs(oldLine.bottom - newLine.bottom) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                    kotlin.math.abs(oldLine.left - newLine.left) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                    kotlin.math.abs(oldLine.right - newLine.right) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                    oldLine.startUtf8 != newLine.startUtf8 ||
                    oldLine.endUtf8 != newLine.endUtf8
                if (geometryChanged) {
                    affectedLines.add(i)
                } else if (i > scanStart || !geometryChanged) {
                    // First unchanged line after the edit starts the stable suffix.
                    // Keep scanning remaining common lines only if later geometry still differs
                    // (rare multi-region reflow); otherwise stop.
                    var laterUnstable = false
                    for (j in (i + 1) until minCommonLines) {
                        val ol = oldRev.lineRanges[j]
                        val nl = newRev.lineRanges[j]
                        val changed = kotlin.math.abs(ol.top - nl.top) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                            kotlin.math.abs(ol.bottom - nl.bottom) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                            kotlin.math.abs(ol.left - nl.left) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                            kotlin.math.abs(ol.right - nl.right) > STABLE_SUFFIX_GEOMETRY_TOLERANCE ||
                            ol.startUtf8 != nl.startUtf8 ||
                            ol.endUtf8 != nl.endUtf8
                        if (changed) {
                            laterUnstable = true
                            break
                        }
                    }
                    if (!laterUnstable) {
                        reachedStableSuffix = true
                        break
                    }
                }
            }
            // Lines present only on one side (soft wrap growth/shrink) always animate.
            for (i in minCommonLines until oldRev.lineRanges.size) {
                affectedLines.add(i)
            }
            for (i in minCommonLines until newRev.lineRanges.size) {
                affectedLines.add(i)
            }
            // If no stable suffix was found, the whole suffix from the edit is already in
            // affectedLines (or was added as exclusive old/new lines). Nothing else to do.
            if (!reachedStableSuffix) {
                // Ensure edit-line itself is included even when range intersection missed it.
                affectedLines.add(oldEditLine)
                affectedLines.add(newEditLine)
            }
        }

        return affectedLines
    }

    private fun findLineForUtf8(rev: AndroidLayoutRevision, byteOffset: Int): Int {
        for (i in rev.lineRanges.indices) {
            val range = rev.lineRanges[i]
            if (byteOffset <= range.endUtf8) return i
        }
        return rev.lineRanges.lastIndex.coerceAtLeast(0)
    }

    private fun parseAnimationMode(mode: AnimationModeDto): AnimationMode {
        return when (mode) {
            AnimationModeDto.GLYPH_ANIMATION -> AnimationMode.GlyphAnimation
            AnimationModeDto.CLUSTER_ANIMATION -> AnimationMode.ClusterAnimation
            AnimationModeDto.RUN_ANIMATION -> AnimationMode.RunAnimation
            AnimationModeDto.LINE_REFLOW_ANIMATION -> AnimationMode.LineReflowAnimation
            AnimationModeDto.SNAPSHOT_ANIMATION -> AnimationMode.SnapshotAnimation
            AnimationModeDto.SYSTEM_SUPPRESSED -> AnimationMode.SystemSuppressed
            else -> AnimationMode.SystemSuppressed
        }
    }

    fun resetOldRevision() {
    }

    private fun applyRebaseToSlices(
        slices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap()
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        val rebaseMatches = mutableMapOf<Int, SliceVisualState?>()
        for ((idx, slice) in slices.withIndex()) {
            val state = findRebaseStateByClusterByteRange(slice, rebaseSnapshot)
                ?: findRebaseStateByLineAndRole(slice, rebaseSnapshot)
                ?: findClosestRebaseStateByPosition(slice, rebaseSnapshot)
            rebaseMatches[idx] = state
        }
        val rebasedNewSlices = slices.mapIndexed { idx, slice ->
            val rebaseState = rebaseMatches[idx]
            if (rebaseState != null) {
                applyRebaseState(slice, rebaseState)
            } else {
                slice
            }
        }
        val matchedRebaseIndices = mutableSetOf<Int>()
        for ((_, state) in rebaseMatches) {
            if (state != null) {
                val idx = rebaseSnapshot.sliceVisualStates.indexOf(state)
                if (idx >= 0) matchedRebaseIndices.add(idx)
            }
        }
        val survivingOldSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        for ((stateIdx, state) in rebaseSnapshot.sliceVisualStates.withIndex()) {
            if (stateIdx in matchedRebaseIndices) continue
            val isFadingOut = state.role == SliceRole.Delete || state.role == SliceRole.CrossfadeOld
            val snapshot = snapshotLookup[state.snapshotId]
            val sourceRect = if (snapshot != null) {
                val cluster = snapshot.clusters.firstOrNull {
                    it.documentByteStart == state.clusterByteStart && it.documentByteEndExclusive == state.clusterByteEndExclusive
                }
                cluster?.sourceRectInLineImage ?: snapshot.sourceRect
            } else {
                android.graphics.Rect(0, 0, 0, 0)
            }
            if (isFadingOut && state.currentAlpha > 0.01f) {
                survivingOldSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = state.role,
                    snapshot = snapshot,
                    sourceRect = sourceRect,
                    destinationRect = android.graphics.RectF(
                        state.currentLeft, state.currentTop,
                        state.currentRight, state.currentBottom
                    ),
                    startAlpha = state.currentAlpha,
                    endAlpha = 0f,
                    clusterByteStart = state.clusterByteStart,
                    clusterByteEndExclusive = state.clusterByteEndExclusive
                ))
            } else if (state.role == SliceRole.Move) {
                val currentRect = android.graphics.RectF(
                    state.currentLeft, state.currentTop,
                    state.currentRight, state.currentBottom
                )
                val destRect = android.graphics.RectF(
                    state.destinationLeft, state.destinationTop,
                    state.destinationRight, state.destinationBottom
                )
                if (currentRect != destRect) {
                    survivingOldSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Move,
                        snapshot = snapshot,
                        sourceRect = sourceRect,
                        destinationRect = destRect,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        fromDestinationRect = currentRect,
                        clusterByteStart = state.clusterByteStart,
                        clusterByteEndExclusive = state.clusterByteEndExclusive
                    ))
                }
            } else if (!isFadingOut && state.currentAlpha < 0.99f) {
                val destRect = android.graphics.RectF(
                    state.currentLeft, state.currentTop,
                    state.currentRight, state.currentBottom
                )
                val endAlpha = when (state.role) {
                    SliceRole.Insert, SliceRole.CrossfadeNew, SliceRole.Move -> 1f
                    else -> state.currentAlpha
                }
                survivingOldSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = state.role,
                    snapshot = snapshot,
                    sourceRect = sourceRect,
                    destinationRect = destRect,
                    startAlpha = state.currentAlpha,
                    endAlpha = endAlpha,
                    fromDestinationRect = if (state.role == SliceRole.Move) destRect else null,
                    clusterByteStart = state.clusterByteStart,
                    clusterByteEndExclusive = state.clusterByteEndExclusive
                ))
            }
        }
        return rebasedNewSlices + survivingOldSlices
    }

    private fun findRebaseStateByClusterByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val cStart = slice.clusterByteStart
        val cEnd = slice.clusterByteEndExclusive
        if (cStart < 0 || cEnd < 0) return null
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val exactMatch = rebaseSnapshot.sliceVisualStates.firstOrNull {
            it.role == slice.role &&
                it.lineIndex == lineIndex &&
                it.clusterByteStart == cStart &&
                it.clusterByteEndExclusive == cEnd
        }
        if (exactMatch != null) return exactMatch
        return rebaseSnapshot.sliceVisualStates.firstOrNull {
            it.role == slice.role &&
                it.clusterByteStart == cStart &&
                it.clusterByteEndExclusive == cEnd
        }
    }

    private fun findRebaseStateByLineAndRole(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val roleKey = "${slice.role}_${lineIndex}"
        return rebaseSnapshot.sliceVisualStates
            .filter { "${it.role}_${it.lineIndex}" == roleKey }
            .firstOrNull()
    }

    private fun findClosestRebaseStateByByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val sliceRole = slice.role
        val byteStart = slice.snapshot?.documentByteStart ?: -1
        val byteEnd = slice.snapshot?.documentByteEndExclusive ?: -1
        if (byteStart < 0 || byteEnd < 0) return null
        return rebaseSnapshot.sliceVisualStates
            .filter { it.role == sliceRole && it.documentByteStart >= 0 && it.documentByteEndExclusive >= 0 }
            .firstOrNull { it.documentByteStart == byteStart && it.documentByteEndExclusive == byteEnd }
    }

    private fun findClosestRebaseStateByPosition(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val sliceTop = slice.destinationRect.top
        val sliceLeft = slice.destinationRect.left
        val sliceRole = slice.role
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val filterByLine = lineIndex >= 0 && sliceRole != SliceRole.Move
        return rebaseSnapshot.sliceVisualStates
            .filter { it.role == sliceRole && (!filterByLine || it.lineIndex == lineIndex) }
            .minByOrNull {
                val dy = kotlin.math.abs(it.currentTop - sliceTop)
                val dx = kotlin.math.abs(it.currentLeft - sliceLeft)
                dy + dx
            }
    }

    private fun applyRebaseState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState
    ): PreparedVisualTransaction.AnimatedSlice {
        return when (slice.role) {
            SliceRole.Move -> {
                slice.copy(
                    fromDestinationRect = android.graphics.RectF(
                        rebaseState.currentLeft,
                        rebaseState.currentTop,
                        rebaseState.currentRight,
                        rebaseState.currentBottom
                    )
                )
            }
            SliceRole.Insert -> {
                slice.copy(startAlpha = rebaseState.currentAlpha)
            }
            SliceRole.Delete -> {
                slice.copy(startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
            }
            SliceRole.CrossfadeOld -> {
                slice.copy(startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
            }
            SliceRole.CrossfadeNew -> {
                slice.copy(startAlpha = rebaseState.currentAlpha)
            }
            SliceRole.Static -> slice
        }
    }

    private fun createSnapshotFromRevision(
        revision: AndroidLayoutRevision,
        lineIndex: Int,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        isNewRevision: Boolean = false
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        if (isNewRevision) {
            val preCapturedNew = preCapturedNewSnapshots[lineIndex]
            if (preCapturedNew != null) {
                return preCapturedNew
            }
        }
        val preCaptured = preCapturedOldSnapshots[lineIndex]
        if (preCaptured != null) {
            return preCaptured
        }
        return null
    }

    private fun buildSelectionDecoration(
        newRev: AndroidLayoutRevision?
    ): PreparedVisualTransaction.SelectionDecoration? {
        if (newRev == null) return null
        val selStart = newRev.selectionStartUtf16
        val selEnd = newRev.selectionEndUtf16
        if (selStart == selEnd) return null

        val rects = mutableListOf<android.graphics.RectF>()
        for (lineRange in newRev.lineRanges) {
            if (selStart < lineRange.endUtf16 && selEnd > lineRange.startUtf16) {
                rects.add(android.graphics.RectF(
                    lineRange.left, lineRange.top, lineRange.right, lineRange.bottom
                ))
            }
        }
        if (rects.isEmpty()) return null
        return PreparedVisualTransaction.SelectionDecoration(selStart, selEnd, rects)
    }

    private fun buildPreeditDecoration(newRev: AndroidLayoutRevision?): PreparedVisualTransaction.PreeditDecoration? {
        if (newRev == null) return null
        val compStart = newRev.compositionStartUtf16
        val compEnd = newRev.compositionEndUtf16
        if (compStart < 0 || compEnd < 0 || compStart >= compEnd) return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = compStart,
            endUtf16 = compEnd,
            underlineColor = android.graphics.Color.BLACK
        )
    }

    private fun collectExcludedNewByteRanges(
        slices: List<PreparedVisualTransaction.AnimatedSlice>
    ): Set<Pair<Int, Int>> {
        val excluded = mutableSetOf<Pair<Int, Int>>()
        for (slice in slices) {
            if (slice.role == SliceRole.Insert || slice.role == SliceRole.CrossfadeNew) {
                if (slice.clusterByteStart >= 0 && slice.clusterByteEndExclusive >= 0) {
                    excluded.add(Pair(slice.clusterByteStart, slice.clusterByteEndExclusive))
                }
            }
        }
        return excluded
    }

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SnapshotAnimation, SystemSuppressed
    }
}
