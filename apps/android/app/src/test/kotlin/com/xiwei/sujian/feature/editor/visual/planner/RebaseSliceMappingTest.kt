package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.RebaseContinuation
import com.xiwei.sujian.feature.editor.visual.RebaseReason
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceRoleAndByteRange
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
 * #606: computeRebaseSliceMappings 正反测试 — 从 RebasePlannerTest 拆出以控制 detekt
 * LargeClass / TooManyFunctions / StringLiteralDuplication 阈值。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebaseSliceMappingTest {
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

    // ── #606: computeRebaseSliceMappings 正反测试 ──

    /**
     * #606 正: 旧/新 slice byte range 相同 + 角色兼容 → 生成 SameByteRange + Continue 映射。
     */
    @Test
    fun computeRebaseSliceMappingsSameByteRangeCompatibleRolesProducesMapping() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
                SliceRoleAndByteRange(SliceRole.Delete, 10, 20),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
                SliceRoleAndByteRange(SliceRole.Delete, 10, 20),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertEquals("应生成 2 条映射", 2, mappings.size)
        assertEquals("第 0 条 oldSliceIndex=0", 0, mappings[0].oldSliceIndex)
        assertEquals("第 0 条 newSliceIndex=0", 0, mappings[0].newSliceIndex)
        assertEquals("第 0 条 continuation=Continue", RebaseContinuation.Continue, mappings[0].continuation)
        assertEquals("第 0 条 reason=SameByteRange", RebaseReason.SameByteRange, mappings[0].reason)
        assertEquals("第 1 条 oldSliceIndex=1", 1, mappings[1].oldSliceIndex)
        assertEquals("第 1 条 newSliceIndex=1", 1, mappings[1].newSliceIndex)
    }

    /**
     * #606 正: Move 与 Insert 角色兼容（都是"新出现的文字"动画）。
     */
    @Test
    fun computeRebaseSliceMappingsMoveCompatibleWithInsert() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Move, 0, 10),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertEquals("Move 与 Insert 兼容应生成映射", 1, mappings.size)
        assertEquals(RebaseReason.SameByteRange, mappings[0].reason)
    }

    /**
     * #606 正: CrossfadeOld 与 Delete 兼容（都是"消失的文字"动画）。
     */
    @Test
    fun computeRebaseSliceMappingsCrossfadeOldCompatibleWithDelete() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.CrossfadeOld, 5, 15),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Delete, 5, 15),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertEquals("CrossfadeOld 与 Delete 兼容应生成映射", 1, mappings.size)
    }

    /**
     * #606 反: 不同 byte range → 不生成映射。
     */
    @Test
    fun computeRebaseSliceMappingsDifferentByteRangeNoMapping() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 20),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertTrue("byte range 不同不应生成映射", mappings.isEmpty())
    }

    /**
     * #606 反: 角色不兼容（Insert 与 Delete）→ 不生成映射。
     */
    @Test
    fun computeRebaseSliceMappingsIncompatibleRolesNoMapping() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Delete, 0, 10),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertTrue("Insert 与 Delete 不兼容不应生成映射", mappings.isEmpty())
    }

    /**
     * #606 反: Move 与 CrossfadeOld 不兼容 → 不生成映射。
     */
    @Test
    fun computeRebaseSliceMappingsMoveIncompatibleWithCrossfadeOld() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.CrossfadeOld, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Move, 0, 10),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertTrue("Move 与 CrossfadeOld 不兼容不应生成映射", mappings.isEmpty())
    }

    /**
     * #606 反: 每个新 slice 至多被一个旧 slice 匹配。
     */
    @Test
    fun computeRebaseSliceMappingsEachNewSliceMatchedAtMostOnce() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertEquals("两个旧 slice 竞争同一新 slice，只应匹配一次", 1, mappings.size)
        assertEquals("第一个旧 slice 优先匹配", 0, mappings[0].oldSliceIndex)
        assertEquals("新 slice index=0", 0, mappings[0].newSliceIndex)
    }

    /**
     * #606 反: 空输入 → 空映射。
     */
    @Test
    fun computeRebaseSliceMappingsEmptyInputsProducesEmptyMappings() {
        val mappings = planner.computeRebaseSliceMappings(emptyList(), emptyList())
        assertTrue("空输入应返回空映射", mappings.isEmpty())
    }

    /**
     * #606 反: 旧 slice 为空 → 空映射。
     */
    @Test
    fun computeRebaseSliceMappingsEmptyOldSlicesProducesEmptyMappings() {
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val mappings = planner.computeRebaseSliceMappings(emptyList(), newSlices)
        assertTrue("旧 slice 为空应返回空映射", mappings.isEmpty())
    }

    /**
     * #606 反: 新 slice 为空 → 空映射。
     */
    @Test
    fun computeRebaseSliceMappingsEmptyNewSlicesProducesEmptyMappings() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
            )
        val mappings = planner.computeRebaseSliceMappings(oldSlices, emptyList())
        assertTrue("新 slice 为空应返回空映射", mappings.isEmpty())
    }

    /**
     * #606 正: 部分匹配 — 3 旧 slice 中 2 个匹配新 slice。
     */
    @Test
    fun computeRebaseSliceMappingsPartialMatch() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
                SliceRoleAndByteRange(SliceRole.Delete, 10, 20),
                SliceRoleAndByteRange(SliceRole.Move, 30, 40),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Move, 0, 10),
                SliceRoleAndByteRange(SliceRole.Delete, 10, 20),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertEquals("应生成 2 条映射（第 0 旧→第 0 新，第 1 旧→第 1 新）", 2, mappings.size)
        assertEquals(0, mappings[0].oldSliceIndex)
        assertEquals(0, mappings[0].newSliceIndex)
        assertEquals(1, mappings[1].oldSliceIndex)
        assertEquals(1, mappings[1].newSliceIndex)
    }

    /**
     * #606 正: Static 角色不参与 rebase（与任何角色都不兼容）。
     */
    @Test
    fun computeRebaseSliceMappingsStaticRoleNeverMatches() {
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Static, 0, 10),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Static, 0, 10),
            )

        val mappings = planner.computeRebaseSliceMappings(oldSlices, newSlices)

        assertTrue("Static 角色不应参与 rebase 匹配", mappings.isEmpty())
    }

    /**
     * #606 正: applyRebaseToSlices 用 computeRebaseSliceMappings 做精确匹配 —
     * byte range 相同 + 角色兼容的旧 slice 视觉状态被应用到新 slice。
     */
    @Test
    fun applyRebaseToSlicesUsesComputeRebaseSliceMappingsForExactMatch() {
        val newSlice = makeAnimatedSlice(SliceRole.Insert, initialFraction = 0f)
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.7f,
                currentAlpha = 0.7f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.7f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(rebaseState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = listOf(newSlice),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals("应生成 1 个 rebased slice", 1, result.size)
        assertNotNull("rebased slice 应携带 revealSpec", result[0].revealSpec)
        assertEquals(
            "rebase 应将旧帧 revealFraction=0.7 写入新 spec 的 initialFraction",
            0.7f,
            result[0].revealSpec!!.initialFraction,
            0.0001f,
        )
    }

    /**
     * #606 反: applyRebaseToSlices — byte range 不同时不做匹配，新 slice 保持原样。
     */
    @Test
    fun applyRebaseToSlicesNoMatchWhenByteRangeDiffers() {
        val newSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = RectF(0f, 0f, 100f, 20f),
                startAlpha = 1f,
                endAlpha = 1f,
                clusterByteStart = 50,
                clusterByteEndExclusive = 60,
            )
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.7f,
                currentAlpha = 0.7f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.7f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(rebaseState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = listOf(newSlice),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        // 新 slice byte range (50,60) 与旧 slice byte range (0,1) 不同 → 不匹配
        // 新 slice 保持原样 (startAlpha=1f)，旧 slice 作为未匹配 fade-out 处理
        assertEquals("应生成 2 个 slice（新 slice 原样 + 旧 slice fade-out）", 2, result.size)
        assertEquals("第 0 个是新 slice，startAlpha 应保持 1f", 1f, result[0].startAlpha, 0.0001f)
    }
}
