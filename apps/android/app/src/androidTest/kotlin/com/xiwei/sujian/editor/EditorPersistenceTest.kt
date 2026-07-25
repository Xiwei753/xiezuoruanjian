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
        session.deps.coordinator.releaseHost()
        val ctx = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        val newSession = AndroidTestEnvironment.createSession(ctx)
        com.xiwei.sujian.runtime.SujianAppDependencies.setTestProvider { _ -> newSession.deps }
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

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText() == "第一段测试正文"
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("，继续写作"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val viewAfterAppend = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals("第一段测试正文，继续写作", viewAfterAppend.getDisplayText())

        restartRuntime()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.isSessionBound && view.getDisplayText() == "第一段测试正文，继续写作"
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
    }

    @Test
    fun commitText_persistsAfterRestart() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重启测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        restartRuntime()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.isSessionBound && view.getDisplayText() == "重启测试正文"
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
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

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.isSessionBound && view.getDisplayText() == testText
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

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

        val insertOffset = "AB".toByteArray(Charsets.UTF_8).size
        view.replaceRangeTyped(insertOffset, insertOffset, "XY", "", uniffi.writer_core.EditorTransactionCauseDto.TYPING)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "ABXYCDE"
        assertEquals(expectedAfterInsert, view.getDisplayText())

        restartRuntime()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val v = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                v != null && v.isSessionBound && v.getDisplayText() == expectedAfterInsert
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
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

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.visibility == android.view.View.VISIBLE && view.isSessionBound
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        return chapterId
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
            } else false
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
                try {
                    composeTestRule.onNodeWithText(testData.projectTitle).performClick()
                } catch (e: Exception) {
                    throw AssertionError(
                        "navigateToTestVolume: Failed to click project '${testData.projectTitle}': ${e.message}"
                    )
                }
                try {
                    composeTestRule.onNodeWithTag(volumeTag).assertExists()
                    true
                } catch (_: AssertionError) {
                    false
                }
            }
        }, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
