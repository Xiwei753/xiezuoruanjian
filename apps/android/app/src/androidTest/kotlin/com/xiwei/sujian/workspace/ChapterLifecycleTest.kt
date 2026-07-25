package com.xiwei.sujian.workspace

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.MainActivity
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.TestSession
import com.xiwei.sujian.R
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ChapterLifecycleTest {

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

        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterId)
    }

    @Test
    fun createTwoChapters_canSwitchBetween() {
        val testData = initTestData()
        val session = getSession()
        val repo = session.deps.workspaceRepository

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
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId)

        val coordinator = session.deps.coordinator
        val targetIdA = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterAId"
        ComposeWait.waitUntil(composeTestRule, {
            coordinator.activeTargetId == targetIdA
        }, timeoutMs = 10_000)

        androidx.test.espresso.Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterBId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId)

        val targetIdB = "chapter-body:${testData.projectId}:${testData.volumeId}:$chapterBId"
        ComposeWait.waitUntil(composeTestRule, {
            coordinator.activeTargetId == targetIdB
        }, timeoutMs = 10_000)

        assert(targetIdA != targetIdB) {
            "Chapters A and B should have different target IDs"
        }
    }

    private fun waitForEditorBoundToChapter(projectId: String, volumeId: String, chapterId: String) {
        val expectedTargetId = "chapter-body:$projectId:$volumeId:$chapterId"
        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<SujianEditorView>(R.id.editor_content)
                view != null && view.visibility == android.view.View.VISIBLE && view.isSessionBound
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val coordinator = getSession().deps.coordinator
                coordinator.activeTargetId == expectedTargetId
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 10_000)

        composeTestRule.onNodeWithText(
            getSession().deps.workspaceRepository.getChapters(projectId, volumeId)
                .firstOrNull { it.id == chapterId }?.title ?: chapterId
        )
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
