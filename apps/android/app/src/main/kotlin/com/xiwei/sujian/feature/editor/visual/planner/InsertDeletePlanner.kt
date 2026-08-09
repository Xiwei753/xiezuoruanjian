package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole

class InsertDeletePlanner {
    private val caretRevealPlanner = CaretRevealPlanner()

    fun planClusterLevelAnimation(
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
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        val isReplace =
            visualIntent.isReplace() ||
                visualIntent.isCompositionCommit() ||
                visualIntent.isCompositionUpdate()

        if (isReplace) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedOldLineIndices, affectedNewLineIndices,
                animatedSlices, staticPatches,
                preCapturedOldSnapshots, preCapturedNewSnapshots,
                createSnapshotFromRevision, offsetMapper,
            )
            return
        }

        if (isInsert) {
            val insertClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedNewLineIndices) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
                val insertClusters =
                    newSnapshot.clusters.filter { cluster ->
                        visualIntent.newAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                        }
                    }
                for (cluster in insertClusters) {
                    insertClusterSnapshots.add(Pair(cluster, newSnapshot))
                }
            }
            val revealSpecs = caretRevealPlanner.planRevealSpecs(insertClusterSnapshots.map { it.first })
            for ((i, pair) in insertClusterSnapshots.withIndex()) {
                val (cluster, snapshot) = pair
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = snapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive,
                        revealSpec = revealSpecs.getOrNull(i),
                    ),
                )
            }
        } else if (isDelete) {
            val deleteClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedOldLineIndices) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
                val deleteClusters =
                    oldSnapshot.clusters.filter { cluster ->
                        visualIntent.oldAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                        }
                    }
                for (cluster in deleteClusters) {
                    deleteClusterSnapshots.add(Pair(cluster, oldSnapshot))
                }
            }
            val swallowSpecs = caretRevealPlanner.planSwallowSpecs(deleteClusterSnapshots.map { it.first })
            for ((i, pair) in deleteClusterSnapshots.withIndex()) {
                val (cluster, snapshot) = pair
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = snapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive,
                        revealSpec = swallowSpecs.getOrNull(i),
                    ),
                )
            }
        }
    }

    fun planRunAnimation(
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
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        val isReplace =
            visualIntent.isReplace() ||
                visualIntent.isCompositionCommit() ||
                visualIntent.isCompositionUpdate()

        if (isReplace) {
            planRunReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedOldLineIndices, affectedNewLineIndices,
                animatedSlices, staticPatches,
                preCapturedOldSnapshots, preCapturedNewSnapshots,
                createSnapshotFromRevision, offsetMapper,
            )
            return
        }

        if (isInsert) {
            val insertClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedNewLineIndices) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
                val insertClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in insertClusters) {
                    insertClusterSnapshots.add(Pair(cluster, newSnapshot))
                }
            }
            val revealSpecs = caretRevealPlanner.planRevealSpecs(insertClusterSnapshots.map { it.first })
            for ((i, pair) in insertClusterSnapshots.withIndex()) {
                val (cluster, snapshot) = pair
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = snapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive,
                        revealSpec = revealSpecs.getOrNull(i),
                    ),
                )
            }
        } else if (isDelete) {
            val deleteClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedOldLineIndices) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
                val deleteClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in deleteClusters) {
                    deleteClusterSnapshots.add(Pair(cluster, oldSnapshot))
                }
            }
            val swallowSpecs = caretRevealPlanner.planSwallowSpecs(deleteClusterSnapshots.map { it.first })
            for ((i, pair) in deleteClusterSnapshots.withIndex()) {
                val (cluster, snapshot) = pair
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = snapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = cluster.documentByteStart,
                        clusterByteEndExclusive = cluster.documentByteEndExclusive,
                        revealSpec = swallowSpecs.getOrNull(i),
                    ),
                )
            }
        }
    }

    fun groupClustersIntoRuns(
        clusters: List<LineClusterSnapshot>,
        affectedRanges: List<Pair<Int, Int>>,
    ): List<LineClusterSnapshot> {
        if (clusters.isEmpty()) return emptyList()
        val affected =
            clusters.filter { cluster ->
                affectedRanges.any { (start, end) ->
                    cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                }
            }
        if (affected.isEmpty()) return emptyList()
        if (affected.size == 1) return affected

        val runs = mutableListOf<LineClusterSnapshot>()
        var runStart = 0
        for (i in 1..affected.size) {
            val isEndOfRun =
                i == affected.size ||
                    affected[i].documentByteStart != affected[i - 1].documentByteEndExclusive ||
                    affected[i].visualRectInDocument.top != affected[i - 1].visualRectInDocument.top
            if (isEndOfRun) {
                val runClusters = affected.subList(runStart, i)
                if (runClusters.size == 1) {
                    runs.add(runClusters[0])
                } else {
                    val first = runClusters.first()
                    val last = runClusters.last()
                    var minSrcL = runClusters[0].sourceRectInLineImage.left
                    var minSrcT = runClusters[0].sourceRectInLineImage.top
                    var maxSrcR = runClusters[0].sourceRectInLineImage.right
                    var maxSrcB = runClusters[0].sourceRectInLineImage.bottom
                    var minVisL = runClusters[0].visualRectInDocument.left
                    var minVisT = runClusters[0].visualRectInDocument.top
                    var maxVisR = runClusters[0].visualRectInDocument.right
                    var maxVisB = runClusters[0].visualRectInDocument.bottom
                    for (c in runClusters) {
                        minSrcL = minOf(minSrcL, c.sourceRectInLineImage.left)
                        minSrcT = minOf(minSrcT, c.sourceRectInLineImage.top)
                        maxSrcR = maxOf(maxSrcR, c.sourceRectInLineImage.right)
                        maxSrcB = maxOf(maxSrcB, c.sourceRectInLineImage.bottom)
                        minVisL = minOf(minVisL, c.visualRectInDocument.left)
                        minVisT = minOf(minVisT, c.visualRectInDocument.top)
                        maxVisR = maxOf(maxVisR, c.visualRectInDocument.right)
                        maxVisB = maxOf(maxVisB, c.visualRectInDocument.bottom)
                    }
                    val mergedSourceRect = android.graphics.Rect(minSrcL, minSrcT, maxSrcR, maxSrcB)
                    val mergedVisualRect = android.graphics.RectF(minVisL, minVisT, maxVisR, maxVisB)
                    val allConfident = runClusters.all { it.shapingIdentityConfident }
                    val mergedFingerprint = runClusters.joinToString("|") { it.shapingFingerprint }
                    runs.add(
                        LineClusterSnapshot(
                            clusterId = first.clusterId,
                            documentByteStart = first.documentByteStart,
                            documentByteEndExclusive = last.documentByteEndExclusive,
                            documentUtf16Start = first.documentUtf16Start,
                            documentUtf16EndExclusive = last.documentUtf16EndExclusive,
                            sourceRectInLineImage = mergedSourceRect,
                            visualRectInDocument = mergedVisualRect,
                            shapingFingerprint = mergedFingerprint,
                            shapingIdentityConfident = allConfident,
                            caretStartX = first.caretStartX,
                            caretEndX = last.caretEndX,
                        ),
                    )
                }
                runStart = i
            }
        }
        return runs
    }

    fun planClusterReplaceAnimation(
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
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
            // #605 评论3: clusters 为空时不生成 alpha fallback slice —
            // 无 cluster caret 几何时无法做 clip reveal，直接静态切换。
            if (oldSnapshot.clusters.isNotEmpty()) {
                for (cluster in oldSnapshot.clusters) {
                    val inOldRange =
                        visualIntent.oldAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                        }
                    if (inOldRange) {
                        allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                    }
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
            // #605 评论3: clusters 为空时不生成 alpha fallback slice —
            // 无 cluster caret 几何时无法做 clip reveal，直接静态切换。
            if (newSnapshot.clusters.isNotEmpty()) {
                for (cluster in newSnapshot.clusters) {
                    val inNewRange =
                        visualIntent.newAffectedByteRanges.any { (start, end) ->
                            cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                        }
                    if (inNewRange) {
                        allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                    }
                }
            }
        }

        val newMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchIdx: Int? = null
            if (mappedStart != null) {
                matchIdx =
                    allNewAffectedClusters.indices.firstOrNull { i ->
                        i !in newMatched &&
                            allNewAffectedClusters[i].first.documentByteStart == mappedStart &&
                            (
                                mappedEnd == null ||
                                    allNewAffectedClusters[i].first.documentByteEndExclusive == mappedEnd
                            ) &&
                            allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
                    }
            }
            if (matchIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates =
                    allNewAffectedClusters.indices.filter { i ->
                        val candidate = allNewAffectedClusters[i].first
                        i !in newMatched &&
                            candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                            candidate.documentByteEndExclusive - candidate.documentByteStart ==
                            oldCluster.documentByteEndExclusive - oldCluster.documentByteStart &&
                            candidate.documentByteStart >= referenceStart
                    }
                val target = mappedStart ?: lastMatchedNewStart
                matchIdx =
                    candidates.minByOrNull { i ->
                        kotlin.math.abs(allNewAffectedClusters[i].first.documentByteStart - target)
                    }
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                lastMatchedNewStart = allNewAffectedClusters[matchIdx].first.documentByteStart
                val (newCluster, newSnapshot) = allNewAffectedClusters[matchIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
                val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
                if (!positionChanged && identityConfident && !fingerprintChanged) {
                    continue
                } else if (identityConfident && !fingerprintChanged && positionChanged) {
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
            } else {
                val swallowSpec = caretRevealPlanner.planSwallowSpecs(listOf(oldCluster)).firstOrNull()
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldCluster.sourceRectInLineImage,
                        destinationRect = oldCluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = oldCluster.documentByteStart,
                        clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                        revealSpec = swallowSpec,
                    ),
                )
            }
        }

        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            val (newCluster, newSnapshot) = pair
            val revealSpec = caretRevealPlanner.planRevealSpecs(listOf(newCluster)).firstOrNull()
            animatedSlices.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 1f,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                    revealSpec = revealSpec,
                ),
            )
        }
    }

    fun planRunReplaceAnimation(
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
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
            if (visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
            if (visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                }
            }
        }

        val newMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchIdx: Int? = null
            if (mappedStart != null) {
                matchIdx =
                    allNewAffectedClusters.indices.firstOrNull { i ->
                        i !in newMatched &&
                            allNewAffectedClusters[i].first.documentByteStart == mappedStart &&
                            (
                                mappedEnd == null ||
                                    allNewAffectedClusters[i].first.documentByteEndExclusive == mappedEnd
                            ) &&
                            allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
                    }
            }
            if (matchIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates =
                    allNewAffectedClusters.indices.filter { i ->
                        val candidate = allNewAffectedClusters[i].first
                        i !in newMatched &&
                            candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                            candidate.documentByteEndExclusive - candidate.documentByteStart ==
                            oldCluster.documentByteEndExclusive - oldCluster.documentByteStart &&
                            candidate.documentByteStart >= referenceStart &&
                            visualIntent.newAffectedByteRanges.none { (start, end) ->
                                candidate.documentByteStart < end && candidate.documentByteEndExclusive > start
                            }
                    }
                val target = mappedStart ?: lastMatchedNewStart
                matchIdx =
                    candidates.minByOrNull { i ->
                        kotlin.math.abs(allNewAffectedClusters[i].first.documentByteStart - target)
                    }
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                lastMatchedNewStart = allNewAffectedClusters[matchIdx].first.documentByteStart
                val (newCluster, newSnapshot) = allNewAffectedClusters[matchIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
                val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
                if (!positionChanged && identityConfident && !fingerprintChanged) {
                    continue
                } else if (identityConfident && !fingerprintChanged && positionChanged) {
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
            } else {
                val swallowSpec = caretRevealPlanner.planSwallowSpecs(listOf(oldCluster)).firstOrNull()
                animatedSlices.add(
                    PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldCluster.sourceRectInLineImage,
                        destinationRect = oldCluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        clusterByteStart = oldCluster.documentByteStart,
                        clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                        revealSpec = swallowSpec,
                    ),
                )
            }
        }

        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            val (newCluster, newSnapshot) = pair
            val revealSpec = caretRevealPlanner.planRevealSpecs(listOf(newCluster)).firstOrNull()
            animatedSlices.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 1f,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                    revealSpec = revealSpec,
                ),
            )
        }
    }
}
