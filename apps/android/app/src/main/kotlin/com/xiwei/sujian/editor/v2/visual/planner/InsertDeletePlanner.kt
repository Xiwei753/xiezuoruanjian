package com.xiwei.sujian.editor.v2.visual.planner

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.SliceRole

class InsertDeletePlanner {

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
        planClusterReplaceAnimation: () -> Unit
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        val isReplace = visualIntent.isReplace()
            || visualIntent.isCompositionCommit()
            || visualIntent.isCompositionUpdate()

        if (isReplace) {
            planClusterReplaceAnimation()
            return
        }

        if (isInsert) {
            for (lineIndex in affectedNewLineIndices) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
                val insertClusters = newSnapshot.clusters.filter { cluster ->
                    visualIntent.newAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
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
            }
        } else if (isDelete) {
            for (lineIndex in affectedOldLineIndices) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
                val deleteClusters = oldSnapshot.clusters.filter { cluster ->
                    visualIntent.oldAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
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
        planRunReplaceAnimation: () -> Unit,
        groupClustersIntoRuns: (List<LineClusterSnapshot>, List<Pair<Int, Int>>) -> List<LineClusterSnapshot>
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete() || visualIntent.isCompositionCancel()
        val isReplace = visualIntent.isReplace()
            || visualIntent.isCompositionCommit()
            || visualIntent.isCompositionUpdate()

        if (isReplace) {
            planRunReplaceAnimation()
            return
        }

        if (isInsert) {
            for (lineIndex in affectedNewLineIndices) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
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
            }
        } else if (isDelete) {
            for (lineIndex in affectedOldLineIndices) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
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

    fun groupClustersIntoRuns(
        clusters: List<LineClusterSnapshot>,
        affectedRanges: List<Pair<Int, Int>>
    ): List<LineClusterSnapshot> {
        if (clusters.isEmpty()) return emptyList()
        val affected = clusters.filter { cluster ->
            affectedRanges.any { (start, end) ->
                cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
            }
        }
        if (affected.isEmpty()) return emptyList()
        if (affected.size == 1) return affected

        val runs = mutableListOf<LineClusterSnapshot>()
        var runStart = 0
        for (i in 1..affected.size) {
            val isEndOfRun = i == affected.size ||
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
                    runs.add(LineClusterSnapshot(
                        clusterId = first.clusterId,
                        documentByteStart = first.documentByteStart,
                        documentByteEndExclusive = last.documentByteEndExclusive,
                        documentUtf16Start = first.documentUtf16Start,
                        documentUtf16EndExclusive = last.documentUtf16EndExclusive,
                        sourceRectInLineImage = mergedSourceRect,
                        visualRectInDocument = mergedVisualRect,
                        shapingFingerprint = mergedFingerprint,
                        shapingIdentityConfident = allConfident
                    ))
                }
                runStart = i
            }
        }
        return runs
    }
}
