package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
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
 * #606: Verifies that [AndroidEditorPipeline.applyCompositionCommit] consumes the
 * Core-returned [VisualIntent] directly instead of re-computing oldAffected/newAffected,
 * isVisualSame, animationMode, and operationKind on the platform side.
 *
 * Positive: the visualIntent fed to the animation engine is exactly Core's visualIntent.
 * Negative: the platform does NOT override animationMode/operationKind based on preeditText.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class CompositionCommitConsumesCoreIntentTest {
    private lateinit var mirror: DisplayTextMirror
    private lateinit var pipeline: AndroidEditorPipeline
    private lateinit var bridge: CapturingBridge

    @Before
    fun setup() {
        mirror = DisplayTextMirror()
        pipeline = AndroidEditorPipeline.create(mirror, android.text.TextPaint())
        bridge = CapturingBridge()
        pipeline.kernelBridge = bridge
        pipeline.loadText("hello", 5)
    }

    /**
     * Positive: when Core returns COMPOSITION_COMMIT with GLYPH_ANIMATION, the pipeline
     * feeds exactly that visualIntent to the animation engine — not a platform-recomputed
     * one with a different animationMode.
     */
    @Test
    fun applyCompositionCommit_usesCoreVisualIntent_directly() {
        val coreIntent =
            EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.TYPING_COMMIT,
                operationKind = EditorOperationKindDto.COMPOSITION_COMMIT,
                oldAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 3u)),
                newAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 5u)),
                animationMode = AnimationModeDto.GLYPH_ANIMATION,
                durationMs = 200uL,
                coordinatedCursor =
                    CoordinatedCursorDto(0u, 5u, true),
                offsetMap = null,
            )
        val dto = buildEditResultDto(coreIntent)

        val output = pipeline.applyCompositionCommit(dto, preeditText = "abc")

        assertNotNull(output)
        assertTrue(output is PipelineOutput.Edited)
        // The pipeline should not have re-computed the visualIntent — the edit result
        // carries Core's visualIntent unchanged.
        val edited = output as PipelineOutput.Edited
        assertEquals(
            EditorOperationKindDto.COMPOSITION_COMMIT,
            edited.result.visualIntent.operationKind,
        )
        assertEquals(
            AnimationModeDto.GLYPH_ANIMATION,
            edited.result.visualIntent.animationMode,
        )
        assertEquals(200L, edited.result.visualIntent.durationMs)
    }

    /**
     * Negative: when Core returns SYSTEM_SUPPRESSED (visual-same), the platform does NOT
     * override it to GLYPH_ANIMATION based on byte-count heuristics. The old platform code
     * re-evaluated animationMode when Core returned SYSTEM_SUPPRESSED but detected byte
     * changes — #606 removes that override.
     */
    @Test
    fun applyCompositionCommit_doesNotOverrideSystemSuppressed() {
        val coreIntent =
            EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.TYPING_COMMIT,
                operationKind = EditorOperationKindDto.COMPOSITION_COMMIT,
                oldAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 3u)),
                newAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 5u)),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor =
                    CoordinatedCursorDto(0u, 5u, false),
                offsetMap = null,
            )
        val dto = buildEditResultDto(coreIntent)

        val output = pipeline.applyCompositionCommit(dto, preeditText = "abc")

        assertNotNull(output)
        val edited = output as PipelineOutput.Edited
        // Core's SYSTEM_SUPPRESSED must be preserved — no platform override.
        assertEquals(
            AnimationModeDto.SYSTEM_SUPPRESSED,
            edited.result.visualIntent.animationMode,
        )
    }

    /**
     * Negative: when Core returns RUN_ANIMATION, the platform does NOT downgrade it to
     * GLYPH_ANIMATION based on a local byte-count threshold. The old platform code had
     * a byteCount <= 24 → GLYPH_ANIMATION override — #606 removes that.
     */
    @Test
    fun applyCompositionCommit_preservesCoreRunAnimation() {
        val coreIntent =
            EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.TYPING_COMMIT,
                operationKind = EditorOperationKindDto.COMPOSITION_COMMIT,
                oldAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 2u)),
                newAffectedByteRanges =
                    listOf(EditorByteRangeDto(0u, 4u)),
                animationMode = AnimationModeDto.RUN_ANIMATION,
                durationMs = 300uL,
                coordinatedCursor =
                    CoordinatedCursorDto(0u, 4u, true),
                offsetMap = null,
            )
        val dto = buildEditResultDto(coreIntent)

        val output = pipeline.applyCompositionCommit(dto, preeditText = "ab")

        val edited = output as PipelineOutput.Edited
        assertEquals(
            AnimationModeDto.RUN_ANIMATION,
            edited.result.visualIntent.animationMode,
        )
    }

    private fun buildEditResultDto(coreIntent: EditorVisualIntentDto): EditorEditResultDto =
        EditorEditResultDto(
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
                        replaceByteEndExclusive = 3u,
                        insertedText = "world",
                        resultingSelectionStart = 5u,
                        resultingSelectionEnd = 5u,
                    ),
                ),
            oldSelectionStart = 0u,
            oldSelectionEnd = 3u,
            newSelectionStart = 5u,
            newSelectionEnd = 5u,
            visualIntent = coreIntent,
            compositionSession = null,
        )

    /**
     * Minimal bridge that returns a snapshot for reloadFromKernel. All edit methods return
     * null — the test only exercises applyCompositionCommit which takes a pre-built dto.
     */
    private class CapturingBridge : EditorKernelBridge {
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
        ): EditorEditResultDto? = null

        override fun sessionSnapshot(): EditorSessionSnapshotDto? = null

        // #606: grapheme 边界 stub — 测试不验证 grapheme 语义
        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset
    }
}
