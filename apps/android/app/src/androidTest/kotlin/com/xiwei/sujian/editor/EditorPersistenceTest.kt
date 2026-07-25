package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.MainActivity
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorCommitTextAction
import com.xiwei.sujian.support.TestSession
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class EditorPersistenceTest {

    private val _composeTestRule = createAndroidComposeRule<MainActivity>()

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

    private fun restartRuntime() {
        val session = getSession()
        composeTestRule.activityRule.scenario.close()
        session.restartRuntime()
        com.xiwei.sujian.runtime.SujianAppDependencies.setTestProvider { _ -> session.deps }
        androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
    }

    @Test
    fun commitText_persistsAfterReopen() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("第一段测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorContent("第一段测试正文", testData, chapterId)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("，继续写作"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val viewAfterAppend = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals("第一段测试正文，继续写作", viewAfterAppend.getDisplayText())

        restartRuntime()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("第一段测试正文，继续写作", testData, chapterId)
    }

    @Test
    fun commitText_persistsAfterRestart() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重启测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        restartRuntime()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("重启测试正文", testData, chapterId)
    }

    @Test
    fun commitText_unicodeAndMultiline_persists() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节C", testData)

        val testText = "你好，素笺。\n第二行🙂"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val viewAfterInput = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(testText, viewAfterInput.getDisplayText())

        restartRuntime()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(testText, testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(testText, viewAfterRestart.getDisplayText())
    }

    @Test
    fun commitText_middleInsert_persists() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器中间插入章节", testData)

        val initialText = "ABCDE"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(initialText, view.getDisplayText())

        val insertByteOffset = "AB".toByteArray(Charsets.UTF_8).size
        view.replaceRangeTyped(insertByteOffset, insertByteOffset, "XY", "", uniffi.writer_core.EditorTransactionCauseDto.TYPING)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "ABXYCDE"
        assertEquals(expectedAfterInsert, view.getDisplayText())

        val utf8Length = expectedAfterInsert.toByteArray(Charsets.UTF_8).size
        assertEquals(
            "UTF-8 byte length mismatch",
            "ABXYCDE".toByteArray(Charsets.UTF_8).size,
            utf8Length
        )

        val utf16Length = expectedAfterInsert.length
        assertEquals(
            "UTF-16 code unit length mismatch",
            "ABXYCDE".length,
            utf16Length
        )

        restartRuntime()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert, testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(expectedAfterInsert, viewAfterRestart.getDisplayText())
    }

    @Test
    fun commitText_unicodeMiddleInsert_persists() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器Unicode中间插入章节", testData)

        val initialText = "你好世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(initialText, view.getDisplayText())

        val insertByteOffset = "你好".toByteArray(Charsets.UTF_8).size
        view.replaceRangeTyped(insertByteOffset, insertByteOffset, "中间", "", uniffi.writer_core.EditorTransactionCauseDto.TYPING)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "你好中间世界"
        assertEquals(expectedAfterInsert, view.getDisplayText())

        restartRuntime()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert, testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(expectedAfterInsert, viewAfterRestart.getDisplayText())
    }

    private fun openTestChapter(chapterTitle: String, testData: AndroidTestEnvironment.TestProjectData): String {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput(chapterTitle)

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle(chapterTitle, testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)

        return chapterId
    }

    private fun navigateToChapterAfterRestart(testData: AndroidTestEnvironment.TestProjectData, chapterId: String) {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()
        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
    }

    private fun waitForEditorReady(projectId: String, volumeId: String, chapterId: String) {
        val expectedTargetId = "chapter-body:$projectId:$volumeId:$chapterId"
        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                if (view == null || view.visibility != android.view.View.VISIBLE || !view.isSessionBound) {
                    throw AssertionError("Editor not visible or session not bound")
                }
                true
            } catch (e: Exception) {
                throw AssertionError("waitForEditorReady: Editor not ready for chapter $chapterId: ${e.message}")
            }
        }, timeoutMs = 15_000)

        ComposeWait.waitUntil(composeTestRule, {
            val coordinator = getSession().deps.coordinator
            if (coordinator.activeTargetId != expectedTargetId) {
                throw AssertionError(
                    "waitForEditorReady: Expected activeTargetId=$expectedTargetId but was ${coordinator.activeTargetId}"
                )
            }
            true
        }, timeoutMs = 10_000)
    }

    private fun waitForEditorContent(
        expectedContent: String,
        testData: AndroidTestEnvironment.TestProjectData,
        chapterId: String
    ) {
        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                if (view == null || !view.isSessionBound) {
                    throw AssertionError("Editor not bound")
                }
                val actual = view.getDisplayText()
                if (actual != expectedContent) {
                    throw AssertionError("Expected content '$expectedContent' but was '$actual'")
                }
                true
            } catch (e: Exception) {
                throw AssertionError(
                    "waitForEditorContent: Content mismatch for chapter $chapterId: ${e.message}"
                )
            }
        }, timeoutMs = 15_000)
    }

    private fun waitForChapterByTitle(title: String, testData: AndroidTestEnvironment.TestProjectData): String {
        val session = getSession()
        val repo = session.deps.workspaceRepository
        var chapterId = ""
        ComposeWait.waitUntil(composeTestRule, {
            val chapters = repo.getChapters(testData.projectId, testData.volumeId)
            val found = chapters.firstOrNull { it.title == title }
            if (found != null) {
                chapterId = found.id
                true
            } else {
                throw AssertionError("Chapter '$title' not found in volume ${testData.volumeId}")
            }
        }, timeoutMs = 15_000)
        return chapterId
    }

    private fun navigateToTestVolume(testData: AndroidTestEnvironment.TestProjectData) {
        val volumeTag = SujianSemanticIds.volume(testData.volumeId)
        ComposeWait.waitUntil(composeTestRule, {
            try {
                composeTestRule.onNodeWithTag(volumeTag).assertExists()
                true
            } catch (_: AssertionError) {
                composeTestRule.onNodeWithText(testData.projectTitle).performClick()
                try {
                    composeTestRule.onNodeWithTag(volumeTag).assertExists()
                    true
                } catch (e: AssertionError) {
                    throw AssertionError(
                        "navigateToTestVolume: Volume tag '$volumeTag' not found after clicking project '${testData.projectTitle}': ${e.message}"
                    )
                }
            }
        }, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
