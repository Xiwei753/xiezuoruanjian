package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.projection.CoordinatedCursor
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论3: InsertDeletePlanner.planClusterReplaceAnimation 契约测试 —
 * 验证 clusters 为空时不生成 alpha 淡入淡出 fallback slice。
 *
 * 背景：clusters 为空意味着无 cluster caret 几何，无法做 clip reveal。
 * 旧路线在 else 分支用 alpha fade（Delete: 1f→0f, Insert: 0f→1f）兜底，
 * 与 planRunReplaceAnimation 不一致且属于已废弃的旧代码模式。
 * #605 评论3 明确要求删除该 fallback，直接静态切换。
 *
 * 这两个测试是回归守卫：如果未来有人重新加回 alpha fallback 分支，
 * 这里会立刻失败。
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

    /** 构造最小 AndroidLayoutRevision，包含一行。 */
    private fun makeSingleLineRevision(revisionId: Long): AndroidLayoutRevision {
        return AndroidLayoutRevision(
            revisionId = revisionId,
            editorRevision = revisionId,
            widthFingerprint = 100f,
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
                        right = 100f,
                    ),
                ),
            cursorUtf8 = 10,
            cursorUtf16 = 10,
            cursorX = 100f,
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

    /** 构造 isReplace()=true 的最小 VisualIntent。 */
    private fun makeReplaceVisualIntent(): VisualIntent {
        return VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 10)),
            newAffectedByteRanges = listOf(Pair(0, 10)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160,
            coordinatedCursor = CoordinatedCursor(0, 10, true),
        )
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
}
