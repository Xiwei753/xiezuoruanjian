package com.xiwei.sujian.ui

import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText
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

        BaseInputConnection(editText, true).setComposingRegion(0, editable.length)

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

        BaseInputConnection(editText, true).setComposingRegion(0, editable.length)
        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length)
        assertTrue(controller.pendingFullRebuildAfterComposition)

        BaseInputConnection.removeComposingSpans(editable)
        controller.updateParagraphIndentSpans(editable, updateStartPos = editable.length)

        assertFalse(controller.pendingFullRebuildAfterComposition)
        assertEquals(2, indentSpans(editText).size)
    }

    @Test
    fun emptyDocumentDoesNotReceiveZeroLengthIndentSpan() {
        val (editText, controller) = newController("")
        val editable = editText.text

        controller.updateParagraphIndentSpans(editable, isFullRebuild = true)

        assertEquals(0, indentSpans(editText).size)
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
