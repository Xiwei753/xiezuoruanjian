package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.TransactionState
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论3: RebasePlanner 契约测试 — 验证 revealFraction 连续性契约。
 *
 * 覆盖场景：
 * 1. Insert slice rebase 时 revealSpec.initialFraction 取自旧帧的 revealFraction。
 * 2. Delete slice rebase 时 revealSpec.initialFraction 取自旧帧的 revealFraction。
 * 3. 未匹配的 Delete slice 在 revealFraction < 0.99 时继续在新事务中吞完。
 * 4. 未匹配的 Delete slice 在 revealFraction >= 0.99 时不继续。
 * 5. 向后兼容：无 revealFraction 但 currentAlpha > 0.01 时仍继续。
 * 6. 向后兼容：无 revealFraction 且 currentAlpha <= 0.01 时不继续。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebasePlannerTest {
    private val planner = RebasePlanner()

    /**
     * 构造一个带 revealSpec 的 AnimatedSlice。
     * [initialFraction] 用于验证 rebase 后是否被正确覆盖。
     */
    private fun makeAnimatedSlice(
        role: SliceRole,
        initialFraction: Float = 0f,
        revealMode: TextRevealMode = TextRevealMode.REVEAL,
    ): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = role,
            snapshot = null,
            sourceRect = Rect(0, 0, 10, 20),
            destinationRect = RectF(0f, 0f, 100f, 20f),
            startAlpha = 1f,
            endAlpha = 1f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            revealSpec =
                TextRevealSpec(
                    mode = revealMode,
                    anchorX = 0f,
                    boundaryFromX = 0f,
                    boundaryToX = 100f,
                    progressStart = 0f,
                    progressEnd = 1f,
                    initialFraction = initialFraction,
                ),
        )
    }

    /** 构造一个 SliceVisualState，[revealFraction] 默认为 null 以测试向后兼容。 */
    private fun makeSliceVisualState(
        role: SliceRole,
        revealFraction: Float? = null,
        currentAlpha: Float = 1f,
    ): SliceVisualState {
        return SliceVisualState(
            snapshotId = 1L,
            role = role,
            lineIndex = 0,
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            currentLeft = 0f,
            currentTop = 0f,
            currentRight = 100f,
            currentBottom = 20f,
            currentAlpha = currentAlpha,
            destinationLeft = 0f,
            destinationTop = 0f,
            destinationRight = 100f,
            destinationBottom = 20f,
            revealFraction = revealFraction,
        )
    }

    @Test
    fun insertRebaseCarriesOldRevealFractionIntoInitialFraction() {
        // Insert slice rebase 时 revealSpec.initialFraction = old revealFraction
        val slice = makeAnimatedSlice(SliceRole.Insert, initialFraction = 0f)
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.5f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull("rebased slice 应携带 revealSpec", rebased.revealSpec)
        assertEquals(
            "Insert rebase 应将旧帧 revealFraction 写入新 spec 的 initialFraction",
            0.5f,
            rebased.revealSpec!!.initialFraction,
            0.0001f,
        )
    }

    @Test
    fun deleteRebaseCarriesOldRevealFractionIntoInitialFraction() {
        // Delete slice rebase 时 revealSpec.initialFraction = old revealFraction
        val slice =
            makeAnimatedSlice(
                SliceRole.Delete,
                initialFraction = 0f,
                revealMode = TextRevealMode.SWALLOW,
            )
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.3f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull("rebased slice 应携带 revealSpec", rebased.revealSpec)
        assertEquals(
            "Delete rebase 应将旧帧 revealFraction 写入新 spec 的 initialFraction",
            0.3f,
            rebased.revealSpec!!.initialFraction,
            0.0001f,
        )
    }

    @Test
    fun unmatchedDeleteWithRevealFractionBelowThresholdContinuesInNewTransaction() {
        // 未匹配的 Delete slice 在 revealFraction < 0.99 时继续在新事务中吞完
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals(
            "revealFraction=0.5 的未匹配 Delete 应继续在新事务中吞完",
            1,
            result.size,
        )
        assertEquals(
            "继续吞完的 slice 角色应保持 Delete",
            SliceRole.Delete,
            result[0].role,
        )
    }

    @Test
    fun unmatchedDeleteWithRevealFractionAtOrAboveThresholdStops() {
        // 未匹配的 Delete slice 在 revealFraction >= 0.99 时不继续
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 1.0f,
                currentAlpha = 1.0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 1.0f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "revealFraction=1.0 的未匹配 Delete 已接近完成，不应继续",
            result.isEmpty(),
        )
    }

    @Test
    fun backwardCompatibleUnmatchedDeleteWithAlphaAboveThresholdContinues() {
        // 向后兼容：无 revealFraction 但 currentAlpha > 0.01 时仍继续
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = null,
                currentAlpha = 0.5f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals(
            "无 revealFraction 但 currentAlpha=0.5 的未匹配 Delete 应按旧 alpha 契约继续",
            1,
            result.size,
        )
        assertEquals(
            "继续吞完的 slice 角色应保持 Delete",
            SliceRole.Delete,
            result[0].role,
        )
    }

    @Test
    fun backwardCompatibleUnmatchedDeleteWithAlphaAtOrBelowThresholdStops() {
        // 向后兼容：无 revealFraction 且 currentAlpha <= 0.01 时不继续
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = null,
                currentAlpha = 0.005f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 1.0f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "无 revealFraction 且 currentAlpha=0.005 的未匹配 Delete 应按旧 alpha 契约停止",
            result.isEmpty(),
        )
    }

    /**
     * #605 评论3: 未匹配的 Delete slice 在有 cluster snapshot 时必须继续用 clip swallow
     * （携带 SWALLOW revealSpec，initialFraction = old revealFraction，alpha 固定 1f），
     * 不退回 alpha 淡出。
     */
    @Test
    fun unmatchedDeleteWithRevealFractionContinuesAsClipSwallow() {
        val cluster =
            LineClusterSnapshot(
                clusterId = 0L,
                documentByteStart = 0,
                documentByteEndExclusive = 1,
                documentUtf16Start = 0,
                documentUtf16EndExclusive = 1,
                sourceRectInLineImage = Rect(0, 0, 10, 20),
                visualRectInDocument = RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp",
                shapingIdentityConfident = true,
                caretStartX = 0f,
                caretEndX = 100f,
            )
        val snapshot =
            AndroidLineSnapshot(
                snapshotId = 1L,
                bitmap = null,
                lineIndex = 0,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = RectF(0f, 0f, 100f, 20f),
                clusters = listOf(cluster),
                documentByteStart = 0,
                documentByteEndExclusive = 10,
                documentUtf16Start = 0,
                documentUtf16EndExclusive = 10,
                baseline = 16f,
                lineHeight = 20f,
            )
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 1f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertEquals("延续的 Delete slice 应存在", 1, result.size)
        val continued = result[0]
        assertEquals("角色应保持 Delete", SliceRole.Delete, continued.role)
        assertNotNull("延续 slice 必须携带 revealSpec（clip swallow，不退回 alpha）", continued.revealSpec)
        assertEquals("revealSpec 模式应为 SWALLOW", TextRevealMode.SWALLOW, continued.revealSpec!!.mode)
        assertEquals(
            "initialFraction 应取自旧帧 revealFraction",
            0.5f,
            continued.revealSpec!!.initialFraction,
            0.0001f,
        )
        assertEquals("anchorX 应为 cluster caretStartX（收缩终点）", 0f, continued.revealSpec!!.anchorX, 0.001f)
        assertEquals("boundaryFromX 应为 cluster caretEndX（起始边界）", 100f, continued.revealSpec!!.boundaryFromX, 0.001f)
        assertEquals("boundaryToX 应为 cluster caretStartX（终止边界）", 0f, continued.revealSpec!!.boundaryToX, 0.001f)
        assertEquals("clip 绘制时 startAlpha 固定 1f", 1f, continued.startAlpha, 0.001f)
        assertEquals("clip 绘制时 endAlpha 固定 1f", 1f, continued.endAlpha, 0.001f)
    }

    /**
     * #605 评论3 反向: 无 cluster snapshot 时回退 alpha（向后兼容）。
     */
    @Test
    fun unmatchedDeleteWithRevealFractionButNoClusterFallsBackToAlpha() {
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals("无 cluster 时仍应延续", 1, result.size)
        val continued = result[0]
        assertEquals("角色应保持 Delete", SliceRole.Delete, continued.role)
        assertTrue("无 cluster 时 revealSpec 应为 null（alpha 回退）", continued.revealSpec == null)
    }
}
