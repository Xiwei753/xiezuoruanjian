package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorSessionSnapshotDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #606: Verifies that [AndroidEditorPipeline.previousGraphemeByteLen] and
 * [AndroidEditorPipeline.nextGraphemeByteLen] call Core's grapheme boundary API
 * via [EditorKernelBridge] instead of computing locally with ICU BreakIterator.
 *
 * Positive: the bridge's previousGraphemeBoundary/nextGraphemeBoundary are invoked
 * and their return values are used to compute the byte length.
 * Negative: when kernelBridge is null, the methods return 0 (no ICU fallback).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class GraphemeBoundaryCallsCoreTest {
    private lateinit var mirror: DisplayTextMirror
    private lateinit var pipeline: AndroidEditorPipeline
    private lateinit var bridge: RecordingGraphemeBridge

    @Before
    fun setup() {
        mirror = DisplayTextMirror()
        pipeline = AndroidEditorPipeline.create(mirror, android.text.TextPaint())
        bridge = RecordingGraphemeBridge()
        pipeline.kernelBridge = bridge
        pipeline.loadText("hello", 5)
    }

    /**
     * Positive: previousGraphemeByteLen calls bridge.previousGraphemeBoundary and uses
     * its result. With boundary = 2, byte len = offset - boundary = 3.
     */
    @Test
    fun previousGraphemeByteLen_callsCoreBridgeAndUsesResult() {
        val offset = 5
        bridge.previousBoundaryResult = 2

        val byteLen = pipeline.previousGraphemeByteLen(offset)

        assertEquals(1, bridge.previousBoundaryCalls.size)
        assertEquals(offset, bridge.previousBoundaryCalls[0])
        assertEquals(3, byteLen)
    }

    /**
     * Positive: nextGraphemeByteLen calls bridge.nextGraphemeBoundary and uses its result.
     * With boundary = 5, byte len = boundary - offset = 4.
     */
    @Test
    fun nextGraphemeByteLen_callsCoreBridgeAndUsesResult() {
        val offset = 1
        bridge.nextBoundaryResult = 5

        val byteLen = pipeline.nextGraphemeByteLen(offset)

        assertEquals(1, bridge.nextBoundaryCalls.size)
        assertEquals(offset, bridge.nextBoundaryCalls[0])
        assertEquals(4, byteLen)
    }

    /**
     * Negative: when kernelBridge is null, previousGraphemeByteLen returns 0
     * (no ICU BreakIterator fallback).
     */
    @Test
    fun previousGraphemeByteLen_nullBridge_returnsZero() {
        pipeline.kernelBridge = null

        val byteLen = pipeline.previousGraphemeByteLen(3)

        assertEquals(0, byteLen)
    }

    /**
     * Negative: when kernelBridge is null, nextGraphemeByteLen returns 0
     * (no ICU BreakIterator fallback).
     */
    @Test
    fun nextGraphemeByteLen_nullBridge_returnsZero() {
        pipeline.kernelBridge = null

        val byteLen = pipeline.nextGraphemeByteLen(3)

        assertEquals(0, byteLen)
    }

    /**
     * Positive: previousGraphemeByteLen at offset 0 returns 0 (boundary = 0).
     */
    @Test
    fun previousGraphemeByteLen_atZeroOffset_returnsZero() {
        bridge.previousBoundaryResult = 0

        val byteLen = pipeline.previousGraphemeByteLen(0)

        assertEquals(0, byteLen)
        assertTrue(bridge.previousBoundaryCalls.isNotEmpty())
    }

    /**
     * Minimal bridge that records grapheme boundary calls and returns configurable results.
     * All edit methods return null — the test only exercises grapheme boundary methods.
     */
    private class RecordingGraphemeBridge : EditorKernelBridge {
        val previousBoundaryCalls = mutableListOf<Int>()
        val nextBoundaryCalls = mutableListOf<Int>()
        var previousBoundaryResult: Int = 0
        var nextBoundaryResult: Int = 0

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

        // #606: grapheme 边界 — 记录调用并返回可配置结果
        override fun previousGraphemeBoundary(byteOffset: Int): Int {
            previousBoundaryCalls.add(byteOffset)
            return previousBoundaryResult
        }

        override fun nextGraphemeBoundary(byteOffset: Int): Int {
            nextBoundaryCalls.add(byteOffset)
            return nextBoundaryResult
        }
    }
}
