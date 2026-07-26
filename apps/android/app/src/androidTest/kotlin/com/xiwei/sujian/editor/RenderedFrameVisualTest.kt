package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
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

        val frames = captureFiveProgressPoints()

        assertAllFramesHaveContent(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertContentBoundsProgression(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentAlphaIsOpaque(frames)

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

        val frames = captureFiveProgressPoints()

        assertAllFramesHaveContent(frames, listOf("start", "25%", "50%", "75%", "end"))

        val endBounds = frames[4].contentBounds()
        assertTrue(
            "After delete, content width should shrink: end ${endBounds.width()} <= before ${beforeBounds.width()}",
            endBounds.width() <= beforeBounds.width() + 1
        )

        assertDeleteProgressionWidthShrinks(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentAlphaIsOpaque(frames)

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

        val frames = captureFiveProgressPoints()

        assertAllFramesHaveContent(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertContentBoundsProgression(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentAlphaIsOpaque(frames)

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

        val frames = captureFiveProgressPoints()

        assertAllFramesHaveContent(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertContentBoundsProgression(frames, listOf("start", "25%", "50%", "75%", "end"))
        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentAlphaIsOpaque(frames)

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

        val frames = captureFiveProgressPoints()

        assertAllFramesHaveContent(frames, listOf("start", "25%", "50%", "75%", "end"))

        for ((i, label) in listOf("start", "25%", "50%", "75%", "end").withIndex()) {
            val bounds = frames[i].contentBounds()
            assertTrue("Multiline at $label should have non-zero height", bounds.height() > 0)
            assertTrue("Multiline at $label should have non-zero width", bounds.width() > 0)
        }

        assertMidFrameContentRegionHasPixels(frames)
        assertBackgroundRegionIsEmpty(frames)
        assertContentAlphaIsOpaque(frames)

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

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val frame2 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame2, "Frame after 'AB' start must have content")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val frame3 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame3, "Frame at 25% must have content")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val frame4 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame4, "Frame at 50% must have content")

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val frame5 = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(frame5, "Frame at 75% must have content")

        advanceClockToEnd()
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
        assertContentAlphaIsOpaque(frames)
        assertBackgroundRegionIsEmpty(frames)

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

        val bounds = startFrame.contentBounds()
        assertTrue("Content must exist for cursor check", bounds.width() > 0 && bounds.height() > 0)

        val cursorRegionLeft = bounds.right
        val cursorRegionRight = minOf(cursorRegionLeft + 20, startFrame.width)
        val cursorRegionTop = bounds.top
        val cursorRegionBottom = bounds.bottom
        assertTrue(
            "Cursor region must be valid for pixel check: right=$cursorRegionRight > left=$cursorRegionLeft && bottom=$cursorRegionBottom > top=$cursorRegionTop",
            cursorRegionRight > cursorRegionLeft && cursorRegionBottom > cursorRegionTop
        )
        EditorBitmapCapture.assertBitmapRegionHasContent(
            startFrame, cursorRegionLeft, cursorRegionTop, cursorRegionRight, cursorRegionBottom,
            "Cursor region right after content should have rendered pixels (cursor line)"
        )

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have content during animation")

        val midBounds = midFrame.contentBounds()
        val midCursorLeft = midBounds.right
        val midCursorRight = minOf(midCursorLeft + 20, midFrame.width)
        assertTrue(
            "Mid-frame cursor region must be valid: right=$midCursorRight > left=$midCursorLeft && bottom=${midBounds.bottom} > top=${midBounds.top}",
            midCursorRight > midCursorLeft && midBounds.bottom > midBounds.top
        )
        EditorBitmapCapture.assertBitmapRegionHasContent(
            midFrame, midCursorLeft, midBounds.top, midCursorRight, midBounds.bottom,
            "Cursor region at mid-frame should have rendered pixels"
        )

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

        val bounds = frame.contentBounds()
        assertTrue("Content must have width > 0", bounds.width() > 0)
        assertTrue("Content must have height > 0", bounds.height() > 0)

        val cursorX = bounds.right + 2
        val cursorY = (bounds.top + bounds.bottom) / 2
        assertTrue(
            "Cursor position must be within bitmap: x=$cursorX < ${frame.width} && y=$cursorY < ${frame.height}",
            cursorX < frame.width && cursorY < frame.height
        )
        assertTrue(
            "Cursor pixel at ($cursorX, $cursorY) must be non-background (cursor drawn above text)",
            frame.isPixelNonBackground(cursorX, cursorY)
        )

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        val midFrame = EditorBitmapCapture.captureEditorBitmap()
        EditorBitmapCapture.assertBitmapHasContent(midFrame, "Mid-frame must have content for cursor layer check")

        val midBounds = midFrame.contentBounds()
        val midCursorX = midBounds.right + 2
        val midCursorY = (midBounds.top + midBounds.bottom) / 2
        assertTrue(
            "Mid-frame cursor position must be within bitmap: x=$midCursorX < ${midFrame.width} && y=$midCursorY < ${midFrame.height}",
            midCursorX < midFrame.width && midCursorY < midFrame.height
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

        val frames = captureFiveProgressPoints()

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
            val alpha = frame.alpha(centerX, centerY)
            assertTrue(
                "Content center alpha at $label must be opaque (>200), got $alpha (background not covering text)",
                alpha > 200
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Test"))
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

        val midBounds = midFrame.contentBounds()
        assertTrue("Delete mid-frame content must have width > 0", midBounds.width() > 0)
        val midCursorX = midBounds.right + 2
        val midCursorY = (midBounds.top + midBounds.bottom) / 2
        assertTrue(
            "Delete mid-frame cursor position must be within bitmap",
            midCursorX < midFrame.width && midCursorY < midFrame.height
        )
        assertTrue(
            "Cursor must remain visible during delete animation at mid-frame",
            midFrame.isPixelNonBackground(midCursorX, midCursorY)
        )

        advanceClockToEnd()
    }

    private fun captureFiveProgressPoints(): List<CapturedFrame> {
        val frames = mutableListOf<CapturedFrame>()

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())

        manualTimeSource.advanceByMs(50)
        dispatchManualFrame()
        frames.add(EditorBitmapCapture.captureEditorBitmap())

        advanceClockToEnd()
        frames.add(EditorBitmapCapture.captureEditorBitmap())

        return frames
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
            EditorBitmapCapture.assertBitmapRegionIsEmpty(
                frame, bounds.right + 2, bounds.bottom + 2, frame.width, frame.height,
                message = "Bottom-right corner after content should be background only"
            )
        }
    }

    private fun assertContentAlphaIsOpaque(frames: List<CapturedFrame>) {
        for (frame in frames) {
            val first = frame.findFirstNonBackgroundPixel()
            assertNotNull("Frame must have content pixel for alpha check", first)
            val alpha = frame.alpha(first!!.first, first.second)
            assertTrue(
                "Content pixel alpha should be opaque (>200), got $alpha at (${first.first},${first.second})",
                alpha > 200
            )
        }
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
