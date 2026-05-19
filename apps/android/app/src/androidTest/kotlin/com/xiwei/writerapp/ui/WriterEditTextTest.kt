package com.xiwei.writerapp.ui

import android.text.style.ForegroundColorSpan
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WriterEditTextTest {

    @Test
    fun testWriterEditTextInstantiation() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        // Should not crash during initialization
        val editText = WriterEditText(appContext)
        assertNotNull(editText)
    }

    @Test
    fun testTypingAnimationCreatesAndClearsSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        // Simulate typing a single character
        editText.text?.append("A")

        // Let it run synchronously and check if span is attached
        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertNotNull(spans)
        assertTrue("Hidden span should be added", spans!!.isNotEmpty())

        // Clear layer
        editText.renderLayer?.clear()
        val spansAfter = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Hidden span should be removed after clear", spansAfter.isNullOrEmpty())
    }

    @Test
    fun testMassPasteDoesNotAddSpan() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        // Simulate pasting multiple characters
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

        // Simulate IME composing: set text and add composing span
        editText.text?.append("nihao")
        android.text.Selection.setSelection(editText.text!!, 5)
        // Use a Composing text span directly
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

        // Simulate deletion
        editText.text?.delete(4, 5)

        val spans = editText.text?.getSpans(0, editText.text!!.length, ForegroundColorSpan::class.java)
        assertTrue("Deletion should not add hidden span", spans.isNullOrEmpty())
    }
}
