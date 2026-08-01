package com.xiwei.sujian.workspace

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
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
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import com.xiwei.sujian.ui.MainActivity
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
    fun createChapter_appearsInList() {
        val testData = initTestData()

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("自动化测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("自动化测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).assertIsDisplayed()
    }

    @Test
    fun createChapter_canOpenInEditor() {
        val testData = initTestData()

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("打开测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("打开测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterId, "打开测试章节", expectedContent = "")
    }

    @Test
    fun createTwoChapters_canSwitchBetween() {
        val testData = initTestData()
        val session = getSession()

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("交替章节A")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterAId = waitForChapterByTitle("交替章节A", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("交替章节B")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterBId = waitForChapterByTitle("交替章节B", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "交替章节A", "")

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
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "交替章节B", "")

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

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("正文隔离A")
        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()
        val chapterAId = waitForChapterByTitle("正文隔离A", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("正文隔离B")
        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()
        val chapterBId = waitForChapterByTitle("正文隔离B", testData)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "正文隔离A", "")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("正文-A"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterBId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "正文隔离B", "")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(""))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("正文-B"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "正文隔离A", "")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("正文-A"))

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
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "正文隔离B", "")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("正文-B"))

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
        expectedTitle: String,
        expectedContent: String
    ) {
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

        composeTestRule.onNodeWithText(expectedTitle).assertIsDisplayed()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedContent))
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
