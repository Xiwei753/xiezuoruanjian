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
        // BlockShift deltaY threshold: shifts below this value are considered sub-pixel
        // rounding noise and are not animated. 1.0f (one pixel) is chosen because
        // (a) Android Layout.getLineTop/Bottom are integer-pixel, so any fractional
        // shift rounds to 0 or 1px, and (b) a 1px vertical shift is imperceptible
        // during normal typing — animating it would create unnecessary BlockShift
        // entries and extra layout.draw() calls for no visual benefit.
        const val STABLE_SUFFIX_GEOMETRY_TOLERANCE = 1.0f

        // Epsilon for merging adjacent BlockShifts with approximately equal deltaY.
        // Sub-pixel deltaY differences (e.g. 20.0f vs 20.3f) arise from different
        // paragraph line heights or floating-point rounding; they are visually
        // indistinguishable and must not prevent merging into a single suffix block.
        // 0.5f (half a pixel) is chosen because any shift difference below this
        // threshold is imperceptible on screen — the merged block uses the first
        // entry's deltaY, and the maximum visual error is < 0.5px throughout the
        // animation, which is below the just-noticeable difference for motion.
        const val BLOCK_SHIFT_DELTA_Y_EPSILON = 0.5f
    }

    /**
     * Capture lines for a single revision (before/after edit).
     * Only returns lines within the edit paragraph — lines that may need per-cluster
     * Bitmap snapshots for Insert/Delete/Move/Crossfade animation.
     *
     * Subsequent paragraphs that only shift vertically are handled via [BlockShift]
     * (no Bitmap needed — the renderer applies a uniform Y translation to the static
     * new-layout text). This prevents unbounded Bitmap allocation when editing near
     * the top of a long document.
     *
     * Expansion stops at the first hard break after the edit point (paragraph boundary).
     *
     * [useNewRanges]: when true, uses [VisualIntent.newAffectedByteRanges] as the primary
     * overlap filter; when false (default), uses [VisualIntent.oldAffectedByteRanges].
     * This matters when only one revision is available (the other is null): for the old
     * revision we filter by old ranges, for the new revision we filter by new ranges,
     * ensuring the captured lines actually overlap the edit in that revision's coordinate
     * space. When both revisions are available, use [computeAffectedLineIndicesFromBothRevisions]
     * instead, which produces the full two-level result including BlockShifts.
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
                if (revision.lineRanges[i].endsWithHardBreak) break
            }
        }
        return affectedLines
    }

    data class AffectedLinesResult(
        val lineIndices: Set<Int>,
        val oldLineIndices: Set<Int>,
        val newLineIndices: Set<Int>,
        val blockShifts: List<PreparedVisualTransaction.BlockShift>
    )

    /**
     * Compute affected line indices using both old and new revisions.
     *
     * When only one revision is available (the other is null), delegates to
     * [computeAffectedLineIndices] with the appropriate range filter. BlockShifts cannot
     * be computed without both revisions (they require comparing paragraph Y positions
     * across old/new), so an empty list is returned.
     *
     * When both revisions are available, delegates to [computeAffectedLines] which
     * produces the full two-level result: per-cluster Bitmap lines + BlockShift entries.
     * This is the primary entry point used by [AndroidTextAnimationEngine.prepareAndSubmit],
     * which calls it twice — once with newRevision=null (to determine old snapshot lines
     * before the mirror update) and once with both revisions (to determine new snapshot
     * lines and BlockShifts after the mirror update).
     */
    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?
    ): AffectedLinesResult {
        if (oldRevision == null || newRevision == null) {
            val indices = computeAffectedLineIndices(visualIntent, newRevision ?: oldRevision, useNewRanges = true)
            return AffectedLinesResult(
                lineIndices = indices,
                oldLineIndices = if (newRevision == null) indices else emptySet(),
                newLineIndices = if (newRevision != null) indices else emptySet(),
                blockShifts = emptyList()
            )
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
        var blockShifts = listOf<PreparedVisualTransaction.BlockShift>()

        val oldRev = oldRevision
        val newRev = newRevision

        if (oldRev != null && newRev != null) {
            val affectedResult = computeAffectedLines(visualIntent, oldRev, newRev)
            val affectedLines = affectedResult.lineIndices
            blockShifts = affectedResult.blockShifts
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

        val finalBlockShifts = if (rebaseSnapshot != null && rebaseSnapshot.blockShiftStates.isNotEmpty()) {
            applyRebaseToBlockShifts(blockShifts, rebaseSnapshot)
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
            selectionDecoration = buildSelectionDecoration(newRev),
            preeditDecoration = buildPreeditDecoration(newRev),
            cursorTransition = cursorTransition,
            durationMs = durationMs,
            blockShifts = finalBlockShifts
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
        // Delete path: includes both explicit deletes and composition cancel.
        // CompositionCancel is semantically a delete — the preedit text is removed and
        // the cursor returns to the pre-edit position. Routing it through the Delete
        // branch produces the correct CrossfadeOld/fade-out slices for the cancelled
        // preedit, with retained text getting Move slices via addMoveSlicesForShiftedClustersCrossLine.
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
     * beginning of the next — same lineIndex matching would miss these pairs, causing
     * retained text to appear at its new position instantly (no Move animation) while
     * the cursor animates, producing the "cursor moves but text jumps" visual artifact.
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
            // mappedEnd == null means the old cluster's end falls inside a deleted/replaced
            // range — the cluster straddles the boundary of the edit. In this case the
            // start may still map, allowing a partial match by start-only position.
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
                // Fingerprint fallback: when offset mapper fails to produce an exact match
                // (e.g. the cluster straddles an edit boundary so mappedStart is null, or
                // the mapped position doesn't correspond to any new cluster), match by
                // shaping fingerprint. This is less precise than offset mapping because
                // identical fingerprints don't guarantee the same text (e.g. repeated
                // characters), so it's only used when the primary matching path fails.
                //
                // Tiebreaker: closest documentByteStart distance (not visual position).
                // Byte-start distance is preferred because (a) it's deterministic and
                // independent of layout, (b) visual position can change due to reflow
                // even when the text hasn't moved semantically, and (c) for same-line
                // clusters with identical fingerprints, byte order is a stable proxy for
                // visual order in LTR text and is at least consistent in RTL.
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

    /**
     * Group adjacent grapheme clusters into runs for RunAnimation mode.
     *
     * Run merging rules (visual continuity, not logical order):
     * - Same visual line: clusters must be byte-adjacent (cluster[i].documentByteEndExclusive
     *   == cluster[i+1].documentByteStart) AND share the same visual top (same line).
     *   Byte adjacency alone is insufficient because soft-wrap breaks byte-adjacent clusters
     *   across lines — merging them would produce a sourceRect/visualRect spanning two lines,
     *   which the renderer cannot draw as a single Bitmap region.
     * - Cross-line: runs are split at line boundaries (different visual top). Each run is
     *   confined to a single visual line, ensuring sourceRect and visualRect are coherent.
     *
     * Merged geometry: sourceRect and visualRect use union (min/max) of all constituent
     * clusters, not first/last logical rect. This is essential for RTL and mixed bidi:
     * the logically "first" cluster may be visually rightmost, so first.left + last.right
     * would produce an inverted or incomplete rect. Union guarantees the merged rect covers
     * every cluster regardless of direction.
     *
     * Merged fingerprint: concatenates all constituent fingerprints with "|" separator.
     * [shapingIdentityConfident] is true only when ALL constituent clusters are confident —
     * a single unconfident cluster makes the whole run unconfident, forcing Crossfade.
     */
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

    /**
     * Line-level reflow animation for mid-paragraph inserts/deletes that cause line wrapping changes.
     * Matches old→new clusters by offset map (primary) or fingerprint (fallback), then decides
     * Move vs Crossfade per pair. Lines only on one side become whole-line Insert/Delete.
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
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)
        val oldParagraphs = buildParagraphRanges(oldRev)
        val newParagraphs = buildParagraphRanges(newRev)

        val affectedOldParagraphIds = mutableSetOf<Int>()
        val affectedNewParagraphIds = mutableSetOf<Int>()
        for (lineIndex in affectedLines) {
            oldRev.lineRanges.getOrNull(lineIndex)?.paragraphId?.let { affectedOldParagraphIds.add(it) }
            newRev.lineRanges.getOrNull(lineIndex)?.paragraphId?.let { affectedNewParagraphIds.add(it) }
        }

        val matchedNewParaIndices = mutableSetOf<Int>()
        for ((oldParaIdx, oldPara) in oldParagraphs.withIndex()) {
            if (oldPara.paragraphId !in affectedOldParagraphIds) continue

            var bestNewParaIdx: Int? = null
            val mappedStart = offsetMapper(oldPara.startUtf8)
            if (mappedStart != null) {
                for ((newParaIdx, newPara) in newParagraphs.withIndex()) {
                    if (newParaIdx in matchedNewParaIndices) continue
                    if (newPara.startUtf8 == mappedStart) {
                        bestNewParaIdx = newParaIdx
                        break
                    }
                }
            }
            if (bestNewParaIdx == null && oldPara.paragraphId in affectedNewParagraphIds) {
                // paragraphId fallback: when offsetMapper cannot match (e.g. the paragraph
                // start falls inside a replaced range so mappedStart is null), fall back to
                // matching by paragraphId. This is unreliable after hard-break insertion/deletion
                // (all subsequent IDs change), but within the affected-line set the IDs are
                // typically still aligned because the affected lines were computed from the
                // same revision. Without this fallback, paragraphs with no offset-map match
                // would be skipped entirely, producing no animation for their lines.
                bestNewParaIdx = newParagraphs.indexOfFirst { it.paragraphId == oldPara.paragraphId }
                    .takeIf { it >= 0 && it !in matchedNewParaIndices }
            }
            if (bestNewParaIdx == null) continue
            matchedNewParaIndices.add(bestNewParaIdx)

            val newPara = newParagraphs[bestNewParaIdx]
            val oldParaLines = oldRev.lineRanges.withIndex()
                .filter { it.value.paragraphId == oldPara.paragraphId }
            val newParaLines = newRev.lineRanges.withIndex()
                .filter { it.value.paragraphId == newPara.paragraphId }
            val maxLocal = maxOf(oldParaLines.size, newParaLines.size)
            for (localIdx in 0 until maxLocal) {
                val oldEntry = oldParaLines.getOrNull(localIdx)
                val newEntry = newParaLines.getOrNull(localIdx)
                val oldLineRange = oldEntry?.value
                val newLineRange = newEntry?.value
                val oldLineIndex = oldEntry?.index
                val newLineIndex = newEntry?.index

                if (oldLineRange != null && newLineRange != null && oldLineIndex != null && newLineIndex != null) {
                    val newSnapshot = createSnapshotFromRevision(newRev, newLineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
                    val oldSnapshot = createSnapshotFromRevision(oldRev, oldLineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots)
                    if (newSnapshot != null && oldSnapshot != null) {
                        val matchedPairs = matchClustersByOffsetMap(
                            oldSnapshot, newSnapshot, visualIntent, oldRev, newRev
                        )
                        if (matchedPairs.isNotEmpty()) {
                            for ((oldCluster, newCluster) in matchedPairs) {
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
                        }
                    }
                } else if (newLineRange != null && newLineIndex != null) {
                    val newSnapshot = createSnapshotFromRevision(newRev, newLineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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
                } else if (oldLineRange != null && oldLineIndex != null) {
                    val oldSnapshot = createSnapshotFromRevision(oldRev, oldLineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots)
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
    }

    /**
     * Primary cluster matching: map old byte offsets to new via [buildOffsetMapper],
     * then find new clusters at the mapped positions. Falls back to [matchClustersByFingerprint]
     * if no offset-mapped pairs are found (e.g. when offset mapper returns null for all clusters
     * because the edit fully replaced the line content). Fingerprint matching is less precise
     * (identical fingerprints don't guarantee same text) but is the only option when the offset
     * map provides no identity information — without it, retained text with no offset mapping
     * would get no Move/Crossfade slices at all.
     *
     * Partial match semantics: when [mappedEnd] is null (the old cluster's end falls inside
     * a deleted/replaced range), the match is still attempted by [mappedStart] alone — a new
     * cluster at the mapped start position is accepted regardless of its end. This handles
     * boundary clusters that straddle the edit: their start maps correctly but their end does
     * not, yet they still represent the same visual content and should be paired for Move/Crossfade.
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
     *
     * First-match wins with no positional tiebreaker. This is intentional: when the
     * offset map provides no identity information (all clusters mapped to null), there
     * is no reliable way to determine which old cluster corresponds to which new cluster
     * by position alone — the layout may have shifted entirely. First-match is deterministic
     * and avoids the complexity of positional heuristics that would be unreliable anyway.
     * The planner compensates by requiring [shapingIdentityConfident] for Move; without
     * confidence, Crossfade is used, which is visually correct regardless of pairing order.
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
     * Determine the set of visual line indices affected by an edit, using both old and new
     * revisions. Two-level result:
     *
     * **Level 1 (lineIndices)**: lines within the edit paragraph that need per-cluster Bitmap
     * snapshots for Insert/Delete/Move/Crossfade animation. Only the edit paragraph's lines
     * are included — subsequent paragraphs do NOT get Bitmap snapshots.
     *
     * **Level 2 (blockShifts)**: paragraphs after the edit paragraph whose Y geometry shifted
     * but whose text content is identical. These are recorded as [BlockShift] entries with a
     * uniform deltaY — the renderer applies a Y translation to the static new-layout text
     * without creating per-line Bitmaps. This prevents unbounded Bitmap allocation when
     * editing near the top of a long document.
     *
     * Paragraph alignment: old/new paragraphs are matched by their UTF-8 byte range via
     * [buildOffsetMapper], NOT by [paragraphId]. Inserting or deleting a hard break changes
     * all subsequent paragraphIds (they are sequential integers), so ID-based matching would
     * pair different paragraphs. Offset-map matching ensures the same text paragraph is
     * aligned even after hard-break insertion/deletion.
     *
     * Hard-break insertion/deletion splits or merges paragraphs. The edit paragraph in the
     * old revision may correspond to two paragraphs in the new revision (split), or two old
     * paragraphs may merge into one new paragraph. Both the old and new "edit paragraph
     * groups" are fully included in the affected line set, so the snapshot capture covers
     * all structurally affected paragraphs. Specifically:
     * - Split (insert hard break): the old edit paragraph's lines + the new edit paragraph's
     *   lines (which span both resulting paragraphs, since [newEditParaLines] includes all
     *   lines sharing the new edit line's [paragraphId]) are all captured.
     * - Merge (delete hard break): the old edit paragraph's lines + the new merged paragraph's
     *   lines are all captured, ensuring the merged-in text has an old snapshot for its
     *   CrossfadeOld/Move exit animation.
     * BlockShifts start only after the last paragraph in the edit group, preventing the same
     * paragraph from appearing both as a Bitmap snapshot target and as a BlockShift target.
     */
    private fun computeAffectedLines(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
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

        val editByteStart = visualIntent.oldAffectedByteRanges.firstOrNull()?.first
            ?: visualIntent.newAffectedByteRanges.firstOrNull()?.first
        if (editByteStart != null) {
            val oldEditLine = findLineForUtf8(oldRev, editByteStart)
            val newEditLine = findLineForUtf8(newRev, editByteStart)

            val editParagraphId = oldRev.lineRanges.getOrNull(oldEditLine)?.paragraphId ?: 0

            val offsetMapper = buildOffsetMapper(visualIntent, oldRev, newRev)

            val oldParagraphs = buildParagraphRanges(oldRev)
            val newParagraphs = buildParagraphRanges(newRev)

            /**
             * Determine structurally affected paragraphs: those whose content or boundaries
             * changed due to hard-break insertion/deletion, beyond the primary edit paragraph.
             *
             * Split (insert hard break): the old edit paragraph becomes two new paragraphs.
             * The second new paragraph has no old counterpart at the same paragraphId, so it
             * must be added to [structurallyAffectedNewParaIds] to ensure its lines get Bitmap
             * snapshots for CrossfadeOld/Move exit animation.
             *
             * Merge (delete hard break): two old paragraphs merge into one new paragraph.
             * The second old paragraph's text is absorbed into the merged paragraph, so it
             * must be in [structurallyAffectedOldParaIds] to ensure its lines have old snapshots.
             *
             * Detection: if offset-mapping an old paragraph's start/end yields a new paragraph
             * with different boundaries, both are structurally affected. If reverse-mapping a
             * new paragraph's start returns null (no old offset maps to it), the new paragraph
             * is structurally affected — it was created by a split.
             *
             * Additionally, if reverse-mapping a new paragraph's start falls inside the old
             * edit paragraph, that new paragraph was split off from the edit paragraph and
             * must be structurally affected. Similarly, if an old paragraph's mapped start
             * falls inside the new edit paragraph, that old paragraph was merged in.
             *
             * BlockShifts start only after the last paragraph in the combined edit group,
             * preventing the same paragraph from appearing both as a Bitmap snapshot target
             * and as a BlockShift target.
             */
            val editOldPara = oldParagraphs.firstOrNull { it.paragraphId == editParagraphId }
            val editNewParaId = newRev.lineRanges.getOrNull(newEditLine)?.paragraphId ?: 0
            val editNewPara = newParagraphs.firstOrNull { it.paragraphId == editNewParaId }
            val structurallyAffectedOldParaIds = mutableSetOf<Int>()
            val structurallyAffectedNewParaIds = mutableSetOf<Int>()
            structurallyAffectedOldParaIds.add(editParagraphId)
            structurallyAffectedNewParaIds.add(editNewParaId)

            // Detect structurally affected paragraphs beyond the primary edit paragraph.
            // An old paragraph is structurally affected if its boundaries changed after the edit:
            // - mappedEnd == null: the old paragraph's end falls inside a deleted/replaced range,
            //   meaning the paragraph was partially consumed by the edit and needs old snapshots.
            // - mapped end matches a new paragraph but mapped start does not: the paragraph's
            //   boundaries shifted (e.g. a hard break was inserted inside it), so both the old
            //   and new paragraphs need snapshots to animate the boundary change.
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
                val reverseMappedStart = reverseMapOffset(newPara.startUtf8, visualIntent, oldRev, newRev)
                if (reverseMappedStart == null) {
                    // New paragraph with no reverse mapping: its startUtf8 falls inside
                    // the new-only affected range, meaning this paragraph was created by
                    // a hard-break split and has no counterpart in the old revision.
                    structurallyAffectedNewParaIds.add(newPara.paragraphId)
                    // Cross-validate via forward offset mapping: find old paragraphs whose
                    // mapped start falls within the new paragraph's byte range. These old
                    // paragraphs contributed text to the split result and need old Bitmap
                    // snapshots for exit animation. Without this, the second half of a
                    // split paragraph would have no old snapshot and would jump to its
                    // final position without animation.
                    //
                    // Condition (1): mappedStart < newPara.endUtf8Exclusive — the old
                    // paragraph's content, when mapped to the new document, starts before
                    // the new paragraph ends, so it overlaps.
                    //
                    // Condition (2): newPara.startUtf8 < oldPara.endUtf8Exclusive + newParaLen.
                    // Cross-coordinate overlap approximation: oldPara.endUtf8Exclusive is in
                    // old-document coordinates while newPara.startUtf8 is in new-document
                    // coordinates, so they cannot be compared directly. Adding newParaLen
                    // (the new paragraph's byte length) approximates the maximum forward shift
                    // from the split — the edit delta cannot move the old paragraph's end
                    // forward by more than the inserted text length, which is bounded by
                    // newParaLen. This is conservative: it may include unrelated old paragraphs,
                    // but false positives only cause extra Bitmap snapshots (not incorrect
                    // animation). Condition (1) constrains the result to paragraphs that
                    // genuinely overlap in the new document; condition (2) widens the
                    // candidate set to avoid missing paragraphs at the split boundary where
                    // forward mapping alone is insufficient.
                    for (oldPara in oldParagraphs) {
                        val ms = offsetMapper(oldPara.startUtf8)
                        if (ms != null && ms < newPara.endUtf8Exclusive && newPara.startUtf8 < oldPara.endUtf8Exclusive + (newPara.endUtf8Exclusive - newPara.startUtf8)) {
                            structurallyAffectedOldParaIds.add(oldPara.paragraphId)
                        }
                    }
                } else if (editOldPara != null) {
                    if (reverseMappedStart >= editOldPara.startUtf8 && reverseMappedStart < editOldPara.endUtf8Exclusive) {
                        structurallyAffectedNewParaIds.add(newPara.paragraphId)
                    }
                }
            }
            // Merge detection: find old paragraphs whose mapped start falls inside the
            // new edit paragraph's byte range. These old paragraphs were absorbed into the
            // merged paragraph during a hard-break deletion — their text needs old Bitmap
            // snapshots for exit animation (CrossfadeOld/Move) because it no longer exists
            // as a separate paragraph in the new revision.
            if (editNewPara != null) {
                for (oldPara in oldParagraphs) {
                    if (oldPara.paragraphId in structurallyAffectedOldParaIds) continue
                    val mappedStart = offsetMapper(oldPara.startUtf8)
                    if (mappedStart != null && mappedStart >= editNewPara.startUtf8 && mappedStart < editNewPara.endUtf8Exclusive) {
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

            // Three-tier paragraph matching for BlockShift generation:
            // 1. Match by mapped startUtf8 (primary): the old paragraph's start maps to the
            //    new paragraph's start via offsetMapper. This is the most reliable because
            //    paragraph start offsets are preserved across edits (unless a hard break is
            //    inserted/deleted at the paragraph boundary).
            // 2. Match by mapped endUtf8Exclusive (secondary): when the start doesn't match
            //    (e.g. text was inserted at the paragraph start), the end may still align.
            //    This catches paragraphs whose start shifted but whose end is unchanged.
            // 3. Match by paragraphId (fallback): when offset mapping fails entirely (e.g.
            //    the paragraph was heavily modified), fall back to sequential ID matching.
            //    This is unreliable after hard-break insertion/deletion (all subsequent IDs
            //    change), but is the last resort for paragraphs that must still shift.
            //    Without this fallback, paragraphs with no offset-map match would produce
            //    no BlockShift at all, causing them to jump to the new position instantly.
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
                    val newParaLines = newRev.lineRanges.withIndex()
                        .filter { it.value.paragraphId == newPara.paragraphId }
                    if (newParaLines.isNotEmpty()) {
                        val firstLine = newParaLines.first()
                        val lastLine = newParaLines.last()
                        rawBlockShifts.add(PreparedVisualTransaction.BlockShift(
                            startLineIndex = firstLine.index,
                            endLineIndexExclusive = lastLine.index + 1,
                            top = firstLine.value.top,
                            bottom = lastLine.value.bottom,
                            // left/right use min/max across all lines in the paragraph (not
                            // just first/last) to ensure the clip rect covers the widest line,
                            // preventing narrow intermediate lines from being clipped short.
                            // Same logic as mergeAdjacentBlockShifts's merged left/right.
                            left = newParaLines.map { it.value.left }.minOrNull() ?: 0f,
                            right = newParaLines.map { it.value.right }.maxOrNull() ?: 0f,
                            deltaY = deltaY,
                            startUtf8 = newPara.startUtf8
                        ))
                    }
                }
            }

            blockShifts.addAll(mergeAdjacentBlockShifts(rawBlockShifts))
        }

        val affectedLines = affectedOldLines + affectedNewLines
        return AffectedLinesResult(
            lineIndices = affectedLines,
            oldLineIndices = affectedOldLines,
            newLineIndices = affectedNewLines,
            blockShifts = blockShifts
        )
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

    /**
     * Merge adjacent BlockShifts whose line ranges are contiguous and whose deltaY is
     * approximately equal (within [BLOCK_SHIFT_DELTA_Y_EPSILON]) into a single entry.
     *
     * Without merging, each paragraph produces a separate BlockShift, and the renderer
     * calls [layout.draw] once per shifted paragraph per frame. For long documents with
     * many paragraphs shifting by the same amount (e.g. inserting a line near the top),
     * this would create O(paragraphs) draw calls per frame. Merging reduces this to
     * O(distinct-deltaY-groups) — typically 1 for a simple insert/delete.
     *
     * Epsilon comparison: exact floating-point equality (`==`) fails when deltaY values
     * differ by sub-pixel amounts due to different paragraph line heights or rounding.
     * Using an epsilon of 0.5f (half a pixel) merges paragraphs whose visual shift is
     * indistinguishable, ensuring the renderer performs at most one base draw + one
     * suffix-block draw per frame for the common case of a single inserted/deleted line.
     * The merged deltaY uses the first entry's value — the visual difference is negligible.
     *
     * Merged [left]/[right] use min/max across constituent paragraphs to ensure the
     * clip rect covers the widest line in the block, preventing narrow intermediate lines
     * from being clipped short.
     */
    private fun mergeAdjacentBlockShifts(
        shifts: List<PreparedVisualTransaction.BlockShift>
    ): List<PreparedVisualTransaction.BlockShift> {
        if (shifts.size <= 1) return shifts
        val sorted = shifts.sortedBy { it.startLineIndex }
        val merged = mutableListOf<PreparedVisualTransaction.BlockShift>()
        var current = sorted[0]
        for (i in 1 until sorted.size) {
            val next = sorted[i]
            val deltaYClose = kotlin.math.abs(next.deltaY - current.deltaY) < BLOCK_SHIFT_DELTA_Y_EPSILON
            if (next.startLineIndex == current.endLineIndexExclusive && deltaYClose) {
                current = current.copy(
                    endLineIndexExclusive = next.endLineIndexExclusive,
                    bottom = next.bottom,
                    left = minOf(current.left, next.left),
                    right = maxOf(current.right, next.right),
                    startUtf8 = if (current.startUtf8 >= 0) current.startUtf8 else next.startUtf8
                )
            } else {
                merged.add(current)
                current = next
            }
        }
        merged.add(current)
        return merged
    }

    private data class ParagraphRange(
        val paragraphId: Int,
        val startUtf8: Int,
        /** Exclusive UTF-8 end of the paragraph's last visual line. Half-open boundary:
         *  the byte at [endUtf8Exclusive] itself belongs to the next paragraph (or is
         *  one past the document end). This convention matches [LineRange.endUtf8] and
         *  is critical for the offset mapper — using inclusive end would cause off-by-one
         *  errors in paragraph boundary detection during split/merge analysis. */
        val endUtf8Exclusive: Int,
        val top: Float
    )

    /**
     * Build paragraph ranges from a layout revision.
     *
     * Groups visual lines by [paragraphId] and produces one [ParagraphRange] per paragraph.
     * [endUtf8Exclusive] is the exclusive UTF-8 end of the paragraph's last visual line —
     * this is a half-open boundary: the byte at [endUtf8Exclusive] itself belongs to the
     * next paragraph (or is one past the document end).
     *
     * [paragraphId] is a sequential integer that increments at each hard break. It is NOT
     * a stable identity across edits — inserting or deleting a hard break renumbers all
     * subsequent paragraphs. Paragraph alignment in the animation planner uses offset-map
     * matching (via [buildOffsetMapper]) rather than paragraphId, so that the same text
     * paragraph is correctly paired even after hard-break insertion/deletion.
     *
     * [top] is the Y coordinate of the paragraph's first visual line. Used by BlockShift
     * computation to calculate deltaY = newTop - oldTop for paragraphs that shifted
     * vertically but whose text content is unchanged.
     */
    private fun buildParagraphRanges(rev: AndroidLayoutRevision): List<ParagraphRange> {
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
     * BlockShifts.
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
     * Matching uses [startUtf8] (byte offset) as the primary identity rather than
     * [startLineIndex]. Line indices shift across revisions when hard breaks are
     * inserted/deleted — the old transaction's line N may become line N+1 in the new
     * revision, causing line-index-based matching to pair the wrong BlockShifts. Byte
     * offsets are stable across revisions (they identify the same paragraph regardless
     * of how many visual lines precede it), so [startUtf8]-based matching is correct
     * even after hard-break insertion/deletion.
     *
     * Fallback: when [startUtf8] is -1 (not tracked) or no byte-offset match exists,
     * falls back to line-index overlap and nearest-by-gap matching (legacy behavior).
     * This handles edge cases where the offset mapper cannot produce a match — e.g. when
     * a BlockShift's paragraph was created by a merge that the offset mapper does not
     * track, or when [startUtf8] was not populated in an earlier version. The fallback
     * is less precise (line indices shift across revisions) but prevents the suffix from
     * jumping to the old position when no rebase data is available.
     */
    private fun applyRebaseToBlockShifts(
        newBlockShifts: List<PreparedVisualTransaction.BlockShift>,
        rebaseSnapshot: VisualFrameSnapshot
    ): List<PreparedVisualTransaction.BlockShift> {
        if (rebaseSnapshot.blockShiftStates.isEmpty() || newBlockShifts.isEmpty()) return newBlockShifts
        return newBlockShifts.map { shift ->
            val byteOffsetMatch = if (shift.startUtf8 >= 0) {
                rebaseSnapshot.blockShiftStates.firstOrNull { oldState ->
                    oldState.startUtf8 == shift.startUtf8
                }
            } else null
            if (byteOffsetMatch != null) {
                shift.copy(deltaY = shift.deltaY - byteOffsetMatch.currentTranslateY)
            } else {
                val exactMatch = rebaseSnapshot.blockShiftStates.firstOrNull { oldState ->
                    oldState.startLineIndex == shift.startLineIndex &&
                        oldState.endLineIndexExclusive == shift.endLineIndexExclusive
                }
                if (exactMatch != null) {
                    shift.copy(deltaY = shift.deltaY - exactMatch.currentTranslateY)
                } else {
                    val overlappingOlds = rebaseSnapshot.blockShiftStates.filter { oldState ->
                        oldState.startLineIndex < shift.endLineIndexExclusive &&
                            oldState.endLineIndexExclusive > shift.startLineIndex
                    }
                    if (overlappingOlds.isNotEmpty()) {
                        // When no exact line-range match exists, find the old BlockShift
                        // with the largest line-range overlap. Maximum overlap is preferred
                        // over minimum gap because an overlapping old shift was actively
                        // animating the same region — its currentTranslateY is the most
                        // representative on-screen position for the new shift's starting point.
                        val bestOld = overlappingOlds.maxByOrNull {
                            val overlapStart = maxOf(it.startLineIndex, shift.startLineIndex)
                            val overlapEnd = minOf(it.endLineIndexExclusive, shift.endLineIndexExclusive)
                            overlapEnd - overlapStart
                        }
                        shift.copy(deltaY = shift.deltaY - (bestOld?.currentTranslateY ?: 0f))
                    } else {
                        val nearestOld = rebaseSnapshot.blockShiftStates.minByOrNull { oldState ->
                            val gap = if (oldState.endLineIndexExclusive <= shift.startLineIndex) {
                                shift.startLineIndex - oldState.endLineIndexExclusive
                            } else {
                                oldState.startLineIndex - shift.endLineIndexExclusive
                            }
                            kotlin.math.abs(gap)
                        }
                        if (nearestOld != null) {
                            shift.copy(deltaY = shift.deltaY - nearestOld.currentTranslateY)
                        } else {
                            shift
                        }
                    }
                }
            }
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
