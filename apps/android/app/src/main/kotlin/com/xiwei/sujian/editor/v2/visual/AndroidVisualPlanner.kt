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
                        { rev, lineIdx, isNew -> createSnapshotFromRevision(rev, lineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = isNew) },
                        { planClusterReplaceAnimation(visualIntent, oldRev, newRev, affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots) }
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        buildOffsetMapper(visualIntent, oldRev, newRev)
                    )
                }
                AnimationMode.RunAnimation -> {
                    insertDeletePlanner.planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots,
                        { rev, lineIdx, isNew -> createSnapshotFromRevision(rev, lineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = isNew) },
                        { planRunReplaceAnimation(visualIntent, oldRev, newRev, affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots) },
                        { clusters, ranges -> insertDeletePlanner.groupClustersIntoRuns(clusters, ranges) }
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        buildOffsetMapper(visualIntent, oldRev, newRev)
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
            buildOffsetMapper(visualIntent, oldRev, newRev)
        } else null

        val reverseMapperForRebase = if (oldRev != null && newRev != null) {
            buildReverseOffsetMapper(visualIntent, oldRev, newRev)
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
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
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
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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

        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
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
     * Map an old-document byte offset into the new document after this edit.
     * Returns null for offsets inside a deleted/replaced range that has no retained identity.
     * Pure Insert/Delete are first-class: retained text after the edit point shifts by delta.
     * Replace uses [mapThroughRanges] for proportional mapping within affected ranges.
     *
     * Boundary convention: all byte ranges are half-open [start, end).
     * In [mapThroughRanges], `offset >= oldStart && offset < oldEnd` (not <= oldEnd)
     * ensures adjacent ranges don't share a boundary offset.
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
                // Half-open boundary: offset < oldAffectedStart is before the edit
                // (unchanged), offset >= oldAffectedEnd is after the edit (shifted by
                // the edit delta). Using >= (not >) for oldAffectedEnd is correct because
                // oldAffectedEnd is exclusive — the byte at oldAffectedEnd itself is the
                // first byte after the affected range and must be shifted.
                offset < oldAffectedStart -> offset
                offset >= oldAffectedEnd -> offset + shift
                else -> {
                    val mapped = mapThroughRanges(offset, oldRanges, newRanges)
                    if (mapped >= 0) mapped else null
                }
            }
        }
    }

    /**
     * Proportionally map [offset] within paired old/new affected ranges.
     * Half-open interval: `offset >= oldStart && offset < oldEnd` — the boundary
     * offset at oldEnd belongs to the *next* range, not this one.
     * Returns -1 if offset falls outside all old ranges.
     */
    private fun mapThroughRanges(offset: Int, oldRanges: List<Pair<Int, Int>>, newRanges: List<Pair<Int, Int>>): Int {
        for (i in oldRanges.indices) {
            val (oldStart, oldEnd) = oldRanges[i]
            if (offset >= oldStart && offset < oldEnd) {
                val newRange = newRanges.getOrNull(i) ?: continue
                val ratio = if (oldEnd == oldStart) 0f else (offset - oldStart).toFloat() / (oldEnd - oldStart)
                return newRange.first + (ratio * (newRange.second - newRange.first)).toInt()
            }
        }
        return -1
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
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
            if (visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = insertDeletePlanner.groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
            if (visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = insertDeletePlanner.groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                }
            }
        }

        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
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
     * [buildOffsetMapper], NOT by [paragraphId]. Inserting or deleting a hard break changes
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
        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)

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
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineEntry.index, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
            for (cluster in oldSnapshot.clusters) {
                allOldClusters.add(Pair(cluster, oldSnapshot))
            }
        }

        val allNewClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for (lineEntry in newRev.lineRanges.withIndex()) {
            if (lineEntry.value.paragraphId !in affectedNewParagraphIds) continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineEntry.index, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
        val matchedNewLineIndices = mutableSetOf<Int>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue

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
                val newSnapshot = createSnapshotFromRevision(newRev, bestNewLineIdx, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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


    /**
     * Reverse-map a new-document byte offset back into the old document.
     * Dual of [buildOffsetMapper]: given an offset in the new revision, returns the
     * corresponding offset in the old revision, or null if the offset falls inside
     * a range that was inserted (no old-document counterpart).
     *
     * Used by [computeAffectedLines] to detect new paragraphs created by a hard-break
     * split: if a new paragraph's [startUtf8] has no reverse mapping (null), the paragraph
     * was created by the split and must be included in [structurallyAffectedNewParaIds].
     *
     * Boundary convention: same half-open intervals as [buildOffsetMapper].
     */
    private fun reverseMapOffset(
        newOffset: Int,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Int? {
        val oldRanges = visualIntent.oldAffectedByteRanges
        val newRanges = visualIntent.newAffectedByteRanges
        if (oldRanges.isEmpty() && newRanges.isEmpty()) return newOffset
        if (oldRanges.isEmpty()) {
            val insertStart = newRanges.first().first
            val insertLen = newRanges.sumOf { (s, e) -> e - s }
            return if (newOffset < insertStart) newOffset
            else if (newOffset < insertStart + insertLen) null
            else newOffset - insertLen
        }
        // Pure Delete: no new bytes were created, so every new-document offset has an
        // unambiguous old-document counterpart — there is no "newly created content" that
        // would require returning null. Offsets before the delete are unchanged; offsets
        // at or after the delete start map forward by the deleted length.
        if (newRanges.isEmpty()) {
            val deleteStart = oldRanges.first().first
            val deleteLen = oldRanges.sumOf { (s, e) -> e - s }
            return if (newOffset < deleteStart) newOffset
            else newOffset + deleteLen
        }
        val newAffectedStart = newRanges.first().first
        val newAffectedEnd = newRanges.last().second
        if (newOffset < newAffectedStart) return newOffset
        if (newOffset >= newAffectedEnd) {
            val shift = newRanges.sumOf { (s, e) -> e - s } - oldRanges.sumOf { (s, e) -> e - s }
            return newOffset - shift
        }
        // Offsets inside the affected range have no unambiguous reverse mapping — the
        // same new offset could correspond to multiple old positions via proportional
        // mapping. Unlike [buildOffsetMapper] which uses [mapThroughRanges] for forward
        // proportional mapping, reverse mapping does not need this precision: callers
        // only use null as a signal that the offset falls inside newly-created content
        // (e.g. a paragraph created by a hard-break split), not for exact offset translation.
        return null
    }

    private fun buildReverseOffsetMapper(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): (Int) -> Int? {
        return { newOffset: Int -> reverseMapOffset(newOffset, visualIntent, oldRev, newRev) }
    }

    /**
     * Find the visual line index containing [byteOffset].
     *
     * Boundary convention: uses `<=` (not `<`) against [LineRange.endUtf8], so an offset
     * exactly at a line's endUtf8 still maps to that line rather than the next. This is
     * intentional for edit-point detection — the edit byte offset may land on the exclusive
     * boundary of the last affected line, and we must include that line in the affected set
     * rather than accidentally starting the scan one line later.
     */
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

    /**
     * Rebase new slices onto the current visual frame of the old transaction.
     *
     * One-to-one matching invariant: each old [SliceVisualState] can be matched to at most
     * one new slice. The matching proceeds in three tiers (most precise first), and each
     * tier respects the [usedRebaseIndices] set to prevent multiple new slices from reusing
     * the same old state — which would cause them to all start from the same position/alpha
     * instead of each continuing from a unique old slice.
     *
     * 1. [findRebaseStateByClusterByteRange] — exact byte range + role compatibility.
     * 2. [findRebaseStateByLineAndRole] — same visual line + role; used when byte ranges
     *    don't match exactly (e.g. cluster boundaries shifted) but the slice is on the
     *    same line with a compatible role.
     * 3. [findClosestRebaseStateByPosition] — nearest position with role compatibility (fallback).
     *
     * Unmatched old slices that are still fading out or mid-move become "surviving" slices
     * appended after the rebased new slices, ensuring no visual discontinuity.
     */
    private fun applyRebaseToSlices(
        slices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap()
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        val usedRebaseIndices = mutableSetOf<Int>()
        val rebaseMatches = mutableMapOf<Int, Int?>()
        for ((idx, slice) in slices.withIndex()) {
            val matchIdx = findRebaseIndexByClusterByteRange(slice, rebaseSnapshot, usedRebaseIndices)
                ?: findRebaseIndexByLineAndRole(slice, rebaseSnapshot, usedRebaseIndices)
                ?: findRebaseIndexClosestByPosition(slice, rebaseSnapshot, usedRebaseIndices)
            rebaseMatches[idx] = matchIdx
            if (matchIdx != null) {
                usedRebaseIndices.add(matchIdx)
            }
        }
        val rebasedNewSlices = slices.mapIndexed { idx, slice ->
            val rebaseIdx = rebaseMatches[idx]
            if (rebaseIdx != null) {
                applyRebaseState(slice, rebaseSnapshot.sliceVisualStates[rebaseIdx])
            } else {
                slice
            }
        }
        val matchedRebaseIndices = mutableSetOf<Int>()
        for ((_, matchIdx) in rebaseMatches) {
            if (matchIdx != null) {
                matchedRebaseIndices.add(matchIdx)
            }
        }
        // Surviving old slices: old-transaction slices that were NOT matched to any new slice.
        // These must continue their animation independently to avoid visual discontinuity.
        //
        // Three categories:
        // 1. Fading-out (Delete/CrossfadeOld) with alpha > 0.01: continue fading from current
        //    alpha to 0. Below 0.01 the visual difference is negligible, so the slice is dropped.
        // 2. Move with incomplete position or alpha: continue moving/fading to final position.
        // 3. Fading-in (Insert/CrossfadeNew) with alpha < 0.99: continue fading from current
        //    alpha toward 1. Fully opaque Insert/CrossfadeNew (alpha >= 0.99) that no new slice
        //    references are *implicitly* dropped — they become static text rendered by the normal
        //    layout, so no surviving slice is needed. This is intentional: an old Insert that
        //    reached near-full opacity and is not referenced by the new transaction has already
        //    been "absorbed" into the static new-layout text.
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
                if (currentRect != destRect || state.currentAlpha < 0.99f) {
                    survivingOldSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Move,
                        snapshot = snapshot,
                        sourceRect = sourceRect,
                        destinationRect = destRect,
                        startAlpha = state.currentAlpha,
                        endAlpha = 1f,
                        fromDestinationRect = currentRect,
                        clusterByteStart = state.clusterByteStart,
                        clusterByteEndExclusive = state.clusterByteEndExclusive
                    ))
                }
            } else if (!isFadingOut && state.currentAlpha < 0.99f) {
                val currentRect = android.graphics.RectF(
                    state.currentLeft, state.currentTop,
                    state.currentRight, state.currentBottom
                )
                val originalDestRect = android.graphics.RectF(
                    state.destinationLeft, state.destinationTop,
                    state.destinationRight, state.destinationBottom
                )
                val endAlpha = when (state.role) {
                    SliceRole.Insert, SliceRole.CrossfadeNew, SliceRole.Move -> 1f
                    else -> state.currentAlpha
                }
                survivingOldSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = state.role,
                    snapshot = snapshot,
                    sourceRect = sourceRect,
                    destinationRect = originalDestRect,
                    startAlpha = state.currentAlpha,
                    endAlpha = endAlpha,
                    fromDestinationRect = currentRect,
                    clusterByteStart = state.clusterByteStart,
                    clusterByteEndExclusive = state.clusterByteEndExclusive
                ))
            }
        }
        return rebasedNewSlices + survivingOldSlices
    }

    /**
     * Tier-1 rebase matching: exact byte range + role compatibility.
     *
     * [clusterByteStart]/[clusterByteEndExclusive] are in the *new* revision's coordinate space
     * (the new slice's byte range). The rebase snapshot's byte ranges are from the *old*
     * transaction's revision. For retained text (not inserted/deleted), byte ranges are
     * typically stable across revisions because the edit only changes text inside the affected
     * ranges — clusters outside those ranges keep the same byte offsets. This makes exact
     * byte-range matching reliable for the common case of retained text during rapid input.
     *
     * Two-pass matching: first try with lineIndex constraint (same visual line), then without.
     * The lineIndex constraint is a performance optimization that avoids scanning all rebase
     * states when the slice is on the same line as the old one. Removing it as a fallback
     * handles cross-line Moves where the new slice's destination line differs from the old
     * slice's source line.
     */
    private fun findRebaseIndexByClusterByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int> = emptySet()
    ): Int? {
        val cStart = slice.clusterByteStart
        val cEnd = slice.clusterByteEndExclusive
        if (cStart < 0 || cEnd < 0) return null
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        val exactMatch = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices &&
            rebaseSnapshot.sliceVisualStates[i].role in compatibleRoles &&
                rebaseSnapshot.sliceVisualStates[i].lineIndex == lineIndex &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteStart == cStart &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteEndExclusive == cEnd
        }
        if (exactMatch != null) return exactMatch
        return rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices &&
            rebaseSnapshot.sliceVisualStates[i].role in compatibleRoles &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteStart == cStart &&
                rebaseSnapshot.sliceVisualStates[i].clusterByteEndExclusive == cEnd
        }
    }

    /** Roles that can rebase onto each other. "Appearing" roles (Move/Insert/CrossfadeNew)
     *  are interchangeable; "disappearing" roles (Delete/CrossfadeOld) are interchangeable.
     *  This allows e.g. a new Move slice to continue from a rebase Insert's current position. */
    private fun compatibleRebaseRoles(role: SliceRole): Set<SliceRole> {
        return when (role) {
            SliceRole.Move -> setOf(SliceRole.Move, SliceRole.Insert, SliceRole.CrossfadeNew)
            SliceRole.Insert -> setOf(SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew)
            SliceRole.CrossfadeNew -> setOf(SliceRole.CrossfadeNew, SliceRole.Move, SliceRole.Insert)
            SliceRole.Delete -> setOf(SliceRole.Delete, SliceRole.CrossfadeOld)
            SliceRole.CrossfadeOld -> setOf(SliceRole.CrossfadeOld, SliceRole.Delete)
            SliceRole.Static -> setOf(SliceRole.Static)
        }
    }

    /**
     * Tier-2 rebase matching: same visual line + compatible role.
     *
     * When exact byte-range matching fails (e.g. cluster boundaries shifted due to the
     * new edit), this tier finds the closest rebase state on the same line with a
     * compatible role, using byte-start distance as the tiebreaker.
     *
     * Byte-start distance is preferred over visual-position distance because:
     * (a) byte offsets are deterministic and independent of layout (visual position
     *     changes due to reflow even when the text hasn't moved semantically);
     * (b) for same-line clusters with identical roles, byte order is a stable proxy
     *     for visual order in LTR text and is at least consistent in RTL;
     * (c) visual-position distance requires computing screen coordinates from the
     *     rebase snapshot's SliceVisualState, which adds complexity without improving
     *     accuracy for the common case of rapid consecutive input on the same line.
     *
     * One-to-one invariant: [usedRebaseIndices] prevents multiple new slices from
     * matching the same old state. Without this, two Insert slices on the same line
     * could both inherit the same rebase position/alpha, causing them to start from
     * identical on-screen coordinates instead of their respective positions.
     */
    private fun findRebaseIndexByLineAndRole(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int> = emptySet()
    ): Int? {
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        val sliceByteStart = slice.clusterByteStart
        return rebaseSnapshot.sliceVisualStates.indices
            .filter { i -> i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role in compatibleRoles && rebaseSnapshot.sliceVisualStates[i].lineIndex == lineIndex }
            .minByOrNull { i ->
                kotlin.math.abs(rebaseSnapshot.sliceVisualStates[i].clusterByteStart - sliceByteStart)
            }
    }

    /**
     * Tier-3 rebase matching: nearest position with role compatibility (fallback).
     *
     * Move slices are NOT filtered by lineIndex because cross-line Moves can originate
     * from a different visual line than their destination — filtering by the destination
     * line would miss the correct rebase source on the old line. All other roles
     * (Insert/Delete/CrossfadeOld/CrossfadeNew) stay on their original line, so they
     * are filtered by line to prevent matching against unrelated positions on other lines.
     *
     * Distance metric: Manhattan distance (|dx| + |dy|) in document coordinates.
     * This is preferred over Euclidean distance because (a) it avoids a sqrt call per
     * candidate, and (b) for text animation the visual error is proportional to the
     * sum of horizontal and vertical displacement, not the diagonal.
     */
    private fun findRebaseIndexClosestByPosition(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot,
        usedRebaseIndices: Set<Int> = emptySet()
    ): Int? {
        val sliceTop = slice.destinationRect.top
        val sliceLeft = slice.destinationRect.left
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val filterByLine = lineIndex >= 0 && slice.role != SliceRole.Move
        return rebaseSnapshot.sliceVisualStates.indices
            .filter { i -> i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role in compatibleRoles && (!filterByLine || rebaseSnapshot.sliceVisualStates[i].lineIndex == lineIndex) }
            .minByOrNull { i ->
                val dy = kotlin.math.abs(rebaseSnapshot.sliceVisualStates[i].currentTop - sliceTop)
                val dx = kotlin.math.abs(rebaseSnapshot.sliceVisualStates[i].currentLeft - sliceLeft)
                dy + dx
            }
    }

    /**
     * Rebase new BlockShifts onto the current visual state of the old transaction's
     * BlockShifts, with one-to-one matching invariant.
     *
     * When a new transaction arrives while a previous BlockShift is still animating,
     * the suffix text is at an intermediate Y position (currentTranslateY != 0).
     * Without rebase, the new transaction would start from translateY = -newDeltaY,
     * causing the suffix text to jump back to the old position before animating forward.
     *
     * Rebase adjusts deltaY so that the new animation starts from the on-screen position:
     *   adjustedDeltaY = newDeltaY - oldCurrentTranslateY
     *
     * Proof that progress=0 yields the old on-screen position:
     *   translateY(0) = adjustedDeltaY * (0 - 1) = -(newDeltaY - oldCurrentTranslateY)
     * The new layout's static text is at layout_2_Y. The old animation had the text at
     * layout_1_Y + currentTranslateY_old on screen. For continuity:
     *   layout_2_Y - adjustedDeltaY = layout_1_Y + currentTranslateY_old
     *   adjustedDeltaY = layout_2_Y - layout_1_Y - currentTranslateY_old
     *                  = newDeltaY - currentTranslateY_old
     *
     * currentTranslateY_old is negative during animation (text has not yet reached the
     * new-layout position), so subtracting it adds a positive correction. This ensures
     * the new animation starts from the exact on-screen position of the old animation.
     *
     * One-to-one matching invariant: each old [BlockShiftVisualState] can be matched
     * to at most one new BlockShift, enforced by [usedRebaseIndices]. Without this,
     * multiple new BlockShifts could all match the same old state (e.g. when a hard-break
     * insertion splits one old suffix block into two new blocks), causing both to inherit
     * the same currentTranslateY instead of each getting a unique rebase anchor.
     *
     * Matching uses [startUtf8] (byte offset) as the primary identity rather than
     * [startLineIndex]. Line indices shift across revisions when hard breaks are
     * inserted/deleted — the old transaction's line N may become line N+1 in the new
     * revision, causing line-index-based matching to pair the wrong BlockShifts. Byte
     * offsets are stable across revisions (they identify the same paragraph regardless
     * of how many visual lines precede it), so [startUtf8]-based matching is correct
     * even after hard-break insertion/deletion.
     *
     * Matching tiers (with one-to-one constraint):
     * 1. [startUtf8] exact match — most reliable across revisions.
     * 2. Exact line-range match — fallback when startUtf8 is -1 or no byte match.
     * 3. Largest line-range overlap — for partially overlapping blocks.
     * 4. Nearest by gap — last resort for non-overlapping blocks.
     *
     * Fallback: when [startUtf8] is -1 (not tracked) or no byte-offset match exists,
     * falls back to line-index overlap and nearest-by-gap matching (legacy behavior).
     * This handles edge cases where the offset mapper cannot produce a match — e.g. when
     * a BlockShift's paragraph was created by a merge that the offset mapper does not
     * track, or when [startUtf8] was not populated in an earlier version. The fallback
     * is less precise (line indices shift across revisions when hard breaks are inserted
     * or deleted — the old transaction's line N may become line N+1 in the new revision,
     * causing line-index-based matching to pair the wrong BlockShifts) but prevents the
     * suffix from jumping to the old position when no rebase data is available.
     */
    private fun applyRebaseToBlockShifts(
        newBlockShifts: List<PreparedVisualTransaction.BlockShift>,
        rebaseSnapshot: VisualFrameSnapshot,
        offsetMapper: ((Int) -> Int?)? = null,
        reverseMapper: ((Int) -> Int?)? = null
    ): List<PreparedVisualTransaction.BlockShift> {
        if (rebaseSnapshot.blockShiftStates.isEmpty() || newBlockShifts.isEmpty()) return newBlockShifts
        val usedRebaseIndices = mutableSetOf<Int>()
        return newBlockShifts.map { shift ->
            if (shift.startUtf8 < 0) {
                val exactMatchIdx = rebaseSnapshot.blockShiftStates.indices.firstOrNull { i ->
                    i !in usedRebaseIndices &&
                        rebaseSnapshot.blockShiftStates[i].startLineIndex == shift.startLineIndex &&
                        rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive == shift.endLineIndexExclusive
                }
                if (exactMatchIdx != null) {
                    usedRebaseIndices.add(exactMatchIdx)
                    shift.copy(deltaY = shift.deltaY - rebaseSnapshot.blockShiftStates[exactMatchIdx].currentTranslateY)
                } else {
                    val matchIdx = findBlockShiftRebaseByLineIndex(
                        shift, rebaseSnapshot, usedRebaseIndices
                    )
                    if (matchIdx != null) {
                        usedRebaseIndices.add(matchIdx)
                        shift.copy(deltaY = shift.deltaY - rebaseSnapshot.blockShiftStates[matchIdx].currentTranslateY)
                    } else {
                        shift
                    }
                }
            } else {
                val matchIdx = findBlockShiftRebaseMatch(
                    shift, rebaseSnapshot, usedRebaseIndices, offsetMapper, reverseMapper
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
        reverseMapper: ((Int) -> Int?)?
    ): Int? {
        val candidates = rebaseSnapshot.blockShiftStates.indices.filter { i ->
            i !in usedRebaseIndices && rebaseSnapshot.blockShiftStates[i].startUtf8 >= 0
        }
        if (candidates.isEmpty()) return null

        // Tier 1: Forward offset mapping with endUtf8Exclusive validation
        offsetMapper?.let { mapper ->
            for (i in candidates) {
                val state = rebaseSnapshot.blockShiftStates[i]
                if (mapper(state.startUtf8) == shift.startUtf8) {
                    val endValidated = if (shift.endUtf8Exclusive >= 0 && state.endUtf8Exclusive >= 0) {
                        val mappedEnd = mapper(state.endUtf8Exclusive)
                        mappedEnd != null && mappedEnd == shift.endUtf8Exclusive
                    } else true
                    if (endValidated) return i
                }
            }
            // Tier 1b: Forward offset mapping start-only match (endUtf8 mismatch or unavailable)
            for (i in candidates) {
                val state = rebaseSnapshot.blockShiftStates[i]
                if (mapper(state.startUtf8) == shift.startUtf8) return i
            }
        }

        // Tier 2: Reverse offset mapping
        reverseMapper?.let { rMapper ->
            val mappedOldStart = rMapper(shift.startUtf8)
            if (mappedOldStart != null) {
                for (i in candidates) {
                    if (rebaseSnapshot.blockShiftStates[i].startUtf8 == mappedOldStart) return i
                }
            }
        }

        // Tier 3: Near-match by forward-mapped distance
        offsetMapper?.let { mapper ->
            val nearIdx = candidates.minByOrNull { i ->
                val mapped = mapper(rebaseSnapshot.blockShiftStates[i].startUtf8)
                if (mapped != null) kotlin.math.abs(mapped - shift.startUtf8)
                else Int.MAX_VALUE
            }
            if (nearIdx != null) {
                val mapped = mapper(rebaseSnapshot.blockShiftStates[nearIdx].startUtf8)
                val dist = if (mapped != null) kotlin.math.abs(mapped - shift.startUtf8) else Int.MAX_VALUE
                if (dist < 100) return nearIdx
            }
        }

        // Tier 4: Direct byte equality (only when offsetMapper is null)
        if (offsetMapper == null) {
            for (i in candidates) {
                if (rebaseSnapshot.blockShiftStates[i].startUtf8 == shift.startUtf8) return i
            }
        }

        // Tier 5: Line-index overlap
        val overlappingIndices = candidates.filter { i ->
            val state = rebaseSnapshot.blockShiftStates[i]
            state.startLineIndex < shift.endLineIndexExclusive &&
                state.endLineIndexExclusive > shift.startLineIndex
        }
        if (overlappingIndices.isNotEmpty()) {
            return overlappingIndices.maxByOrNull { i ->
                val overlapStart = maxOf(rebaseSnapshot.blockShiftStates[i].startLineIndex, shift.startLineIndex)
                val overlapEnd = minOf(rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive, shift.endLineIndexExclusive)
                overlapEnd - overlapStart
            }
        }

        // Tier 6: Nearest by line-index gap
        return candidates.minByOrNull { i ->
            val state = rebaseSnapshot.blockShiftStates[i]
            val gap = if (state.endLineIndexExclusive <= shift.startLineIndex) {
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
        usedRebaseIndices: MutableSet<Int>
    ): Int? {
        val overlappingIndices = rebaseSnapshot.blockShiftStates.indices.filter { i ->
            i !in usedRebaseIndices &&
                rebaseSnapshot.blockShiftStates[i].startLineIndex < shift.endLineIndexExclusive &&
                rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive > shift.startLineIndex
        }
        if (overlappingIndices.isNotEmpty()) {
            return overlappingIndices.maxByOrNull { i ->
                val overlapStart = maxOf(rebaseSnapshot.blockShiftStates[i].startLineIndex, shift.startLineIndex)
                val overlapEnd = minOf(rebaseSnapshot.blockShiftStates[i].endLineIndexExclusive, shift.endLineIndexExclusive)
                overlapEnd - overlapStart
            }
        }
        return rebaseSnapshot.blockShiftStates.indices
            .filter { i -> i !in usedRebaseIndices }
            .minByOrNull { i ->
                val state = rebaseSnapshot.blockShiftStates[i]
                val gap = if (state.endLineIndexExclusive <= shift.startLineIndex) {
                    shift.startLineIndex - state.endLineIndexExclusive
                } else {
                    state.startLineIndex - shift.endLineIndexExclusive
                }
                kotlin.math.abs(gap)
            }
    }

    private fun applyRebaseState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState
    ): PreparedVisualTransaction.AnimatedSlice {
        val fromRect = android.graphics.RectF(
            rebaseState.currentLeft,
            rebaseState.currentTop,
            rebaseState.currentRight,
            rebaseState.currentBottom
        )
        return when (slice.role) {
            SliceRole.Move -> {
                slice.copy(
                    fromDestinationRect = fromRect,
                    startAlpha = rebaseState.currentAlpha
                )
            }
            SliceRole.Insert -> {
                // When rebasing onto a Move slice, the new Insert inherits the Move's
                // current position as its starting point. Without fromDestinationRect,
                // the Insert would appear at its destination immediately rather than
                // sliding in from the Move's current on-screen position — causing a
                // visual jump during rapid consecutive input where an old Move becomes
                // a new Insert (e.g. text that was moving is now fully inserted).
                if (rebaseState.role == SliceRole.Move) {
                    slice.copy(
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromRect
                    )
                } else {
                    slice.copy(startAlpha = rebaseState.currentAlpha)
                }
            }
            SliceRole.Delete -> {
                // Rebase direction: Delete/CrossfadeOld interrupted mid-fade must continue
                // fading from the current alpha to 0, not restart from 1. Setting startAlpha
                // = currentAlpha ensures no flash-back; endAlpha = 0 ensures the fade completes.
                slice.copy(startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
            }
            SliceRole.CrossfadeOld -> {
                // Same rebase direction as Delete — fade from current to 0.
                slice.copy(startAlpha = rebaseState.currentAlpha, endAlpha = 0f)
            }
            SliceRole.CrossfadeNew -> {
                // Same fromDestinationRect inheritance as Insert when rebasing onto Move:
                // the new CrossfadeNew starts from the old Move's current position rather
                // than appearing at its destination instantly.
                if (rebaseState.role == SliceRole.Move) {
                    slice.copy(
                        startAlpha = rebaseState.currentAlpha,
                        fromDestinationRect = fromRect
                    )
                } else {
                    slice.copy(startAlpha = rebaseState.currentAlpha)
                }
            }
            SliceRole.Static -> slice
        }
    }

    /**
     * Look up a pre-captured line snapshot by revision type and line index.
     *
     * Pre-captured snapshots are always preferred over creating new ones because they
     * contain the Bitmap and cluster data captured at the correct moment in the
     * prepare-and-submit sequence (old snapshots before mirror update, new snapshots
     * after). Creating a snapshot here would use the *current* layout state, which
     * may have already been invalidated by a subsequent edit — the Bitmap would show
     * the wrong text content.
     *
     * Returns null when no pre-captured snapshot exists for the requested line index,
     * which causes the caller to skip that line (no slice is generated). This is
     * intentional: if a snapshot wasn't captured during the prepare phase, the line
     * is either outside the affected range or the capture failed, and generating a
     * slice with stale data would produce incorrect animation.
     */
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
        return PreparedVisualTransaction.SelectionDecoration(selStart, selEnd)
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

    /** Collect byte ranges of "appearing" slices (Insert/CrossfadeNew/Move) to prevent
     *  [addMoveSlicesForShiftedClustersCrossLine] from creating duplicate Move slices
     *  for clusters already covered by the primary planner. */
    private fun collectExcludedNewByteRanges(
        slices: List<PreparedVisualTransaction.AnimatedSlice>
    ): Set<Pair<Int, Int>> {
        val excluded = mutableSetOf<Pair<Int, Int>>()
        for (slice in slices) {
            if (slice.role == SliceRole.Insert || slice.role == SliceRole.CrossfadeNew || slice.role == SliceRole.Move) {
                if (slice.clusterByteStart >= 0 && slice.clusterByteEndExclusive >= 0) {
                    excluded.add(Pair(slice.clusterByteStart, slice.clusterByteEndExclusive))
                }
            }
        }
        return excluded
    }

    /** Collect byte ranges of "disappearing" slices (Delete/CrossfadeOld) to prevent
     *  [addMoveSlicesForShiftedClustersCrossLine] from creating duplicate slices
     *  for clusters already covered by the primary planner. */
    private fun collectExcludedOldByteRanges(
        slices: List<PreparedVisualTransaction.AnimatedSlice>
    ): Set<Pair<Int, Int>> {
        val excluded = mutableSetOf<Pair<Int, Int>>()
        for (slice in slices) {
            if (slice.role == SliceRole.Delete || slice.role == SliceRole.CrossfadeOld) {
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
