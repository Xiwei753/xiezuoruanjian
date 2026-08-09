package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole

class MoveCrossfadePlanner {
    fun addMoveSlicesForShiftedClustersCrossLine(
        allOldSnapshots: Map<Int, AndroidLineSnapshot>,
        allNewSnapshots: Map<Int, AndroidLineSnapshot>,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        excludedNewByteRanges: Set<Pair<Int, Int>>,
        excludedOldByteRanges: Set<Pair<Int, Int>>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        offsetMapper: (Int) -> Int?,
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
        val newUsed = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldInfo) in allOldClusters) {
            val isDeleted =
                visualIntent.oldAffectedByteRanges.any { (start, end) ->
                    oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
                }
            if (isDeleted) continue
            val isAlreadyHandled =
                excludedOldByteRanges.any { (start, end) ->
                    oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
                }
            if (isAlreadyHandled) continue
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchedNewIdx: Int? = null
            if (mappedStart != null) {
                matchedNewIdx =
                    allNewClusters.indices.firstOrNull { i ->
                        i !in newUsed && allNewClusters[i].first.documentByteStart == mappedStart &&
                            (mappedEnd == null || allNewClusters[i].first.documentByteEndExclusive == mappedEnd) &&
                            allNewClusters[i].first.documentByteStart >= lastMatchedNewStart
                    }
            }
            if (matchedNewIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates =
                    allNewClusters.indices.filter { i ->
                        val candidate = allNewClusters[i].first
                        i !in newUsed &&
                            candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                            candidate.documentByteStart >= referenceStart &&
                            visualIntent.newAffectedByteRanges.none { (start, end) ->
                                candidate.documentByteStart < end && candidate.documentByteEndExclusive > start
                            }
                    }
                matchedNewIdx =
                    candidates.minByOrNull { i ->
                        val candidateStart = allNewClusters[i].first.documentByteStart
                        val target = mappedStart ?: lastMatchedNewStart
                        kotlin.math.abs(candidateStart - target)
                    }
            }
            if (matchedNewIdx != null) {
                newUsed.add(matchedNewIdx)
                lastMatchedNewStart = allNewClusters[matchedNewIdx].first.documentByteStart
                val (newCluster, newInfo) = allNewClusters[matchedNewIdx]
                val isExcluded =
                    excludedNewByteRanges.any { (start, end) ->
                        newCluster.documentByteStart < end && newCluster.documentByteEndExclusive > start
                    }
                if (isExcluded) continue
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
                val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
                if (!positionChanged && identityConfident && !fingerprintChanged) continue
                val newSnapshot = newInfo.second
                val oldSnapshot = oldInfo.second
                if (identityConfident && !fingerprintChanged && positionChanged) {
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.Move,
                            snapshot = newSnapshot,
                            sourceRect = newCluster.sourceRectInLineImage,
                            destinationRect = newCluster.visualRectInDocument,
                            startAlpha = 1f,
                            endAlpha = 1f,
                            fromDestinationRect = oldCluster.visualRectInDocument,
                            clusterByteStart = newCluster.documentByteStart,
                            clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                        ),
                    )
                } else {
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.CrossfadeOld,
                            snapshot = oldSnapshot,
                            sourceRect = oldCluster.sourceRectInLineImage,
                            destinationRect = oldCluster.visualRectInDocument,
                            startAlpha = 1f,
                            endAlpha = 0f,
                            clusterByteStart = oldCluster.documentByteStart,
                            clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                        ),
                    )
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.CrossfadeNew,
                            snapshot = newSnapshot,
                            sourceRect = newCluster.sourceRectInLineImage,
                            destinationRect = newCluster.visualRectInDocument,
                            startAlpha = 0f,
                            endAlpha = 1f,
                            clusterByteStart = newCluster.documentByteStart,
                            clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                        ),
                    )
                }
            }
        }
    }

    fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        createSnapshotFromRevision: (AndroidLayoutRevision, Int, Boolean) -> AndroidLineSnapshot?,
        offsetMapper: (Int) -> Int?,
    ) {
        val affectedOldParagraphIds = mutableSetOf<Int>()
        val affectedNewParagraphIds = mutableSetOf<Int>()
        for (lineIndex in affectedOldLineIndices) {
            oldRev.lineRanges.getOrNull(lineIndex)?.paragraphId?.let { affectedOldParagraphIds.add(it) }
        }
        for (lineIndex in affectedNewLineIndices) {
            newRev.lineRanges.getOrNull(lineIndex)?.paragraphId?.let { affectedNewParagraphIds.add(it) }
        }

        val allOldClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for (lineEntry in oldRev.lineRanges.withIndex()) {
            if (lineEntry.value.paragraphId !in affectedOldParagraphIds) continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineEntry.index, false) ?: continue
            for (cluster in oldSnapshot.clusters) {
                allOldClusters.add(Pair(cluster, oldSnapshot))
            }
        }

        val allNewClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for (lineEntry in newRev.lineRanges.withIndex()) {
            if (lineEntry.value.paragraphId !in affectedNewParagraphIds) continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineEntry.index, true) ?: continue
            for (cluster in newSnapshot.clusters) {
                allNewClusters.add(Pair(cluster, newSnapshot))
            }
        }

        val newUsed = mutableSetOf<Int>()
        val oldMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0

        for ((oldIdx, pair) in allOldClusters.withIndex()) {
            val (oldCluster, oldSnapshot) = pair
            val isDeleted =
                visualIntent.oldAffectedByteRanges.any { (start, end) ->
                    oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
                }
            if (isDeleted) continue

            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchedNewIdx: Int? = null
            if (mappedStart != null) {
                matchedNewIdx =
                    allNewClusters.indices.firstOrNull { i ->
                        i !in newUsed && allNewClusters[i].first.documentByteStart == mappedStart &&
                            (mappedEnd == null || allNewClusters[i].first.documentByteEndExclusive == mappedEnd) &&
                            allNewClusters[i].first.documentByteStart >= lastMatchedNewStart
                    }
            }
            if (matchedNewIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates =
                    allNewClusters.indices.filter { i ->
                        val candidate = allNewClusters[i].first
                        i !in newUsed &&
                            candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                            candidate.documentByteStart >= referenceStart &&
                            visualIntent.newAffectedByteRanges.none { (start, end) ->
                                candidate.documentByteStart < end && candidate.documentByteEndExclusive > start
                            }
                    }
                val target = mappedStart ?: lastMatchedNewStart
                matchedNewIdx =
                    candidates.minByOrNull { i ->
                        kotlin.math.abs(allNewClusters[i].first.documentByteStart - target)
                    }
            }
            if (matchedNewIdx != null) {
                newUsed.add(matchedNewIdx)
                oldMatched.add(oldIdx)
                lastMatchedNewStart = allNewClusters[matchedNewIdx].first.documentByteStart
                val (newCluster, newSnapshot) = allNewClusters[matchedNewIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
                val fingerprintSame = oldCluster.shapingFingerprint == newCluster.shapingFingerprint
                if (identityConfident && fingerprintSame && !positionChanged) {
                } else if (identityConfident && fingerprintSame && positionChanged) {
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.Move,
                            snapshot = newSnapshot,
                            sourceRect = newCluster.sourceRectInLineImage,
                            destinationRect = newCluster.visualRectInDocument,
                            startAlpha = 1f,
                            endAlpha = 1f,
                            fromDestinationRect = oldCluster.visualRectInDocument,
                            clusterByteStart = newCluster.documentByteStart,
                            clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                        ),
                    )
                } else {
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.CrossfadeOld,
                            snapshot = oldSnapshot,
                            sourceRect = oldCluster.sourceRectInLineImage,
                            destinationRect = oldCluster.visualRectInDocument,
                            startAlpha = 1f,
                            endAlpha = 0f,
                            clusterByteStart = oldCluster.documentByteStart,
                            clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                        ),
                    )
                    animatedSlices.add(
                        PreparedVisualTransaction.AnimatedSlice(
                            role = SliceRole.CrossfadeNew,
                            snapshot = newSnapshot,
                            sourceRect = newCluster.sourceRectInLineImage,
                            destinationRect = newCluster.visualRectInDocument,
                            startAlpha = 0f,
                            endAlpha = 1f,
                            clusterByteStart = newCluster.documentByteStart,
                            clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                        ),
                    )
                }
            }
        }

        for ((oldIdx, pair) in allOldClusters.withIndex()) {
            if (oldIdx in oldMatched) continue
            val (oldCluster, oldSnapshot) = pair
            animatedSlices.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Delete,
                    snapshot = oldSnapshot,
                    sourceRect = oldCluster.sourceRectInLineImage,
                    destinationRect = oldCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 0f,
                    clusterByteStart = oldCluster.documentByteStart,
                    clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                ),
            )
        }

        for ((newIdx, pair) in allNewClusters.withIndex()) {
            if (newIdx in newUsed) continue
            val (newCluster, newSnapshot) = pair
            animatedSlices.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    startAlpha = 0f,
                    endAlpha = 1f,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                ),
            )
        }
    }

    fun planCrossfadeAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        createSnapshotFromRevision: (AndroidLayoutRevision, Int, Boolean) -> AndroidLineSnapshot?,
        offsetMapper: (Int) -> Int?,
    ) {
        val matchedNewLineIndices = mutableSetOf<Int>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue

            var bestNewLineIdx: Int? = null
            val mappedStart = offsetMapper(oldLineRange.startUtf8)
            if (mappedStart != null) {
                for (newLineIdx in affectedNewLineIndices) {
                    if (newLineIdx in matchedNewLineIndices) continue
                    val newLineRange = newRev.lineRanges.getOrNull(newLineIdx) ?: continue
                    if (newLineRange.startUtf8 == mappedStart) {
                        bestNewLineIdx = newLineIdx
                        break
                    }
                }
            }
            if (bestNewLineIdx == null) {
                val mappedEnd = offsetMapper(oldLineRange.endUtf8)
                if (mappedEnd != null) {
                    for (newLineIdx in affectedNewLineIndices) {
                        if (newLineIdx in matchedNewLineIndices) continue
                        val newLineRange = newRev.lineRanges.getOrNull(newLineIdx) ?: continue
                        if (newLineRange.endUtf8 == mappedEnd) {
                            bestNewLineIdx = newLineIdx
                            break
                        }
                    }
                }
            }
            if (bestNewLineIdx == null) {
                for (newLineIdx in affectedNewLineIndices) {
                    if (newLineIdx in matchedNewLineIndices) continue
                    val newLineRange = newRev.lineRanges.getOrNull(newLineIdx) ?: continue
                    if (oldLineRange.startUtf8 < newLineRange.endUtf8 &&
                        oldLineRange.endUtf8 > newLineRange.startUtf8
                    ) {
                        bestNewLineIdx = newLineIdx
                        break
                    }
                }
            }

            if (bestNewLineIdx != null) {
                matchedNewLineIndices.add(bestNewLineIdx)
                val newLineRange = newRev.lineRanges.getOrNull(bestNewLineIdx) ?: continue
                val newSnapshot = createSnapshotFromRevision(newRev, bestNewLineIdx, true) ?: continue
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeOld,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect =
                            android.graphics.RectF(
                                oldLineRange.left,
                                oldLineRange.top,
                                oldLineRange.right,
                                oldLineRange.bottom,
                            ),
                        startAlpha = 1f,
                        endAlpha = 0f,
                    ),
                )
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeNew,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect =
                            android.graphics.RectF(
                                newLineRange.left,
                                newLineRange.top,
                                newLineRange.right,
                                newLineRange.bottom,
                            ),
                        startAlpha = 0f,
                        endAlpha = 1f,
                    ),
                )
            } else {
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect =
                            android.graphics.RectF(
                                oldLineRange.left,
                                oldLineRange.top,
                                oldLineRange.right,
                                oldLineRange.bottom,
                            ),
                        startAlpha = 1f,
                        endAlpha = 0f,
                    ),
                )
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            if (lineIndex in matchedNewLineIndices) continue
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
            animatedSlices.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = newSnapshot,
                    sourceRect = newSnapshot.sourceRect,
                    destinationRect =
                        android.graphics.RectF(
                            newLineRange.left,
                            newLineRange.top,
                            newLineRange.right,
                            newLineRange.bottom,
                        ),
                    startAlpha = 0f,
                    endAlpha = 1f,
                ),
            )
        }
    }
}
