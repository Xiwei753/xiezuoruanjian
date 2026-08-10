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
import uniffi.writer_core.RebaseContinuationDto
import uniffi.writer_core.RebaseReasonDto
import uniffi.writer_core.RebaseSliceMappingDto

/**
 * #606: RebasePlanner 只消费 Core 计算的映射 — 本地匹配逻辑已删除。
 *
 * 这些测试验证 `applyRebaseToSlices` 把 Core 给的映射（oldSliceIndex → newSliceIndex）
 * 翻译为旧帧视觉状态（RectF/alpha/revealFraction）填入新 slice，以及无映射旧 slice
 * 的继续/结束处理。映射本身由 Core 计算（Rust 侧测试覆盖），Android 不再生成。
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

    /**
     * #606 正: applyRebaseToSlices 消费 Core 给的映射 —
     * 旧帧 revealFraction/alpha 被填入新 slice。
     */
    @Test
    fun applyRebaseToSlicesConsumesCoreMappingForExactMatch() {
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
        val coreMappings =
            listOf(
                RebaseSliceMappingDto(
                    oldSliceIndex = 0u,
                    newSliceIndex = 0u,
                    continuation = RebaseContinuationDto.CONTINUE,
                    reason = RebaseReasonDto.SAME_BYTE_RANGE,
                ),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = listOf(newSlice),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
                mappings = coreMappings,
            )

        assertEquals("应生成 1 个 rebased slice", 1, result.size)
        assertNotNull("rebased slice 应携带 revealSpec", result[0].revealSpec)
        assertEquals(
            "rebase 应将旧帧 revealFraction=0.7 写入新 spec 的 initialFraction",
            0.7f,
            result[0].revealSpec!!.initialFraction,
            0.0001f,
        )
        assertEquals(
            "rebase 应将旧帧 currentAlpha=0.7 写入新 slice 的 startAlpha",
            0.7f,
            result[0].startAlpha,
            0.0001f,
        )
    }

    /**
     * #606 正: OffsetMapMatched 映射（Core 判定旧/新 range 经偏移映射指向同一逻辑对象）
     * 与 SameByteRange 一样被消费。
     */
    @Test
    fun applyRebaseToSlicesConsumesOffsetMapMatchedMapping() {
        val newSlice = makeAnimatedSlice(SliceRole.Move, initialFraction = 0f)
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(rebaseState),
            )
        val coreMappings =
            listOf(
                RebaseSliceMappingDto(
                    oldSliceIndex = 0u,
                    newSliceIndex = 0u,
                    continuation = RebaseContinuationDto.CONTINUE,
                    reason = RebaseReasonDto.OFFSET_MAP_MATCHED,
                ),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = listOf(newSlice),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
                mappings = coreMappings,
            )

        assertEquals("应生成 1 个 rebased slice", 1, result.size)
        assertEquals(
            "OffsetMapMatched 映射同样把旧帧 alpha 带入新 slice",
            0.5f,
            result[0].startAlpha,
            0.0001f,
        )
        assertNotNull(result[0].fromDestinationRect)
    }

    /**
     * #606 反: Core 未给出映射（空列表）时，新 slice 保持原样，
     * 旧 slice 按无对应关系处理（仍在进行中的动画继续）。
     */
    @Test
    fun applyRebaseToSlicesNoMappingKeepsNewSliceAndContinuesOld() {
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
                mappings = emptyList(),
            )

        // Core 无映射 → 新 slice 原样 (startAlpha=1f)，旧 slice 作为未匹配 slice 继续动画
        assertEquals("应生成 2 个 slice（新 slice 原样 + 旧 slice 继续）", 2, result.size)
        assertEquals("第 0 个是新 slice，startAlpha 应保持 1f", 1f, result[0].startAlpha, 0.0001f)
        assertEquals("第 1 个是旧 slice 继续动画", SliceRole.Insert, result[1].role)
        assertEquals("旧 slice 继续动画应带旧帧 alpha", 0.7f, result[1].startAlpha, 0.0001f)
    }

    /**
     * #606 反: 映射指向的旧 slice 索引越界时安全降级（新 slice 保持原样）。
     */
    @Test
    fun applyRebaseToSlicesOutOfRangeMappingDegradesGracefully() {
        val newSlice = makeAnimatedSlice(SliceRole.Insert, initialFraction = 0f)
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.7f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(makeSliceVisualState(SliceRole.Insert)),
            )
        val coreMappings =
            listOf(
                RebaseSliceMappingDto(
                    oldSliceIndex = 5u,
                    newSliceIndex = 0u,
                    continuation = RebaseContinuationDto.CONTINUE,
                    reason = RebaseReasonDto.SAME_BYTE_RANGE,
                ),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = listOf(newSlice),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
                mappings = coreMappings,
            )

        assertTrue("越界映射不应崩溃", result.isNotEmpty())
        assertEquals("越界映射的新 slice 保持原样", 1f, result[0].startAlpha, 0.0001f)
    }
}
