package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import uniffi.writer_core.AnimationModeDto

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
class AndroidVisualPlanner {

    private companion object {
        const val STABLE_SUFFIX_GEOMETRY_TOLERANCE = 1.0f
    }

    /**
     * Provisional capture lines for a single revision (before/after edit).
     * When the preferred ranges are empty (pure Insert old / pure Delete new),
     * fall back to the other side's edit point and expand forward.
     * Expansion stops at the first paragraph boundary after the edit point
     * (detected via [AndroidLayoutRevision.LineRange.endsWithHardBreak] — a hard line break
     * means the visual line ends at a paragraph boundary, so reflow cannot propagate past it),
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
                // Half-open overlap: [start, end) ∩ [lineRange.startUtf8, lineRange.endUtf8)
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

    /**
     * Detect whether [lineIndex] is the first visual line of a new paragraph
     * (i.e. the previous visual line ended with a hard break).
     *
     * This relies on [AndroidLayoutRevision.LineRange.endsWithHardBreak], which is set
     * during revision construction by inspecting the source text — not by detecting byte
     * gaps between adjacent visual lines. Android Layout's visual line endUtf8 is the
     * position after the last character, and the next line's startUtf8 is normally
     * contiguous; a `\n` character belongs to the text range and does not create a byte
     * gap, so byte-gap-based detection would never match.
     */
    private fun isParagraphBoundary(revision: AndroidLayoutRevision, lineIndex: Int): Boolean {
        if (lineIndex <= 0 || lineIndex >= revision.lineRanges.size) return false
        return revision.lineRanges[lineIndex - 1].endsWithHardBreak
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
                        collectExcludedOldByteRanges(animatedSlices),
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
                        collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                    addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        collectExcludedNewByteRanges(animatedSlices),
                        collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices
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
        // CompositionUpdate/Commit are replace-like: old preedit/range fades out, new text
        // fades in, retained text with same fingerprint Moves. This avoids unnecessary
        // Crossfade when the visual text is identical (e.g. composition candidate unchanged).
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
            } else if (isDelete && oldSnapshot != null && oldLineRange != null) {
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

    /**
     * After the primary animation planner creates Insert/Delete/Crossfade slices,
     * find retained clusters that shifted across lines and create Move or Crossfade slices
     * for them.
     *
     * Design: old/new clusters are matched by [buildOffsetMapper] (which maps byte offsets
     * through the edit's affected ranges), not by same visual lineIndex. This is essential
     * because a soft-wrap reflow can move text from the end of one visual line to the
     * beginning of the next — same lineIndex matching would miss these pairs.
     *
     * [excludedNewByteRanges]/[excludedOldByteRanges] prevent duplicate slices
     * for clusters already handled by the primary planner.
     */
    private fun addMoveSlicesForShiftedClustersCrossLine(
        allOldSnapshots: Map<Int, AndroidLineSnapshot>,
        allNewSnapshots: Map<Int, AndroidLineSnapshot>,
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        excludedNewByteRanges: Set<Pair<Int, Int>>,
        excludedOldByteRanges: Set<Pair<Int, Int>>,
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
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
            }
            if (isDeleted) continue
            val isAlreadyHandled = excludedOldByteRanges.any { (start, end) ->
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
            }
            if (isAlreadyHandled) continue
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
                val candidates = allNewClusters.indices.filter { i ->
                    val candidate = allNewClusters[i].first
                    i !in newUsed &&
                        candidate.shapingFingerprint == oldCluster.shapingFingerprint &&
                        visualIntent.newAffectedByteRanges.none { (start, end) ->
                            candidate.documentByteStart < end && candidate.documentByteEndExclusive > start
                        }
                }
                matchedNewIdx = candidates.minByOrNull { i ->
                    kotlin.math.abs(allNewClusters[i].first.documentByteStart - oldCluster.documentByteStart)
                }
            }
            if (matchedNewIdx != null) {
                newUsed.add(matchedNewIdx)
                val (newCluster, newInfo) = allNewClusters[matchedNewIdx]
                val isExcluded = excludedNewByteRanges.any { (start, end) ->
                    newCluster.documentByteStart < end && newCluster.documentByteEndExclusive > start
                }
                if (isExcluded) continue
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                if (!positionChanged) continue
                val newSnapshot = newInfo.second
                val oldSnapshot = oldInfo.second
                // Move requires BOTH clusters to have confident shaping fingerprints.
                // A false Move (same fingerprint but different actual glyphs) causes visual
                // glitches. On API < 31, shapingIdentityConfident is false, so Crossfade
                // is always used as a safe fallback.
                if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint
                    && oldCluster.shapingIdentityConfident
                    && newCluster.shapingIdentityConfident
                ) {
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
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (oldSnapshot != null && oldLineRange != null) {
                if (oldSnapshot.clusters.isNotEmpty()) {
                    for (cluster in oldSnapshot.clusters) {
                        // Half-open overlap: cluster [start, end) ∩ affected [start, end)
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

            if (newSnapshot != null && newLineRange != null) {
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
        }

        val newMatched = mutableSetOf<Int>()
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val candidates = allNewAffectedClusters.indices.filter { i ->
                i !in newMatched && allNewAffectedClusters[i].first.shapingFingerprint == oldCluster.shapingFingerprint &&
                    allNewAffectedClusters[i].first.documentByteEndExclusive - allNewAffectedClusters[i].first.documentByteStart ==
                    oldCluster.documentByteEndExclusive - oldCluster.documentByteStart
            }
            val matchIdx = candidates.minByOrNull { i ->
                kotlin.math.abs(allNewAffectedClusters[i].first.documentByteStart - oldCluster.documentByteStart)
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                val (newCluster, newSnapshot) = allNewAffectedClusters[matchIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                if (positionChanged) {
                    // Move requires BOTH clusters to have confident shaping fingerprints;
                    // see addMoveSlicesForShiftedClustersCrossLine for the full invariant.
                    if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint
                    && oldCluster.shapingIdentityConfident
                    && newCluster.shapingIdentityConfident
                ) {
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
     * Run-level animation: groups clusters into runs, then matches old→new runs
     * by shaping fingerprint + byte length with closest-offset tiebreaker.
     * Matched runs with same shaping + confident fingerprint → Move; otherwise → Crossfade pair.
     * Unmatched old runs → Delete; unmatched new runs → Insert.
     *
     * RunAnimation is a *granularity* mode only — it groups clusters into larger visual units
     * for Insert/Delete/Crossfade, but retained text with changed position still gets Move
     * or Crossfade slices (via [addMoveSlicesForShiftedClustersCrossLine]), not whole-line
     * crossfade. Long text mid-paragraph edits must not revert to full-line old crossfade.
     */
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
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (oldSnapshot != null && oldLineRange != null && visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }

            if (newSnapshot != null && newLineRange != null && visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                }
            }
        }

        val newMatched = mutableSetOf<Int>()
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            val candidates = allNewAffectedClusters.indices.filter { i ->
                i !in newMatched && allNewAffectedClusters[i].first.shapingFingerprint == oldCluster.shapingFingerprint &&
                    allNewAffectedClusters[i].first.documentByteEndExclusive - allNewAffectedClusters[i].first.documentByteStart ==
                    oldCluster.documentByteEndExclusive - oldCluster.documentByteStart
            }
            val matchIdx = candidates.minByOrNull { i ->
                kotlin.math.abs(allNewAffectedClusters[i].first.documentByteStart - oldCluster.documentByteStart)
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                val (newCluster, newSnapshot) = allNewAffectedClusters[matchIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                if (positionChanged) {
                    // Move requires BOTH clusters to have confident shaping fingerprints;
                    // see addMoveSlicesForShiftedClustersCrossLine for the full invariant.
                    if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint
                        && oldCluster.shapingIdentityConfident
                        && newCluster.shapingIdentityConfident
                    ) {
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

    private fun groupClustersIntoRuns(
        clusters: List<LineClusterSnapshot>,
        affectedRanges: List<Pair<Int, Int>>
    ): List<LineClusterSnapshot> {
        if (clusters.isEmpty()) return emptyList()
        val affected = clusters.filter { cluster ->
            affectedRanges.any { (start, end) ->
                cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
            }
        }
        return affected
    }

    /**
     * Line-level reflow animation for mid-paragraph inserts/deletes that cause line wrapping changes.
     * Matches old→new clusters by offset map (primary) or fingerprint (fallback), then decides
     * Move vs Crossfade per pair. Lines only on one side become whole-line Insert/Delete.
     */
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
                            // Move requires BOTH clusters to have confident shaping fingerprints;
                            // see addMoveSlicesForShiftedClustersCrossLine for the full invariant.
                            if (oldCluster.shapingFingerprint == newCluster.shapingFingerprint
                                && oldCluster.shapingIdentityConfident
                                && newCluster.shapingIdentityConfident
                            ) {
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

    /**
     * Primary cluster matching: map old byte offsets to new via [buildOffsetMapper],
     * then find new clusters at the mapped positions. Falls back to [matchClustersByFingerprint]
     * if no offset-mapped pairs are found (e.g. when offset mapper returns null for all clusters).
     */
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
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
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

    /**
     * Fallback cluster matching: pair old→new clusters by shaping fingerprint,
     * excluding clusters inside affected (inserted/deleted) byte ranges.
     * First-match wins; no positional tiebreaker since offset map was unavailable.
     */
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
                cluster.documentByteStart < end && cluster.documentByteEndExclusive > start
            }
            if (isInserted) continue
            newByFp.getOrPut(cluster.shapingFingerprint) { mutableListOf() }.add(i)
        }
        for (oldCluster in oldSnapshot.clusters) {
            val isDeleted = deleteByteRanges.any { (start, end) ->
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
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

    /**
     * Whole-line crossfade for SnapshotAnimation mode: old line fades out, new line fades in.
     * No per-cluster matching — used when layout is too complex for cluster-level animation.
     */
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
                }
            }
        }
    }

    private fun planNoAnimation(
        newRev: AndroidLayoutRevision,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
    }

    /**
     * Determine the set of visual line indices affected by an edit, using both old and new
     * revisions. Scans forward from the edit point until reaching a stable suffix — a line
     * whose geometry and byte range are identical in both revisions, and beyond which no
     * further lines differ.
     *
     * Paragraph boundary truncation: a hard line break ([LineRange.endsWithHardBreak]) means
     * this visual line ends a paragraph; reflow cannot propagate into the next paragraph, so
     * the forward scan stops. This check uses [endsWithHardBreak] from the revision metadata
     * (set during construction by inspecting source text), not byte-gap heuristics — adjacent
     * visual lines in Android Layout have contiguous byte ranges even across `\n`, so byte-gap
     * detection would never identify a paragraph boundary.
     *
     * When geometry changes on a line that also has [endsWithHardBreak], the line itself is
     * included in the affected set but the scan stops (reachedStableSuffix = true), preventing
     * the next paragraph's lines from being unnecessarily captured.
     */
    private fun computeAffectedLines(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Set<Int> {
        val affectedLines = mutableSetOf<Int>()

        for ((start, end) in visualIntent.oldAffectedByteRanges) {
            for (i in oldRev.lineRanges.indices) {
                val lineRange = oldRev.lineRanges[i]
                // Half-open overlap: [start, end) ∩ [lineRange.startUtf8, lineRange.endUtf8)
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }

        for ((start, end) in visualIntent.newAffectedByteRanges) {
            for (i in newRev.lineRanges.indices) {
                val lineRange = newRev.lineRanges[i]
                // Half-open overlap: same convention as above.
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
                    if (i > scanStart && (oldLine.endsWithHardBreak || newLine.endsWithHardBreak)) {
                        reachedStableSuffix = true
                        break
                    }
                } else {
                    if (i > scanStart && (oldLine.endsWithHardBreak || newLine.endsWithHardBreak)) {
                        reachedStableSuffix = true
                        break
                    }
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
                // Safety fallback: hard break stops reflow at paragraph boundary.
                // This is logically redundant with the earlier checks inside both the
                // geometryChanged and !geometryChanged branches, but serves as a guard
                // in case the branching logic is restructured.
                if (i > scanStart && (oldLine.endsWithHardBreak || newLine.endsWithHardBreak)) {
                    break
                }
            }
            // Lines present only on one side (soft wrap growth/shrink) always animate.
            for (i in minCommonLines until oldRev.lineRanges.size) {
                affectedLines.add(i)
            }
            for (i in minCommonLines until newRev.lineRanges.size) {
                affectedLines.add(i)
            }
            // If no stable suffix was found (e.g. edit at end of document with no matching
            // suffix lines), the entire suffix from the edit line is already in affectedLines
            // or was added as exclusive old/new lines above. As a safety fallback, ensure the
            // edit line itself is included even when the byte-range intersection above missed it
            // (possible when old/new revisions have different line counts at the edit position).
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

    /**
     * Rebase new slices onto the current visual frame of the old transaction.
     *
     * Three-tier matching (most precise first):
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

    private fun findRebaseStateByClusterByteRange(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val cStart = slice.clusterByteStart
        val cEnd = slice.clusterByteEndExclusive
        if (cStart < 0 || cEnd < 0) return null
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        val exactMatch = rebaseSnapshot.sliceVisualStates.firstOrNull {
            it.role in compatibleRoles &&
                it.lineIndex == lineIndex &&
                it.clusterByteStart == cStart &&
                it.clusterByteEndExclusive == cEnd
        }
        if (exactMatch != null) return exactMatch
        return rebaseSnapshot.sliceVisualStates.firstOrNull {
            it.role in compatibleRoles &&
                it.clusterByteStart == cStart &&
                it.clusterByteEndExclusive == cEnd
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

    private fun findRebaseStateByLineAndRole(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        return rebaseSnapshot.sliceVisualStates
            .filter { it.role in compatibleRoles && it.lineIndex == lineIndex }
            .firstOrNull()
    }

    private fun findClosestRebaseStateByPosition(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseSnapshot: VisualFrameSnapshot
    ): SliceVisualState? {
        val sliceTop = slice.destinationRect.top
        val sliceLeft = slice.destinationRect.left
        val compatibleRoles = compatibleRebaseRoles(slice.role)
        val lineIndex = slice.snapshot?.lineIndex ?: -1
        val filterByLine = lineIndex >= 0 && slice.role != SliceRole.Move
        return rebaseSnapshot.sliceVisualStates
            .filter { it.role in compatibleRoles && (!filterByLine || it.lineIndex == lineIndex) }
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
