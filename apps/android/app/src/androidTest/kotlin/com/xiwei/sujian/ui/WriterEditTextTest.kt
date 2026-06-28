package com.xiwei.sujian.ui

import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.view.inputmethod.BaseInputConnection
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.ui.span.EmptyParagraphIndentSpan
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
 * EditorRenderLayer 不再持有 animatedSkipRanges / activeSkipSpans，静态正文永远由系统完整绘制。
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
    fun testRenderLayerDoesNotModifyEditable() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")
        editText.text?.append("B")
        editText.text?.append("C")

        // 静态正文永远由系统完整绘制，动画只做 overlay
        val layer = editText.renderLayer
        assertNotNull(layer)

        // clear() 不应抛异常
        layer?.clear()
    }

    @Test
    fun testMassPasteDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        val longString = "A".repeat(150)
        editText.text?.append(longString)

        // 大段粘贴不应导致异常
        assertNotNull(editText.text)
    }

    @Test
    fun testDeletionDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("Hello")
        editText.text?.delete(4, 5)

        assertNotNull(editText.text)
    }

    @Test
    fun testRunWithoutTextAnimationsDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        editText.runWithoutTextAnimations {
            editText.text?.append("B")
        }

        assertEquals("AB", editText.text.toString())
    }

    @Test
    fun testAutoIndentComposingSkipsSpanRebuild() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.layoutParams = android.widget.FrameLayout.LayoutParams(800, 400)
        editText.layout(0, 0, 800, 400)

        editText.setAutoIndent(true, 2f)
        editText.setText("abc\n")
        val editable = editText.text ?: return

        val controller = editText.autoIndentController ?: return
        assertTrue("autoIndent should be enabled", controller.autoIndentEnabled)
        assertTrue("autoIndentPx should be > 0", controller.autoIndentPx > 0)

        val spansBeforeComposing = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java).toList()
        assertTrue("Should have at least one indent span", spansBeforeComposing.isNotEmpty())

        BaseInputConnection.setComposingSpans(editable)
        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        assertTrue("Should have composing span", composingStart != -1 && composingEnd != -1)

        controller.updateParagraphIndentSpans(editable, updateStartPos = 4)
        assertTrue("pendingFullRebuildAfterComposition should be set during composing", controller.pendingFullRebuildAfterComposition)

        val spansDuringComposing = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java).toList()
        assertEquals("No span changes during composing", spansBeforeComposing.size, spansDuringComposing.size)

        BaseInputConnection.removeComposingSpans(editable)
        controller.updateParagraphIndentSpans(editable, isFullRebuild = true)
        assertFalse("pendingFullRebuildAfterComposition should be cleared after composing", controller.pendingFullRebuildAfterComposition)

        val spansAfterComposing = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java).toList()
        assertTrue("Should have indent spans after composing ends", spansAfterComposing.isNotEmpty())
    }

    @Test
    fun testAutoIndentUsesExclusiveExclusiveFlag() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.layoutParams = android.widget.FrameLayout.LayoutParams(800, 400)
        editText.layout(0, 0, 800, 400)

        editText.setAutoIndent(true, 2f)
        editText.setText("abc\n")
        val editable = editText.text ?: return

        val spans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
        for (span in spans) {
            val flags = editable.getSpanFlags(span)
            if (span is EmptyParagraphIndentSpan) {
                assertEquals("Empty paragraph span should use SPAN_PARAGRAPH", Spanned.SPAN_PARAGRAPH, flags and Spanned.SPAN_PARAGRAPH)
            } else {
                assertEquals("Normal span should use SPAN_EXCLUSIVE_EXCLUSIVE", Spanned.SPAN_EXCLUSIVE_EXCLUSIVE, flags and Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                assertNotEquals("Normal span should NOT use SPAN_PARAGRAPH", Spanned.SPAN_PARAGRAPH, flags and Spanned.SPAN_PARAGRAPH)
            }
        }
    }

    @Test
    fun testEmptyLineCursorTargetWithAutoIndent() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.layoutParams = android.widget.FrameLayout.LayoutParams(800, 400)
        editText.layout(0, 0, 800, 400)

        editText.setAutoIndent(true, 2f)
        editText.setSmoothCursorEnabled(true, 80L)
        editText.setText("abc\n")
        val editable = editText.text ?: return

        editText.setSelection(4)
        val layout = editText.layout ?: return
        val line = layout.getLineForOffset(4)
        val renderer = editText.renderLayer?.smoothCursorRenderer ?: return

        val coords = renderer.computeCursorTarget(4)

        val controller = editText.autoIndentController ?: return
        val expectedX = layout.getLineLeft(line) + controller.autoIndentPx
        assertEquals("Empty line cursor x should be at indent position", expectedX, coords.x, 2f)
    }
}
