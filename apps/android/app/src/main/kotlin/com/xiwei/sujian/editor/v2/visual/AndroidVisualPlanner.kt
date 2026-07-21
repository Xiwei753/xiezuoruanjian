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
     * Expansion stops at the first hard break after the edit point (paragraph boundary),
     * UNLESS the edit is a delete/replace that may remove a hard break — in that case,
     * the next paragraph is also included because it will be structurally merged into
     * the current paragraph.
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
            val editParagraphId = revision.lineRanges.getOrNull(editLine)?.paragraphId ?: 0
            for (i in editLine downTo 0) {
                if (revision.lineRanges.getOrNull(i)?.paragraphId != editParagraphId) break
                affectedLines.add(i)
            }
            for (i in editLine until revision.lineRanges.size) {
                if (revision.lineRanges.getOrNull(i)?.paragraphId != editParagraphId) break
                affectedLines.add(i)
            }
            val isDeleteOrReplace = visualIntent.isDelete() || visualIntent.isReplace()
                || visualIntent.isCompositionCancel() || visualIntent.isCompositionCommit()
                || visualIntent.isCompositionUpdate()
            if (isDeleteOrReplace) {
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

    /**
     * Find the first visual line that belongs to the paragraph AFTER [afterLine]'s paragraph.
     * Returns null if [afterLine] is already in the last paragraph.
     */
    private fun findNextParagraphStartLine(revision: AndroidLayoutRevision, afterLine: Int): Int? {
        val currentParaId = revision.lineRanges.getOrNull(afterLine)?.paragraphId ?: return null
        for (i in (afterLine + 1) until revision.lineRanges.size) {
            if (revision.lineRanges[i].paragraphId != currentParaId) return i
        }
        return null
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
            val revision = newRevision ?: oldRevision ?: return AffectedLinesResult(emptySet(), emptySet(), emptySet(), emptyList())
            val useNewRanges = newRevision != null
            val indices = computeAffectedLineIndices(visualIntent, revision, useNewRanges = useNewRanges)
            if (newRevision == null) {
                val structuralIndices = computeStructurallyAffectedOldLineIndices(visualIntent, oldRevision!!)
                val combined = indices + structuralIndices
                return AffectedLinesResult(
                    lineIndices = emptySet(),
                    oldLineIndices = combined,
                    newLineIndices = emptySet(),
                    blockShifts = emptyList()
                )
            }
            return AffectedLinesResult(
                lineIndices = emptySet(),
                oldLineIndices = emptySet(),
                newLineIndices = indices,
                blockShifts = emptyList()
            )
        }
        return computeAffectedLines(visualIntent, oldRevision, newRevision)
    }

    /**
     * Compute structurally affected old paragraph line indices using ONLY the old revision
     * and visual intent — no new revision required.
     *
     * This is called during Phase 1 of [AndroidTextAnimationEngine.prepareAndSubmit], before
     * the mirror update, to ensure all old paragraphs that will be structurally affected by
     * the edit have their Bitmaps captured. Without this, deleting a hard break between two
     * paragraphs would only capture the first paragraph's Bitmap; the second paragraph's
     * Bitmap cannot be captured after the mirror update because the layout has already changed.
     *
     * Detection strategy:
     * - Find all paragraphs whose byte range overlaps with [VisualIntent.oldAffectedByteRanges].
     * - For delete/replace, also include the paragraph immediately AFTER each affected paragraph,
     *   because deleting a hard break merges the next paragraph into the current one.
     * - Expand each affected paragraph to include ALL its visual lines (not just the ones
     *   overlapping with the affected byte range).
     */
    fun computeStructurallyAffectedOldLineIndices(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision
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

        val isDeleteOrReplace = visualIntent.isDelete() || visualIntent.isReplace()
            || visualIntent.isCompositionCancel() || visualIntent.isCompositionCommit()
            || visualIntent.isCompositionUpdate()
        if (isDeleteOrReplace) {
            val extraParaIds = mutableSetOf<Int>()
            for (pid in affectedParaIds) {
                val firstLineOfPara = oldRevision.lineRanges.withIndex()
                    .firstOrNull { it.value.paragraphId == pid }?.index ?: continue
                val lastLineOfPara = oldRevision.lineRanges.withIndex()
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
            val affectedResult = computeAffectedLines(visualIntent, oldRev, newRev)
            val affectedOldLineIndices = affectedResult.oldLineIndices
            val affectedNewLineIndices = affectedResult.newLineIndices
            blockShifts = affectedResult.blockShifts
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planClusterLevelAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
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
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
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
            applyRebaseToSlices(animatedSlices, rebaseSnapshot, snapshotLookup)
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
            applyRebaseToBlockShifts(blockShifts, rebaseSnapshot, offsetMapperForRebase, reverseMapperForRebase)
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
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
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
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
            )
            return
        }

        if (isInsert) {
            for (lineIndex in affectedNewLineIndices) {
                val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
                val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
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
        // One-to-one matching invariant: each new cluster can be matched by at most one old
        // cluster. Without [newUsed], two old clusters with the same fingerprint could both
        // match the same new cluster, producing duplicate Move/Crossfade slices for the same
        // destination. This is the forward-matching counterpart of the rebase one-to-one
        // invariant in [applyRebaseToSlices] (which prevents multiple new slices from reusing
        // the same old SliceVisualState).
        val newUsed = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
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
                matchedNewIdx = candidates.minByOrNull { i ->
                    allNewClusters[i].first.documentByteStart
                }
            }
            if (matchedNewIdx != null) {
                newUsed.add(matchedNewIdx)
                lastMatchedNewStart = allNewClusters[matchedNewIdx].first.documentByteStart
                val (newCluster, newInfo) = allNewClusters[matchedNewIdx]
                val isExcluded = excludedNewByteRanges.any { (start, end) ->
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

        val newMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0
        for ((oldCluster, oldSnapshot) in allOldAffectedClusters) {
            // Byte-length equality is a necessary precondition for Move: clusters with
            // different byte lengths represent different amounts of text (e.g. "a" vs "ab"),
            // so they cannot be the same visual content even if their shaping fingerprints
            // happen to match. Without this check, a 1-byte cluster could be paired with a
            // 2-byte cluster that has the same fingerprint for the first byte, producing a
            // Move slice that visually stretches or compresses the text.
            //
            // Monotonicity: candidates are filtered to only include new clusters whose
            // documentByteStart >= lastMatchedNewStart, enforcing document-order one-to-one
            // matching. This prevents a later old cluster from matching an earlier new
            // cluster, which would cause crossed animations for repeated text.
            val candidates = allNewAffectedClusters.indices.filter { i ->
                i !in newMatched && allNewAffectedClusters[i].first.shapingFingerprint == oldCluster.shapingFingerprint &&
                    allNewAffectedClusters[i].first.documentByteEndExclusive - allNewAffectedClusters[i].first.documentByteStart ==
                    oldCluster.documentByteEndExclusive - oldCluster.documentByteStart &&
                    allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
            }
            val matchIdx = candidates.minByOrNull { i ->
                allNewAffectedClusters[i].first.documentByteStart
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                lastMatchedNewStart = allNewAffectedClusters[matchIdx].first.documentByteStart
                val (newCluster, newSnapshot) = allNewAffectedClusters[matchIdx]
                val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
                val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
                val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
                if (!positionChanged && identityConfident && !fingerprintChanged) {
                    // continue
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
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
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
                affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches, preCapturedOldSnapshots, preCapturedNewSnapshots
            )
            return
        }

        if (isInsert) {
            for (lineIndex in affectedNewLineIndices) {
                val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
                val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex) ?: continue
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots) ?: continue
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
                val oldRunClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) ?: continue
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
            // Byte-length equality precondition: same rationale as planClusterReplaceAnimation —
            // clusters with different byte lengths represent different amounts of text and must
            // not be paired for Move even if shaping fingerprints happen to match.
            //
            // Monotonicity: candidates are filtered to only include new clusters whose
            // documentByteStart >= lastMatchedNewStart, enforcing document-order one-to-one
            // matching. This prevents a later old cluster from matching an earlier new
            // cluster, which would cause crossed animations for repeated text.
            val candidates = allNewAffectedClusters.indices.filter { i ->
                i !in newMatched && allNewAffectedClusters[i].first.shapingFingerprint == oldCluster.shapingFingerprint &&
                    allNewAffectedClusters[i].first.documentByteEndExclusive - allNewAffectedClusters[i].first.documentByteStart ==
                    oldCluster.documentByteEndExclusive - oldCluster.documentByteStart &&
                    allNewAffectedClusters[i].first.documentByteStart >= lastMatchedNewStart
            }
            val matchIdx = candidates.minByOrNull { i ->
                allNewAffectedClusters[i].first.documentByteStart
            }
            if (matchIdx != null) {
                newMatched.add(matchIdx)
                lastMatchedNewStart = allNewAffectedClusters[matchIdx].first.documentByteStart
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
     * FIFO matching via [MutableList.removeAt](0): when multiple new clusters share the same
     * fingerprint, the first one in document order is consumed first. This is deterministic
     * and consistent with visual order for LTR text. For RTL text the document-order match
     * is less intuitive but still deterministic, and the planner compensates by requiring
     * [shapingIdentityConfident] for Move — without confidence, Crossfade is used regardless
     * of pairing order, which is visually correct even if the pairing is suboptimal.
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
        var lastMatchedNewStart = 0
        for (oldCluster in oldSnapshot.clusters) {
            val isDeleted = deleteByteRanges.any { (start, end) ->
                oldCluster.documentByteStart < end && oldCluster.documentByteEndExclusive > start
            }
            if (isDeleted) continue
            val candidates = newByFp[oldCluster.shapingFingerprint]
            if (candidates != null && candidates.isNotEmpty()) {
                val validIdx = candidates.indexOfFirst { i ->
                    newSnapshot.clusters[i].documentByteStart >= lastMatchedNewStart
                }
                if (validIdx >= 0) {
                    val newIdx = candidates.removeAt(validIdx)
                    lastMatchedNewStart = newSnapshot.clusters[newIdx].documentByteStart
                    pairs.add(Pair(oldCluster, newSnapshot.clusters[newIdx]))
                }
            }
        }
        return pairs
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
     * BlockShift lifecycle invariants (#541 comment 7):
     * 1. Merged into contiguous blocks: [mergeAdjacentBlockShifts] combines adjacent paragraphs
     *    with approximately equal deltaY into a single [BlockShift] entry. The renderer draws
     *    each merged block with one [layout.draw] call per frame (grouped by deltaY), not one
     *    per paragraph — critical for long documents where many paragraphs shift by the same
     *    amount after a single insert/delete.
     * 2. Rebase continuity: [applyRebaseToBlockShifts] adjusts deltaY to
     *    (newDeltaY - oldCurrentTranslateY) so that consecutive inputs continue the suffix
     *    block's animation from its on-screen position rather than jumping back. The rebase
     *    snapshot includes [BlockShiftVisualState] via [computeBlockShiftVisualStates].
     * 3. Structurally affected paragraphs: hard-break insertion/deletion splits or merges
     *    paragraphs. Both the old and new "edit paragraph groups" are fully included in the
     *    affected line set via [structurallyAffectedOldParaIds]/[structurallyAffectedNewParaIds],
     *    so the snapshot capture covers all structurally affected paragraphs. BlockShifts start
     *    only after the last paragraph in the combined edit group.
     * 4. Pre-computed geometry: each [BlockShift] stores [startLineIndex]/[endLineIndexExclusive]
     *    and pre-computed [top]/[bottom]/[left]/[right] geometry. The renderer uses these
     *    directly rather than converting from UTF-8 exclusive-end offsets at draw time, avoiding
     *    [getLineForOffset] on an exclusive boundary that could land on the next paragraph's
     *    first line. [startUtf8] is stored only for rebase matching, not for line lookup.
     *
     * Paragraph alignment: old/new paragraphs are matched by their UTF-8 byte range via
     * [buildOffsetMapper], NOT by [paragraphId]. Inserting or deleting a hard break changes
     * all subsequent paragraphIds (they are sequential integers), so ID-based matching would
     * pair different paragraphs. Offset-map matching ensures the same text paragraph is
     * aligned even after hard-break insertion/deletion.
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

        // editByteStart: primary source is oldAffectedByteRanges (the edit's origin in the
        // old document). Falls back to newAffectedByteRanges for pure-insert edits where
        // oldAffectedByteRanges is empty — the insert position is the only available anchor.
        // editByteStart cross-revision invariant: this offset comes from oldAffectedByteRanges
        // (or newAffectedByteRanges for pure-insert), but it is safe to use with BOTH revisions
        // because it is the byte offset *before* any inserted text — text preceding the edit
        // point is unchanged, so the same offset maps to the correct line in both old and new
        // revisions. This allows a single editByteStart to anchor the edit paragraph in both
        // coordinate spaces without separate old/new start offsets.
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
            // The edit paragraph itself is always structurally affected: its lines need Bitmap
            // snapshots for Insert/Delete/Move/Crossfade animation. This is unconditional —
            // even a single-character insert within the edit paragraph changes the paragraph's
            // visual line layout, and the animation must capture old/new snapshots to show the
            // transition. Omitting the edit paragraph from the structurally-affected set would
            // cause its lines to be skipped during snapshot capture, producing no animation.
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
                    //
                    // Why newParaLen is a valid upper bound: the split inserts at most
                    // newParaLen bytes of new text (the paragraph's entire content), so
                    // oldPara.endUtf8Exclusive + newParaLen >= any possible mapped position
                    // of oldPara's end in the new document. If condition (2) fails, the old
                    // paragraph's end is too far from the new paragraph's start for any
                    // overlap to exist regardless of the edit delta.
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
            //
            //    Invariant: paragraphId fallback is only used within the non-structurally-
            //    affected set (structurallyAffectedOldParaIds are excluded above), so the
            //    risk of false matches is limited to paragraphs whose text content is
            //    identical but whose paragraphId may have shifted due to a hard-break
            //    change elsewhere. A false match produces an incorrect deltaY for one
            //    BlockShift, which is a minor visual glitch (slight Y offset during
            //    animation) compared to the alternative (no animation at all).
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
                            left = newParaLines.map { it.value.left }.minOrNull() ?: 0f,
                            right = newParaLines.map { it.value.right }.maxOrNull() ?: 0f,
                            deltaY = deltaY,
                            startUtf8 = newPara.startUtf8,
                            endUtf8Exclusive = newPara.endUtf8Exclusive
                        ))
                    }
                }
            }

            blockShifts.addAll(mergeAdjacentBlockShifts(rawBlockShifts))
        }

        return AffectedLinesResult(
            lineIndices = emptySet(),
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

    private fun buildStandaloneReverseOffsetMapper(
        visualIntent: VisualIntent
    ): (Int) -> Int? {
        val oldRanges = visualIntent.oldAffectedByteRanges
        val newRanges = visualIntent.newAffectedByteRanges
        if (oldRanges.isEmpty() && newRanges.isEmpty()) return { newOffset -> newOffset }
        if (oldRanges.isEmpty()) {
            val insertStart = newRanges.first().first
            val insertLen = newRanges.sumOf { (s, e) -> e - s }
            return { newOffset ->
                if (newOffset < insertStart) newOffset
                else if (newOffset < insertStart + insertLen) null
                else newOffset - insertLen
            }
        }
        if (newRanges.isEmpty()) {
            val deleteStart = oldRanges.first().first
            val deleteLen = oldRanges.sumOf { (s, e) -> e - s }
            return { newOffset ->
                if (newOffset < deleteStart) newOffset
                else newOffset + deleteLen
            }
        }
        val newAffectedStart = newRanges.first().first
        val newAffectedEnd = newRanges.last().second
        return { newOffset ->
            if (newOffset < newAffectedStart) newOffset
            else if (newOffset >= newAffectedEnd) {
                val shift = newRanges.sumOf { (s, e) -> e - s } - oldRanges.sumOf { (s, e) -> e - s }
                newOffset - shift
            }
            else null
        }
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
                    startUtf8 = if (current.startUtf8 >= 0) current.startUtf8 else next.startUtf8,
                    endUtf8Exclusive = if (next.endUtf8Exclusive >= 0) next.endUtf8Exclusive else current.endUtf8Exclusive
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
