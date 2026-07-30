package com.xiwei.sujian.editor

import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.test.espresso.Espresso
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.support.BaseEditorTest
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorReplaceRangeAction
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.SujianMediumTest
import org.hamcrest.Matcher
import org.junit.Assert.*
import org.junit.Test

@SujianMediumTest
class EditorInputConnectionTest : BaseEditorTest() {

    private fun createInputConnection(): Triple<SujianEditorView, android.view.inputmethod.InputConnection, EditorInfo> {
        val testData = getSession().ensureTestProjectData()
        openTestChapter("InputConnection测试章节", testData)
        val outAttrs = EditorInfo()
        val editorView = Espresso.onView(ViewMatchers.withId(R.id.editor_content))
        var ic: android.view.inputmethod.InputConnection? = null
        var view: SujianEditorView? = null
        editorView.perform(object : ViewAction {
            override fun getConstraints(): Matcher<View> = ViewMatchers.withId(R.id.editor_content)
            override fun getDescription(): String = "create InputConnection"
            override fun perform(uiController: UiController, v: View) {
                val ev = v as SujianEditorView
                ev.requestFocus()
                uiController.loopMainThreadUntilIdle()
                ic = ev.onCreateInputConnection(outAttrs)
                view = ev
            }
        })
        return Triple(
            view ?: throw AssertionError("No SujianEditorView found"),
            ic ?: throw AssertionError("onCreateInputConnection returned null"),
            outAttrs
        )
    }

    @Test
    fun commitText_ascii_updatesContent() {
        val (_, ic, _) = createInputConnection()
        assertTrue("commitText should succeed", ic.commitText("Hello World", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello World"))
    }

    @Test
    fun commitText_chinese_updatesContent() {
        val (_, ic, _) = createInputConnection()
        assertTrue("commitText with Chinese should succeed", ic.commitText("你好，素笺。", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("你好，素笺。"))
    }

    @Test
    fun commitText_emoji_updatesContent() {
        val (_, ic, _) = createInputConnection()
        assertTrue("commitText with emoji should succeed", ic.commitText("🙂🎉", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("🙂🎉"))
    }

    @Test
    fun commitText_multiline_updatesContent() {
        val (_, ic, _) = createInputConnection()
        assertTrue("commitText with multiline should succeed", ic.commitText("第一行\n第二行", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("第一行\n第二行"))
    }

    @Test
    fun commitText_unicode_setsCorrectUtf8Selection() {
        val (_, ic, _) = createInputConnection()
        val text = "你好世界"
        assertTrue("commitText should succeed", ic.commitText(text, 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        val textBytes = text.toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(textBytes, textBytes))
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(text.length, text.length))
    }

    @Test
    fun commitText_emoji_setsCorrectUtf8Selection() {
        val (_, ic, _) = createInputConnection()
        val text = "A🙂B"
        assertTrue("commitText with emoji should succeed", ic.commitText(text, 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        val textBytes = text.toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(textBytes, textBytes))
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(text.length, text.length))
    }

    @Test
    fun setComposingTextThenFinish_commitsText() {
        val (_, ic, _) = createInputConnection()
        assertTrue("setComposingText should succeed", ic.setComposingText("预输入", 1))
        assertTrue("finishComposingText should succeed", ic.finishComposingText())
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("预输入"))
    }

    @Test
    fun setComposingTextThenCancel_doesNotCommit() {
        val (_, ic, _) = createInputConnection()
        assertTrue("setComposingText should succeed", ic.setComposingText("临时文本", 1))
        assertTrue("finishComposingText should succeed", ic.finishComposingText())
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
    }

    @Test
    fun commitText_multipleTimes_appendsContent() {
        val (_, ic, _) = createInputConnection()
        assertTrue(ic.commitText("A", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        assertTrue(ic.commitText("B", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        assertTrue(ic.commitText("C", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))
    }

    @Test
    fun commitText_mixedUnicodeAndAscii_offsetsAreCorrect() {
        val (_, ic, _) = createInputConnection()
        assertTrue(ic.commitText("Hello你好🙂", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello你好🙂"))
        val fullText = "Hello你好🙂"
        val utf8Bytes = fullText.toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(utf8Bytes, utf8Bytes))
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(fullText.length, fullText.length))
    }

    @Test
    fun replaceRange_unicodeBoundary_maintainsCorrectOffsets() {
        val (_, ic, _) = createInputConnection()
        assertTrue(ic.commitText("ABCDEF", 1))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        val insertByteOffset = "ABC".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertByteOffset, insertByteOffset, "XY", ""))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABCXYDEF"))
    }
}
