package com.xiwei.sujian.feature.editor

import android.content.Context
import android.os.SystemClock
import android.provider.Settings
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.InputMethodManager
import androidx.compose.ui.test.junit4.v2.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.feature.editor.host.SujianEditorView
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorViewAssertions
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.testime.TestImeCommands
import org.hamcrest.Matcher
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

/**
 * Full-stack IME instrumented tests (issue #589).
 *
 * These tests exercise the REAL system input path:
 * InputMethodService (the deterministic test IME from the androidTest APK, set
 * as the emulator's default IME by the CI workflow / local setup) -> system
 * InputMethodManager binding -> InputConnection -> AndroidInputAdapter -> Rust
 * kernel -> editor UI.
 *
 * The test IME is passive: it never echoes, never recorrects and never reacts
 * to updateSelection. Each command is sent explicitly through
 * [TestImeCommands] (startService with the test IME component) and the test
 * then polls the REAL editor UI state (display text, selection) with
 * ComposeWait.waitUntil — no sleeps, no lengthened timeouts to mask failures.
 *
 * Selection observations follow the real product behavior:
 * - During an active composition the editor's reported selection stays at the
 *   committed-text boundary (the preedit cursor lives in the adapter/kernel
 *   composition state); it only moves when the composition is committed,
 *   finished or cancelled.
 * - After commit/finish the selection is the kernel-computed result (UTF-8
 *   byte offsets, as exposed by SujianEditorView).
 *
 * The direct InputConnection-driving helpers (EditorCommitTextAction /
 * EditorCompositionAction) are intentionally NOT used here: they bypass the
 * IME binding path and are reserved for the rendering/animation tests.
 */
@RunWith(AndroidJUnit4::class)
class EditorImeInputTest {
    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule =
        AndroidComposeTestRule(
            activityRule,
            activityProvider = activityRule.composeActivityProvider,
        ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain =
        RuleChain
            .outerRule(AndroidTestEnvironment.TestDependenciesRule())
            .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private val instrumentation get() = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation()

    // ------------------------------------------------------------------
    // Editor open + IME binding helpers
    // ------------------------------------------------------------------

    /**
     * Opens a chapter and waits until the test IME is bound to the editor.
     * The editor requests focus + showSoftInput when a chapter opens; the tap
     * additionally walks the real touch path (handleTap -> setSelectionTyped +
     * showSoftInput). `InputMethodManager.isActive(view)` is true only after
     * the system started input on that view, i.e. the default IME (the
     * deterministic test IME) was bound and holds the editor's
     * InputConnection. Also sanity-checks that the default IME really is the
     * test IME so a broken CI/local setup fails fast with a clear message.
     */
    private fun openEditorAndBindTestIme(chapterTitle: String): String {
        val testData = AndroidTestEnvironment.ensureTestProjectAndVolume(instrumentation.targetContext)
        val chapterId = openTestChapter(chapterTitle, testData)
        waitForEditorEmpty()

        tapEditorCenter()

        waitForTestImeBoundToEditor()
        return chapterId
    }

    private fun tapEditorCenter() {
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(
                object : ViewAction {
                    override fun getConstraints(): Matcher<View> = ViewMatchers.isDisplayed()

                    override fun getDescription(): String = "Tap the editor center"

                    override fun perform(
                        uiController: UiController,
                        view: View,
                    ) {
                        val x = view.width / 2f
                        val y = view.height / 2f
                        val downTime = SystemClock.uptimeMillis()
                        view.dispatchTouchEvent(
                            MotionEvent.obtain(downTime, downTime, MotionEvent.ACTION_DOWN, x, y, 0),
                        )
                        view.dispatchTouchEvent(
                            MotionEvent.obtain(downTime, downTime + 50, MotionEvent.ACTION_UP, x, y, 0),
                        )
                        uiController.loopMainThreadUntilIdle()
                    }
                },
            )
    }

    private fun waitForTestImeBoundToEditor(timeoutMs: Long = 20_000) {
        var lastDiagnostic = "not polled yet"
        ComposeWait.waitUntil(
            composeTestRule,
            {
                var bound = false
                try {
                    activityRule.onActivity { activity ->
                        val defaultIme =
                            Settings.Secure.getString(
                                activity.contentResolver,
                                Settings.Secure.DEFAULT_INPUT_METHOD,
                            ) ?: ""
                        if (defaultIme != TestImeCommands.IME_COMPONENT) {
                            lastDiagnostic =
                                "default_input_method=$defaultIme (expected ${TestImeCommands.IME_COMPONENT})"
                            bound = false
                            return@onActivity
                        }
                        val imm = activity.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
                        val view = activity.findViewById<View>(R.id.editor_content)
                        val isActive = imm?.isActive(view) == true
                        lastDiagnostic = "default=$defaultIme isActive=$isActive"
                        bound = isActive
                    }
                } catch (e: Exception) {
                    lastDiagnostic = "onActivity failed: ${e.message}"
                }
                bound
            },
            timeoutMs,
            message = { "Test IME (${TestImeCommands.IME_COMPONENT}) did not bind to the editor" },
            diagnostic = { lastDiagnostic },
        )
    }

    // ------------------------------------------------------------------
    // IME command senders
    // ------------------------------------------------------------------

    private fun sendImeCommit(
        text: String,
        cursor: Int = 1,
    ) {
        TestImeCommands.commitText(instrumentation.targetContext, text, cursor)
    }

    private fun sendImeSetComposing(
        text: String,
        cursor: Int = 1,
    ) {
        TestImeCommands.setComposingText(instrumentation.targetContext, text, cursor)
    }

    private fun sendImeSetComposingRegion(
        startUtf16: Int,
        endUtf16: Int,
    ) {
        TestImeCommands.setComposingRegion(instrumentation.targetContext, startUtf16, endUtf16)
    }

    private fun sendImeFinishComposing() {
        TestImeCommands.finishComposingText(instrumentation.targetContext)
    }

    private fun sendImeSetSelection(
        startUtf16: Int,
        endUtf16: Int,
    ) {
        TestImeCommands.setSelection(instrumentation.targetContext, startUtf16, endUtf16)
    }

    // ------------------------------------------------------------------
    // Editor UI state polls (real product behavior, no sleeps)
    // ------------------------------------------------------------------

    private fun waitForEditorText(
        expected: String,
        timeoutMs: Long = 10_000,
    ) {
        var lastObserved = "not polled yet"
        ComposeWait.waitUntil(
            composeTestRule,
            {
                var ok = false
                try {
                    Espresso.onView(ViewMatchers.withId(R.id.editor_content))
                        .check { view, _ ->
                            val editorView =
                                view as? SujianEditorView
                                    ?: throw AssertionError(
                                        "View is not a SujianEditorView: ${view?.javaClass?.simpleName}",
                                    )
                            val actual = editorView.getDisplayText()
                            lastObserved = actual
                            ok = actual == expected
                        }
                } catch (e: Exception) {
                    lastObserved = "editor check failed: ${e.message}"
                }
                ok
            },
            timeoutMs,
            message = { "Editor display text should be '$expected'" },
            diagnostic = { "last observed: '$lastObserved'" },
        )
    }

    private fun waitForEditorSelection(
        expectedStart: Int,
        expectedEnd: Int,
        timeoutMs: Long = 10_000,
    ) {
        var lastObserved = "not polled yet"
        ComposeWait.waitUntil(
            composeTestRule,
            {
                var ok = false
                try {
                    Espresso.onView(ViewMatchers.withId(R.id.editor_content))
                        .check { view, _ ->
                            val editorView =
                                view as? SujianEditorView
                                    ?: throw AssertionError(
                                        "View is not a SujianEditorView: ${view?.javaClass?.simpleName}",
                                    )
                            val start = editorView.getSelectionStart()
                            val end = editorView.getSelectionEnd()
                            lastObserved = "($start, $end)"
                            ok = start == expectedStart && end == expectedEnd
                        }
                } catch (e: Exception) {
                    lastObserved = "editor check failed: ${e.message}"
                }
                ok
            },
            timeoutMs,
            message = { "Editor selection should be ($expectedStart, $expectedEnd)" },
            diagnostic = { "last observed: $lastObserved" },
        )
    }

    private fun waitForEditorEmpty(timeoutMs: Long = 10_000) {
        waitForEditorText("", timeoutMs)
    }

    // ------------------------------------------------------------------
    // Display-layer composition state polls (via the editor's display mirror)
    // ------------------------------------------------------------------

    private fun waitForCompositionActive(
        expected: Boolean,
        timeoutMs: Long = 10_000,
    ) {
        waitForMirrorCondition("Editor composition state should be active=$expected", timeoutMs) { mirror ->
            mirror.hasComposition() == expected
        }
    }

    private fun waitForCompositionRangeUtf16(
        expectedStart: Int,
        expectedEnd: Int,
        timeoutMs: Long = 10_000,
    ) {
        waitForMirrorCondition(
            "Editor composition range should be [$expectedStart,$expectedEnd) UTF-16",
            timeoutMs,
        ) { mirror ->
            mirror.getCompositionRangeUtf16() == Pair(expectedStart, expectedEnd)
        }
    }

    private fun waitForCommittedText(
        expected: String,
        timeoutMs: Long = 10_000,
    ) {
        waitForMirrorCondition("Committed (non-preedit) text should be '$expected'", timeoutMs) { mirror ->
            mirror.getCommittedText() == expected
        }
    }

    private fun waitForMirrorCondition(
        description: String,
        timeoutMs: Long,
        check: (com.xiwei.sujian.feature.editor.mirror.DisplayTextMirror) -> Boolean,
    ) {
        var lastObserved = "not polled yet"
        ComposeWait.waitUntil(composeTestRule, {
            var ok = false
            try {
                Espresso.onView(ViewMatchers.withId(R.id.editor_content))
                    .check { view, _ ->
                        val editorView =
                            view as? SujianEditorView
                                ?: throw AssertionError(
                                    "View is not a SujianEditorView: ${view?.javaClass?.simpleName}",
                                )
                        val mirror = editorView.getPipeline().mirror
                        lastObserved = "hasComposition=${mirror.hasComposition()} " +
                            "range=${mirror.getCompositionRangeUtf16()} " +
                            "committed='${mirror.getCommittedText()}'"
                        ok = check(mirror)
                    }
            } catch (e: Exception) {
                lastObserved = "editor check failed: ${e.message}"
            }
            ok
        }, timeoutMs, message = { description }, diagnostic = { "last observed: $lastObserved" })
    }

    // ------------------------------------------------------------------
    // Scenarios: commitText
    // ------------------------------------------------------------------

    @Test
    fun commitText_fromBoundTestIme_showsTextOnScreen() {
        openEditorAndBindTestIme("IME提交测试")

        sendImeCommit("Hello")
        waitForEditorText("Hello")
        waitForEditorSelection(5, 5)
    }

    @Test
    fun commitText_cjkText_commitsWithCorrectByteSelection() {
        openEditorAndBindTestIme("IME中文提交测试")

        sendImeCommit("你好")
        waitForEditorText("你好")
        waitForEditorSelection(6, 6)
    }

    // ------------------------------------------------------------------
    // Scenarios: composition
    // ------------------------------------------------------------------

    /** setComposingText shows the preedit; finishComposingText settles it. */
    @Test
    fun setComposingText_thenFinishComposingText_settlesPreedit() {
        openEditorAndBindTestIme("IME组合落定测试")

        sendImeSetComposing("测试")
        waitForEditorText("测试")
        // During composition the editor reports the committed boundary.
        waitForEditorSelection(0, 0)
        waitForCompositionActive(true)
        waitForCommittedText("")

        sendImeFinishComposing()
        waitForEditorText("测试")
        waitForEditorSelection(6, 6)
        waitForCompositionActive(false)
        waitForCommittedText("测试")
    }

    /** A second setComposingText replaces the existing preedit (not appends). */
    @Test
    fun setComposingText_replacesExistingComposition() {
        openEditorAndBindTestIme("IME组合替换测试")

        sendImeSetComposing("你")
        waitForEditorText("你")
        waitForEditorSelection(0, 0)
        waitForCompositionActive(true)

        sendImeSetComposing("你好")
        waitForEditorText("你好")
        waitForEditorSelection(0, 0)

        sendImeFinishComposing()
        waitForEditorText("你好")
        waitForEditorSelection(6, 6)
        waitForCompositionActive(false)
    }

    /** Pinyin-style sequence: the preedit is updated in place, then settles. */
    @Test
    fun setComposingText_pinyinSequence_updatesPreeditInPlace() {
        openEditorAndBindTestIme("IME组合序列测试")

        sendImeCommit("前")
        waitForEditorText("前")
        waitForEditorSelection(3, 3)

        sendImeSetComposing("n")
        waitForEditorText("前n")
        sendImeSetComposing("ni")
        waitForEditorText("前ni")
        sendImeSetComposing("nih")
        waitForEditorText("前nih")
        // Composition cursor at the preedit end; selection still at committed boundary.
        waitForEditorSelection(3, 3)

        sendImeFinishComposing()
        waitForEditorText("前nih")
        waitForEditorSelection(6, 6)
    }

    /** setComposingRegion marks an existing region composing; a later preedit replaces only that region. */
    @Test
    fun setComposingRegion_marksExistingTextAsComposing() {
        openEditorAndBindTestIme("IME组合区域测试")

        sendImeCommit("ABCDE")
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)

        // Mark [1,3) = "BC" as composing; the text itself stays unchanged.
        sendImeSetComposingRegion(1, 3)
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)
        waitForCompositionActive(true)
        waitForCompositionRangeUtf16(1, 3)
        waitForCommittedText("ABCDE")

        // The preedit replaces only the composing region: "A" + "X" + "DE".
        sendImeSetComposing("X")
        waitForEditorText("AXDE")
        waitForEditorSelection(5, 5)
        waitForCommittedText("ABCDE")

        sendImeFinishComposing()
        waitForEditorText("AXDE")
        waitForEditorSelection(2, 2)
        waitForCompositionActive(false)
        waitForCommittedText("AXDE")
    }

    /**
     * Reversed setComposingRegion must be normalized (contract fix in
     * AndroidInputConnection.setComposingRegion, issue #589): [3,1) means the
     * same region as [1,3), so the preedit replaces "BC".
     */
    @Test
    fun setComposingRegion_reversedRange_isNormalized() {
        openEditorAndBindTestIme("IME反向组合区域测试")

        sendImeCommit("ABCDE")
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)

        sendImeSetComposingRegion(3, 1)
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)
        waitForCompositionActive(true)
        waitForCompositionRangeUtf16(1, 3)

        sendImeSetComposing("Y")
        waitForEditorText("AYDE")
        waitForEditorSelection(5, 5)

        sendImeFinishComposing()
        waitForEditorText("AYDE")
        waitForEditorSelection(2, 2)
        waitForCompositionActive(false)
    }

    /**
     * Region-only composition: finishComposingText keeps the text and — per
     * Android's contract — does not move the cursor (the kernel session for a
     * region-only composition carries an empty preedit, so finish is a no-op
     * for text and selection; cursor movement after region composition is the
     * IME's job via setSelection).
     */
    @Test
    fun setComposingRegion_thenFinishComposingText_keepsText() {
        openEditorAndBindTestIme("IME组合区域落定测试")

        sendImeCommit("ABCDE")
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)

        sendImeSetComposingRegion(1, 4)
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)
        waitForCompositionActive(true)

        sendImeFinishComposing()
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)
        waitForCompositionActive(false)
    }

    // ------------------------------------------------------------------
    // Scenarios: selection
    // ------------------------------------------------------------------

    @Test
    fun setSelection_movesCaretAndSelectionRange() {
        openEditorAndBindTestIme("IME选区测试")

        sendImeCommit("ABCDE")
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)

        sendImeSetSelection(2, 2)
        waitForEditorSelection(2, 2)

        sendImeSetSelection(1, 4)
        waitForEditorSelection(1, 4)
        waitForEditorText("ABCDE")
    }

    /** A selected range is replaced by the next commit. */
    @Test
    fun commitText_replacesSelectedRange() {
        openEditorAndBindTestIme("IME选区替换测试")

        sendImeCommit("ABCDE")
        waitForEditorText("ABCDE")
        waitForEditorSelection(5, 5)

        sendImeSetSelection(1, 4)
        waitForEditorSelection(1, 4)

        sendImeCommit("X")
        waitForEditorText("AXE")
        waitForEditorSelection(2, 2)
    }

    // ------------------------------------------------------------------
    // Scenarios: composition + rapid commits
    // ------------------------------------------------------------------

    /** commitText while composing replaces the preedit (IME convention). */
    @Test
    fun commitText_duringComposition_replacesPreedit() {
        openEditorAndBindTestIme("IME组合中提交测试")

        sendImeSetComposing("预")
        waitForEditorText("预")
        waitForEditorSelection(0, 0)
        waitForCompositionActive(true)

        sendImeCommit("编")
        waitForEditorText("编")
        waitForEditorSelection(3, 3)
        waitForCompositionActive(false)
        waitForCommittedText("编")
    }

    /** Back-to-back rapid commits through the IME channel all land in order. */
    @Test
    fun rapidSequentialCommits_backToBack_allAppliedInOrder() {
        openEditorAndBindTestIme("IME连续提交测试")

        sendImeCommit("A")
        sendImeCommit("B")
        sendImeCommit("C")

        waitForEditorText("ABC")
        waitForEditorSelection(3, 3)
    }

    // ------------------------------------------------------------------
    // Navigation helpers (same flow as the other instrumented tests)
    // ------------------------------------------------------------------

    private fun openTestChapter(
        chapterTitle: String,
        testData: AndroidTestEnvironment.TestProjectData,
    ): String {
        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput(chapterTitle)

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle(chapterTitle, testData)
        ComposeWait.waitForTag(
            composeTestRule,
            SujianSemanticIds.chapter(testData.volumeId, chapterId),
            timeoutMs = 15_000,
        )
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

    private fun waitForEditorReady(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        ComposeWait.waitForEspressoViewCondition(
            composeTestRule,
            EditorViewAssertions.isEditorReady(),
            timeoutMs = 15_000,
        ) { "Editor did not become ready for chapter $chapterId" }

        val expectedTargetId = "chapter-body:$projectId:$volumeId:$chapterId"
        var lastTargetId: String? = null
        ComposeWait.waitUntil(composeTestRule, {
            val coordinator = AndroidTestEnvironment.requireCurrentSession().deps.coordinator
            lastTargetId = coordinator.activeTargetId
            coordinator.activeTargetId == expectedTargetId
        }, timeoutMs = 10_000, message = { "activeTargetId should be $expectedTargetId but was $lastTargetId" })
    }

    private fun waitForChapterByTitle(
        title: String,
        testData: AndroidTestEnvironment.TestProjectData,
    ): String {
        val s = AndroidTestEnvironment.requireCurrentSession()
        val repo = s.deps.projectRepository
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
