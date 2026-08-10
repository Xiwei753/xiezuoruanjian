package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.projection.OffsetMap
import com.xiwei.sujian.feature.editor.projection.OffsetMapEntry
import com.xiwei.sujian.feature.editor.projection.OffsetMapKind
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceRoleAndByteRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimatedSliceRoleDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.OffsetMapDto
import uniffi.writer_core.RebaseContinuationDto
import uniffi.writer_core.RebaseReasonDto
import uniffi.writer_core.RebaseSliceMappingDto

/**
 * #606: 平台侧 rebase 数据管道正反测试 — 只做 DTO 转换与索引翻译，不包含匹配逻辑。
 *
 * 匹配逻辑在 Core（`compute_rebase_slice_mappings`，Rust 测试覆盖）；本测试验证
 * AndroidEditorPipeline.computeRebaseSliceMappings 把平台 slice 列表翻译为 Core 入参、
 * 把 Core 返回的（过滤后列表）索引翻译回完整列表索引，以及 Static 过滤。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebaseMappingBridgeTest {
    /** 记录 Core 入参并返回预设 DTO 映射的 fake bridge。
     * #606 测试 fake bridge，函数由 EditorKernelBridge 接口契约决定 — 无法裁减。
     */
    @Suppress("TooManyFunctions")
    private class RecordingRebaseBridge(
        private val mappingResults: List<RebaseSliceMappingDto>,
    ) : EditorKernelBridge {
        var oldRoles: List<AnimatedSliceRoleDto> = emptyList()
        var oldRanges: List<EditorByteRangeDto> = emptyList()
        var newRoles: List<AnimatedSliceRoleDto> = emptyList()
        var newRanges: List<EditorByteRangeDto> = emptyList()
        var offsetMap: OffsetMapDto? = null
        var callCount = 0

        override fun computeRebaseSliceMappings(
            oldSliceRoles: List<AnimatedSliceRoleDto>,
            oldSliceByteRanges: List<EditorByteRangeDto>,
            newSliceRoles: List<AnimatedSliceRoleDto>,
            newSliceByteRanges: List<EditorByteRangeDto>,
            offsetMap: OffsetMapDto?,
        ): List<RebaseSliceMappingDto> {
            callCount++
            oldRoles = oldSliceRoles
            oldRanges = oldSliceByteRanges
            newRoles = newSliceRoles
            newRanges = newSliceByteRanges
            this.offsetMap = offsetMap
            return mappingResults
        }

        override fun insert(
            byteOffset: Int,
            text: String,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun delete(
            byteStart: Int,
            byteEndExclusive: Int,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun replace(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun setSelection(
            anchorByteOffset: Int,
            headByteOffset: Int,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun undo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto? = null

        override fun redo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto? = null

        override fun loadText(
            text: String,
            cursorUtf8: Int,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun commitText(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            resultingSelectionAnchor: Int,
            resultingSelectionHead: Int,
            compositionSessionId: Long,
            compositionBaseRevision: Long,
            compositionGeneration: Long,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun deleteSurrounding(
            beforeByteStart: Int,
            beforeByteEndExclusive: Int,
            afterByteStart: Int,
            afterByteEndExclusive: Int,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun beginComposition(
            replaceStart: Int,
            replaceEndExclusive: Int,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun updateComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            newPreeditText: String,
            newPreeditCursorOffset: Int,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun finishComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun cancelComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun setAnimationEnabled(enabled: Boolean) {}

        override fun setAnimationDurationMs(durationMs: Long) {}

        override fun replaceAll(
            search: String,
            replacement: String,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun insertLineBreak(
            byteOffset: Int,
            autoIndentEnabled: Boolean,
            cause: uniffi.writer_core.EditorTransactionCauseDto,
            expectedRevision: Long,
        ): uniffi.writer_core.EditorEditResultDto? = null

        override fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto? = null

        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset
    }

    /**
     * #606 正: 入参翻译 — 平台 SliceRoleAndByteRange 被翻译为 Core 的 DTO 列表
     * （角色、byte range、OffsetMap 均传递）。
     */
    @Test
    fun bridgeInputTranslationPassesRolesRangesAndOffsetMap() {
        val bridge = RecordingRebaseBridge(emptyList())
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Insert, 0, 10),
                SliceRoleAndByteRange(SliceRole.Move, 20, 30),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Delete, 5, 15),
            )
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(
                            oldByteOffset = 0,
                            newByteOffset = 2,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )

        AndroidEditorPipeline.computeRebaseSliceMappings(bridge, oldSlices, newSlices, offsetMap)

        assertEquals(1, bridge.callCount)
        assertEquals(
            listOf(AnimatedSliceRoleDto.INSERT, AnimatedSliceRoleDto.MOVE),
            bridge.oldRoles,
        )
        assertEquals(
            listOf(EditorByteRangeDto(0u, 10u), EditorByteRangeDto(20u, 30u)),
            bridge.oldRanges,
        )
        assertEquals(listOf(AnimatedSliceRoleDto.DELETE), bridge.newRoles)
        assertEquals(listOf(EditorByteRangeDto(5u, 15u)), bridge.newRanges)
        assertEquals("OffsetMap 应原样传递给 Core", 1, bridge.offsetMap!!.entries.size)
        assertEquals(0u, bridge.offsetMap!!.entries[0].oldByteOffset)
        assertEquals(2u, bridge.offsetMap!!.entries[0].newByteOffset)
        assertEquals(10u, bridge.offsetMap!!.entries[0].length)
    }

    /**
     * #606 正: 索引翻译 — Core 返回的索引（过滤后列表）被翻译回完整列表索引。
     */
    @Test
    fun bridgeOutputTranslatesFilteredIndicesBackToFullListIndices() {
        val bridge =
            RecordingRebaseBridge(
                listOf(
                    RebaseSliceMappingDto(
                        oldSliceIndex = 0u,
                        newSliceIndex = 1u,
                        continuation = uniffi.writer_core.RebaseContinuationDto.CONTINUE,
                        reason = uniffi.writer_core.RebaseReasonDto.SAME_BYTE_RANGE,
                    ),
                ),
            )
        // 完整列表：old[0]=Static（过滤），old[1]=Insert（过滤后索引 0）
        // new[0]=Delete（过滤后索引 0），new[1]=Insert（过滤后索引 1）
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Static, 0, 10),
                SliceRoleAndByteRange(SliceRole.Insert, 100, 110),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Delete, 200, 210),
                SliceRoleAndByteRange(SliceRole.Insert, 100, 110),
            )

        val mappings =
            AndroidEditorPipeline.computeRebaseSliceMappings(
                bridge,
                oldSlices,
                newSlices,
                null,
            )

        assertEquals(1, mappings.size)
        // Core 索引 0 → 完整列表索引 1（Static 被过滤掉）
        assertEquals("Core old 索引 0 应翻译为完整列表索引 1", 1u, mappings[0].oldSliceIndex)
        assertEquals("Core new 索引 1 应翻译为完整列表索引 1", 1u, mappings[0].newSliceIndex)
        assertEquals(RebaseContinuationDto.CONTINUE, mappings[0].continuation)
        assertEquals(RebaseReasonDto.SAME_BYTE_RANGE, mappings[0].reason)
    }

    /**
     * #606 反: Static 角色被过滤 — Core 入参不包含 Static（Core 无 Static 概念）。
     */
    @Test
    fun bridgeFiltersStaticRolesFromCoreInput() {
        val bridge = RecordingRebaseBridge(emptyList())
        val oldSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Static, 0, 10),
                SliceRoleAndByteRange(SliceRole.Insert, 10, 20),
            )
        val newSlices =
            listOf(
                SliceRoleAndByteRange(SliceRole.Static, 0, 10),
                SliceRoleAndByteRange(SliceRole.Insert, 10, 20),
            )

        AndroidEditorPipeline.computeRebaseSliceMappings(bridge, oldSlices, newSlices, null)

        assertEquals("Core 入参只含非 Static 角色", listOf(AnimatedSliceRoleDto.INSERT), bridge.oldRoles)
        assertEquals(listOf(AnimatedSliceRoleDto.INSERT), bridge.newRoles)
    }

    /**
     * #606 反: bridge 为 null → 空映射（平台端按无对应关系处理），不崩溃。
     */
    @Test
    fun nullBridgeProducesEmptyMappings() {
        val mappings =
            AndroidEditorPipeline.computeRebaseSliceMappings(
                null,
                listOf(SliceRoleAndByteRange(SliceRole.Insert, 0, 10)),
                listOf(SliceRoleAndByteRange(SliceRole.Insert, 0, 10)),
                null,
            )
        assertTrue(mappings.isEmpty())
    }

    /**
     * #606 反: bridge 返回 null（桥接失败）→ 空映射。
     */
    @Test
    fun failedBridgeCallProducesEmptyMappings() {
        val bridge = RecordingRebaseBridge(emptyList())
        val failing =
            object : EditorKernelBridge by bridge {
                override fun computeRebaseSliceMappings(
                    oldSliceRoles: List<AnimatedSliceRoleDto>,
                    oldSliceByteRanges: List<EditorByteRangeDto>,
                    newSliceRoles: List<AnimatedSliceRoleDto>,
                    newSliceByteRanges: List<EditorByteRangeDto>,
                    offsetMap: OffsetMapDto?,
                ): List<RebaseSliceMappingDto>? = null
            }
        val mappings =
            AndroidEditorPipeline.computeRebaseSliceMappings(
                failing,
                listOf(SliceRoleAndByteRange(SliceRole.Insert, 0, 10)),
                listOf(SliceRoleAndByteRange(SliceRole.Insert, 0, 10)),
                null,
            )
        assertTrue(mappings.isEmpty())
    }
}
