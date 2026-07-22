package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class EditPipelineTest {

    private lateinit var mirror: DisplayTextMirror
    private lateinit var pipeline: EditPipeline

    @Before
    fun setup() {
        mirror = DisplayTextMirror()
        pipeline = EditPipeline(mirror)
    }

    @Test
    fun editPipelineOwnsMirror() {
        assertEquals(mirror, pipeline.mirror)
    }

    @Test
    fun kernelBridgeInitiallyNull() {
        assertNull(pipeline.kernelBridge)
    }

    @Test
    fun insertTextReturnsNullWithoutBridge() {
        assertNull(pipeline.insertText(0, "hello"))
    }

    @Test
    fun deleteRangeReturnsNullWithoutBridge() {
        assertNull(pipeline.deleteRange(0, 5))
    }

    @Test
    fun replaceRangeReturnsNullWithoutBridge() {
        assertNull(pipeline.replaceRange(0, 5, "hello", "world"))
    }

    @Test
    fun setSelectionReturnsNullWithoutBridge() {
        assertNull(pipeline.setSelection(0, 5))
    }

    @Test
    fun undoReturnsNullWithoutBridge() {
        assertNull(pipeline.undo())
    }

    @Test
    fun redoReturnsNullWithoutBridge() {
        assertNull(pipeline.redo())
    }

    @Test
    fun replaceAllReturnsNullWithoutBridge() {
        assertNull(pipeline.replaceAll("old", "new"))
    }

    @Test
    fun loadTextReturnsFailedWithoutBridge() {
        val result = pipeline.loadText("hello", 0)
        assertEquals(AndroidEditorPipeline.LoadTextResult.Failed, result)
    }

    @Test
    fun getCursorUtf8DelegatesToMirror() {
        assertEquals(0, pipeline.getCursorUtf8())
    }

    @Test
    fun getCursorUtf16DelegatesToMirror() {
        assertEquals(0, pipeline.getCursorUtf16())
    }

    @Test
    fun getSelectionStartUtf8DelegatesToMirror() {
        assertEquals(0, pipeline.getSelectionStartUtf8())
    }

    @Test
    fun getSelectionEndUtf8DelegatesToMirror() {
        assertEquals(0, pipeline.getSelectionEndUtf8())
    }

    @Test
    fun getSpannableReturnsNonNull() {
        assertNotNull(pipeline.getSpannable())
    }

    @Test
    fun reloadFromKernelReturnsFalseWithoutBridge() {
        assert(!pipeline.reloadFromKernel())
    }

    @Test
    fun insertLineBreakReturnsNullWithoutBridge() {
        assertNull(pipeline.insertLineBreak(0, ""))
    }
}
