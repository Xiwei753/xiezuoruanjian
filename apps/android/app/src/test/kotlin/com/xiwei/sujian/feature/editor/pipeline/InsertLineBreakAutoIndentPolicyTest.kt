package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorSessionSnapshotDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * #606: Verifies that [EditPipeline.insertLineBreak] passes the [autoIndentEnabled] policy
 * (Boolean) to Core, not a platform-computed indent prefix string. Core now computes the
 * actual indent prefix based on the text, cursor, and auto-indent strategy.
 *
 * Positive: insertLineBreak(true) forwards autoIndentEnabled=true to the bridge.
 * Negative: insertLineBreak(false) forwards autoIndentEnabled=false -- no prefix string.
 */
class InsertLineBreakAutoIndentPolicyTest {
    private lateinit var mirror: DisplayTextMirror
    private lateinit var pipeline: EditPipeline
    private lateinit var bridge: CapturingInsertLineBreakBridge

    @Before
    fun setup() {
        mirror = DisplayTextMirror()
        pipeline = EditPipeline(mirror)
        bridge = CapturingInsertLineBreakBridge()
        pipeline.setKernelBridge(bridge)
    }

    /**
     * Positive: when autoIndentEnabled=true, the bridge receives autoIndentEnabled=true.
     * Core is responsible for computing the actual indent prefix.
     */
    @Test
    fun insertLineBreak_withAutoIndentEnabled_forwardsTrueToBridge() {
        pipeline.insertLineBreak(0, true)

        assertEquals(1, bridge.callCount)
        assertEquals(0, bridge.lastByteOffset)
        assertTrue(bridge.lastAutoIndentEnabled)
    }

    /**
     * Negative: when autoIndentEnabled=false, the bridge receives autoIndentEnabled=false.
     * The platform does NOT compute an indent prefix and pass it as a string.
     */
    @Test
    fun insertLineBreak_withoutAutoIndent_forwardsFalseToBridge() {
        pipeline.insertLineBreak(0, false)

        assertEquals(1, bridge.callCount)
        assertEquals(0, bridge.lastByteOffset)
        assertEquals(false, bridge.lastAutoIndentEnabled)
    }

    private class CapturingInsertLineBreakBridge : EditorKernelBridge {
        var callCount: Int = 0
            private set
        var lastByteOffset: Int = -1
            private set
        var lastAutoIndentEnabled: Boolean = false
            private set

        override fun insert(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun delete(
            byteStart: Int,
            byteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun replace(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun setSelection(
            anchorByteOffset: Int,
            headByteOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun undo(expectedRevision: Long): EditorEditResultDto? = null

        override fun redo(expectedRevision: Long): EditorEditResultDto? = null

        override fun loadText(
            text: String,
            cursorUtf8: Int,
        ): EditorEditResultDto? = null

        override fun commitText(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            resultingSelectionAnchor: Int,
            resultingSelectionHead: Int,
            compositionSessionId: Long,
            compositionBaseRevision: Long,
            compositionGeneration: Long,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun deleteSurrounding(
            beforeByteStart: Int,
            beforeByteEndExclusive: Int,
            afterByteStart: Int,
            afterByteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun beginComposition(
            replaceStart: Int,
            replaceEndExclusive: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun updateComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            newPreeditText: String,
            newPreeditCursorOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun finishComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun cancelComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun setAnimationEnabled(enabled: Boolean) = Unit

        override fun setAnimationDurationMs(durationMs: Long) = Unit

        override fun replaceAll(
            search: String,
            replacement: String,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun insertLineBreak(
            byteOffset: Int,
            autoIndentEnabled: Boolean,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? {
            callCount++
            lastByteOffset = byteOffset
            lastAutoIndentEnabled = autoIndentEnabled
            return EditorEditResultDto(
                outcome = EditorEditOutcomeDto.APPLIED,
                transactionId = 1uL,
                baseRevision = 0uL,
                newRevision = 1uL,
                displayPatches =
                    listOf(
                        DisplayPatchDto(
                            baseRevision = 0uL,
                            newRevision = 1uL,
                            replaceByteStart = 0u,
                            replaceByteEndExclusive = 0u,
                            insertedText = "\n",
                            resultingSelectionStart = 1u,
                            resultingSelectionEnd = 1u,
                        ),
                    ),
                oldSelectionStart = 0u,
                oldSelectionEnd = 0u,
                newSelectionStart = 1u,
                newSelectionEnd = 1u,
                visualIntent =
                    EditorVisualIntentDto(
                        cause = cause,
                        operationKind = EditorOperationKindDto.INSERT,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges =
                            listOf(EditorByteRangeDto(0u, 1u)),
                        animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                        durationMs = 0uL,
                        coordinatedCursor =
                            CoordinatedCursorDto(0u, 1u, false),
                        offsetMap = null,
                    ),
                compositionSession = null,
                contentDelta = uniffi.writer_core.EditorContentDeltaDto(0u, 0u, 0u, 0u),
                composition = null,
            )
        }

        override fun sessionSnapshot(): EditorSessionSnapshotDto? = null

        // #606: grapheme 边界 stub — 测试不验证 grapheme 语义
        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset

        // #606: 测试不覆盖 rebase 映射 — 返回 null（平台端按无映射处理）。
        override fun computeRebaseSliceMappings(
            oldSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            oldSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            newSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            newSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            offsetMap: uniffi.writer_core.OffsetMapDto?,
        ): List<uniffi.writer_core.RebaseSliceMappingDto>? = null
    }
}
