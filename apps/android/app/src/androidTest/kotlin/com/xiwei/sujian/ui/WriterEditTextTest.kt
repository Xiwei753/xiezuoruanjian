package com.xiwei.sujian.ui

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
 *
 * ## 重要约束
 * 打字动画通过 Canvas overlay 绘制，**禁止**向正文 Editable 注入透明 ForegroundColorSpan 隐藏文字。
 * 以下测试确保没有任何 ForegroundColorSpan 被注入到正文中。
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
    fun testTypingAnimationDoesNotInjectForegroundColorSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertNotNull(spans)
        assertTrue("Typing animation must NOT inject ForegroundColorSpan into body text", spans!!.isEmpty())
    }

    @Test
    fun testRenderLayerClearDoesNotLeaveForegroundColorSpans() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")
        editText.text?.append("B")
        editText.text?.append("C")

        val spansBefore = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("No ForegroundColorSpan should exist before clear", spansBefore.isNullOrEmpty())

        editText.renderLayer?.clear()

        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("No ForegroundColorSpan should exist after clear", spansAfter.isNullOrEmpty())
    }

    @Test
    fun testMassPasteDoesNotInjectForegroundColorSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        val longString = "A".repeat(150)
        editText.text?.append(longString)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Mass paste must NOT inject ForegroundColorSpan", spans.isNullOrEmpty())
    }

    @Test
    fun testComposingTextDoesNotInjectForegroundColorSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")

        editText.text?.append("nihao")
        android.text.Selection.setSelection(editText.text!!, 5)
        editText.text?.setSpan(android.text.style.UnderlineSpan(), 0, 5, android.text.Spanned.SPAN_COMPOSING)
        android.view.inputmethod.BaseInputConnection.setComposingSpans(editText.text!!)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Composing text must NOT inject ForegroundColorSpan", spans.isNullOrEmpty())
    }

    @Test
    fun testDeletionDoesNotInjectForegroundColorSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("Hello")

        editText.text?.delete(4, 5)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Deletion must NOT inject ForegroundColorSpan", spans.isNullOrEmpty())
    }

    @Test
    fun testRunWithoutTextAnimationsDoesNotLeaveForegroundColorSpans() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        val spansBefore = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("No ForegroundColorSpan should exist before runWithoutTextAnimations", spansBefore.isNullOrEmpty())

        editText.runWithoutTextAnimations {
            editText.text?.append("B")
        }

        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("No ForegroundColorSpan should exist after runWithoutTextAnimations", spansAfter.isNullOrEmpty())
    }
}
