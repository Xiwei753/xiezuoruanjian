package com.xiwei.sujian.support

import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment.TestProjectData
import com.xiwei.sujian.ui.MainActivity
import org.junit.Rule
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

/**
 * Base class for instrumented tests that need a full editor environment.
 *
 * Provides a shared TestDependenciesRule + RestartableMainActivityRule chain,
 * and eliminates duplication of initTestData(), navigateToTestVolume(),
 * waitForEditorReady(), waitForChapterByTitle(), openTestChapter(),
 * and navigateToChapterAfterRestart() across test classes.
 *
 * Subclass constructors pass custom time sources / clock via constructor params.
 */
@RunWith(AndroidJUnit4::class)
open class BaseEditorTest(
    val manualTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource? = null,
    val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource? = null,
    val manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock? = null
) {
    protected val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule,
        activityRule.composeActivityProvider
    ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(
            AndroidTestEnvironment.TestDependenciesRule(
                animationTimeSource = manualTimeSource
                    ?: com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
                transactionIdSource = transactionIdSource
                    ?: com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
                manualFrameClock = manualFrameClock
            )
        )
        .around(_composeTestRule)

    protected val composeTestRule get() = _composeTestRule

    protected fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    protected fun ensureTestProjectData(): TestProjectData {
        return AndroidTestEnvironment.ensureTestProjectAndVolume(
            InstrumentationRegistry.getInstrumentation().targetContext
        )
    }

    protected fun openTestChapter(chapterTitle: String, testData: TestProjectData): String {
        navigateToTestVolume(testData)
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextReplacement(chapterTitle)
        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()
        val chapterId = waitForChapterByTitle(chapterTitle, testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()
        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
        return chapterId
    }

    protected fun navigateToChapterAfterRestart(testData: TestProjectData, chapterId: String) {
        navigateToTestVolume(testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()
        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
    }

    protected fun waitForEditorReady(projectId: String, volumeId: String, chapterId: String) {
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

    protected fun waitForEditorContent(expectedContent: String) {
        ComposeWait.waitForEspressoViewCondition(
            composeTestRule,
            EditorViewAssertions.hasDisplayText(expectedContent),
            timeoutMs = 15_000
        ) { "Content mismatch: expected '$expectedContent'" }
    }

    protected fun waitForChapterByTitle(title: String, testData: TestProjectData): String {
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

    protected fun navigateToTestVolume(testData: TestProjectData) {
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
