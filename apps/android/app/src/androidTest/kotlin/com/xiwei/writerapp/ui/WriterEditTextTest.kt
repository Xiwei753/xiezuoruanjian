package com.xiwei.writerapp.ui

import android.text.style.ForegroundColorSpan
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/**
 * WriterEditTextTest — WriterEditText 仪器化测试
 *
 * 测试 WriterEditText 的打字动画、平滑光标等功能在真机/模拟器上的行为。
 */

@RunWith(AndroidJUnit4::class)
class WriterEditTextTest {

    @Test
    fun testWriterEditTextInstantiation() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        assertNotNull(editText)
    }

    @Test
    fun testWriterEditTextHasRenderLayer() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        assertNotNull("WriterEditText should have a renderLayer", editText.renderLayer)
    }

    @Test
    fun testWriterEditTextDoesNotDirectlyManageRenderers() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        val layer = editText.renderLayer
        assertNotNull("renderLayer should not be null", layer)
        assertNotNull("renderLayer should manage typingOverlayRenderer", layer?.typingOverlayRenderer)
        assertNotNull("renderLayer should manage smoothCursorRenderer", layer?.smoothCursorRenderer)
    }

    @Test
    fun testTypingAnimationCreatesAndClearsSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertNotNull(spans)
        assertTrue("Hidden span should be added", spans!!.isNotEmpty())

        editText.renderLayer?.clear()
        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Hidden span should be removed after clear", spansAfter.isNullOrEmpty())
    }

    @Test
    fun testRenderLayerClearRemovesAllSpans() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")
        editText.text?.append("B")
        editText.text?.append("C")

        val spansBefore = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertNotNull(spansBefore)
        assertTrue("Should have hidden spans after typing", spansBefore!!.isNotEmpty())

        editText.renderLayer?.clear()

        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("All hidden spans should be removed after renderLayer.clear()", spansAfter.isNullOrEmpty())
    }

    @Test
    fun testMassPasteDoesNotAddSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        val longString = "A".repeat(150)
        editText.text?.append(longString)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Mass paste should not add hidden span", spans.isNullOrEmpty())
    }

    @Test
    fun testComposingTextDoesNotAddSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")

        editText.text?.append("nihao")
        android.text.Selection.setSelection(editText.text!!, 5)
        editText.text?.setSpan(android.text.style.UnderlineSpan(), 0, 5, android.text.Spanned.SPAN_COMPOSING)
        android.view.inputmethod.BaseInputConnection.setComposingSpans(editText.text!!)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Composing text should not add hidden span", spans.isNullOrEmpty())
    }

    @Test
    fun testDeletionDoesNotAddSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("Hello")

        editText.text?.delete(4, 5)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Deletion should not add hidden span", spans.isNullOrEmpty())
    }

    @Test
    fun testRunWithoutTextAnimationsClearsSpans() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        val spansBefore = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertNotNull(spansBefore)
        assertTrue("Should have hidden span before runWithoutTextAnimations", spansBefore!!.isNotEmpty())

        editText.runWithoutTextAnimations {
            editText.text?.append("B")
        }

        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Hidden spans should be cleared after runWithoutTextAnimations", spansAfter.isNullOrEmpty())
    }
}
