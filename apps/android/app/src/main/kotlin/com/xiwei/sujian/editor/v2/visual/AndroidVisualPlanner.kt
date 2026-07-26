package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.visual.planner.AffectedLayoutPlanner
import com.xiwei.sujian.editor.v2.visual.planner.InsertDeletePlanner
import com.xiwei.sujian.editor.v2.visual.planner.MoveCrossfadePlanner
import com.xiwei.sujian.editor.v2.visual.planner.RebasePlanner
import com.xiwei.sujian.editor.v2.visual.planner.BlockShiftPlanner
import com.xiwei.sujian.editor.v2.visual.planner.SnapshotPlanner
import uniffi.writer_core.AnimationModeDto

typealias AffectedLinesResult = AffectedLayoutPlanner.AffectedLinesResult

/**
 * Pure visual planner: transforms (VisualIntent + old/new layout snapshots) into
 * [PreparedVisualTransaction] without holding mutable state, advancing time, or
 * releasing resources.
 *
 * Unified output model: every animation mode (Glyph/Cluster/Run/LineReflow/Snapshot)
 * produces the same set of slice roles — Static, Insert, Delete, Move, CrossfadeOld,
 * CrossfadeNew. The mode only determines the *granularity* of Insert/Delete/Crossfade
 * grouping; retained text whose position changed always gets Move (if shaping identity
 * is confident) or Crossfade (if not), regardless of mode.
 */
class AndroidVisualPlanner(
    internal val affectedLayoutPlanner: AffectedLayoutPlanner = AffectedLayoutPlanner(),
    internal val insertDeletePlanner: InsertDeletePlanner = InsertDeletePlanner(),
    internal val moveCrossfadePlanner: MoveCrossfadePlanner = MoveCrossfadePlanner(),
    internal val rebasePlanner: RebasePlanner = RebasePlanner(),
    internal val blockShiftPlanner: BlockShiftPlanner = BlockShiftPlanner(),
    internal val snapshotPlanner: SnapshotPlanner = SnapshotPlanner()
) {

    fun computeAffectedLineIndices(
        visualIntent: VisualIntent,
        revision: AndroidLayoutRevision?,
        useNewRanges: Boolean = false
    ): Set<Int> = affectedLayoutPlanner.computeAffectedLineIndices(visualIntent, revision, useNewRanges)

    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?
    ): AffectedLinesResult = affectedLayoutPlanner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRevision, newRevision)

    fun computeStructurallyAffectedOldLineIndices(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision
    ): Set<Int> = affectedLayoutPlanner.computeStructurallyAffectedOldLineIndices(visualIntent, oldRevision)

    /**
     * Build a [PreparedVisualTransaction] from visual intent, layout revisions, and snapshots.
     *
     * [transactionKey] is generated exactly once by [AndroidTextAnimationEngine.prepare] and
     * passed here — the planner must NOT generate its own key. All snapshots registered by the
     * engine are owned by [OwnedByTransaction(transactionKey)], and the planner must use this
     * same key in the returned [PreparedVisualTransaction.transactionId] so that
     * [AndroidTextAnimationEngine.completeTransaction]/[cancelTransaction] can release resources
     * under the correct owner. A mismatched key would cause [VisualResourceStore.release] to
     * silently ignore the release (owner check fails), leaking Bitmaps.
     *
     * [ownedSnapshotIds] is the optimistic set from the engine (all captured + rebase snapshots).
     * [AndroidTextAnimationEngine.submit] trims it to the precise set referenced by slices/patches.
     *
     * [snapshotLookup] provides access to snapshots from both fresh captures and the rebase frame's
     * surviving slices. The planner looks up snapshots by ID when constructing slices — without
     * rebase entries, surviving slices would reference snapshot IDs that produce null lookups,
     * causing the renderer to silently skip them (lost animation state).
     */
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
        var blockShifts = listOf<PreparedVisualTransaction.BlockShift>()

        val oldRev = oldRevision
        val newRev = newRevision

        if (oldRev != null && newRev != null) {
            val affectedResult = affectedLayoutPlanner.computeAffectedLines(visualIntent, oldRev, newRev)
            val affectedOldLineIndices = affectedResult.oldLineIndices
            val affectedNewLineIndices = affectedResult.newLineIndices
            blockShifts = affectedResult.blockShifts
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    insertDeletePlanner.planClusterLevelAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots,
                        { rev, lineIdx, isNew -> snapshotPlanner.createSnapshotFromRevision(rev, lineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = isNew) },
                        { planClusterReplaceAnimation(visualIntent, oldRev, newRev, affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots) }
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
                    )
                }
                AnimationMode.RunAnimation -> {
                    insertDeletePlanner.planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots,
                        { rev, lineIdx, isNew -> snapshotPlanner.createSnapshotFromRevision(rev, lineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = isNew) },
                        { planRunReplaceAnimation(visualIntent, oldRev, newRev, affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots) },
                        { clusters, ranges -> insertDeletePlanner.groupClustersIntoRuns(clusters, ranges) }
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.SnapshotAnimation -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
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
            rebasePlanner.applyRebaseToSlices(animatedSlices, rebaseSnapshot, snapshotLookup)
        } else {
            animatedSlices
        }

        val offsetMapperForRebase = if (oldRev != null && newRev != null) {
            affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
        } else null

        val reverseMapperForRebase = if (oldRev != null && newRev != null) {
            affectedLayoutPlanner.buildReverseOffsetMapper(visualIntent, oldRev, newRev)
        } else null

        val finalBlockShifts = if (rebaseSnapshot != null && rebaseSnapshot.blockShiftStates.isNotEmpty()) {
            blockShiftPlanner.applyRebaseToBlockShifts(blockShifts, rebaseSnapshot, offsetMapperForRebase, reverseMapperForRebase)
        } else {
            blockShifts
        }

        val referencedSnapshotIds = mutableSetOf<Long>()
        for (slice in finalSlices) {
            val sid = slice.snapshot?.snapshotId
            if (sid != null && sid > 0) {
                referencedSnapshotIds.add(sid)
            }
        }
        for (patch in staticPatches) {
            referencedSnapshotIds.add(patch.newSnapshotId)
        }

         return PreparedVisualTransaction(
             transactionId = transactionKey,
             oldRevision = oldRev,
             newRevision = newRev,
             staticPatches = staticPatches,
             animatedSlices = finalSlices,
             ownedSnapshotIds = ownedSnapshotIds,
             referencedSnapshotIds = referencedSnapshotIds,
              selectionDecoration = newRev?.let { snapshotPlanner.buildSelectionDecoration(it) },
              preeditDecoration = newRev?.let { snapshotPlanner.buildPreeditDecoration(it) },
             cursorTransition = cursorTransition,
             durationMs = durationMs,
             blockShifts = finalBlockShifts,
             operationKind = visualIntent.operationKind
         )
    }

    /**
     * Replace-mode cluster animation: matches old→new clusters by shaping fingerprint +
     * byte-length equality with closest-offset tiebreaker.
     *
     * This differs from [addMoveSlicesForShiftedClustersCrossLine] which uses the edit's
     * OffsetMap for identity matching. Replace uses direct fingerprint matching because
     * offset mapping is ambiguous when old/new ranges overlap differently (e.g. composition
     * commit where the replaced range differs from the committed text length). The
     * byte-length equality check prevents matching clusters of different visual width.
     */
    private fun planClusterReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = snapshotPlanner.createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
            if (oldSnapshot.clusters.isNotEmpty()) {
                for (cluster in oldSnapshot.clusters) {
                    val inOldRange = visualIntent.oldAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                    }
                    if (inOldRange) {
                        allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
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

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = snapshotPlanner.createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
            if (newSnapshot.clusters.isNotEmpty()) {
                for (cluster in newSnapshot.clusters) {
                    val inNewRange = visualIntent.newAffectedByteRanges.any { (start, end) ->
                        cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
                    }
                    if (inNewRange) {
                        allNewAffectedClusters.add(Pair(cluster, newSnapshot))
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

        val offsetMapper = affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
        val newMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchIdx: Int? = null
            if (mappedStart != null) {
                matchIdx = allNewAffectedClusters.indices.firstOrNull { i ->
                    i !in newMatched && allNewAffectedClusters[i].first.documentByteStart == mappedStart &&
                        (mappedEnd == null || allNewAffectedClusters[i].first.documentByteEndExclusive == mappedEnd) &&
                        allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
                }
            }
            if (matchIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates = allNewAffectedClusters.indices.filter { i ->
                    val candidate = allNewAffectedClusters[i].first
                    i !in newMatched &&
                        candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                        candidate.documentByteEndExclusive - candidate.documentByteStart ==
                        oldCluster.documentByteEndExclusive - oldCluster.documentByteStart &&
                        candidate.documentByteStart >= referenceStart
                }
                val target = mappedStart ?: lastMatchedNewStart
                matchIdx = candidates.minByOrNull { i ->
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
            } else {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Delete,
                    snapshot = oldSnapshot,
                    sourceRect = oldCluster.sourceRectInLineImage,
                    destinationRect = oldCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 0f,
                    clusterByteStart = oldCluster.documentByteStart,
                    clusterByteEndExclusive = oldCluster.documentByteEndExclusive
                ))
            }
        }

        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            val (newCluster, newSnapshot) = pair
            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
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

    /**
     * Run-level replace animation: groups clusters into runs, then matches old→new runs
     * by shaping fingerprint + byte length with closest-offset tiebreaker.
     * Matched runs with same shaping + confident fingerprint → Move; otherwise → Crossfade pair.
     * Unmatched old runs → Delete; unmatched new runs → Insert.
     */
    private fun planRunReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = snapshotPlanner.createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
            if (visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = insertDeletePlanner.groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = snapshotPlanner.createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
            if (visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = insertDeletePlanner.groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                }
            }
        }

        val offsetMapper = affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
        val newMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchIdx: Int? = null
            if (mappedStart != null) {
                matchIdx = allNewAffectedClusters.indices.firstOrNull { i ->
                    i !in newMatched && allNewAffectedClusters[i].first.documentByteStart == mappedStart &&
                        (mappedEnd == null || allNewAffectedClusters[i].first.documentByteEndExclusive == mappedEnd) &&
                        allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
                }
            }
            if (matchIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates = allNewAffectedClusters.indices.filter { i ->
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
                matchIdx = candidates.minByOrNull { i ->
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
            } else {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Delete,
                    snapshot = oldSnapshot,
                    sourceRect = oldCluster.sourceRectInLineImage,
                    destinationRect = oldCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 0f,
                    clusterByteStart = oldCluster.documentByteStart,
                    clusterByteEndExclusive = oldCluster.documentByteEndExclusive
                ))
            }
        }

        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            val (newCluster, newSnapshot) = pair
            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
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

    /**
     * Line-level reflow animation for mid-paragraph inserts/deletes that cause line wrapping changes.
     *
     * Global cluster matching: all clusters from the affected paragraph group are collected,
     * then matched one-to-one by offset map (primary) or fingerprint (fallback). Matched
     * clusters with confident shaping → Move; otherwise → CrossfadeOld + CrossfadeNew.
     * Unmatched old clusters → per-cluster Delete. Unmatched new clusters → per-cluster Insert.
     *
     * No whole-line Insert/Delete: when a visual line exists only on one side (e.g. soft-wrap
     * added/removed a line), its clusters are still matched individually against clusters in
     * the paired paragraph. Only truly unmatched clusters become Insert/Delete. This prevents
     * the same text from being animated twice — once as a whole-line slice and once as
     * per-cluster Move/Crossfade — which caused ghosting during line-count changes and
     * hard-break split/merge.
     *
     * Paragraph alignment: old/new paragraphs are matched by their UTF-8 byte range via
     * [affectedLayoutPlanner.buildOffsetMapper], NOT by [paragraphId]. Inserting or deleting a hard break changes
     * all subsequent paragraphIds (they are sequential integers), so ID-based matching would
     * pair different paragraphs. Offset-map matching ensures the same text paragraph is
     * aligned even after hard-break insertion/deletion.
     *
     * No reliable cluster identity match → CrossfadeOld + CrossfadeNew (not whole-line Move).
     * Whole-line Move is only valid when the line content is confirmed identical; without
     * cluster matching, the new text would appear at the old position, and the old text
     * would have no exit animation.
     */
    private fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val offsetMapper = affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)

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
            val oldSnapshot = snapshotPlanner.createSnapshotFromRevision(oldRev, lineEntry.index, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
            for (cluster in oldSnapshot.clusters) {
                allOldClusters.add(Pair(cluster, oldSnapshot))
            }
        }

        val allNewClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for (lineEntry in newRev.lineRanges.withIndex()) {
            if (lineEntry.value.paragraphId !in affectedNewParagraphIds) continue
            val newSnapshot = snapshotPlanner.createSnapshotFromRevision(newRev, lineEntry.index, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
            for (cluster in newSnapshot.clusters) {
                allNewClusters.add(Pair(cluster, newSnapshot))
            }
        }

        val newUsed = mutableSetOf<Int>()
        val oldMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0

        for ((oldIdx, pair) in allOldClusters.withIndex()) {
            val (oldCluster, oldSnapshot) = pair
            val isDeleted = visualIntent.oldAffectedByteRanges.any { (start, end) ->
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
            }
            if (isDeleted) continue

            val mappedStart = offsetMapper(oldCluster.documentByteStart)
            val mappedEnd = offsetMapper(oldCluster.documentByteEndExclusive)
            var matchedNewIdx: Int? = null
            if (mappedStart != null) {
                matchedNewIdx = allNewClusters.indices.firstOrNull { i ->
                    i !in newUsed && allNewClusters[i].first.documentByteStart == mappedStart &&
                        (mappedEnd == null || allNewClusters[i].first.documentByteEndExclusive == mappedEnd) &&
                        allNewClusters[i].first.documentByteStart >= lastMatchedNewStart
                }
            }
            if (matchedNewIdx == null) {
                val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
                val candidates = allNewClusters.indices.filter { i ->
                    val candidate = allNewClusters[i].first
                    i !in newUsed &&
                        candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                        candidate.documentByteStart >= referenceStart &&
                        visualIntent.newAffectedByteRanges.none { (start, end) ->
                            candidate.documentByteStart < end && candidate.documentByteEndExclusive > start
                        }
                }
                val target = mappedStart ?: lastMatchedNewStart
                matchedNewIdx = candidates.minByOrNull { i ->
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
                    // Identity reliable, position unchanged → static new layout handles it
                } else if (identityConfident && fingerprintSame && positionChanged) {
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
                    // Identity unreliable or fingerprint differs → CrossfadeOld + CrossfadeNew
                    // regardless of position change
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

        for ((oldIdx, pair) in allOldClusters.withIndex()) {
            if (oldIdx in oldMatched) continue
            val (oldCluster, oldSnapshot) = pair
            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = oldSnapshot,
                sourceRect = oldCluster.sourceRectInLineImage,
                destinationRect = oldCluster.visualRectInDocument,
                startAlpha = 1f,
                endAlpha = 0f,
                clusterByteStart = oldCluster.documentByteStart,
                clusterByteEndExclusive = oldCluster.documentByteEndExclusive
            ))
        }

        for ((newIdx, pair) in allNewClusters.withIndex()) {
            if (newIdx in newUsed) continue
            val (newCluster, newSnapshot) = pair
            animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
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



    /**
     * Whole-line crossfade for SnapshotAnimation mode.
     *
     * Old/new lines are paired by offset map: each pair generates both CrossfadeOld (old
     * line fades out) and CrossfadeNew (new line fades in). Lines only on the old side
     * become Delete; lines only on the new side become Insert.
     *
     * Previous implementation skipped new lines whose byte range overlapped with a mapped
     * old line, causing old content to fade out without a corresponding new-content fade-in.
     * The new implementation explicitly pairs old→new lines and always generates both halves
     * of the crossfade, ensuring the visual transition is complete: old text fades out while
     * new text simultaneously fades in at the same position.
     */
    private fun planCrossfadeAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val offsetMapper = affectedLayoutPlanner.buildOffsetMapper(visualIntent, oldRev, newRev)
        val matchedNewLineIndices = mutableSetOf<Int>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = snapshotPlanner.createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue

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
                    if (oldLineRange.startUtf8 < newLineRange.endUtf8 && oldLineRange.endUtf8 > newLineRange.startUtf8) {
                        bestNewLineIdx = newLineIdx
                        break
                    }
                }
            }

            if (bestNewLineIdx != null) {
                matchedNewLineIndices.add(bestNewLineIdx)
                val newLineRange = newRev.lineRanges.getOrNull(bestNewLineIdx) ?: continue
                val newSnapshot = snapshotPlanner.createSnapshotFromRevision(newRev, bestNewLineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.CrossfadeNew,
                    snapshot = newSnapshot,
                    sourceRect = newSnapshot.sourceRect,
                    destinationRect = android.graphics.RectF(
                        newLineRange.left, newLineRange.top,
                        newLineRange.right, newLineRange.bottom
                    ),
                    startAlpha = 0f,
                    endAlpha = 1f
                ))
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

        for (lineIndex in affectedNewLineIndices) {
            if (lineIndex in matchedNewLineIndices) continue
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = snapshotPlanner.createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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

    /**
     * No-animation path: produces no slices or static patches. When animation is suppressed
     * (SystemSuppressed), the new layout is rendered directly without any transition — the
     * static renderer draws the full text from the new layout, and no overlay animation is needed.
     */
    private fun planNoAnimation(
        newRev: AndroidLayoutRevision,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
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

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SnapshotAnimation, SystemSuppressed
    }
}
