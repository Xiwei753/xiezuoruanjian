package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.createEmptyComposeRule
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

    private val _composeTestRule = createEmptyComposeRule()

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
        val session = getSession()
        session.launchActivity()
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("第一段测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
        waitForEditorContent("第一段测试正文", testData, chapterId)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("，继续写作"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val viewAfterAppend = session.withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
        assertEquals("第一段测试正文，继续写作", viewAfterAppend.getDisplayText())

        session.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("第一段测试正文，继续写作", testData, chapterId)
    }

    @Test
    fun commitText_persistsAfterRestart() {
        val session = getSession()
        session.launchActivity()
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重启测试正文"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        session.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent("重启测试正文", testData, chapterId)
    }

    @Test
    fun commitText_unicodeAndMultiline_persists() {
        val session = getSession()
        session.launchActivity()
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节C", testData)

        val testText = "你好，素笺。\n第二行🙂"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val viewAfterInput = session.withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
        assertEquals(testText, viewAfterInput.getDisplayText())

        session.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(testText, testData, chapterId)

        val viewAfterRestart = getSession().withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
        assertEquals(testText, viewAfterRestart.getDisplayText())
    }

    @Test
    fun commitText_middleInsert_persists() {
        val session = getSession()
        session.launchActivity()
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器中间插入章节", testData)

        val initialText = "ABCDE"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val view = session.withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
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

        session.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert, testData, chapterId)

        val viewAfterRestart = getSession().withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
        assertEquals(expectedAfterInsert, viewAfterRestart.getDisplayText())
    }

    @Test
    fun commitText_unicodeMiddleInsert_persists() {
        val session = getSession()
        session.launchActivity()
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器Unicode中间插入章节", testData)

        val initialText = "你好世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(initialText))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val view = session.withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
        assertEquals(initialText, view.getDisplayText())

        val insertByteOffset = "你好".toByteArray(Charsets.UTF_8).size
        view.replaceRangeTyped(insertByteOffset, insertByteOffset, "中间", "", uniffi.writer_core.EditorTransactionCauseDto.TYPING)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        val expectedAfterInsert = "你好中间世界"
        assertEquals(expectedAfterInsert, view.getDisplayText())

        session.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        waitForEditorContent(expectedAfterInsert, testData, chapterId)

        val viewAfterRestart = getSession().withActivity { activity ->
            activity.findViewById<SujianEditorView>(R.id.editor_content)
        }
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
        var lastState = "editor missing"
        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = getSession().withActivity { activity ->
                    activity.findViewById<SujianEditorView>(R.id.editor_content)
                }
                when {
                    view == null -> { lastState = "editor missing"; false }
                    view.visibility != android.view.View.VISIBLE -> { lastState = "editor not visible"; false }
                    !view.isSessionBound -> { lastState = "editor not bound"; false }
                    else -> true
                }
            } catch (e: Exception) {
                lastState = "exception: ${e.message}"
                false
            }
        }, timeoutMs = 15_000, message = { "Editor did not become ready for chapter $chapterId. Last state: $lastState" })

        var lastTargetId: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            val coordinator = getSession().deps.coordinator
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == expectedTargetId
        }, timeoutMs = 10_000, message = { "activeTargetId should be $expectedTargetId but was $lastTargetId for chapter $chapterId" })
    }

    private fun waitForEditorContent(
        expectedContent: String,
        testData: AndroidTestEnvironment.TestProjectData,
        chapterId: String
    ) {
        var lastContent = ""
        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = getSession().withActivity { activity ->
                    activity.findViewById<SujianEditorView>(R.id.editor_content)
                }
                if (view == null || !view.isSessionBound) {
                    lastContent = "editor not bound"
                    false
                } else {
                    val actual = view.getDisplayText()
                    lastContent = actual
                    actual == expectedContent
                }
            } catch (e: Exception) {
                lastContent = "exception: ${e.message}"
                false
            }
        }, timeoutMs = 15_000, message = { "Content mismatch for chapter $chapterId: expected '$expectedContent' but was '$lastContent'" })
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
                false
            }
        }, timeoutMs = 15_000, message = { "Chapter '$title' not found in volume ${testData.volumeId}" })
        return chapterId
    }

    private fun navigateToTestVolume(testData: AndroidTestEnvironment.TestProjectData) {
        val volumeTag = SujianSemanticIds.volume(testData.volumeId)
        var clickedProject = false
        ComposeWait.waitUntil(composeTestRule, {
            try {
                composeTestRule.onNodeWithTag(volumeTag).assertExists()
                true
            } catch (_: AssertionError) {
                if (!clickedProject) {
                    composeTestRule.onNodeWithText(testData.projectTitle).performClick()
                    clickedProject = true
                }
                false
            }
        }, timeoutMs = 15_000, message = { "Volume tag '$volumeTag' not found after clicking project '${testData.projectTitle}'" })
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
