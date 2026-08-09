package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
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
}
