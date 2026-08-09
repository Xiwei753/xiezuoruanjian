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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论5 问题1: RunAnimation hard-break 回归测试。
 *
 * groupClustersIntoRuns 在合并前排除 hard-break cluster，避免 abc+换行 整个 run
 * 被标成 hard break 导致 abc 失去吐字/吞字动画。换行造成的排版变化继续交给
 * 现有 Move/BlockShift。
 *
 * 从 InsertDeletePlannerTest 拆分到独立类以满足 detekt LargeClass 阈值（500行）。
 * Helper 自包含以保持测试独立性。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class InsertDeletePlannerRunAnimationTest {
    private val planner = InsertDeletePlanner()

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

    /**
     * #605 评论5 问题1: RunAnimation Insert 路径，abc+换行 中 abc 仍生成 REVEAL slice。
     * groupClustersIntoRuns 在合并前排除 hard-break，abc 合并成一个 run(byte 0-3)，
     * 不因换行符被整体过滤而丢失吐字动画。
     */
    @Test
    fun runAnimationInsertWithHardBreakKeepsVisibleReveal() {
        val a = makeCluster(0f, 10f, 0, 1, "a")
        val b = makeCluster(10f, 20f, 1, 2, "b")
        val c = makeCluster(20f, 30f, 2, 3, "c")
        val hardBreak = makeCluster(30f, 30f, 3, 4, "hb", isHardBreak = true)
        val newSnapshot = makeSnapshotWithClusters(2L, 0, listOf(a, b, c, hardBreak))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.INSERT,
                oldRanges = emptyList(),
                newRanges = listOf(Pair(0, 4)),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planRunAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = emptySet(),
            affectedNewLineIndices = setOf(0),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, _, _ -> newSnapshot },
            offsetMapper = { _ -> null },
        )

        assertEquals("应生成 1 个 Insert slice (abc 合并成一个 run)", 1, animatedSlices.size)
        assertEquals(SliceRole.Insert, animatedSlices[0].role)
        val spec = animatedSlices[0].revealSpec
        assertNotNull("Insert slice 必须带 revealSpec", spec)
        assertEquals(0f, spec!!.progressStart, 0.001f)
        assertEquals(1f, spec.progressEnd, 0.001f)
        assertEquals(0, animatedSlices[0].clusterByteStart)
        assertEquals(3, animatedSlices[0].clusterByteEndExclusive)
    }

    /**
     * #605 评论5 问题1: RunAnimation Delete 路径，abc+换行 中 abc 仍生成 SWALLOW slice。
     */
    @Test
    fun runAnimationDeleteWithHardBreakKeepsVisibleSwallow() {
        val a = makeCluster(0f, 10f, 0, 1, "a")
        val b = makeCluster(10f, 20f, 1, 2, "b")
        val c = makeCluster(20f, 30f, 2, 3, "c")
        val hardBreak = makeCluster(30f, 30f, 3, 4, "hb", isHardBreak = true)
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, listOf(a, b, c, hardBreak))

        val visualIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                oldRanges = listOf(Pair(0, 4)),
                newRanges = emptyList(),
            )
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()

        planner.planRunAnimation(
            visualIntent = visualIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = animatedSlices,
            staticPatches = staticPatches,
            createSnapshotFromRevision = { _, _, _ -> oldSnapshot },
            offsetMapper = { _ -> null },
        )

        assertEquals("应生成 1 个 Delete slice (abc 合并成一个 run)", 1, animatedSlices.size)
        assertEquals(SliceRole.Delete, animatedSlices[0].role)
        val spec = animatedSlices[0].revealSpec
        assertNotNull("Delete slice 必须带 revealSpec", spec)
        assertEquals(TextRevealMode.SWALLOW, spec!!.mode)
        assertEquals(0, animatedSlices[0].clusterByteStart)
        assertEquals(3, animatedSlices[0].clusterByteEndExclusive)
    }

    /**
     * #605 评论5 问题1: 纯换行符（只有 hard-break cluster）不生成 reveal slice。
     * Insert 和 Delete 路径都应为空。
     */
    @Test
    fun runAnimationPureHardBreakEmitsNoRevealSlice() {
        val hardBreak = makeCluster(0f, 0f, 0, 1, "hb", isHardBreak = true)
        val snapshot = makeSnapshotWithClusters(1L, 0, listOf(hardBreak))
        val oldRev = makeSingleLineRevision(revisionId = 0)
        val newRev = makeSingleLineRevision(revisionId = 1)

        // Insert 路径
        val insertIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.INSERT,
                oldRanges = emptyList(),
                newRanges = listOf(Pair(0, 1)),
            )
        val insertSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val insertPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        planner.planRunAnimation(
            visualIntent = insertIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = emptySet(),
            affectedNewLineIndices = setOf(0),
            animatedSlices = insertSlices,
            staticPatches = insertPatches,
            createSnapshotFromRevision = { _, _, _ -> snapshot },
            offsetMapper = { _ -> null },
        )
        assertTrue("纯 hard break Insert 应无 reveal slice", insertSlices.isEmpty())

        // Delete 路径
        val deleteIntent =
            makeVisualIntent(
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                oldRanges = listOf(Pair(0, 1)),
                newRanges = emptyList(),
            )
        val deleteSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        val deletePatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        planner.planRunAnimation(
            visualIntent = deleteIntent,
            oldRev = oldRev,
            newRev = newRev,
            affectedOldLineIndices = setOf(0),
            affectedNewLineIndices = emptySet(),
            animatedSlices = deleteSlices,
            staticPatches = deletePatches,
            createSnapshotFromRevision = { _, _, _ -> snapshot },
            offsetMapper = { _ -> null },
        )
        assertTrue("纯 hard break Delete 应无 reveal slice", deleteSlices.isEmpty())
    }
}
