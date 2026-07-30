package com.xiwei.sujian.workspace

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnySibling
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.BaseEditorTest
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorCommitTextAction
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.SujianLargeTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

@SujianLargeTest
class ChapterLifecycleTest : BaseEditorTest() {

    @Test
    fun createChapter_appearsInList() {
        val testData = getSession().ensureTestProjectData()

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
        val testData = getSession().ensureTestProjectData()

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("打开测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("打开测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterId, "打开测试章节", expectedContent = "")
    }

    @Test
    fun createTwoChapters_canSwitchBetween() {
        val testData = getSession().ensureTestProjectData()
        val session = getSession()

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

        composeTestRule.onNode(hasText("交替章节A") and hasAnySibling(hasTestTag(SujianSemanticIds.EditorSaveStatus))).assertIsDisplayed()

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

        composeTestRule.onNode(hasText("交替章节B") and hasAnySibling(hasTestTag(SujianSemanticIds.EditorSaveStatus))).assertIsDisplayed()

        assertNotEquals("Chapters A and B should have different target IDs", targetIdA, targetIdB)
    }

    @Test
    fun twoChapters_textIsolation_noCrossContamination() {
        val session = getSession()
        val testData = getSession().ensureTestProjectData()

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

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(""))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("正文-B"))
        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterAId)).performClick()
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterAId, "正文隔离A")

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
        waitForEditorBoundToChapter(testData.projectId, testData.volumeId, chapterBId, "正文隔离B")

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
        expectedContent: String? = null
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

        composeTestRule.onNode(hasText(expectedTitle) and hasAnySibling(hasTestTag(SujianSemanticIds.EditorSaveStatus))).assertIsDisplayed()

        if (expectedContent != null) {
            Espresso.onView(ViewMatchers.withId(R.id.editor_content))
                .check(EditorViewAssertions.hasDisplayText(expectedContent))
        }
    }
}
