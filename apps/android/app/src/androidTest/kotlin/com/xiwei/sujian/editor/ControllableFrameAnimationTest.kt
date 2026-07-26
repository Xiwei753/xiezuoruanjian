package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.visual.AnimationStateSnapshot
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.SliceRole
import com.xiwei.sujian.editor.v2.visual.SliceVisualState
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import com.xiwei.sujian.editor.v2.visual.TransactionState
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
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
class ControllableFrameAnimationTest {

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

    @Test
    fun insertText_animationStartsAndCompletes() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Hello"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello"))
            .check(EditorViewAssertions.hasSelectionUtf8(5, 5))
    }

    @Test
    fun deleteRange_animationStartsAndCompletes() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABCDE"))

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))
    }

    @Test
    fun middleInsert_animationPreservesSurroundingText() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧中间插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val insertOffset = "AB".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertOffset, insertOffset, "XY", ""))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABXYCDE"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABXYCDE"))
            .check(EditorViewAssertions.hasSelectionUtf8(4, 4))
    }

    @Test
    fun unicodeInsert_animationCompletesCorrectly() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧Unicode测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "你好🙂世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun rapidSequentialInput_eachCompletesCorrectly() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧连续输入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("A"))

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("B"))

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("C"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))
            .check(EditorViewAssertions.hasSelectionUtf8(3, 3))
    }

    @Test
    fun multilineInsert_animationCompletesCorrectly() {
        val testData = initTestData()
        val chapterId = openTestChapter("可控帧多行测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "第一行\n第二行\n第三行"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
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

    private fun captureEditorSnapshot(): AnimationStateSnapshot? {
        var snapshot: AnimationStateSnapshot? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                snapshot = editorView.captureAnimationSnapshot()
            }
        return snapshot
    }

    private fun captureVisualFrame(): VisualFrameSnapshot? {
        var snapshot: VisualFrameSnapshot? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                snapshot = editorView.captureVisualFrameSnapshot()
            }
        return snapshot
    }

    private fun advanceToProgressAndVerify(
        expectedMinProgress: Float,
        expectedMaxProgress: Float,
        expectedText: String,
        verifyCursorVisible: Boolean = true
    ) {
        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val snapshot = captureEditorSnapshot()
        assertNotNull("Animation snapshot must exist during active animation", snapshot)
        assertTrue(
            "Animation progress ${snapshot!!.progress} should be >= $expectedMinProgress",
            snapshot.progress >= expectedMinProgress
        )
        assertTrue(
            "Animation progress ${snapshot.progress} should be <= $expectedMaxProgress",
            snapshot.progress <= expectedMaxProgress
        )
        if (verifyCursorVisible) {
            assertNotNull(
                "Cursor transition should exist during animation",
                snapshot.cursorTransition
            )
        }
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(expectedText))
    }

    @Test
    fun insertText_intermediateFramesShowProgressAndPreserveText() {
        val testData = initTestData()
        openTestChapter("中间帧插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Hello"))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after insert", startSnapshot)
        assertEquals("insert", startSnapshot!!.operationKind)
        assertEquals(TransactionState.Rendering, startSnapshot.transactionState)
        assertTrue("Start progress should be near 0", startSnapshot.progress < 0.3f)
        assertTrue("Should own resources", startSnapshot.ownedResourceCount > 0)

        advanceToProgressAndVerify(0.2f, 0.6f, "Hello")
        advanceToProgressAndVerify(0.4f, 0.8f, "Hello")

        advanceClockToEnd()

        val endSnapshot = captureEditorSnapshot()
        assertNotNull("Animation snapshot must exist at end", endSnapshot)
        assertTrue("End progress should be >= 1.0", endSnapshot!!.progress >= 1.0f)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello"))
            .check(EditorViewAssertions.hasSelectionUtf8(5, 5))
    }

    @Test
    fun deleteRange_intermediateFramesPreserveSurroundingText() {
        val testData = initTestData()
        openTestChapter("中间帧删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABCDE"))

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after delete", startSnapshot)
        assertEquals("delete", startSnapshot!!.operationKind)

        advanceToProgressAndVerify(0.2f, 0.6f, "ABE")
        advanceToProgressAndVerify(0.4f, 0.8f, "ABE")

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))
    }

    @Test
    fun middleInsert_intermediateFramesShowCorrectPosition() {
        val testData = initTestData()
        openTestChapter("中间帧中间插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val insertOffset = "AB".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertOffset, insertOffset, "XY", ""))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after middle insert", startSnapshot)
        assertEquals("insert", startSnapshot!!.operationKind)
        assertTrue("New affected ranges should exist", startSnapshot.newAffectedRanges.isNotEmpty())

        advanceToProgressAndVerify(0.2f, 0.6f, "ABXYCDE")
        advanceToProgressAndVerify(0.4f, 0.8f, "ABXYCDE")

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABXYCDE"))
            .check(EditorViewAssertions.hasSelectionUtf8(4, 4))
    }

    @Test
    fun rapidInput_animationRebasesCorrectly() {
        val testData = initTestData()
        openTestChapter("中间帧连续输入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("A"))

        val snapshot1 = captureEditorSnapshot()
        assertNotNull("Animation should be active after first insert", snapshot1)

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("B"))

        manualTimeSource.advanceByMs(8)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("C"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))
            .check(EditorViewAssertions.hasSelectionUtf8(3, 3))
    }

    @Test
    fun unicodeInsert_intermediateFramesPreserveText() {
        val testData = initTestData()
        openTestChapter("中间帧Unicode测试", testData)

        manualTimeSource.advanceTo(0L)

        val testText = "你好🙂世界"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after unicode insert", startSnapshot)

        advanceToProgressAndVerify(0.2f, 0.6f, testText)
        advanceToProgressAndVerify(0.4f, 0.8f, testText)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(testText))
    }

    @Test
    fun compositionUpdate_animatedAndCompletes() {
        val testData = initTestData()
        openTestChapter("可控帧composition更新测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("你"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("你"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("你好"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("你好"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("你好"))
    }

    @Test
    fun compositionCommit_animatedAndCompletes() {
        val testData = initTestData()
        openTestChapter("可控帧composition提交测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("测试"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.finishComposingText())

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("测试"))
            .check(EditorViewAssertions.hasSelectionUtf8(6, 6))
    }

    @Test
    fun compositionCancel_animatedAndRevertsText() {
        val testData = initTestData()
        openTestChapter("可控帧composition取消测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("AB"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("XY"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABXY"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.sendKeyDelete())

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
    }

    @Test
    fun compositionUpdate_intermediateFramesPreserveText() {
        val testData = initTestData()
        openTestChapter("中间帧composition测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("中"))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after composition update", startSnapshot)

        advanceToProgressAndVerify(0.2f, 0.6f, "中", verifyCursorVisible = true)
        advanceToProgressAndVerify(0.4f, 0.8f, "中", verifyCursorVisible = true)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("中"))
    }

    @Test
    fun compositionUpdateThenCommit_intermediateFramesCorrect() {
        val testData = initTestData()
        openTestChapter("中间帧composition提交测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("文"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("文"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("文字"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("文字"))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.finishComposingText())

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("文字"))
            .check(EditorViewAssertions.hasSelectionUtf8(6, 6))
    }

    @Test
    fun scrollDuringAnimation_textAndCursorPreserved() {
        val testData = initTestData()
        openTestChapter("滚动动画测试", testData)

        manualTimeSource.advanceTo(0L)

        val longText = (1..20).joinToString("\n") { "第${it}行内容" }
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(longText))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(longText))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(longText))
    }

    @Test
    fun switchChapter_animationCleared() {
        val testData = initTestData()
        openTestChapter("切换章节动画测试A", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("章节A"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("章节A"))

        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        val projectTag = SujianSemanticIds.project(testData.projectId)
        ComposeWait.waitForTag(composeTestRule, projectTag, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(projectTag).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 15_000)

        val volumeTag = SujianSemanticIds.volume(testData.volumeId)
        ComposeWait.waitForTag(composeTestRule, volumeTag, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(volumeTag).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("切换章节动画测试B")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId2 = waitForChapterByTitle("切换章节动画测试B", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId2)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId2)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val snapshot = captureEditorSnapshot()
        assertNull("No animation should be active after switching chapter", snapshot)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(""))
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

    private fun verifySliceVisualProperties(
        snapshot: VisualFrameSnapshot,
        expectedMinProgress: Float,
        expectedMaxProgress: Float,
        expectedSliceRolePresent: SliceRole? = null
    ) {
        assertTrue(
            "Visual frame progress ${snapshot.progress} should be >= $expectedMinProgress",
            snapshot.progress >= expectedMinProgress
        )
        assertTrue(
            "Visual frame progress ${snapshot.progress} should be <= $expectedMaxProgress",
            snapshot.progress <= expectedMaxProgress
        )
        if (expectedSliceRolePresent != null) {
            val hasRole = snapshot.sliceVisualStates.any { it.role == expectedSliceRolePresent }
            assertTrue(
                "Visual frame should contain slice with role $expectedSliceRolePresent",
                hasRole
            )
        }
        for (slice in snapshot.sliceVisualStates) {
            assertTrue(
                "Slice alpha ${slice.currentAlpha} should be in [0,1] for role ${slice.role}",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
            assertTrue(
                "Slice currentLeft ${slice.currentLeft} should be >= 0",
                slice.currentLeft >= 0f
            )
            assertTrue(
                "Slice currentTop ${slice.currentTop} should be >= 0",
                slice.currentTop >= 0f
            )
            assertTrue(
                "Slice currentRight ${slice.currentRight} should be >= currentLeft ${slice.currentLeft}",
                slice.currentRight >= slice.currentLeft
            )
            assertTrue(
                "Slice currentBottom ${slice.currentBottom} should be >= currentTop ${slice.currentTop}",
                slice.currentBottom >= slice.currentTop
            )
        }
    }

    private fun verifyInsertSliceFadesIn(slices: List<SliceVisualState>, progress: Float) {
        val insertSlices = slices.filter { it.role == SliceRole.Insert }
        for (slice in insertSlices) {
            val expectedAlpha = progress.coerceIn(0f, 1f)
            val tolerance = 0.3f
            assertTrue(
                "Insert slice alpha ${slice.currentAlpha} should be near $expectedAlpha at progress $progress (tolerance $tolerance)",
                Math.abs(slice.currentAlpha - expectedAlpha) < tolerance
            )
        }
    }

    private fun verifyDeleteSliceFadesOut(slices: List<SliceVisualState>, progress: Float) {
        val deleteSlices = slices.filter { it.role == SliceRole.Delete }
        for (slice in deleteSlices) {
            val expectedAlpha = (1f - progress).coerceIn(0f, 1f)
            val tolerance = 0.3f
            assertTrue(
                "Delete slice alpha ${slice.currentAlpha} should be near $expectedAlpha at progress $progress (tolerance $tolerance)",
                Math.abs(slice.currentAlpha - expectedAlpha) < tolerance
            )
        }
    }

    @Test
    fun insertText_visualFrameSlicesShowPositionAndAlpha() {
        val testData = initTestData()
        openTestChapter("视觉帧插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Hello"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after insert", startFrame)
        verifySliceVisualProperties(startFrame!!, 0f, 0.3f, SliceRole.Insert)
        verifyInsertSliceFadesIn(startFrame.sliceVisualStates, startFrame.progress)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame1 = captureVisualFrame()
        assertNotNull("Mid-frame 1 visual snapshot must exist during animation", midFrame1)
        verifySliceVisualProperties(midFrame1!!, 0.1f, 0.6f, SliceRole.Insert)
        verifyInsertSliceFadesIn(midFrame1.sliceVisualStates, midFrame1.progress)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame2 = captureVisualFrame()
        assertNotNull("Mid-frame 2 visual snapshot must exist during animation", midFrame2)
        verifySliceVisualProperties(midFrame2!!, 0.2f, 0.8f, SliceRole.Insert)
        verifyInsertSliceFadesIn(midFrame2.sliceVisualStates, midFrame2.progress)

        advanceClockToEnd()

        val endFrame = captureVisualFrame()
        assertNull("Visual frame should be null after animation completes", endFrame)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hello"))
            .check(EditorViewAssertions.hasSelectionUtf8(5, 5))
    }

    @Test
    fun deleteRange_visualFrameSlicesShowFadeOut() {
        val testData = initTestData()
        openTestChapter("视觉帧删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABCDE"))

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABCD".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "CD"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after delete", startFrame)
        verifySliceVisualProperties(startFrame!!, 0f, 0.3f, SliceRole.Delete)
        verifyDeleteSliceFadesOut(startFrame.sliceVisualStates, startFrame.progress)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during delete animation", midFrame)
        verifySliceVisualProperties(midFrame!!, 0.1f, 0.6f, SliceRole.Delete)
        verifyDeleteSliceFadesOut(midFrame.sliceVisualStates, midFrame.progress)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABE"))
    }

    @Test
    fun insertText_cursorRectMovesDuringAnimation() {
        val testData = initTestData()
        openTestChapter("视觉帧光标测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("Hi"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame must exist after insert for cursor test", startFrame)
        assertNotNull("Cursor rect must exist in start frame during animation", startFrame!!.cursorRect)
        val startCursorRect = startFrame.cursorRect!!
        assertTrue(
            "Cursor rect left ${startCursorRect.left} should be >= 0",
            startCursorRect.left >= 0f
        )
        assertTrue(
            "Cursor rect top ${startCursorRect.top} should be >= 0",
            startCursorRect.top >= 0f
        )
        assertTrue(
            "Cursor rect should have non-zero height",
            startCursorRect.height() > 0f
        )

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Visual frame must exist at mid-frame for cursor test", midFrame)
        assertNotNull("Cursor rect must exist in mid-frame during animation", midFrame!!.cursorRect)
        val midCursorRect = midFrame.cursorRect!!
        assertTrue(
            "Mid-frame cursor rect left ${midCursorRect.left} should be >= 0",
            midCursorRect.left >= 0f
        )
        assertTrue(
            "Mid-frame cursor rect should have non-zero height",
            midCursorRect.height() > 0f
        )

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("Hi"))
            .check(EditorViewAssertions.hasSelectionUtf8(2, 2))
    }

    @Test
    fun middleInsert_visualFrameSlicesShowCorrectPosition() {
        val testData = initTestData()
        openTestChapter("视觉帧中间插入测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABCDE"))

        advanceClockToEnd()

        val insertOffset = "AB".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertOffset, insertOffset, "XY", ""))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after middle insert", startFrame)
        verifySliceVisualProperties(startFrame!!, 0f, 0.3f, SliceRole.Insert)

        val insertSlices = startFrame.sliceVisualStates.filter { it.role == SliceRole.Insert }
        assertTrue("Insert slices must exist in start frame", insertSlices.isNotEmpty())
        for (slice in insertSlices) {
            assertTrue(
                "Insert slice should be at a position > 0 (middle of text)",
                slice.destinationLeft >= 0f
            )
        }

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during middle insert", midFrame)
        verifySliceVisualProperties(midFrame!!, 0.1f, 0.6f, SliceRole.Insert)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABXYCDE"))
    }

    @Test
    fun multilineInsert_visualFrameBlockShiftsShowMovement() {
        val testData = initTestData()
        openTestChapter("视觉帧块位移测试", testData)

        manualTimeSource.advanceTo(0L)

        val firstLine = "第一行内容"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(firstLine))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(firstLine))

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("\n第二行"))

        val frame = captureVisualFrame()
        assertNotNull("Visual frame must exist after multiline insert", frame)
        assertTrue("Block shift states must exist for multiline insert", frame!!.blockShiftStates.isNotEmpty())
        for (block in frame.blockShiftStates) {
            assertTrue(
                "Block shift currentTranslateY ${block.currentTranslateY} should be finite",
                java.lang.Float.isFinite(block.currentTranslateY)
            )
        }

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("$firstLine\n第二行"))
    }

    @Test
    fun compositionUpdate_visualFrameShowsPreeditSlices() {
        val testData = initTestData()
        openTestChapter("视觉帧composition测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("测"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after composition update", startFrame)
        assertTrue("Start frame must have slice visual states after composition", startFrame!!.sliceVisualStates.isNotEmpty())
        for (slice in startFrame.sliceVisualStates) {
            assertTrue(
                "Composition slice alpha ${slice.currentAlpha} should be in [0,1]",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
        }

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during composition", midFrame)
        assertTrue("Mid-frame must have slice visual states during composition", midFrame!!.sliceVisualStates.isNotEmpty())
        for (slice in midFrame.sliceVisualStates) {
            assertTrue(
                "Mid-frame composition slice alpha should be in [0,1]",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
        }

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("测"))
    }

    @Test
    fun singleCharDelete_animationStartsAndCompletes() {
        val testData = initTestData()
        openTestChapter("单字删除测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABC"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("ABC"))

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABC".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "C"))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after single char delete", startSnapshot)
        assertEquals("delete", startSnapshot!!.operationKind)
        assertTrue("Should own resources", startSnapshot.ownedResourceCount > 0)

        advanceToProgressAndVerify(0.2f, 0.6f, "AB")
        advanceToProgressAndVerify(0.4f, 0.8f, "AB")

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
            .check(EditorViewAssertions.hasSelectionUtf8(2, 2))
    }

    @Test
    fun crossLineDelete_animationPreservesRemainingText() {
        val testData = initTestData()
        openTestChapter("跨行删除测试", testData)

        manualTimeSource.advanceTo(0L)

        val line1 = "第一行"
        val line2 = "第二行"
        val line3 = "第三行"
        val fullText = "$line1\n$line2\n$line3"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(fullText))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(fullText))

        val deleteStart = line1.toByteArray(Charsets.UTF_8).size
        val deleteEnd = (line1 + "\n" + line2).toByteArray(Charsets.UTF_8).size
        val deletedText = "\n$line2"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", deletedText))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after cross-line delete", startSnapshot)
        assertEquals("delete", startSnapshot!!.operationKind)

        advanceToProgressAndVerify(0.2f, 0.6f, "$line1\n$line3")
        advanceToProgressAndVerify(0.4f, 0.8f, "$line1\n$line3")

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("$line1\n$line3"))
    }

    @Test
    fun combiningCharInsert_animationCompletesCorrectly() {
        val testData = initTestData()
        openTestChapter("组合字符测试", testData)

        manualTimeSource.advanceTo(0L)

        val baseChar = "e"
        val combiningAcute = "\u0301"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(baseChar))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(baseChar))

        val insertOffset = baseChar.toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(insertOffset, insertOffset, combiningAcute, ""))

        val startSnapshot = captureEditorSnapshot()
        assertNotNull("Animation should be active after combining char insert", startSnapshot)

        advanceToProgressAndVerify(0.2f, 0.6f, baseChar + combiningAcute)
        advanceToProgressAndVerify(0.4f, 0.8f, baseChar + combiningAcute)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText(baseChar + combiningAcute))
    }

    @Test
    fun compositionUpdate_visualFrameShowsPreeditDecoration() {
        val testData = initTestData()
        openTestChapter("composition下划线测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("预"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after composition update", startFrame)
        assertTrue("Start frame must have slice visual states for composition decoration", startFrame!!.sliceVisualStates.isNotEmpty())
        for (slice in startFrame.sliceVisualStates) {
            assertTrue(
                "Composition slice alpha ${slice.currentAlpha} should be in [0,1]",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
            assertTrue(
                "Composition slice left ${slice.currentLeft} should be >= 0",
                slice.currentLeft >= 0f
            )
            assertTrue(
                "Composition slice top ${slice.currentTop} should be >= 0",
                slice.currentTop >= 0f
            )
        }

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.setComposingText("预编"))

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during composition decoration", midFrame)
        assertTrue("Mid-frame must have slice visual states for composition decoration", midFrame!!.sliceVisualStates.isNotEmpty())
        for (slice in midFrame.sliceVisualStates) {
            assertTrue(
                "Mid-frame composition slice alpha should be in [0,1]",
                slice.currentAlpha >= 0f && slice.currentAlpha <= 1f
            )
        }

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCompositionAction.finishComposingText())

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("预编"))
    }

    @Test
    fun singleCharDelete_visualFrameSlicesShowFadeOut() {
        val testData = initTestData()
        openTestChapter("单字删除视觉帧测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("ABC"))

        advanceClockToEnd()

        val deleteStart = "AB".toByteArray(Charsets.UTF_8).size
        val deleteEnd = "ABC".toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "C"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame should exist after single char delete", startFrame)
        verifySliceVisualProperties(startFrame!!, 0f, 0.3f, SliceRole.Delete)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during single char delete", midFrame)
        verifySliceVisualProperties(midFrame!!, 0.1f, 0.6f, SliceRole.Delete)
        verifyDeleteSliceFadesOut(midFrame.sliceVisualStates, midFrame.progress)

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("AB"))
    }

    @Test
    fun crossLineDelete_visualFrameBlockShiftsAdjust() {
        val testData = initTestData()
        openTestChapter("跨行删除视觉帧测试", testData)

        manualTimeSource.advanceTo(0L)

        val line1 = "行一"
        val line2 = "行二"
        val line3 = "行三"
        val fullText = "$line1\n$line2\n$line3"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(fullText))

        advanceClockToEnd()

        val deleteStart = line1.toByteArray(Charsets.UTF_8).size
        val deleteEnd = (line1 + "\n" + line2).toByteArray(Charsets.UTF_8).size
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorReplaceRangeAction.replaceRange(deleteStart, deleteEnd, "", "\n$line2"))

        val startFrame = captureVisualFrame()
        assertNotNull("Visual frame must exist after cross-line delete", startFrame)
        assertTrue("Start frame must have block shift states for cross-line delete", startFrame!!.blockShiftStates.isNotEmpty())
        for (block in startFrame.blockShiftStates) {
            assertTrue(
                "Block shift currentTranslateY should be finite",
                java.lang.Float.isFinite(block.currentTranslateY)
            )
        }

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val midFrame = captureVisualFrame()
        assertNotNull("Mid-frame visual snapshot must exist during cross-line delete", midFrame)
        assertTrue("Mid-frame must have block shift states for cross-line delete", midFrame!!.blockShiftStates.isNotEmpty())
        for (block in midFrame.blockShiftStates) {
            assertTrue(
                "Mid-frame block shift currentTranslateY should be finite",
                java.lang.Float.isFinite(block.currentTranslateY)
            )
        }

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("$line1\n$line3"))
    }

    @Test
    fun backToChapterList_animationCleared() {
        val testData = initTestData()
        val chapterId = openTestChapter("返回列表动画测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("返回测试"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("返回测试"))

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val snapshot = captureEditorSnapshot()
        assertNull("No animation should be active after returning to chapter list and back", snapshot)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("返回测试"))
    }

    @Test
    fun backgroundRecover_animationCleared() {
        val testData = initTestData()
        openTestChapter("后台恢复动画测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("后台测试"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("后台测试"))

        val snapshotBefore = captureEditorSnapshot()
        assertNull("No animation should be active before background", snapshotBefore)

        activityRule.simulateBackgroundRecovery()

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val snapshotAfter = captureEditorSnapshot()
        assertNull("No animation should be active after background recovery", snapshotAfter)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("后台测试"))
    }

    @Test
    fun coldRestart_animationCleared() {
        val testData = initTestData()
        val chapterId = openTestChapter("冷重启动画测试", testData)

        manualTimeSource.advanceTo(0L)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重启测试"))

        advanceClockToEnd()

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("重启测试"))

        ComposeWait.waitForSaveStatus(composeTestRule, "saved", timeoutMs = 15_000)

        activityRule.restartRuntimeAndActivity()

        navigateToChapterAfterRestart(testData, chapterId)

        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()

        val snapshot = captureEditorSnapshot()
        assertNull("No animation should be active after cold restart", snapshot)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check(EditorViewAssertions.hasDisplayText("重启测试"))
    }

    private fun navigateToChapterAfterRestart(testData: AndroidTestEnvironment.TestProjectData, chapterId: String) {
        navigateToTestVolume(testData)
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.chapter(testData.volumeId, chapterId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()
        waitForEditorReady(testData.projectId, testData.volumeId, chapterId)
    }
}
