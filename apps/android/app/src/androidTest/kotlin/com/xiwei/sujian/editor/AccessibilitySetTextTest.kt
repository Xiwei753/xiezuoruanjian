package com.xiwei.sujian.editor

import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.support.AccessibilitySetTextAction
import com.xiwei.sujian.support.BaseEditorTest
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.SujianLargeTest
import org.junit.Test

@SujianLargeTest
class AccessibilitySetTextTest : BaseEditorTest() {

    @Test
    fun actionSetText_updatesEditorThroughPipeline() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.isSessionBound())

        val testText = "无障碍输入测试正文"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(AccessibilitySetTextAction.setText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        val endByteOffset = testText.toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf8(endByteOffset, endByteOffset))
    }

    @Test
    fun actionSetText_persistsAfterColdRestart() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.isSessionBound())

        val testText = "重启后持久化正文"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(AccessibilitySetTextAction.setText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun actionSetText_unicode_persistsWithCorrectOffsets() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节C", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.isSessionBound())

        val testText = "你好，素笺。\n第二行🙂"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(AccessibilitySetTextAction.setText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        val expectedUtf16Length = testText.length
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasSelectionUtf16(expectedUtf16Length, expectedUtf16Length))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun actionSetText_replacesExistingContent() {
        val testData = getSession().ensureTestProjectData()
        val chapterId = openTestChapter("无障碍SET_TEXT替换测试章节", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.isSessionBound())

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(AccessibilitySetTextAction.setText("初始内容"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("初始内容"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(AccessibilitySetTextAction.setText("替换后的内容"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("替换后的内容"))

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("替换后的内容"))
    }

    @Test
    fun actionSetText_isExposedAsAccessibilityAction() {
        val testData = getSession().ensureTestProjectData()
        openTestChapter("无障碍SET_TEXT来源测试章节", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.isSessionBound())

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasActionSetText())
    }
}
