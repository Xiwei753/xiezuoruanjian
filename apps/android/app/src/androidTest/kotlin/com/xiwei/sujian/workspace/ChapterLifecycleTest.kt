package com.xiwei.sujian.workspace

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
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
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ChapterLifecycleTest {

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule, activityRule::getActivity
    ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(activityRule)
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    private fun initTestData(): AndroidTestEnvironment.TestProjectData {
        return AndroidTestEnvironment.ensureTestProjectAndVolume(
            androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        )
    }

    @Test
    fun createChapter_appearsInList() {
        val testData = initTestData()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("自动化测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("自动化测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).assertIsDisplayed()
    }

    @Test
    fun createChapter_canOpenInEditor() {
        val testData = initTestData()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("打开测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("打开测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterId, "打开测试章节")
    }

    @Test
    fun createTwoChapters_canSwitchBetween() {
        val testData = initTestData()
        val session = getSession()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("交替章节A")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterAId = waitForChapterByTitle("交替章节A", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("交替章节B")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterBId = waitForChapterByTitle("交替章节B", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "交替章节A")

        val coordinator = session.deps.coordinator
        val targetIdA = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterAId"
        var lastTargetId: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == targetIdA
        }, timeoutMs = 10_000, message = { "activeTargetId should be $targetIdA but was $lastTargetId after opening chapter A" })

        composeTestRule.onNodeWithText("交替章节A").assertIsDisplayed()

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterBId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "交替章节B")

        val targetIdB = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterBId"
        var lastTargetIdB: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            lastTargetIdB = coordinator.activeTargetId
            coordinator.activeTargetId == targetIdB
        }, timeoutMs = 10_000, message = { "activeTargetId should be $targetIdB but was $lastTargetIdB after opening chapter B" })

        composeTestRule.onNodeWithText("交替章节B").assertIsDisplayed()

        assertNotEquals("Chapters A and B should have different target IDs", targetIdA, targetIdB)
    }

    @Test
    fun twoChapters_textIsolation_noCrossContamination() {
        val session = getSession()
        val testData = initTestData()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)
        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("正文隔离A")
        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()
        val chapterAId = waitForChapterByTitle("正文隔离A", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("正文隔离B")
        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()
        val chapterBId = waitForChapterByTitle("正文隔离B", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "正文隔离A")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("正文-A"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterBId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "正文隔离B")

        val viewB = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(
            "Chapter B should have empty content when first opened",
            "",
            viewB.getDisplayText()
        )

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("正文-B"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "正文隔离A")

        val viewAReopened = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(
            "Chapter A content should still be 正文-A after switching back",
            "正文-A",
            viewAReopened.getDisplayText()
        )

        val coordinator = session.deps.coordinator
        val targetIdA = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterAId"
        assertEquals(
            "activeTargetId should point to chapter A",
            targetIdA,
            coordinator.activeTargetId
        )

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterBId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "正文隔离B")

        val viewBReopened = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
        assertEquals(
            "Chapter B content should still be 正文-B after switching back",
            "正文-B",
            viewBReopened.getDisplayText()
        )

        val targetIdB = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterBId"
        assertEquals(
            "activeTargetId should point to chapter B",
            targetIdB,
            coordinator.activeTargetId
        )
    }

    private fun waitForEditorBoundToChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
        expectedTitle: String
    ) {
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

        composeTestRule.onNodeWithText(expectedTitle).assertIsDisplayed()
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
                    composeTestRule.onNodeWithText(testData.projectTitle).performClick()
                    clickedProject = true
                }
                false
            }
        }, timeoutMs = 15_000, message = { "Volume tag '$volumeTag' not found after clicking project '${testData.projectTitle}'" })
        composeTestRule.onNodeWithTag(volumeTag).performClick()
    }
}
