package com.xiwei.sujian.ui

import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.widget.EditText
import com.xiwei.sujian.ui.span.EmptyParagraphIndentSpan
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class AutoIndentControllerTest {
    private fun newController(text: String): Pair<EditText, AutoIndentController> {
        val editText = EditText(RuntimeEnvironment.getApplication())
        editText.setText(text)
        val controller = AutoIndentController(editText)
        controller.setAutoIndent(enabled = true, widthChars = 2f)
        return editText to controller
    }

    private fun indentSpans(editText: EditText): Array<LeadingMarginSpan.Standard> {
        val editable = editText.text
        return editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
    }

    @Test
    fun composingPeriodDoesNotRebuildIndentSpans() {
        val (editText, controller) = newController("第一段\n第二段")
        val editable = editText.text
        val before = indentSpans(editText)
        assertEquals(2, before.size)

        controller.markComposingActive()

        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length)

        val after = indentSpans(editText)
        assertEquals(before.size, after.size)
        assertSame(before[0], after[0])
        assertSame(before[1], after[1])
        assertTrue(controller.pendingFullRebuildAfterComposition)
    }

    @Test
    fun pendingRebuildRunsAfterCompositionEnds() {
        val (editText, controller) = newController("第一段\n第二段")
        val editable = editText.text

        controller.markComposingActive()
        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length)
        assertTrue(controller.pendingFullRebuildAfterComposition)

        controller.markComposingFinished()
        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length)

        assertFalse(controller.pendingFullRebuildAfterComposition)
        assertEquals(2, indentSpans(editText).size)
    }

    @Test
    fun emptyDocumentReceivesIndentSpanForStableLayout() {
        val (editText, controller) = newController("")
        val editable = editText.text

        controller.updateParagraphIndentSpans(editable, isFullRebuild = true)

        val spans = indentSpans(editText)
        assertEquals(1, spans.size)
        assertEquals(0, editable.getSpanStart(spans[0]))
        assertEquals(0, editable.getSpanEnd(spans[0]))
    }

    @Test
    fun trailingEmptyParagraphReceivesStableIndentSpan() {
        val (editText, controller) = newController("第一段\n")
        val editable = editText.text

        controller.updateParagraphIndentSpans(editable, isFullRebuild = true)

        val emptySpans = mutableSetOf<EmptyParagraphIndentSpan>()
        emptySpans.addAll(editable.getSpans(0, editable.length, EmptyParagraphIndentSpan::class.java).toList())
        emptySpans.addAll(editable.getSpans(editable.length, editable.length, EmptyParagraphIndentSpan::class.java).toList())
        assertEquals(1, emptySpans.size)
        val marker = emptySpans.first()
        assertEquals(editable.length, editable.getSpanStart(marker))
        assertEquals(editable.length, editable.getSpanEnd(marker))
        assertEquals(Spanned.SPAN_PARAGRAPH, editable.getSpanFlags(marker))

        editable.insert(editable.length, "第")
        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length, isFullRebuild = false)

        val emptyAfter = mutableSetOf<EmptyParagraphIndentSpan>()
        emptyAfter.addAll(editable.getSpans(0, editable.length, EmptyParagraphIndentSpan::class.java).toList())
        emptyAfter.addAll(editable.getSpans(editable.length, editable.length, EmptyParagraphIndentSpan::class.java).toList())
        assertEquals(0, emptyAfter.size)

        val normalSpans = editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
            .filter { it !is EmptyParagraphIndentSpan }
        assertTrue(normalSpans.isNotEmpty())
        val normalSpan = normalSpans.last()
        val nlPos = editable.indexOf('\n')
        assertEquals(nlPos + 1, editable.getSpanStart(normalSpan))
        assertTrue(editable.getSpanEnd(normalSpan) >= editable.length)
    }

    @Test
    fun paragraphIndentSpansAreExclusiveExclusive() {
        val (editText, controller) = newController("第一段\n第二段")
        val editable = editText.text

        controller.updateParagraphIndentSpans(editable, isFullRebuild = true)

        val spans = indentSpans(editText)
        assertEquals(2, spans.size)
        spans.forEach { span ->
            assertEquals(Spanned.SPAN_EXCLUSIVE_EXCLUSIVE, editable.getSpanFlags(span))
        }
    }
}
