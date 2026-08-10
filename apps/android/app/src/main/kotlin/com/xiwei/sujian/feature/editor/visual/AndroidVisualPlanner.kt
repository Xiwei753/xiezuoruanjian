package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.planner.AffectedLayoutPlanner
import com.xiwei.sujian.feature.editor.visual.planner.BlockShiftPlanner
import com.xiwei.sujian.feature.editor.visual.planner.InsertDeletePlanner
import com.xiwei.sujian.feature.editor.visual.planner.MoveCrossfadePlanner
import com.xiwei.sujian.feature.editor.visual.planner.RebasePlanner
import com.xiwei.sujian.feature.editor.visual.planner.SnapshotPlanner
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
    internal val snapshotPlanner: SnapshotPlanner = SnapshotPlanner(),
) {
    fun computeAffectedLineIndices(
        visualIntent: VisualIntent,
        revision: AndroidLayoutRevision?,
        useNewRanges: Boolean = false,
    ): Set<Int> = affectedLayoutPlanner.computeAffectedLineIndices(visualIntent, revision, useNewRanges)

    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
    ): AffectedLinesResult =
        affectedLayoutPlanner.computeAffectedLineIndicesFromBothRevisions(
            visualIntent,
            oldRevision,
            newRevision,
        )

    fun computeStructurallyAffectedOldLineIndices(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision,
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
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
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
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches,
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        {
                                rev,
                                lineIdx,
                                isNew,
                            ->
                            snapshotPlanner.createSnapshotFromRevision(
                                rev,
                                lineIdx,
                                preCapturedOldSnapshots,
                                preCapturedNewSnapshots,
                                isNewRevision = isNew,
                            )
                        },
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                }
                AnimationMode.RunAnimation -> {
                    insertDeletePlanner.planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches,
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        {
                                rev,
                                lineIdx,
                                isNew,
                            ->
                            snapshotPlanner.createSnapshotFromRevision(
                                rev,
                                lineIdx,
                                preCapturedOldSnapshots,
                                preCapturedNewSnapshots,
                                isNewRevision = isNew,
                            )
                        },
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                    moveCrossfadePlanner.addMoveSlicesForShiftedClustersCrossLine(
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        visualIntent, oldRev, newRev,
                        snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                        snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                        animatedSlices,
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    moveCrossfadePlanner.planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches,
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        {
                                rev,
                                lineIdx,
                                isNew,
                            ->
                            snapshotPlanner.createSnapshotFromRevision(
                                rev,
                                lineIdx,
                                preCapturedOldSnapshots,
                                preCapturedNewSnapshots,
                                isNewRevision = isNew,
                            )
                        },
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                }
                AnimationMode.SnapshotAnimation -> {
                    moveCrossfadePlanner.planCrossfadeAnimation(
                        visualIntent, oldRev, newRev,
                        affectedOldLineIndices, affectedNewLineIndices, animatedSlices, staticPatches,
                        preCapturedOldSnapshots, preCapturedNewSnapshots,
                        {
                                rev,
                                lineIdx,
                                isNew,
                            ->
                            snapshotPlanner.createSnapshotFromRevision(
                                rev,
                                lineIdx,
                                preCapturedOldSnapshots,
                                preCapturedNewSnapshots,
                                isNewRevision = isNew,
                            )
                        },
                        affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                    )
                }
                AnimationMode.SystemSuppressed -> {
                }
            }
        } else if (newRev != null) {
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

            cursorTransition =
                PreparedVisualTransaction.CursorTransition(
                    fromX = fromX,
                    fromY = fromY,
                    fromHeight = fromHeight,
                    toX = newRev.cursorX,
                    toY = newRev.cursorY,
                    toHeight = newRev.cursorHeight,
                    shouldAnimate = true,
                )
        } else if (newRev != null) {
            cursorTransition =
                PreparedVisualTransaction.CursorTransition(
                    fromX = newRev.cursorX,
                    fromY = newRev.cursorY,
                    fromHeight = newRev.cursorHeight,
                    toX = newRev.cursorX,
                    toY = newRev.cursorY,
                    toHeight = newRev.cursorHeight,
                    shouldAnimate = false,
                )
        }

        val finalSlices =
            if (rebaseSnapshot != null && rebaseSnapshot.sliceVisualStates.isNotEmpty()) {
                rebasePlanner.applyRebaseToSlices(animatedSlices, rebaseSnapshot, snapshotLookup)
            } else {
                animatedSlices
            }

        val offsetMapperForRebase =
            if (oldRev != null && newRev != null) {
                affectedLayoutPlanner.buildOffsetMapper(visualIntent)
            } else {
                null
            }

        val reverseMapperForRebase =
            if (oldRev != null && newRev != null) {
                affectedLayoutPlanner.buildReverseOffsetMapper(visualIntent)
            } else {
                null
            }

        val finalBlockShifts =
            if (rebaseSnapshot != null && rebaseSnapshot.blockShiftStates.isNotEmpty()) {
                blockShiftPlanner.applyRebaseToBlockShifts(
                    blockShifts,
                    rebaseSnapshot,
                    offsetMapperForRebase,
                    reverseMapperForRebase,
                )
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
            operationKind = visualIntent.operationKind,
        )
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
        GlyphAnimation,
        ClusterAnimation,
        RunAnimation,
        LineReflowAnimation,
        SnapshotAnimation,
        SystemSuppressed,
    }
}
