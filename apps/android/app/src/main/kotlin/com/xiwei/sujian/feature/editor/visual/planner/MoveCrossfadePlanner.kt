package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole

class MoveCrossfadePlanner {
    /** #639 评论 5425871530 第一部分：从 cluster 构造 [PreparedVisualTransaction.CaretRevealGeometry]。 */
    private fun caretGeometryOf(cluster: LineClusterSnapshot): PreparedVisualTransaction.CaretRevealGeometry =
        PreparedVisualTransaction.CaretRevealGeometry(
            visualRect = cluster.visualRectInDocument,
            caretStartX = cluster.caretStartX,
            caretEndX = cluster.caretEndX,
        )

    /**
     * #639 评论 5420317382：reflow 规划真实统计 — 在 [appendRetainedTransition]
     * 做出判断的那一刻累计，不从最终 slice 角色反推。
     *
     * - [sameLineMoves]：只有 `sameShape && oldLineIndex == newLineIndex && positionChanged` 时 +1。
     * - [crossLinePairs]：只有 `oldLineIndex != newLineIndex` 时 +1（一对 CrossfadeOld+CrossfadeNew 计为 1）。
     *   同行 shaping 变化虽然也生成 Crossfade pair，但不计入跨行数。
     */
    data class ReflowPlanStats(
        var sameLineMoves: Int = 0,
        var crossLinePairs: Int = 0,
    )

    /**
     * #639 评论 5419182722：自动折行（Core 看不到 `\n`，仍走 Glyph/Cluster/Run）
     * 的保留字符 reflow 规划。匹配成功后统一委托 [appendRetainedTransition]：
     * 同视觉行且形状不变 → SliceRole.Move 位置插值；跨视觉行或形状变化 →
     * CrossfadeOld+CrossfadeNew（旧位置淡出 + 新位置淡入），不再生成跨行 Move
     * （二维直线飞字会导致乱跳闪烁）。
     */
    fun addRetainedReflowSlices(
        allOldSnapshots: Map<Int, AndroidLineSnapshot>,
        allNewSnapshots: Map<Int, AndroidLineSnapshot>,
        visualIntent: VisualIntent,
        oldRev: LayoutRevisionSource,
        newRev: LayoutRevisionSource,
        excludedNewByteRanges: Set<Pair<Int, Int>>,
        excludedOldByteRanges: Set<Pair<Int, Int>>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        offsetMapper: (Int) -> Int?,
    ): ReflowPlanStats {
        val stats = ReflowPlanStats()
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
                // #639 评论 5419182722：跨视觉行的保留字符不再生成 SliceRole.Move。
                // 统一走 appendRetainedTransition：同行且形状不变 → Move 位置插值；
                // 跨行或形状变化 → CrossfadeOld+CrossfadeNew。
                appendRetainedTransition(
                    oldCluster = oldCluster,
                    oldSnapshot = oldInfo.second,
                    oldLineIndex = oldInfo.first,
                    newCluster = newCluster,
                    newSnapshot = newInfo.second,
                    newLineIndex = newInfo.first,
                    out = animatedSlices,
                    stats = stats,
                )
            }
        }
        return stats
    }

    fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: LayoutRevisionSource,
        newRev: LayoutRevisionSource,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        createSnapshotFromRevision: (LayoutRevisionSource, Int, Boolean) -> AndroidLineSnapshot?,
        offsetMapper: (Int) -> Int?,
    ): ReflowPlanStats {
        val stats = ReflowPlanStats()
        val affectedOldParagraphIds = mutableSetOf<Int>()
        val affectedNewParagraphIds = mutableSetOf<Int>()
        for (lineIndex in affectedOldLineIndices) {
            oldRev.lineRangeAt(lineIndex)?.paragraphId?.let { affectedOldParagraphIds.add(it) }
        }
        for (lineIndex in affectedNewLineIndices) {
            newRev.lineRangeAt(lineIndex)?.paragraphId?.let { affectedNewParagraphIds.add(it) }
        }

        val allOldClusters = mutableListOf<Triple<LineClusterSnapshot, AndroidLineSnapshot, Int>>()
        for ((lineIndex, lineRange) in oldRev.lineEntries()) {
            if (lineRange.paragraphId !in affectedOldParagraphIds) continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
            for (cluster in oldSnapshot.clusters) {
                allOldClusters.add(Triple(cluster, oldSnapshot, lineIndex))
            }
        }

        val allNewClusters = mutableListOf<Triple<LineClusterSnapshot, AndroidLineSnapshot, Int>>()
        for ((lineIndex, lineRange) in newRev.lineEntries()) {
            if (lineRange.paragraphId !in affectedNewParagraphIds) continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
            for (cluster in newSnapshot.clusters) {
                allNewClusters.add(Triple(cluster, newSnapshot, lineIndex))
            }
        }

        val newUsed = mutableSetOf<Int>()
        val oldMatched = mutableSetOf<Int>()
        var lastMatchedNewStart = 0

        for ((oldIdx, triple) in allOldClusters.withIndex()) {
            val (oldCluster, oldSnapshot, oldLineIndex) = triple
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
                val (newCluster, newSnapshot, newLineIndex) = allNewClusters[matchedNewIdx]
                // #639 评论 5419182722：手动换行也统一走 appendRetainedTransition，
                // 与自动折行共用同一份跨行规则，不再生成跨行 Move。
                appendRetainedTransition(
                    oldCluster = oldCluster,
                    oldSnapshot = oldSnapshot,
                    oldLineIndex = oldLineIndex,
                    newCluster = newCluster,
                    newSnapshot = newSnapshot,
                    newLineIndex = newLineIndex,
                    out = animatedSlices,
                    stats = stats,
                )
            }
        }

        for ((oldIdx, triple) in allOldClusters.withIndex()) {
            if (oldIdx in oldMatched) continue
            val (oldCluster, oldSnapshot, _) = triple
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

        for ((newIdx, triple) in allNewClusters.withIndex()) {
            if (newIdx in newUsed) continue
            val (newCluster, newSnapshot, _) = triple
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
        return stats
    }

    /**
     * #639 评论 5419182722：保留字符的统一跨行/同行过渡规则。
     *
     * - 形状不变且位置不变：不生成任何 slice（静态保留）。
     * - 形状不变且仍在同一视觉行（[oldLineIndex] == [newLineIndex]）：生成 SliceRole.Move
     *   做位置插值（同行横向挤动，不会乱跳）。
     * - 跨视觉行（[oldLineIndex] != [newLineIndex]）或形状变化：生成 CrossfadeOld +
     *   CrossfadeNew，旧位置淡出、新位置淡入，两者都钉死在各自 Layout 真值坐标，
     *   不再从右上角斜着飞到左下角。
     *
     * [oldLineIndex]/[newLineIndex] 直接取自 [AndroidLineSnapshot.lineIndex]，
     * 不按 Y 坐标猜是否同一行。
     */
    private fun appendRetainedTransition(
        oldCluster: LineClusterSnapshot,
        oldSnapshot: AndroidLineSnapshot,
        oldLineIndex: Int,
        newCluster: LineClusterSnapshot,
        newSnapshot: AndroidLineSnapshot,
        newLineIndex: Int,
        out: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        stats: ReflowPlanStats,
    ) {
        val sameShape =
            oldCluster.shapingIdentityConfident &&
                newCluster.shapingIdentityConfident &&
                oldCluster.shapingFingerprint == newCluster.shapingFingerprint

        val positionChanged =
            oldCluster.visualRectInDocument != newCluster.visualRectInDocument

        if (sameShape && !positionChanged) return

        // 只有还在同一条视觉行里的保留字符才允许做位置插值。
        if (sameShape && oldLineIndex == newLineIndex) {
            // #639 评论 5420317382：真实统计 — 只有 sameShape && 同行 && 位置变化
            // 才计为 sameLineMove。
            stats.sameLineMoves++
            out.add(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Move,
                    snapshot = newSnapshot,
                    sourceRect = newCluster.sourceRectInLineImage,
                    destinationRect = newCluster.visualRectInDocument,
                    fromDestinationRect = oldCluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 1f,
                    clusterByteStart = newCluster.documentByteStart,
                    clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                    caretRevealGeometry = caretGeometryOf(newCluster),
                ),
            )
            return
        }

        // 一旦跨视觉行，不从右上角斜着飞到左下角。
        // 旧位置退场，新位置进场；两者都固定在各自 Layout 真值坐标。
        // #639 评论 5420317382：真实统计 — 只有 oldLineIndex != newLineIndex
        // 才计为跨行 pair（一对 CrossfadeOld+CrossfadeNew 计为 1）。同行 shaping
        // 变化虽然也生成 Crossfade pair，但不计入跨行数。
        if (oldLineIndex != newLineIndex) {
            stats.crossLinePairs++
        }
        out.add(
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeOld,
                snapshot = oldSnapshot,
                sourceRect = oldCluster.sourceRectInLineImage,
                destinationRect = oldCluster.visualRectInDocument,
                startAlpha = 1f,
                endAlpha = 0f,
                clusterByteStart = oldCluster.documentByteStart,
                clusterByteEndExclusive = oldCluster.documentByteEndExclusive,
                caretRevealGeometry = caretGeometryOf(oldCluster),
            ),
        )
        out.add(
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeNew,
                snapshot = newSnapshot,
                sourceRect = newCluster.sourceRectInLineImage,
                destinationRect = newCluster.visualRectInDocument,
                startAlpha = 0f,
                endAlpha = 1f,
                clusterByteStart = newCluster.documentByteStart,
                clusterByteEndExclusive = newCluster.documentByteEndExclusive,
                caretRevealGeometry = caretGeometryOf(newCluster),
            ),
        )
    }

    fun planCrossfadeAnimation(
        visualIntent: VisualIntent,
        oldRev: LayoutRevisionSource,
        newRev: LayoutRevisionSource,
        affectedOldLineIndices: Set<Int>,
        affectedNewLineIndices: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        createSnapshotFromRevision: (LayoutRevisionSource, Int, Boolean) -> AndroidLineSnapshot?,
        offsetMapper: (Int) -> Int?,
    ) {
        val matchedNewLineIndices = mutableSetOf<Int>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRangeAt(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue

            var bestNewLineIdx: Int? = null
            val mappedStart = offsetMapper(oldLineRange.startUtf8)
            if (mappedStart != null) {
                for (newLineIdx in affectedNewLineIndices) {
                    if (newLineIdx in matchedNewLineIndices) continue
                    val newLineRange = newRev.lineRangeAt(newLineIdx) ?: continue
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
                        val newLineRange = newRev.lineRangeAt(newLineIdx) ?: continue
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
                    val newLineRange = newRev.lineRangeAt(newLineIdx) ?: continue
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
                val newLineRange = newRev.lineRangeAt(bestNewLineIdx) ?: continue
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
            val newLineRange = newRev.lineRangeAt(lineIndex) ?: continue
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
