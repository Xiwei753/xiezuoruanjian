package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.projection.CoordinatedCursor
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论3 + 评论4: InsertDeletePlanner 契约测试。
 *
 * 评论3: 验证 clusters 为空时不生成 alpha 淡入淡出 fallback slice。
 * 评论4 问题1: 多 cluster Delete 每个 slice 的 revealSpec 必须绑定到自己的 cluster
 *   （anchorX/boundaryFromX/boundaryToX 来自该 cluster 的 caretStartX/caretEndX），
 *   不再靠列表下标对齐 — planSwallowSpecs 内部按 byte 降序排序后下标会错位。
 * 评论4 问题2: Replace 多个 unmatched old/new cluster 必须分享 [0,1] progress 窗口，
 *   不能每个独占完整窗口。COMPOSITION_UPDATE/COMPOSITION_COMMIT 走同一 replace 路径。
 * 评论4 问题3: hard break cluster 不生成带 revealSpec 的 Insert/Delete slice。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class InsertDeletePlannerTest {
    private val planner = InsertDeletePlanner()

    /** 构造一个 clusters 为空的 AndroidLineSnapshot。 */
    private fun makeEmptyClusterSnapshot(
        snapshotId: Long,
        lineIndex: Int,
    ): AndroidLineSnapshot {
        return AndroidLineSnapshot(
            snapshotId = snapshotId,
            bitmap = null,
            lineIndex = lineIndex,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = RectF(0f, 0f, 100f, 20f),
            clusters = emptyList(),
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            documentUtf16Start = 0,
            documentUtf16EndExclusive = 10,
            baseline = 16f,
            lineHeight = 20f,
        )
    }

    /**
     * 构造带 [clusters] 的 AndroidLineSnapshot。clusters 的 sourceRectInLineImage /
     * visualRectInDocument 已由调用方通过 [makeCluster] 设定。
     */
    private fun makeSnapshotWithClusters(
        snapshotId: Long,
        lineIndex: Int,
        clusters: List<LineClusterSnapshot>,
    ): AndroidLineSnapshot {
        return AndroidLineSnapshot(
            snapshotId = snapshotId,
            bitmap = null,
            lineIndex = lineIndex,
            sourceRect = Rect(0, 0, 150, 20),
            destinationRect = RectF(0f, 0f, 150f, 20f),
            clusters = clusters,
            documentByteStart = 0,
            documentByteEndExclusive = clusters.lastOrNull()?.documentByteEndExclusive ?: 10,
            documentUtf16Start = 0,
            documentUtf16EndExclusive = clusters.lastOrNull()?.documentUtf16EndExclusive ?: 10,
            baseline = 16f,
            lineHeight = 20f,
        )
    }

    /**
     * 构造一个 LineClusterSnapshot，caret 几何由 [caretStartX]/[caretEndX] 决定，
     * byte 区间 [byteStart, byteEnd)，fingerprint 唯一以避免 Replace 匹配。
     */
    private fun makeCluster(
        caretStartX: Float,
        caretEndX: Float,
        byteStart: Int,
        byteEnd: Int,
        fingerprint: String = "fp_$byteStart",
        isHardBreak: Boolean = false,
    ): LineClusterSnapshot {
        val left = kotlin.math.min(caretStartX, caretEndX)
        val right = kotlin.math.max(caretStartX, caretEndX)
        return LineClusterSnapshot(
            clusterId = byteStart.toLong(),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            documentUtf16Start = byteStart,
            documentUtf16EndExclusive = byteEnd,
            sourceRectInLineImage = Rect(left.toInt().coerceAtLeast(0), 0, right.toInt().coerceAtLeast(1), 20),
            visualRectInDocument = RectF(left, 0f, right, 20f),
            shapingFingerprint = fingerprint,
            shapingIdentityConfident = true,
            caretStartX = caretStartX,
            caretEndX = caretEndX,
            isHardBreak = isHardBreak,
        )
    }

    /** 构造最小 AndroidLayoutRevision，包含一行。 */
    private fun makeSingleLineRevision(revisionId: Long): AndroidLayoutRevision {
        return AndroidLayoutRevision(
            revisionId = revisionId,
            editorRevision = revisionId,
            widthFingerprint = 150f,
            fontFingerprint = "fp",
            lineCount = 1,
            lineRanges =
                listOf(
                    AndroidLayoutRevision.LineRange(
                        startUtf8 = 0,
                        endUtf8 = 10,
                        startUtf16 = 0,
                        endUtf16 = 10,
                        top = 0f,
                        bottom = 20f,
                        baseline = 16f,
                        left = 0f,
                        right = 150f,
                    ),
                ),
            cursorUtf8 = 10,
            cursorUtf16 = 10,
            cursorX = 150f,
            cursorY = 0f,
            cursorHeight = 20f,
            selectionAnchorUtf8 = 10,
            selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10,
            selectionHeadUtf16 = 10,
            compositionStartUtf16 = 0,
            compositionEndUtf16 = 0,
            snapshotHandles = emptyList(),
        )
    }

    /** 构造指定 operationKind 的最小 VisualIntent。 */
    private fun makeVisualIntent(
        operationKind: uniffi.writer_core.EditorOperationKindDto,
        oldRanges: List<Pair<Int, Int>> = listOf(Pair(0, 10)),
        newRanges: List<Pair<Int, Int>> = listOf(Pair(0, 10)),
    ): VisualIntent {
        return VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = operationKind,
            oldAffectedByteRanges = oldRanges,
            newAffectedByteRanges = newRanges,
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160,
            coordinatedCursor = CoordinatedCursor(0, 10, true),
        )
    }

    /** 构造 isReplace()=true 的最小 VisualIntent。 */
    private fun makeReplaceVisualIntent(): VisualIntent {
        return makeVisualIntent(uniffi.writer_core.EditorOperationKindDto.REPLACE)
    }

    /**
     * old line clusters 为空时，planClusterReplaceAnimation 不应生成
     * Delete role 且 startAlpha=1f/endAlpha=0f 的 alpha 淡出 fallback slice。
     *
     * 修改前该场景会走 else 分支生成 Delete(1f→0f) slice；
     * 修改后应直接静态切换，animatedSlices 整体为空。
     */
    @Test
    fun replaceWithEmptyOldClustersDoesNotEmitAlphaDeleteSlice() {
        val visualIntent = makeReplaceVisualIntent()
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterReplaceAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            preCapturedOldSnapshots = emptyMap(),
            preCapturedNewSnapshots = emptyMap(),
            createSnapshotFromRevision = { _, lineIndex, _ -> makeEmptyClusterSnapshot(1L, lineIndex) },
            offsetMapper = { _ -> null },
        )

        val hasAlphaDeleteFallback =
            animatedSlices.any { slice ->
                slice.role == SliceRole.Delete && slice.startAlpha == 1f && slice.endAlpha == 0f
            }
        assertFalse(
            "clusters 为空时不应生成 Delete alpha 淡出 fallback slice (startAlpha=1f/endAlpha=0f)",
            hasAlphaDeleteFallback,
        )
        assertTrue(
            "clusters 为空且无 new affected lines 时 animatedSlices 应整体为空（直接静态切换）",
            animatedSlices.isEmpty(),
        )
    }

    /**
     * new line clusters 为空时，planClusterReplaceAnimation 不应生成
     * Insert role 且 startAlpha=0f/endAlpha=1f 的 alpha 淡入 fallback slice。
     *
     * 修改前该场景会走 else 分支生成 Insert(0f→1f) slice；
     * 修改后应直接静态切换，animatedSlices 整体为空。
     */
    @Test
    fun replaceWithEmptyNewClustersDoesNotEmitAlphaInsertSlice() {
        val visualIntent = makeReplaceVisualIntent()
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterReplaceAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = emptySet(),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            preCapturedOldSnapshots = emptyMap(),
            preCapturedNewSnapshots = emptyMap(),
            createSnapshotFromRevision = { _, lineIndex, _ -> makeEmptyClusterSnapshot(2L, lineIndex) },
            offsetMapper = { _ -> null },
        )

        val hasAlphaInsertFallback =
            animatedSlices.any { slice ->
                slice.role == SliceRole.Insert && slice.startAlpha == 0f && slice.endAlpha == 1f
            }
        assertFalse(
            "clusters 为空时不应生成 Insert alpha 淡入 fallback slice (startAlpha=0f/endAlpha=1f)",
            hasAlphaInsertFallback,
        )
        assertTrue(
            "clusters 为空且无 old affected lines 时 animatedSlices 应整体为空（直接静态切换）",
            animatedSlices.isEmpty(),
        )
    }

    /**
     * #605 评论4 问题1: 三个 cluster 删除后，每个 Delete slice 的 revealSpec 必须绑定到
     * 它自己的 cluster — anchorX/caretStartX、boundaryFromX/caretEndX 来自该 cluster，
     * 不再靠列表下标对齐（planSwallowSpecs 内部按 byte 降序排序后下标会错位）。
     */
    @Test
    fun multiClusterDeleteRevealSpecBoundToOwnCluster() {
        // 三个 cluster，byte 0/1/2，caret 0-10 / 10-50 / 50-150
        val near = makeCluster(0f, 10f, 0, 1, "near")
        val mid = makeCluster(10f, 50f, 1, 2, "mid")
        val far = makeCluster(50f, 150f, 2, 3, "far")
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(near, mid, far))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                oldRanges = listOf(Pair(0, 3)),
                newRanges = emptyList(),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterLevelAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, lineIndex, _ -> oldSnapshot },
            offsetMapper = { _ -> null },
        )

        assertEquals("应生成 3 个 Delete slice", 3, animatedSlices.size)
        for (slice in animatedSlices) {
            assertEquals("role 应为 Delete", SliceRole.Delete, slice.role)
            val spec = slice.revealSpec
            assertNotNull("Delete slice 必须带 revealSpec", spec)
            assertEquals("mode 应为 SWALLOW", TextRevealMode.SWALLOW, spec!!.mode)
        }

        // planSwallowSpecs 按 documentByteStart 降序：far(2) → mid(1) → near(0)
        // slice[0] = far: anchorX=50, boundaryFromX=150, boundaryToX=50
        val farSpec = animatedSlices[0].revealSpec!!
        assertEquals(50f, farSpec.anchorX, 0.001f)
        assertEquals(150f, farSpec.boundaryFromX, 0.001f)
        assertEquals(50f, farSpec.boundaryToX, 0.001f)
        assertEquals(2, animatedSlices[0].clusterByteStart)

        // slice[1] = mid: anchorX=10, boundaryFromX=50, boundaryToX=10
        val midSpec = animatedSlices[1].revealSpec!!
        assertEquals(10f, midSpec.anchorX, 0.001f)
        assertEquals(50f, midSpec.boundaryFromX, 0.001f)
        assertEquals(10f, midSpec.boundaryToX, 0.001f)
        assertEquals(1, animatedSlices[1].clusterByteStart)

        // slice[2] = near: anchorX=0, boundaryFromX=10, boundaryToX=0
        val nearSpec = animatedSlices[2].revealSpec!!
        assertEquals(0f, nearSpec.anchorX, 0.001f)
        assertEquals(10f, nearSpec.boundaryFromX, 0.001f)
        assertEquals(0f, nearSpec.boundaryToX, 0.001f)
        assertEquals(0, animatedSlices[2].clusterByteStart)
    }

    /**
     * #605 评论4 问题1+排序: 多 cluster Delete 的 progress window 按 far → near 连续，
     * far cluster 先吞。验证 planSwallowSpecs 的降序排序与连续窗口。
     */
    @Test
    fun multiClusterDeleteWindowContiguousFarToNear() {
        val near = makeCluster(0f, 10f, 0, 1, "near")
        val mid = makeCluster(10f, 50f, 1, 2, "mid")
        val far = makeCluster(50f, 150f, 2, 3, "far")
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(near, mid, far))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                oldRanges = listOf(Pair(0, 3)),
                newRanges = emptyList(),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterLevelAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, lineIndex, _ -> oldSnapshot },
            offsetMapper = { _ -> null },
        )

        assertEquals(3, animatedSlices.size)
        // far cluster 先吞：clusterByteStart=2 在 slice[0]
        assertEquals("far cluster 应先吞", 2, animatedSlices[0].clusterByteStart)
        assertEquals("mid cluster 应第二", 1, animatedSlices[1].clusterByteStart)
        assertEquals("near cluster 应最后", 0, animatedSlices[2].clusterByteStart)

        // progress window 连续：[0, w0] [w0, w1] [w1, 1]
        val s0 = animatedSlices[0].revealSpec!!
        val s1 = animatedSlices[1].revealSpec!!
        val s2 = animatedSlices[2].revealSpec!!
        assertEquals(0f, s0.progressStart, 0.001f)
        assertEquals(s0.progressEnd, s1.progressStart, 0.001f)
        assertEquals(s1.progressEnd, s2.progressStart, 0.001f)
        assertEquals(1f, s2.progressEnd, 0.001f)
        // 每个 window 非空
        assertTrue("far window 非空", s0.progressEnd > s0.progressStart)
        assertTrue("mid window 非空", s1.progressEnd > s1.progressStart)
        assertTrue("near window 非空", s2.progressEnd > s2.progressStart)
    }

    /**
     * #605 评论4 问题2: Replace 两个 unmatched old/new cluster 不能全部拿 [0,1]，
     * 必须分享 progress 窗口。fingerprint 不同确保全部 unmatched。
     */
    @Test
    fun replaceTwoUnmatchedClustersShareProgressWindow() {
        val oldC1 = makeCluster(0f, 50f, 0, 1, "old1")
        val oldC2 = makeCluster(50f, 100f, 1, 2, "old2")
        val newC1 = makeCluster(0f, 50f, 0, 1, "new1")
        val newC2 = makeCluster(50f, 100f, 1, 2, "new2")
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(oldC1, oldC2))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, listOf(newC1, newC2))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.REPLACE,
                oldRanges = listOf(Pair(0, 2)),
                newRanges = listOf(Pair(0, 2)),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterReplaceAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, _, isNew -> if (isNew) newSnapshot else oldSnapshot },
            offsetMapper = { _ -> null },
        )

        val deleteSlices = animatedSlices.filter { it.role == SliceRole.Delete }
        val insertSlices = animatedSlices.filter { it.role == SliceRole.Insert }
        assertEquals("应有 2 个 Delete (swallow) slice", 2, deleteSlices.size)
        assertEquals("应有 2 个 Insert (reveal) slice", 2, insertSlices.size)

        // 关键契约：不是所有 slice 都独占 [0,1]
        val allFullWindow =
            animatedSlices.all { slice ->
                val spec = slice.revealSpec ?: return@all false
                spec.progressStart == 0f && spec.progressEnd == 1f
            }
        assertFalse("多 cluster Replace 不应每个 slice 独占 [0,1] 窗口", allFullWindow)

        // Delete (swallow) window 连续且分享
        val d0 = deleteSlices[0].revealSpec!!
        val d1 = deleteSlices[1].revealSpec!!
        assertEquals(0f, d0.progressStart, 0.001f)
        assertEquals(d0.progressEnd, d1.progressStart, 0.001f)
        assertEquals(1f, d1.progressEnd, 0.001f)
        assertTrue("Delete window 分享：d0 不应独占", d0.progressEnd < 1f)
        assertTrue("Delete window 分享：d1 不应独占", d1.progressStart > 0f)

        // Insert (reveal) window 连续且分享
        val i0 = insertSlices[0].revealSpec!!
        val i1 = insertSlices[1].revealSpec!!
        assertEquals(0f, i0.progressStart, 0.001f)
        assertEquals(i0.progressEnd, i1.progressStart, 0.001f)
        assertEquals(1f, i1.progressEnd, 0.001f)
        assertTrue("Insert window 分享：i0 不应独占", i0.progressEnd < 1f)
        assertTrue("Insert window 分享：i1 不应独占", i1.progressStart > 0f)
    }

    /**
     * #605 评论4 问题2: COMPOSITION_UPDATE 多 cluster 同样走 replace 路径，
     * unmatched cluster 分享连续窗口。
     */
    @Test
    fun compositionUpdateMultiClusterSharesProgressWindow() {
        val oldC1 = makeCluster(0f, 50f, 0, 1, "pre1")
        val oldC2 = makeCluster(50f, 100f, 1, 2, "pre2")
        val newC1 = makeCluster(0f, 50f, 0, 1, "post1")
        val newC2 = makeCluster(50f, 100f, 1, 2, "post2")
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(oldC1, oldC2))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, listOf(newC1, newC2))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE,
                oldRanges = listOf(Pair(0, 2)),
                newRanges = listOf(Pair(0, 2)),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterReplaceAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, _, isNew -> if (isNew) newSnapshot else oldSnapshot },
            offsetMapper = { _ -> null },
        )

        val deleteSlices = animatedSlices.filter { it.role == SliceRole.Delete }
        val insertSlices = animatedSlices.filter { it.role == SliceRole.Insert }
        assertEquals(2, deleteSlices.size)
        assertEquals(2, insertSlices.size)
        // 分享窗口：不全部独占 [0,1]
        val allFull =
            animatedSlices.all { slice ->
                val spec = slice.revealSpec ?: return@all false
                spec.progressStart == 0f && spec.progressEnd == 1f
            }
        assertFalse("COMPOSITION_UPDATE 多 cluster 不应独占 [0,1]", allFull)
    }

    /**
     * #605 评论4 问题2: COMPOSITION_COMMIT 多 cluster 同样走 replace 路径。
     */
    @Test
    fun compositionCommitMultiClusterSharesProgressWindow() {
        val oldC1 = makeCluster(0f, 50f, 0, 1, "pre1")
        val oldC2 = makeCluster(50f, 100f, 1, 2, "pre2")
        val newC1 = makeCluster(0f, 50f, 0, 1, "commit1")
        val newC2 = makeCluster(50f, 100f, 1, 2, "commit2")
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(oldC1, oldC2))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, listOf(newC1, newC2))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
                oldRanges = listOf(Pair(0, 2)),
                newRanges = listOf(Pair(0, 2)),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterReplaceAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, _, isNew -> if (isNew) newSnapshot else oldSnapshot },
            offsetMapper = { _ -> null },
        )

        val deleteSlices = animatedSlices.filter { it.role == SliceRole.Delete }
        val insertSlices = animatedSlices.filter { it.role == SliceRole.Insert }
        assertEquals(2, deleteSlices.size)
        assertEquals(2, insertSlices.size)
        val allFull =
            animatedSlices.all { slice ->
                val spec = slice.revealSpec ?: return@all false
                spec.progressStart == 0f && spec.progressEnd == 1f
            }
        assertFalse("COMPOSITION_COMMIT 多 cluster 不应独占 [0,1]", allFull)
    }

    /**
     * #605 评论4 问题3: hard break cluster 不生成带 revealSpec 的 Insert/Delete slice。
     * Insert 路径：visible + hardBreak → 只有 visible 生成 slice，且独占 [0,1]。
     */
    @Test
    fun hardBreakClusterDoesNotEmitInsertRevealSlice() {
        val visible = makeCluster(0f, 100f, 0, 1, "vis")
        val hardBreak = makeCluster(100f, 100f, 1, 2, "hb", isHardBreak = true)
        val newSnapshot = makeSnapshotWithClusters(2L, 0, listOf(visible, hardBreak))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.INSERT,
                oldRanges = emptyList(),
                newRanges = listOf(Pair(0, 2)),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterLevelAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = emptySet(),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, lineIndex, _ -> newSnapshot },
            offsetMapper = { _ -> null },
        )

        // 只有 visible cluster 生成 Insert slice
        assertEquals("hard break 应被过滤，只 1 个 Insert slice", 1, animatedSlices.size)
        assertEquals(SliceRole.Insert, animatedSlices[0].role)
        val spec = animatedSlices[0].revealSpec
        assertNotNull("visible cluster 的 slice 必须带 revealSpec", spec)
        // visible 独占完整窗口（hard break 不消耗 progress）
        assertEquals(0f, spec!!.progressStart, 0.001f)
        assertEquals(1f, spec.progressEnd, 0.001f)
        // 该 slice 绑定的是 visible cluster (byte 0)
        assertEquals(0, animatedSlices[0].clusterByteStart)
    }

    /**
     * #605 评论4 问题3: hard break cluster 不生成带 revealSpec 的 Delete slice。
     * Delete 路径：visible + hardBreak → 只有 visible 生成 slice。
     */
    @Test
    fun hardBreakClusterDoesNotEmitDeleteRevealSlice() {
        val visible = makeCluster(0f, 100f, 0, 1, "vis")
        val hardBreak = makeCluster(100f, 100f, 1, 2, "hb", isHardBreak = true)
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(visible, hardBreak))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                oldRanges = listOf(Pair(0, 2)),
                newRanges = emptyList(),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planClusterLevelAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, lineIndex, _ -> oldSnapshot },
            offsetMapper = { _ -> null },
        )

        assertEquals("hard break 应被过滤，只 1 个 Delete slice", 1, animatedSlices.size)
        assertEquals(SliceRole.Delete, animatedSlices[0].role)
        val spec = animatedSlices[0].revealSpec
        assertNotNull("visible cluster 的 slice 必须带 revealSpec", spec)
        assertEquals(0f, spec!!.progressStart, 0.001f)
        assertEquals(1f, spec.progressEnd, 0.001f)
        assertEquals(0, animatedSlices[0].clusterByteStart)
    }
}
