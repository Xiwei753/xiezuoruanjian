package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.feature.editor.layout.AffectedLayoutRevision
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
    /**
     * #606: 旧→新逻辑 slice 对应关系的唯一来源 — 由 Core 计算。
     *
     * 生产路径由 [AndroidEditorPipeline] 注入（经 EditorKernelBridge 调用 Core
     * `compute_rebase_slice_mappings`）。为 null 时（如纯 planner 单元测试）
     * 视为无映射（所有旧 slice 按 Core 无对应关系处理，走平台侧继续/结束逻辑）。
     */
    internal val rebaseMappingProvider: RebaseMappingProvider? = null,
) {
    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AffectedLayoutRevision?,
        newRevision: AffectedLayoutRevision?,
    ): AffectedLinesResult =
        affectedLayoutPlanner.computeAffectedLineIndicesFromBothRevisions(
            visualIntent,
            oldRevision,
            newRevision,
        )

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
        oldRevision: AffectedLayoutRevision?,
        newRevision: AffectedLayoutRevision?,
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
        var affectedOldLineIndices: Set<Int> = emptySet()
        var affectedNewLineIndices: Set<Int> = emptySet()
        // #639 评论 5420317382：reflow 真实统计 — 由 MoveCrossfadePlanner 在做出
        // 判断的那一刻累计，不从最终 slice 角色反推。
        var reflowStats = MoveCrossfadePlanner.ReflowPlanStats()

        val oldRev = oldRevision
        val newRev = newRevision

        if (oldRev != null && newRev != null) {
            val affectedResult =
                affectedLayoutPlanner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRev, newRev)
            affectedOldLineIndices = affectedResult.oldLineIndices
            affectedNewLineIndices = affectedResult.newLineIndices
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
                    // #639 评论 5419182722：自动折行 Core 看不到 \n，仍走 Glyph/Cluster；
                    // 平台 Layout 才拥有视觉行真值。保留字符 reflow 由 addRetainedReflowSlices
                    // 按 old/new AndroidLineSnapshot.lineIndex 决定是否跨行。
                    reflowStats =
                        moveCrossfadePlanner.addRetainedReflowSlices(
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
                    // #639 评论 5419182722：Run 动画同样走 addRetainedReflowSlices，
                    // 跨行由 old/new lineIndex 决定，不再生成跨行 Move。
                    reflowStats =
                        moveCrossfadePlanner.addRetainedReflowSlices(
                            preCapturedOldSnapshots, preCapturedNewSnapshots,
                            visualIntent, oldRev, newRev,
                            snapshotPlanner.collectExcludedNewByteRanges(animatedSlices),
                            snapshotPlanner.collectExcludedOldByteRanges(animatedSlices),
                            animatedSlices,
                            affectedLayoutPlanner.buildOffsetMapper(visualIntent),
                        )
                }
                AnimationMode.LineReflowAnimation -> {
                    reflowStats =
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

            // #630 评论 5312333045 项2: only animate cursor if geometry actually changed
            val animateCursor =
                visualIntent.coordinatedCursor.shouldAnimate &&
                    (fromX != newRev.cursorX || fromY != newRev.cursorY || fromHeight != newRev.cursorHeight)

            // #637 评论 5386573878：rebase continuation 窗口 — 直接消费旧帧保存的
            // cursorRemainingFraction，连续 rebase 不会反复减速。
            val cursorWindow =
                if (rebaseSnapshot != null && rebaseSnapshot.cursorRect != null) {
                    VisualProgressWindow.fromRemainingFraction(rebaseSnapshot.cursorRemainingFraction)
                } else {
                    VisualProgressWindow.Full
                }
            cursorTransition =
                PreparedVisualTransaction.CursorTransition(
                    fromX = fromX,
                    fromY = fromY,
                    fromHeight = fromHeight,
                    toX = newRev.cursorX,
                    toY = newRev.cursorY,
                    toHeight = newRev.cursorHeight,
                    shouldAnimate = animateCursor,
                    progressWindow = cursorWindow,
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
                // #606: 旧→新逻辑 slice 对应关系由 Core 唯一计算。Android 只提供
                // 平台侧的输入（旧帧 slice 角色/range、新事务 slice 角色/range）和
                // 本次事务的 OffsetMap，不再本地实现任何匹配逻辑。
                val oldSlices =
                    rebaseSnapshot.sliceVisualStates.map { state ->
                        SliceRoleAndByteRange(
                            role = state.role,
                            byteStart = state.clusterByteStart,
                            byteEndExclusive = state.clusterByteEndExclusive,
                        )
                    }
                val newSlicesForMapping =
                    animatedSlices.map { slice ->
                        SliceRoleAndByteRange(
                            role = slice.role,
                            byteStart = slice.clusterByteStart,
                            byteEndExclusive = slice.clusterByteEndExclusive,
                        )
                    }
                val mappings =
                    rebaseMappingProvider?.compute(
                        oldSlices,
                        newSlicesForMapping,
                        visualIntent.offsetMap,
                    ) ?: emptyList()
                rebasePlanner.applyRebaseToSlices(
                    animatedSlices,
                    rebaseSnapshot,
                    snapshotLookup,
                    mappings,
                )
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

        // #639 评论 5420317382：reflow 规划诊断 — 用 MoveCrossfadePlanner 在做出
        // 判断的那一刻累计的真实统计，不从最终 slice 角色反推。sameLineMoves 只有
        // 同行位置变化才计；crossLineCrossfadePairs 只有真正跨行才计（一对计为 1），
        // 同行 shaping 变化、fallback Crossfade、rebase continuation 都不计入。
        // 不记录正文。
        DiagnosticsEvents.editorReflowPlan(
            transactionId = transactionKey,
            oldAffectedLines = affectedOldLineIndices.size,
            newAffectedLines = affectedNewLineIndices.size,
            sameLineMoves = reflowStats.sameLineMoves,
            crossLineCrossfadePairs = reflowStats.crossLinePairs,
            suffixBlockShift = finalBlockShifts.isNotEmpty(),
        )

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
