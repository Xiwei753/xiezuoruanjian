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
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import com.xiwei.sujian.editor.v2.visual.TransactionState
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

    private fun advanceToProgressAndVerify(
        expectedMinProgress: Float,
        expectedMaxProgress: Float,
        expectedText: String,
        verifyCursorVisible: Boolean = true
    ) {
        manualTimeSource.advanceByMs(16)
        dispatchManualFrame()
        val snapshot = captureEditorSnapshot()
        if (snapshot != null) {
            assertTrue(
                "Animation progress ${snapshot.progress} should be >= $expectedMinProgress",
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
        if (endSnapshot != null) {
            assertTrue("End progress should be >= 1.0", endSnapshot.progress >= 1.0f)
        }

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
}
