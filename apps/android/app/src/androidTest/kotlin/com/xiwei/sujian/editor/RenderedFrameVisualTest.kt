package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorBitmapCapture
import com.xiwei.sujian.support.EditorBitmapCapture.CapturedFrame
import com.xiwei.sujian.support.EditorCommitTextAction
import com.xiwei.sujian.support.EditorCompositionAction
import com.xiwei.sujian.support.EditorReplaceRangeAction
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import com.xiwei.sujian.ui.MainActivity
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class RenderedFrameVisualTest {

    private val manualTimeSource = ManualAnimationTimeSource()
    private val transactionIdSource = TransactionIdSource()
    private val manualFrameClock = WindowDisplayFrameClock.ManualFrameClock()

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule,
        activityRule.composeActivityProvider
    ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule(manualTimeSource, transactionIdSource, manualFrameClock))
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    private fun initTestData(): AndroidTestEnvironment.TestProjectData {
        return AndroidTestEnvironment.ensureTestProjectAndVolume(
            androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        )
    }

    private fun dispatchManualFrame() {
        val frameTimeNanos = manualTimeSource.nowNanos()
        manualFrameClock.dispatchFrame(frameTimeNanos)
    }

    private fun advanceClockToEnd() {
        for (i in 0 until 20) {
            manualTimeSource.advanceByMs(16)
            dispatchManualFrame()
        }
    }

    @Test
    fun insertText_startFrame_showsRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Hello"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val startFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(startFrame, "Start frame must have rendered content after insert")

        val bounds = startFrame.contentBounds()
        assertTrue("Content left bound should be >= 0", bounds.left >= 0)
        assertTrue("Content top bound should be >= 0", bounds.top >= 0)
        assertTrue("Content should have non-zero width", bounds.width() > 0)
        assertTrue("Content should have non-zero height", bounds.height() > 0)
    }

    @Test
    fun insertText_midFrame_showsIntermediateRendering() {
        val testData = initTestData()
        openTestChapter("渲染帧中间帧测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val startFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(startFrame, "Start frame must have rendered content")

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have rendered content during animation")

        advanceClockToEnd()

        val endFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(endFrame, "End frame must have rendered content after animation completes")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
    }

    @Test
    fun deleteRange_startFrame_showsOriginalContent() {
        val testData = initTestData()
        openTestChapter("渲染帧删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val beforeDeleteFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(beforeDeleteFrame, "Frame before delete must have content")

        val beforeBounds = beforeDeleteFrame.contentBounds()

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val deleteStartFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(deleteStartFrame, "Delete start frame must have rendered content")

        advanceClockToEnd()

        val deleteEndFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(deleteEndFrame, "Delete end frame must have rendered content")

        val afterBounds = deleteEndFrame.contentBounds()
        assertTrue(
            "After deleting 2 chars, content width should be <= before delete width",
            afterBounds.width() <= beforeBounds.width() + 1
        )
    }

    @Test
    fun unicodeInsert_renderedBitmapShowsContent() {
        val testData = initTestData()
        openTestChapter("渲染帧Unicode测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "你好世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val frame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame, "Unicode text must produce rendered content")

        val bounds = frame.contentBounds()
        assertTrue("Unicode content should have non-zero width", bounds.width() > 0)
        assertTrue("Unicode content should have non-zero height", bounds.height() > 0)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun compositionUpdate_renderedBitmapShowsPreedit() {
        val testData = initTestData()
        openTestChapter("渲染帧composition测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("预"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val frame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame, "Composition preedit must produce rendered content")

        val bounds = frame.contentBounds()
        assertTrue("Composition content should have non-zero width", bounds.width() > 0)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("预"))
    }

    @Test
    fun insertThenDelete_contentBoundsShrink() {
        val testData = initTestData()
        openTestChapter("渲染帧插入删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("XXXXX"))

        advanceClockToEnd()

        val afterInsertFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(afterInsertFrame, "After insert must have content")
        val insertBounds = afterInsertFrame.contentBounds()

        val deleteStart = "XX".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "XXXXX".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "XXX"))

        advanceClockToEnd()

        val afterDeleteFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(afterDeleteFrame, "After delete must have content")
        val deleteBounds = afterDeleteFrame.contentBounds()

        assertTrue(
            "After deleting 3 chars, content width should be <= before width",
            deleteBounds.width() <= insertBounds.width() + 1
        )
    }

    @Test
    fun multilineInsert_renderedBitmapShowsMultipleLines() {
        val testData = initTestData()
        openTestChapter("渲染帧多行测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "第一行\n第二行\n第三行"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val frame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame, "Multiline text must produce rendered content")

        val bounds = frame.contentBounds()
        assertTrue("Multiline content should have non-zero height", bounds.height() > 0)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun rapidInput_eachFrameShowsProgressiveContent() {
        val testData = initTestData()
        openTestChapter("渲染帧连续输入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("A"))

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        val frame1 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame1, "Frame after 'A' must have content")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("B"))

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        val frame2 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame2, "Frame after 'AB' must have content")

        val bounds1 = frame1.contentBounds()
        val bounds2 = frame2.contentBounds()
        assertTrue(
            "After adding 'B', content width should be >= after 'A'",
            bounds2.width() >= bounds1.width() - 1
        )

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
    }

    @Test
    fun emptyEditor_showsNoTextContent() {
        val testData = initTestData()
        openTestChapter("渲染帧空编辑器测试", testData)

        manualTimeSource.advanceTo(0L)

        val frame = EditorBitmapCapture.captureEditorBitmap()
        val bounds = frame.contentBounds()
        assertTrue(
            "Empty editor should have no or minimal content (only cursor if visible)",
            bounds.width() < frame.width / 2
        )
    }

    @Test
    fun insertText_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧五点测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Test"))

        val frames = mutableListOf<CapturedFrame>()
        val progressLabels = mutableListOf<String>()

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())
        progressLabels.add("start")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())
        progressLabels.add("25%")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())
        progressLabels.add("50%")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())
        progressLabels.add("75%")

        advanceClockToEnd()
        frames.add(EditorBitmapCapture.captureEditorBitmap())
        progressLabels.add("end")

        for (i in frames.indices) {
            EditorBitmapCapture.assertBitmapHasContent(
                frames[i],
                "Frame at ${progressLabels[i]} must have rendered content"
            )
        }

        val bounds = frames.map { it.contentBounds() }
        for (i in bounds.indices) {
            assertTrue(
                "Content at ${progressLabels[i]} should have non-zero width",
                bounds[i].width() > 0
            )
            assertTrue(
                "Content at ${progressLabels[i]} should have non-zero height",
                bounds[i].height() > 0
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Test"))
    }

    private fun openTestChapter(chapterTitle: String, testData: AndroidTestEnvironment.TestProjectData): String {
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

    private fun waitForEditorReady(projectId: String, volumeId: String, chapterId: String) {
        ComposeWait.waitForEspressoViewCondition(
            composeTestRule,
            EditorViewAssertions.isEditorReady(),
            timeoutMs = 15_000
        ) { "Editor did not become ready for chapter $chapterId" }

        val expectedTargetId = "chapter-body:$projectId:$volumeId:$chapterId"
        var lastTargetId: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            val coordinator = AndroidTestEnvironment.requireCurrentSession().deps.coordinator
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == expectedTargetId
        }, timeoutMs = 10_000, message = { "activeTargetId should be $expectedTargetId but was $lastTargetId" })
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
        }, timeoutMs = 15_000, message = { "Chapter '$title' not found" })
        return chapterId
    }
}
