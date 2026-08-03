package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.v2.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorCommitTextAction
import com.xiwei.sujian.support.EditorReplaceRangeAction
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import com.xiwei.sujian.ui.MainActivity
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class EditorPersistenceTest {

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule,
        activityProvider = activityRule.composeActivityProvider
    ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    private fun initTestData(): AndroidTestEnvironment.TestProjectData {
        return AndroidTestEnvironment.ensureTestProjectAndVolume(
            androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        )
    }

    @Test
    fun commitText_persistsAfterReopen() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("第一段测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.closeSoftKeyboard()
        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.chapter(testData.volumeId, chapterId), timeoutMs = 15_000)
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
        val testData = initTestData()
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
        val testData = initTestData()
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
        val testData = initTestData()
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
        org.junit.Assert.assertEquals(
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
        val testData = initTestData()
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

    private fun openTestChapter(chapterTitle: String, testData: AndroidTestEnvironment.TestProjectData): String {
        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput(chapterTitle)

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle(chapterTitle, testData)
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.chapter(testData.volumeId, chapterId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)

        return chapterId
    }

    private fun navigateToChapterAfterRestart(testData: AndroidTestEnvironment.TestProjectData, chapterId: String) {
        navigateToTestVolume(testData)
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.chapter(testData.volumeId, chapterId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()
        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
    }

    private fun waitForEditorReady(projectId: String, volumeId: String, chapterId: String) {
        val expectedTargetId = "chapter-body:$projectId:$volumeId:$chapterId"
        ComposeWait.waitForEspressoViewCondition(
            composeTestRule,
            EditorViewAssertions.isEditorReady(),
            timeoutMs = 15_000
        ) { "Editor did not become ready for chapter $chapterId" }

        var lastTargetId: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            val coordinator = AndroidTestEnvironment.requireCurrentSession().deps.coordinator
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == expectedTargetId
        }, timeoutMs = 10_000, message = { "activeTargetId should be $expectedTargetId but was $lastTargetId for chapter $chapterId" })
    }

    private fun waitForEditorContent(expectedContent: String) {
        ComposeWait.waitForEspressoViewCondition(
            composeTestRule,
            EditorViewAssertions.hasDisplayText(expectedContent),
            timeoutMs = 15_000
        ) { "Content mismatch: expected '$expectedContent'" }
    }

    private fun waitForChapterByTitle(title: String, testData: AndroidTestEnvironment.TestProjectData): String {
        val s = AndroidTestEnvironment.requireCurrentSession()
        val repo = s.deps.workspaceRepository
        var chapterId = ""
        ComposeWait.waitUntil(composeTestRule, {
            val chapters = repo.getChapters(testData.projectId, testData.volumeId)
            val found = chapters.firstOrNull { it.title == title }
            if (found != null) {
                chapterId = found.id
                true
            } else {
                false
            }
        }, timeoutMs = 15_000, message = { "Chapter '$title' not found in volume ${testData.volumeId}" })
        return chapterId
    }

    private fun navigateToTestVolume(testData: AndroidTestEnvironment.TestProjectData) {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        val projectTag = SujianSemanticIds.project(testData.projectId)
        ComposeWait.waitForTag(composeTestRule, projectTag, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(projectTag).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 15_000)

        val volumeTag = SujianSemanticIds.volume(testData.volumeId)
        ComposeWait.waitForTag(composeTestRule, volumeTag, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
