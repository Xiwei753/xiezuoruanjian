package com.xiwei.sujian.editor

import android.graphics.RectF
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.visual.CaptureMethod
import com.xiwei.sujian.editor.v2.visual.ColorDistance
import com.xiwei.sujian.editor.v2.visual.AnimationStateSnapshot
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import com.xiwei.sujian.editor.v2.visual.TransactionState
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot
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

    private fun captureVisualFrameSnapshot(): VisualFrameSnapshot? {
        var snapshot: VisualFrameSnapshot? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                snapshot = editorView.captureVisualFrameSnapshot()
            }
        return snapshot
    }

    private fun getActiveAnimationDurationMs(): Long {
        var durationMs = 0L
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                durationMs = editorView.getActiveAnimationDurationMs()
            }
        return durationMs
    }

    private fun getActiveAnimationStartTimeMs(): Long {
        var startTimeMs = 0L
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                startTimeMs = editorView.getActiveAnimationStartTimeMs() ?: 0L
            }
        return startTimeMs
    }

    private fun captureAnimationStateSnapshot(): AnimationStateSnapshot? {
        var snapshot: AnimationStateSnapshot? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                snapshot = editorView.captureAnimationSnapshot()
            }
        return snapshot
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

        val result = captureFiveProgressPoints()
        val frames = result.frames

        val labels = listOf("start", "25%", "50%", "75%", "end")
        assertAllFramesHaveContent(frames, labels)
        assertContentBoundsProgression(frames, labels)
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertCrossFrameAlphaProgression(frames, labels)
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, labels)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Test"))
    }

    @Test
    fun deleteRange_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧删除五点测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val beforeDeleteFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(beforeDeleteFrame, "Before delete must have content")
        val beforeBounds = beforeDeleteFrame.contentBounds()

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        val result = captureFiveProgressPoints()
        val frames = result.frames

        val deleteLabels = listOf("start", "25%", "50%", "75%", "end")
        assertAllFramesHaveContent(frames, deleteLabels)

        val endBounds = frames[4].contentBounds()
        assertTrue(
            "After delete, content width should shrink: end ${endBounds.width()} <= before ${beforeBounds.width()}",
            endBounds.width() <= beforeBounds.width() + 1
        )

        assertDeleteProgressionWidthShrinks(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertCrossFrameAlphaProgression(frames, deleteLabels)
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, deleteLabels)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))
    }

    @Test
    fun compositionUpdate_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧composition五点测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("预输入"))

        val result = captureFiveProgressPoints()
        val frames = result.frames

        val compLabels = listOf("start", "25%", "50%", "75%", "end")
        assertAllFramesHaveContent(frames, compLabels)
        assertContentBoundsProgression(frames, compLabels)
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertCrossFrameAlphaProgression(frames, compLabels)
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, compLabels)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("预输入"))
    }

    @Test
    fun unicodeInsert_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧Unicode五点测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "你好世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        val result = captureFiveProgressPoints()
        val frames = result.frames

        val uniLabels = listOf("start", "25%", "50%", "75%", "end")
        assertAllFramesHaveContent(frames, uniLabels)
        assertContentBoundsProgression(frames, uniLabels)
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertCrossFrameAlphaProgression(frames, uniLabels)
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, uniLabels)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun multilineInsert_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧多行五点测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "第一行\n第二行\n第三行"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        val result = captureFiveProgressPoints()
        val frames = result.frames

        val mlLabels = listOf("start", "25%", "50%", "75%", "end")
        assertAllFramesHaveContent(frames, mlLabels)

        for ((i, label) in mlLabels.withIndex()) {
            val bounds = frames[i].contentBounds()
            assertTrue("Multiline at $label should have non-zero height", bounds.height() > 0)
            assertTrue("Multiline at $label should have non-zero width", bounds.width() > 0)
        }

        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertCrossFrameAlphaProgression(frames, mlLabels)
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, mlLabels)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun rapidInput_fiveProgressPoints_allHaveRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧连续输入五点测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("A"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val frame1 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame1, "Frame after 'A' must have content")

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("B"))

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue(
            "Rapid input must produce an active animation: durationMs=$durationMs must be > 0",
            durationMs > 0
        )
        assertTrue(
            "Rapid input animation startTimeMs=$startTimeMs must be >= 0",
            startTimeMs >= 0
        )

        manualTimeSource.advanceToProgress(0f, durationMs, startTimeMs)
        dispatchManualFrame()
        val frame2 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame2, "Frame at 0% must have content")

        manualTimeSource.advanceToProgress(0.25f, durationMs, startTimeMs)
        dispatchManualFrame()
        val frame3 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame3, "Frame at 25% must have content")

        manualTimeSource.advanceToProgress(0.5f, durationMs, startTimeMs)
        dispatchManualFrame()
        val frame4 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame4, "Frame at 50% must have content")

        manualTimeSource.advanceToProgress(0.75f, durationMs, startTimeMs)
        dispatchManualFrame()
        val frame5 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame5, "Frame at 75% must have content")

        manualTimeSource.advanceToProgress(1f, durationMs, startTimeMs)
        dispatchManualFrame()
        val frame6 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame6, "End frame must have content")

        val bounds1 = frame1.contentBounds()
        val bounds2 = frame2.contentBounds()
        val bounds6 = frame6.contentBounds()
        assertTrue(
            "After 'AB', content width should be >= after 'A': ${bounds2.width()} >= ${bounds1.width()}",
            bounds2.width() >= bounds1.width() - 1
        )
        assertTrue(
            "End frame width should be >= after 'A': ${bounds6.width()} >= ${bounds1.width()}",
            bounds6.width() >= bounds1.width() - 1
        )

        val frames = listOf(frame1, frame2, frame3, frame4, frame5, frame6)
        assertContentPixelsVisuallyDistinctFromBackground(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertCrossFrameAlphaProgression(frames, listOf("after-A", "0%", "25%", "50%", "75%", "end"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
    }

    @Test
    fun insertText_pixelLevel_cursorRegionHasContent() {
        val testData = initTestData()
        openTestChapter("渲染帧光标像素测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("X"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val startFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(startFrame, "Start frame must have content")

        val startSnapshot = captureVisualFrameSnapshot()
        val cursorRect = startSnapshot?.cursorRect
        assertNotNull("Start frame visual snapshot must provide cursorRect", cursorRect)
        assertCursorRegionFromSnapshotRect(startFrame, cursorRect!!, "Start frame cursor from snapshot")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have content during animation")

        val midSnapshot = captureVisualFrameSnapshot()
        val midCursorRect = midSnapshot?.cursorRect
        assertNotNull("Mid-frame visual snapshot must provide cursorRect", midCursorRect)
        assertCursorRegionFromSnapshotRect(midFrame, midCursorRect!!, "Mid-frame cursor from snapshot")

        advanceClockToEnd()
    }

    @Test
    fun insertText_crossFrame_movementDirectionIsRightward() {
        val testData = initTestData()
        openTestChapter("渲染帧位移方向测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val startFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(startFrame, "Start frame must have content")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have content")

        advanceClockToEnd()
        val endFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(endFrame, "End frame must have content")

        val startFirst = startFrame.findFirstNonBackgroundPixel()
        val endFirst = endFrame.findFirstNonBackgroundPixel()
        assertNotNull("Start frame must have non-background pixel", startFirst)
        assertNotNull("End frame must have non-background pixel", endFirst)

        val startLast = startFrame.findLastNonBackgroundPixel()
        val endLast = endFrame.findLastNonBackgroundPixel()
        assertNotNull("Start frame must have last non-background pixel", startLast)
        assertNotNull("End frame must have last non-background pixel", endLast)

        assertTrue(
            "End frame content should extend rightward: end last x ${endLast!!.first} >= start last x ${startLast!!.first}",
            endLast.first >= startLast.first
        )

        val midLast = midFrame.findLastNonBackgroundPixel()
        assertNotNull("Mid frame must have last non-background pixel", midLast)
        assertTrue(
            "Mid frame content should be between start and end: mid last x ${midLast!!.first} between ${startLast.first} and ${endLast.first}",
            midLast.first >= startLast.first - 2 && midLast.first <= endLast.first + 2
        )
    }

    @Test
    fun deleteRange_crossFrame_movementDirectionIsLeftward() {
        val testData = initTestData()
        openTestChapter("渲染帧删除位移测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val beforeFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(beforeFrame, "Before delete must have content")
        val beforeLast = beforeFrame.findLastNonBackgroundPixel()
        assertNotNull("Before delete must have last content pixel", beforeLast)

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val deleteStartFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(deleteStartFrame, "Delete start frame must have content")

        advanceClockToEnd()
        val deleteEndFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(deleteEndFrame, "Delete end frame must have content")

        val afterLast = deleteEndFrame.findLastNonBackgroundPixel()
        assertNotNull("After delete must have last content pixel", afterLast)
        assertTrue(
            "After delete, content right edge should move leftward: after ${afterLast!!.first} <= before ${beforeLast!!.first}",
            afterLast.first <= beforeLast.first + 2
        )
    }

    @Test
    fun insertText_cursorLayerAboveText_notCovered() {
        val testData = initTestData()
        openTestChapter("渲染帧光标层级测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val frame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame, "Frame must have content for cursor layer check")

        val snapshot = captureVisualFrameSnapshot()
        val cursorRect = snapshot?.cursorRect
        assertNotNull("Visual frame snapshot must provide cursorRect for cursor layer check", cursorRect)
        val cursorX = cursorRect!!.left.toInt()
        val cursorY = ((cursorRect.top + cursorRect.bottom) / 2).toInt()
        assertTrue(
            "Cursor position must be within bitmap: x=$cursorX < ${frame.width} && y=$cursorY < ${frame.height}",
            cursorX in 0 until frame.width && cursorY in 0 until frame.height
        )
        assertTrue(
            "Cursor pixel at ($cursorX, $cursorY) must be non-background (cursor drawn above text)",
            frame.isPixelNonBackground(cursorX, cursorY)
        )

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have content for cursor layer check")

        val midSnapshot = captureVisualFrameSnapshot()
        val midCursorRect = midSnapshot?.cursorRect
        assertNotNull("Mid-frame visual snapshot must provide cursorRect", midCursorRect)
        val midCursorX = midCursorRect!!.left.toInt()
        val midCursorY = ((midCursorRect.top + midCursorRect.bottom) / 2).toInt()
        assertTrue(
            "Mid-frame cursor position must be within bitmap: x=$midCursorX < ${midFrame.width} && y=$midCursorY < ${midFrame.height}",
            midCursorX in 0 until midFrame.width && midCursorY in 0 until midFrame.height
        )
        assertTrue(
            "Mid-frame cursor pixel must be non-background (cursor visible during animation)",
            midFrame.isPixelNonBackground(midCursorX, midCursorY)
        )

        advanceClockToEnd()
    }

    @Test
    fun insertText_backgroundDoesNotCoverContent() {
        val testData = initTestData()
        openTestChapter("渲染帧背景覆盖测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Test"))

        val result = captureFiveProgressPoints()
        val frames = result.frames

        for ((i, label) in listOf("start", "25%", "50%", "75%", "end").withIndex()) {
            val frame = frames[i]
            val bounds = frame.contentBounds()
            assertTrue(
                "Content at $label must have non-zero bounds for background coverage check",
                bounds.width() > 0 && bounds.height() > 0
            )
            val centerX = (bounds.left + bounds.right) / 2
            val centerY = (bounds.top + bounds.bottom) / 2
            assertTrue(
                "Content center at $label ($centerX, $centerY) must be non-background (background not covering text)",
                frame.isPixelNonBackground(centerX, centerY)
            )
            val pixel = frame.bitmap.getPixel(centerX, centerY)
            val bg = frame.backgroundColor
            val dr = ColorDistance.red(pixel) - ColorDistance.red(bg)
            val dg = ColorDistance.green(pixel) - ColorDistance.green(bg)
            val db = ColorDistance.blue(pixel) - ColorDistance.blue(bg)
            val rgbDistSq = dr * dr + dg * dg + db * db
            assertTrue(
                "Content center at $label must be visually distinct from background: RGB distance²=$rgbDistSq must be > ${ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE} (background not covering text)",
                rgbDistSq > ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE
            )
        }

        val snapshot = captureVisualFrameSnapshot()
        assertNotNull("Visual frame snapshot must exist for logical alpha check", snapshot)
        for (slice in snapshot!!.sliceVisualStates) {
            assertTrue(
                "Slice logical alpha ${slice.currentAlpha} must be in [0,1] for role ${slice.role}",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
        }
        assertCrossFrameLogicalAlphaProgression(result.visualSnapshots, listOf("start", "25%", "50%", "75%", "end"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Test"))
    }

    @Test
    fun insertText_finalFrameResourcesNotRecycledBeforeDraw() {
        val testData = initTestData()
        openTestChapter("最终帧插入生命周期测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Test"))

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue("Animation must be active", durationMs > 0)

        manualTimeSource.advanceToProgress(1f, durationMs, startTimeMs)

        val preDrawSnapshot = captureAnimationStateSnapshot()
        assertNotNull(
            "Before 100% frame draw, animation state snapshot must exist (transaction still active)",
            preDrawSnapshot
        )
        assertTrue(
            "Before 100% frame draw, transaction must still be Rendering, but was ${preDrawSnapshot!!.transactionState}",
            preDrawSnapshot.transactionState == TransactionState.Rendering
                    || preDrawSnapshot.transactionState == TransactionState.Prepared
        )
        assertTrue(
            "Before 100% frame draw, ownedResourceCount must be > 0 (Bitmaps not yet recycled), but was ${preDrawSnapshot.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        dispatchManualFrame()

        val finalFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(finalFrame, "100% frame must have rendered content")

        val finalStateSnapshot = captureAnimationStateSnapshot()
        assertTrue(
            "After 100% frame draw, transaction must be completed or null",
            finalStateSnapshot == null
                    || finalStateSnapshot.transactionState == TransactionState.Completed
        )
        if (finalStateSnapshot != null) {
            assertEquals(
                "After 100% insert frame draw, owned resources must be released",
                0,
                finalStateSnapshot.ownedResourceCount
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Test"))
    }

    @Test
    fun deleteRange_finalFrameResourcesNotRecycledBeforeDraw() {
        val testData = initTestData()
        openTestChapter("最终帧删除生命周期测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue("Animation must be active", durationMs > 0)

        manualTimeSource.advanceToProgress(1f, durationMs, startTimeMs)

        val preDrawSnapshot = captureAnimationStateSnapshot()
        assertNotNull(
            "Before 100% delete frame draw, animation state snapshot must exist (transaction still active)",
            preDrawSnapshot
        )
        assertTrue(
            "Before 100% delete frame draw, transaction must still be Rendering, but was ${preDrawSnapshot!!.transactionState}",
            preDrawSnapshot.transactionState == TransactionState.Rendering
                    || preDrawSnapshot.transactionState == TransactionState.Prepared
        )
        assertTrue(
            "Before 100% delete frame draw, ownedResourceCount must be > 0 (Bitmaps not yet recycled), but was ${preDrawSnapshot.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        dispatchManualFrame()

        val finalFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(finalFrame, "100% delete frame must have rendered content")

        val finalStateSnapshot = captureAnimationStateSnapshot()
        assertTrue(
            "After 100% delete frame draw, transaction must be completed or null",
            finalStateSnapshot == null
                    || finalStateSnapshot.transactionState == TransactionState.Completed
        )
        if (finalStateSnapshot != null) {
            assertEquals(
                "After 100% delete frame draw, owned resources must be released",
                0,
                finalStateSnapshot.ownedResourceCount
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))
    }

    @Test
    fun compositionUpdate_finalFrameResourcesNotRecycledBeforeDraw() {
        val testData = initTestData()
        openTestChapter("最终帧预输入生命周期测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("预输入"))

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue("Animation must be active", durationMs > 0)

        manualTimeSource.advanceToProgress(1f, durationMs, startTimeMs)

        val preDrawSnapshot = captureAnimationStateSnapshot()
        assertNotNull(
            "Before 100% composition frame draw, animation state snapshot must exist (transaction still active)",
            preDrawSnapshot
        )
        assertTrue(
            "Before 100% composition frame draw, transaction must still be Rendering, but was ${preDrawSnapshot!!.transactionState}",
            preDrawSnapshot.transactionState == TransactionState.Rendering
                    || preDrawSnapshot.transactionState == TransactionState.Prepared
        )
        assertTrue(
            "Before 100% composition frame draw, ownedResourceCount must be > 0 (Bitmaps not yet recycled), but was ${preDrawSnapshot.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        dispatchManualFrame()

        val finalFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(finalFrame, "100% composition frame must have rendered content")

        val finalStateSnapshot = captureAnimationStateSnapshot()
        assertTrue(
            "After 100% composition frame draw, transaction must be completed or null",
            finalStateSnapshot == null
                    || finalStateSnapshot.transactionState == TransactionState.Completed
        )
        if (finalStateSnapshot != null) {
            assertEquals(
                "After 100% composition frame draw, owned resources must be released",
                0,
                finalStateSnapshot.ownedResourceCount
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("预输入"))
    }

    @Test
    fun rapidInput_finalFrameResourcesNotRecycledBeforeDraw() {
        val testData = initTestData()
        openTestChapter("最终帧连续输入生命周期测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("A"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("B"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("C"))

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue("Animation must be active after rapid input", durationMs > 0)

        manualTimeSource.advanceToProgress(1f, durationMs, startTimeMs)

        val preDrawSnapshot = captureAnimationStateSnapshot()
        assertNotNull(
            "Before rapid input final frame draw, animation state snapshot must exist (transaction still active)",
            preDrawSnapshot
        )
        assertTrue(
            "Before rapid input final frame draw, transaction must still be Rendering, but was ${preDrawSnapshot!!.transactionState}",
            preDrawSnapshot.transactionState == TransactionState.Rendering
                    || preDrawSnapshot.transactionState == TransactionState.Prepared
        )
        assertTrue(
            "Before rapid input final frame draw, ownedResourceCount must be > 0 (Bitmaps not yet recycled), but was ${preDrawSnapshot.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        dispatchManualFrame()

        val finalFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(finalFrame, "Final frame after rapid input must have rendered content")

        val finalStateSnapshot = captureAnimationStateSnapshot()
        assertTrue(
            "After rapid input final frame draw, transaction must be completed or null",
            finalStateSnapshot == null
                    || finalStateSnapshot.transactionState == TransactionState.Completed
        )
        if (finalStateSnapshot != null) {
            assertEquals(
                "After rapid input final frame draw, owned resources must be released",
                0,
                finalStateSnapshot.ownedResourceCount
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))
    }

    @Test
    fun deleteRange_cursorRemainsVisibleDuringAnimation() {
        val testData = initTestData()
        openTestChapter("渲染帧删除光标可见测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val startFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(startFrame, "Delete start frame must have content")

        val startBounds = startFrame.contentBounds()
        assertTrue("Delete start content must have width > 0", startBounds.width() > 0)

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Delete mid-frame must have content")

        val midSnapshot = captureVisualFrameSnapshot()
        val midCursorRect = midSnapshot?.cursorRect
        assertNotNull("Delete mid-frame snapshot must provide cursorRect", midCursorRect)
        val midCursorX = midCursorRect!!.left.toInt()
        val midCursorY = ((midCursorRect.top + midCursorRect.bottom) / 2).toInt()
        assertTrue(
            "Delete mid-frame cursor position must be within bitmap",
            midCursorX in 0 until midFrame.width && midCursorY in 0 until midFrame.height
        )
        assertTrue(
            "Cursor must remain visible during delete animation at mid-frame",
            midFrame.isPixelNonBackground(midCursorX, midCursorY)
        )

        advanceClockToEnd()
    }

    @Test
    fun insertText_pixelCopy_showsRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧PixelCopy测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val pixelCopyFrame = EditorBitmapCapture.capturePixelCopyBitmap()
        assertEquals("PixelCopy frame must use PIXEL_COPY capture method", CaptureMethod.PIXEL_COPY, pixelCopyFrame.captureMethod)
        EditorBitmapCapture.assertBitmapHasContent(pixelCopyFrame, "PixelCopy frame must have content")

        val pcBounds = pixelCopyFrame.contentBounds()
        assertTrue("PixelCopy content should have non-zero width", pcBounds.width() > 0)
        assertTrue("PixelCopy content should have non-zero height", pcBounds.height() > 0)

        advanceClockToEnd()
    }

    @Test
    fun insertText_softwareDraw_showsRenderedContent() {
        val testData = initTestData()
        openTestChapter("渲染帧软件绘制测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val softwareFrame = EditorBitmapCapture.captureSoftwareBitmap()
        assertEquals("Software frame must use SOFTWARE_DRAW capture method", CaptureMethod.SOFTWARE_DRAW, softwareFrame.captureMethod)
        EditorBitmapCapture.assertBitmapHasContent(softwareFrame, "Software-drawn frame must have content")

        val swBounds = softwareFrame.contentBounds()
        assertTrue("Software content should have non-zero width", swBounds.width() > 0)
        assertTrue("Software content should have non-zero height", swBounds.height() > 0)

        advanceClockToEnd()
    }

    private data class FiveProgressResult(
        val frames: List<CapturedFrame>,
        val visualSnapshots: List<VisualFrameSnapshot?>
    )

    private fun captureFiveProgressPoints(): FiveProgressResult {
        val frames = mutableListOf<CapturedFrame>()
        val visualSnapshots = mutableListOf<VisualFrameSnapshot?>()
        val progressLabels = listOf("0%", "25%", "50%", "75%", "100%")
        val progressValues = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)

        manualTimeSource.advanceByMs(1)
        dispatchManualFrame()

        val durationMs = getActiveAnimationDurationMs()
        val startTimeMs = getActiveAnimationStartTimeMs()
        assertTrue(
            "captureFiveProgressPoints requires an active animation: durationMs=$durationMs must be > 0",
            durationMs > 0
        )
        assertTrue(
            "captureFiveProgressPoints requires an active animation: startTimeMs=$startTimeMs must be >= 0",
            startTimeMs >= 0
        )

        val stateSnapshot = captureAnimationStateSnapshot()
        assertNotNull(
            "captureFiveProgressPoints requires an active animation state snapshot",
            stateSnapshot
        )
        assertTrue(
            "captureFiveProgressPoints requires transaction state to be Rendering/Prepared/Pending, but was ${stateSnapshot!!.transactionState}",
            stateSnapshot.transactionState == TransactionState.Rendering
                    || stateSnapshot.transactionState == TransactionState.Prepared
                    || stateSnapshot.transactionState == TransactionState.Pending
        )

        for ((i, progress) in progressValues.withIndex()) {
            manualTimeSource.advanceToProgress(progress, durationMs, startTimeMs)
            dispatchManualFrame()
            frames.add(EditorBitmapCapture.captureEditorBitmap())

            val visualSnapshot = captureVisualFrameSnapshot()
            if (i < 4) {
                assertNotNull(
                    "Visual frame snapshot must exist at ${progressLabels[i]} for logical alpha check",
                    visualSnapshot
                )
                for (slice in visualSnapshot!!.sliceVisualStates) {
                    assertTrue(
                        "Slice logical alpha ${slice.currentAlpha} must be in [0,1] at ${progressLabels[i]} for role ${slice.role}",
                        slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
                    )
                }
            }
            visualSnapshots.add(visualSnapshot)
        }

        val finalStateSnapshot = captureAnimationStateSnapshot()
        assertTrue(
            "After 100% frame, transaction must be completed (no active animation state)",
            finalStateSnapshot == null
                    || finalStateSnapshot.transactionState == TransactionState.Completed
        )
        if (finalStateSnapshot != null) {
            assertEquals(
                "After 100% frame, owned resources must be released (ownedResourceCount must be 0)",
                0,
                finalStateSnapshot.ownedResourceCount
            )
        }

        return FiveProgressResult(frames, visualSnapshots)
    }

    private fun assertCursorRegionFromSnapshotRect(frame: CapturedFrame, cursorRect: RectF, messagePrefix: String) {
        val cursorLeft = cursorRect.left.toInt()
        val cursorRight = minOf(cursorRect.right.toInt(), frame.width)
        val cursorTop = cursorRect.top.toInt()
        val cursorBottom = cursorRect.bottom.toInt()
        assertTrue(
            "$messagePrefix: cursor region must be valid: right=$cursorRight > left=$cursorLeft && bottom=$cursorBottom > top=$cursorTop",
            cursorRight > cursorLeft && cursorBottom > cursorTop
        )
        EditorBitmapCapture.assertBitmapRegionHasContent(
            frame, cursorLeft, cursorTop, cursorRight, cursorBottom,
            "$messagePrefix: cursor region should have rendered pixels"
        )
    }

    private fun assertAllFramesHaveContent(frames: List<CapturedFrame>, labels: List<String>) {
        assertEquals("Frame count must match label count", labels.size, frames.size)
        for (i in frames.indices) {
            EditorBitmapCapture.assertBitmapHasContent(
                frames[i],
                "Frame at ${labels[i]} must have rendered content"
            )
        }
    }

    private fun assertContentBoundsProgression(frames: List<CapturedFrame>, labels: List<String>) {
        val bounds = frames.map { it.contentBounds() }
        for (i in bounds.indices) {
            assertTrue(
                "Content at ${labels[i]} should have non-zero width",
                bounds[i].width() > 0
            )
            assertTrue(
                "Content at ${labels[i]} should have non-zero height",
                bounds[i].height() > 0
            )
        }
    }

    private fun assertMidFrameContentRegionHasPixels(frames: List<CapturedFrame>) {
        if (frames.size < 3) return
        val midFrame = frames[2]
        val bounds = midFrame.contentBounds()
        assertTrue("Mid-frame content must have width > 0 for region check", bounds.width() > 0)
        assertTrue("Mid-frame content must have height > 0 for region check", bounds.height() > 0)
        EditorBitmapCapture.assertBitmapRegionHasContent(
            midFrame, bounds.left, bounds.top, bounds.right, bounds.bottom,
            "Mid-frame content region must have non-background pixels"
        )
    }

    private fun assertBackgroundRegionIsEmpty(frames: List<CapturedFrame>) {
        for (frame in frames) {
            val bounds = frame.contentBounds()
            assertTrue(
                "Content must be inset from top-left for background check: left=${bounds.left} > 4 && top=${bounds.top} > 4",
                bounds.left > 4 && bounds.top > 4
            )
            EditorBitmapCapture.assertBitmapRegionIsEmpty(
                frame, 0, 0, bounds.left - 2, bounds.top - 2,
                message = "Top-left corner before content should be background only"
            )
            assertTrue(
                "Content must be inset from bottom-right for background check: right=${bounds.right}+4 < ${frame.width} && bottom=${bounds.bottom}+4 < ${frame.height}",
                bounds.right + 4 < frame.width && bounds.bottom + 4 < frame.height
            )
            val bgRight = minOf(bounds.right + 4, frame.width - 1)
            val bgBottom = minOf(bounds.bottom + 4, frame.height - 1)
            EditorBitmapCapture.assertBitmapRegionIsEmpty(
                frame, bgRight, bgBottom, frame.width, frame.height,
                message = "Bottom-right corner after content should be background only"
            )
        }
    }

    private fun assertContentPixelsVisuallyDistinctFromBackground(frames: List<CapturedFrame>) {
        for (frame in frames) {
            val bounds = frame.contentBounds()
            assertTrue("Frame must have non-zero content bounds for visual distinctness check", bounds.width() > 0 && bounds.height() > 0)
            val coverageCount = frame.countNonBackgroundPixels(bounds.left, bounds.top, bounds.right, bounds.bottom)
            assertTrue(
                "Content region must have non-background pixels: coverageCount=$coverageCount must be > 0",
                coverageCount > 0
            )
            val first = frame.findFirstNonBackgroundPixel()
            assertNotNull("Frame must have content pixel for RGB distance check", first)
            val pixel = frame.bitmap.getPixel(first!!.first, first.second)
            val bg = frame.backgroundColor
            val dr = ColorDistance.red(pixel) - ColorDistance.red(bg)
            val dg = ColorDistance.green(pixel) - ColorDistance.green(bg)
            val db = ColorDistance.blue(pixel) - ColorDistance.blue(bg)
            val rgbDistSq = dr * dr + dg * dg + db * db
            assertTrue(
                "Content pixel must be visually distinct from background: RGB distance²=$rgbDistSq must be > ${ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE} at (${first.first},${first.second})",
                rgbDistSq > ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE
            )
        }
    }

    private fun assertCrossFrameAlphaProgression(frames: List<CapturedFrame>, labels: List<String>) {
        if (frames.size < 3 || labels.size < 3) return
        val perFrameBounds = frames.map { it.contentBounds() }
        for (i in perFrameBounds.indices) {
            assertTrue(
                "Frame at ${labels[i]} must have content for cross-frame check",
                perFrameBounds[i].width() > 0 && perFrameBounds[i].height() > 0
            )
        }

        val unionLeft = perFrameBounds.minOf { it.left }
        val unionTop = perFrameBounds.minOf { it.top }
        val unionRight = perFrameBounds.maxOf { it.right }
        val unionBottom = perFrameBounds.maxOf { it.bottom }
        assertTrue(
            "Union region must have non-zero area for same-region cross-frame comparison",
            unionRight > unionLeft && unionBottom > unionTop
        )

        val countsInUnion = frames.map { frame ->
            frame.countNonBackgroundPixels(unionLeft, unionTop, unionRight, unionBottom)
        }
        val startCount = countsInUnion.first()
        val midCount = countsInUnion[countsInUnion.size / 2]
        val endCount = countsInUnion.last()
        assertTrue(
            "Cross-frame pixel count in same region must vary: start=$startCount, mid=$midCount, end=$endCount at ${labels[0]}/${labels[frames.size / 2]}/${labels[frames.size - 1]} — animation should change coverage",
            !(startCount == midCount && midCount == endCount) || startCount == 0
        )

        val rgbDistancesInUnion = frames.mapIndexed { i, frame ->
            val first = frame.findFirstNonBackgroundPixel()
            if (first != null && first.first in unionLeft until unionRight && first.second in unionTop until unionBottom) {
                val pixel = frame.bitmap.getPixel(first.first, first.second)
                val bg = frame.backgroundColor
                val dr = ColorDistance.red(pixel) - ColorDistance.red(bg)
                val dg = ColorDistance.green(pixel) - ColorDistance.green(bg)
                val db = ColorDistance.blue(pixel) - ColorDistance.blue(bg)
                dr * dr + dg * dg + db * db
            } else {
                0
            }
        }
        for (i in rgbDistancesInUnion.indices) {
            assertTrue(
                "Frame at ${labels[i]} content pixel must be visually distinct from background in same region: RGB distance²=${rgbDistancesInUnion[i]}",
                rgbDistancesInUnion[i] > ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE
            )
        }
    }

    private fun assertCrossFrameLogicalAlphaProgression(
        visualSnapshots: List<VisualFrameSnapshot?>,
        labels: List<String>
    ) {
        val nonNullSnapshots = visualSnapshots.mapIndexedNotNull { i, snapshot ->
            if (snapshot != null) Pair(i, snapshot) else null
        }
        if (nonNullSnapshots.size < 2 || labels.size < 2) return
        val allAlphas = nonNullSnapshots.map { (i, snapshot) ->
            val alphas = snapshot.sliceVisualStates.map { it.currentAlpha }
            for (slice in snapshot.sliceVisualStates) {
                assertTrue(
                    "Slice logical alpha ${slice.currentAlpha} must be in [0,1] at ${labels[i]} for role ${slice.role}",
                    slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
                )
            }
            alphas
        }
        val hasAlphaVariation = allAlphas.flatMap { it }.distinct().size > 1
        assertTrue(
            "Cross-frame logical alpha must vary across frames — animation should change slice transparency",
            hasAlphaVariation
        )
    }

    private fun assertDeleteProgressionWidthShrinks(frames: List<CapturedFrame>) {
        val startBounds = frames[0].contentBounds()
        val endBounds = frames[4].contentBounds()
        assertTrue(
            "Delete animation: end width ${endBounds.width()} should be <= start width ${startBounds.width()}",
            endBounds.width() <= startBounds.width() + 2
        )
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
