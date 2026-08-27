package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole

class InsertDeletePlanner {
    private val caretRevealPlanner = CaretRevealPlanner()

    /** #639 评论 5425871530 第一部分：从 cluster 构造 [PreparedVisualTransaction.CaretRevealGeometry]。 */
    private fun caretGeometryOf(cluster: LineClusterSnapshot): PreparedVisualTransaction.CaretRevealGeometry =
        PreparedVisualTransaction.CaretRevealGeometry(
            visualRect = cluster.visualRectInDocument,
            caretStartX = cluster.caretStartX,
            caretEndX = cluster.caretEndX,
        )

    fun planClusterLevelAnimation(
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
        if (visualIntent.isReplaceRenderRole()) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedOldLineIndices, affectedNewLineIndices,
                animatedSlices, staticPatches,
                preCapturedOldSnapshots, preCapturedNewSnapshots,
                createSnapshotFromRevision, offsetMapper,
            )
            return
        }

        if (visualIntent.isInsertRenderRole()) {
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
            // #605 评论4 问题1: 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot。
            // 不再靠列表下标对齐 — planSwallowSpecs 内部排序后下标会与原列表错位。
            val revealSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
            for ((cluster, snapshot) in insertClusterSnapshots) {
                revealSnapshotByCluster[cluster] = snapshot
            }
            for (plan in caretRevealPlanner.planRevealSpecs(insertClusterSnapshots.map { it.first })) {
                val cluster = plan.cluster
                val snapshot = revealSnapshotByCluster[cluster] ?: continue
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
                        revealSpec = plan.spec,
                        caretRevealGeometry = caretGeometryOf(cluster),
                    ),
                )
            }
        } else if (visualIntent.isDeleteRenderRole()) {
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
            // #605 评论4 问题1: 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot。
            // planSwallowSpecs 内部按 documentByteStart 降序排序，下标与原列表错位，
            // 必须用 plan.cluster 引用匹配而非 specs[i]。
            val swallowSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
            for ((cluster, snapshot) in deleteClusterSnapshots) {
                swallowSnapshotByCluster[cluster] = snapshot
            }
            for (plan in caretRevealPlanner.planSwallowSpecs(deleteClusterSnapshots.map { it.first })) {
                val cluster = plan.cluster
                val snapshot = swallowSnapshotByCluster[cluster] ?: continue
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
                        revealSpec = plan.spec,
                        caretRevealGeometry = caretGeometryOf(cluster),
                    ),
                )
            }
        }
    }

    fun planRunAnimation(
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
        if (visualIntent.isReplaceRenderRole()) {
            planRunReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedOldLineIndices, affectedNewLineIndices,
                animatedSlices, staticPatches,
                preCapturedOldSnapshots, preCapturedNewSnapshots,
                createSnapshotFromRevision, offsetMapper,
            )
            return
        }

        if (visualIntent.isInsertRenderRole()) {
            val insertClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedNewLineIndices) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
                val insertClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in insertClusters) {
                    insertClusterSnapshots.add(Pair(cluster, newSnapshot))
                }
            }
            // #605 评论4 问题1: 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot。
            // 不再靠列表下标对齐 — planSwallowSpecs 内部排序后下标会与原列表错位。
            val revealSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
            for ((cluster, snapshot) in insertClusterSnapshots) {
                revealSnapshotByCluster[cluster] = snapshot
            }
            for (plan in caretRevealPlanner.planRevealSpecs(insertClusterSnapshots.map { it.first })) {
                val cluster = plan.cluster
                val snapshot = revealSnapshotByCluster[cluster] ?: continue
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
                        revealSpec = plan.spec,
                        caretRevealGeometry = caretGeometryOf(cluster),
                    ),
                )
            }
        } else if (visualIntent.isDeleteRenderRole()) {
            val deleteClusterSnapshots = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
            for (lineIndex in affectedOldLineIndices) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
                val deleteClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in deleteClusters) {
                    deleteClusterSnapshots.add(Pair(cluster, oldSnapshot))
                }
            }
            // #605 评论4 问题1: 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot。
            // planSwallowSpecs 内部按 documentByteStart 降序排序，下标与原列表错位，
            // 必须用 plan.cluster 引用匹配而非 specs[i]。
            val swallowSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
            for ((cluster, snapshot) in deleteClusterSnapshots) {
                swallowSnapshotByCluster[cluster] = snapshot
            }
            for (plan in caretRevealPlanner.planSwallowSpecs(deleteClusterSnapshots.map { it.first })) {
                val cluster = plan.cluster
                val snapshot = swallowSnapshotByCluster[cluster] ?: continue
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
                        revealSpec = plan.spec,
                        caretRevealGeometry = caretGeometryOf(cluster),
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
        // #605 评论5 问题1: 在合并前就排除 hard-break cluster，避免 abc\n 整个 run
        // 被标成 hard break 导致 abc 失去吐字/吞字动画。换行造成的排版变化继续交给
        // 现有 Move/BlockShift。
        val affected =
            clusters.filter { cluster ->
                !cluster.isHardBreak &&
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
                            isHardBreak = false,
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
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            oldRev.lineRangeAt(lineIndex) ?: continue
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
            newRev.lineRangeAt(lineIndex) ?: continue
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

        // #605 评论4 问题2: 收集未匹配 old/new cluster，循环结束后一次性规划，
        // 让多个 cluster 共享 [0,1] progress 窗口。逐个 planXxxSpecs(listOf(...)) 会让
        // 每个 cluster 独占完整窗口，多字替换时同时 0→1，丢失序列感。
        val unmatchedOld = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
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
                            caretRevealGeometry = caretGeometryOf(newCluster),
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
                            caretRevealGeometry = caretGeometryOf(oldCluster),
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
                            caretRevealGeometry = caretGeometryOf(newCluster),
                        ),
                    )
                }
            } else {
                unmatchedOld.add(Pair(oldCluster, oldSnapshot))
            }
        }

        val unmatchedNew = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            unmatchedNew.add(pair)
        }

        // #605 评论4 问题1+2: 一次性规划 swallow/reveal，多字替换时 cluster 按距离/顺序分享 [0,1] 窗口；
        // 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot，不靠下标对齐。
        val swallowSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
        for ((cluster, snapshot) in unmatchedOld) {
            swallowSnapshotByCluster[cluster] = snapshot
        }
        for (plan in caretRevealPlanner.planSwallowSpecs(unmatchedOld.map { it.first })) {
            val cluster = plan.cluster
            val snapshot = swallowSnapshotByCluster[cluster] ?: continue
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
                    revealSpec = plan.spec,
                ),
            )
        }
        val revealSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
        for ((cluster, snapshot) in unmatchedNew) {
            revealSnapshotByCluster[cluster] = snapshot
        }
        for (plan in caretRevealPlanner.planRevealSpecs(unmatchedNew.map { it.first })) {
            val cluster = plan.cluster
            val snapshot = revealSnapshotByCluster[cluster] ?: continue
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
                    revealSpec = plan.spec,
                ),
            )
        }
    }

    fun planRunReplaceAnimation(
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
        val allOldAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        val allNewAffectedClusters = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()

        for (lineIndex in affectedOldLineIndices) {
            val oldLineRange = oldRev.lineRangeAt(lineIndex) ?: continue
            val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, false) ?: continue
            if (visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                val oldRunClusters = groupClustersIntoRuns(oldSnapshot.clusters, visualIntent.oldAffectedByteRanges)
                for (cluster in oldRunClusters) {
                    allOldAffectedClusters.add(Pair(cluster, oldSnapshot))
                }
            }
        }

        for (lineIndex in affectedNewLineIndices) {
            val newLineRange = newRev.lineRangeAt(lineIndex) ?: continue
            val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, true) ?: continue
            if (visualIntent.newAffectedByteRanges.isNotEmpty()) {
                val newRunClusters = groupClustersIntoRuns(newSnapshot.clusters, visualIntent.newAffectedByteRanges)
                for (cluster in newRunClusters) {
                    allNewAffectedClusters.add(Pair(cluster, newSnapshot))
                }
            }
        }

        // #605 评论4 问题2: 收集未匹配 old/new cluster，循环结束后一次性规划，
        // 让多个 cluster 共享 [0,1] progress 窗口。逐个 planXxxSpecs(listOf(...)) 会让
        // 每个 cluster 独占完整窗口，多字替换时同时 0→1，丢失序列感。
        val unmatchedOld = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
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
                            caretRevealGeometry = caretGeometryOf(newCluster),
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
                            caretRevealGeometry = caretGeometryOf(oldCluster),
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
                            caretRevealGeometry = caretGeometryOf(newCluster),
                        ),
                    )
                }
            } else {
                unmatchedOld.add(Pair(oldCluster, oldSnapshot))
            }
        }

        val unmatchedNew = mutableListOf<Pair<LineClusterSnapshot, AndroidLineSnapshot>>()
        for ((i, pair) in allNewAffectedClusters.withIndex()) {
            if (i in newMatched) continue
            unmatchedNew.add(pair)
        }

        // #605 评论4 问题1+2: 一次性规划 swallow/reveal，多字替换时 cluster 按距离/顺序分享 [0,1] 窗口；
        // 用 CaretRevealPlan 绑定 cluster+spec，按 cluster 引用找回 snapshot，不靠下标对齐。
        val swallowSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
        for ((cluster, snapshot) in unmatchedOld) {
            swallowSnapshotByCluster[cluster] = snapshot
        }
        for (plan in caretRevealPlanner.planSwallowSpecs(unmatchedOld.map { it.first })) {
            val cluster = plan.cluster
            val snapshot = swallowSnapshotByCluster[cluster] ?: continue
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
                    revealSpec = plan.spec,
                ),
            )
        }
        val revealSnapshotByCluster = java.util.IdentityHashMap<LineClusterSnapshot, AndroidLineSnapshot>()
        for ((cluster, snapshot) in unmatchedNew) {
            revealSnapshotByCluster[cluster] = snapshot
        }
        for (plan in caretRevealPlanner.planRevealSpecs(unmatchedNew.map { it.first })) {
            val cluster = plan.cluster
            val snapshot = revealSnapshotByCluster[cluster] ?: continue
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
                    revealSpec = plan.spec,
                ),
            )
        }
    }
}
