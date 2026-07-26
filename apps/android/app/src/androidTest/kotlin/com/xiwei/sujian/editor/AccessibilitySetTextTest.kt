package com.xiwei.sujian.editor

import android.os.Build
import android.os.Bundle
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AccessibilitySetTextTest {

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule, activityRule::getActivity
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
    fun actionSetText_updatesEditorThroughPipeline() {
        val testData = initTestData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节A", testData)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertTrue("Editor should be session bound", view.isSessionBound)

        val testText = "无障碍输入测试正文"
        performActionSetText(view, testText)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        assertEquals(testText, view.getDisplayText())

        val endByteOffset = testText.toByteArray(Charsets.UTF_8).size
        assertEquals(
            "Cursor should be at end of text after ACTION_SET_TEXT",
            endByteOffset,
            view.getSelectionEnd()
        )
    }

    @Test
    fun actionSetText_persistsAfterColdRestart() {
        val testData = initTestData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节B", testData)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertTrue("Editor should be session bound", view.isSessionBound)

        val testText = "重启后持久化正文"
        performActionSetText(view, testText)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        assertEquals(testText, view.getDisplayText())

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(testText, viewAfterRestart.getDisplayText())
    }

    @Test
    fun actionSetText_unicode_persistsWithCorrectOffsets() {
        val testData = initTestData()
        val chapterId = openTestChapter("无障碍SET_TEXT测试章节C", testData)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertTrue("Editor should be session bound", view.isSessionBound)

        val testText = "你好，素笺。\n第二行🙂"
        performActionSetText(view, testText)

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        assertEquals(testText, view.getDisplayText())

        val expectedByteLength = testText.toByteArray(Charsets.UTF_8).size
        val actualByteLength = view.getDisplayText().toByteArray(Charsets.UTF_8).size
        assertEquals(
            "UTF-8 byte length mismatch after ACTION_SET_TEXT",
            expectedByteLength,
            actualByteLength
        )

        val expectedUtf16Length = testText.length
        val actualUtf16Length = view.getDisplayText().length
        assertEquals(
            "UTF-16 code unit length mismatch after ACTION_SET_TEXT",
            expectedUtf16Length,
            actualUtf16Length
        )

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(testText, viewAfterRestart.getDisplayText())
    }

    @Test
    fun actionSetText_replacesExistingContent() {
        val testData = initTestData()
        val chapterId = openTestChapter("无障碍SET_TEXT替换测试章节", testData)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertTrue("Editor should be session bound", view.isSessionBound)

        performActionSetText(view, "初始内容")
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        assertEquals("初始内容", view.getDisplayText())

        performActionSetText(view, "替换后的内容")
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)
        assertEquals("替换后的内容", view.getDisplayText())

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        val viewAfterRestart = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals("替换后的内容", viewAfterRestart.getDisplayText())
    }

    @Test
    fun actionSetText_isExposedAsAccessibilityAction() {
        val testData = initTestData()
        openTestChapter("无障碍SET_TEXT来源测试章节", testData)

        val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertTrue("Editor should be session bound", view.isSessionBound)

        val nodeInfo = view.createAccessibilityNodeInfo()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            val hasSetTextAction = nodeInfo.actionList.any {
                it.id == AccessibilityNodeInfo.ACTION_SET_TEXT
            }
            assertTrue(
                "SujianEditorView should expose ACTION_SET_TEXT as standard Android accessibility action",
                hasSetTextAction
            )
        }
        nodeInfo.recycle()
    }

    private fun performActionSetText(view: SujianEditorView, text: String) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.LOLLIPOP) {
            throw AssertionError("ACTION_SET_TEXT requires API 21+")
        }
        val args = Bundle()
        args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        val result = view.performAccessibilityAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
        assertTrue(
            "performAccessibilityAction(ACTION_SET_TEXT) returned false for text='$text'",
            result
        )
    }

    private fun openTestChapter(chapterTitle: String, testData: AndroidTestEnvironment.TestProjectData): String {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput)
            .performTextInput(chapterTitle)

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
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
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
            val coordinator = AndroidTestEnvironment.requireCurrentSession().deps.coordinator
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == expectedTargetId
        }, timeoutMs = 10_000, message = { "activeTargetId should be $expectedTargetId but was $lastTargetId for chapter $chapterId" })
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
        val volumeTag = SujianSemanticIds.volume(testData.volumeId)
        var clickedProject = false
        ComposeWait.waitUntil(composeTestRule, {
            try {
                composeTestRule.onNodeWithTag(volumeTag).assertExists()
                true
            } catch (_: AssertionError) {
                if (!clickedProject) {
                    composeTestRule.onNodeWithTag(
                        SujianSemanticIds.project(testData.projectId)
                    ).performClick()
                    clickedProject = true
                }
                false
            }
        }, timeoutMs = 15_000, message = { "Volume tag '$volumeTag' not found after clicking project '${testData.projectId}'" })
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
