package com.xiwei.sujian.editor

import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.BaseEditorTest
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorCommitTextAction
import com.xiwei.sujian.support.EditorReplaceRangeAction
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.SujianLargeTest
import org.junit.Assert.assertEquals
import org.junit.Test

@SujianLargeTest
class EditorPersistenceTest : BaseEditorTest() {

    @Test
    fun commitText_persistsAfterReopen() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("编辑器持久化章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("第一段测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
        waitForEditorContent("第一段测试正文")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("，继续写作"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("第一段测试正文，继续写作"))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("第一段测试正文，继续写作")
    }

    @Test
    fun commitText_persistsAfterColdRestart() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("编辑器持久化章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重启测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("重启测试正文")
    }

    @Test
    fun commitText_unicodeAndMultiline_persists() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("编辑器持久化章节C", testData)

        val testText = "你好，素笺。\n第二行🙂"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(testText)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun commitText_middleInsert_persists() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("编辑器中间插入章节", testData)

        val initialText = "ABCDE"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(initialText))

        val prefix = "AB"
        val insertByteOffset = prefix.toByteArray(Charsets.UTF_8).size
        val expectedUtf16InsertOffset = prefix.length
        assertEquals(
            "ASCII text: UTF-8 byte offset should equal UTF-16 offset",
            expectedUtf16InsertOffset,
            insertByteOffset
        )

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertByteOffset, insertByteOffset, "XY", ""))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "ABXYCDE"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedAfterInsert))

        val expectedCursorUtf8 =
            insertByteOffset + "XY".toByteArray(Charsets.UTF_8).size
        val expectedCursorUtf16 =
            expectedUtf16InsertOffset + "XY".length

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(
                expectedCursorUtf8,
                expectedCursorUtf8
            ))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(
                expectedCursorUtf16,
                expectedCursorUtf16
            ))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedAfterInsert))
    }

    @Test
    fun commitText_unicodeMiddleInsert_persistsWithOffsets() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("编辑器Unicode中间插入章节", testData)

        val initialText = "你好🙂世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(initialText))

        val prefix = "你好🙂"
        val expectedByteOffset = prefix.toByteArray(Charsets.UTF_8).size
        val expectedUtf16Offset = prefix.length

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(expectedByteOffset, expectedByteOffset, "中间", ""))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "你好🙂中间世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedAfterInsert))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(
                expectedUtf16Offset + "中间".length,
                expectedUtf16Offset + "中间".length
            ))

        val insertEndByteOffset = expectedByteOffset + "中间".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(insertEndByteOffset, insertEndByteOffset))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedAfterInsert))
    }
}
