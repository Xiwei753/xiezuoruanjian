package com.xiwei.sujian.editor

import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.MainActivity
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.EditorCommitTextAction
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class EditorPersistenceTest {

    private val _composeTestRule = createAndroidComposeRule<MainActivity>()

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun initTestData(): AndroidTestEnvironment.TestProjectData {
        val ctx = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        val provider = com.xiwei.sujian.runtime.SujianAppDependencies.getTestProvider()
        val testDeps = provider?.invoke(ctx) as? com.xiwei.sujian.support.TestSujianAppDependencies
            ?: com.xiwei.sujian.support.TestSujianAppDependencies(ctx)
        return AndroidTestEnvironment.ensureTestProjectAndVolume(ctx, testDeps)
    }

    @Test
    fun commitText_persistsAfterReopen() {
        val testData = initTestData()
        val chapterId = openTestChapter("编辑器持久化章节A", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("第一段测试正文"))

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val node = composeTestRule.onNodeWithTag(SujianSemanticIds.EditorSaveStatus)
                node.assertExists()
                true
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        Espresso.pressBack()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 5_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("第一段测试正文")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("，继续写作"))

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("第一段测试正文，继续写作")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        val scenario = composeTestRule.activityRule.scenario
        scenario.recreate()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("第一段测试正文，继续写作")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
    }

    @Test
    fun commitText_persistsAcrossRecreate() {
        val testData = initTestData()
        openTestChapter("编辑器持久化章节B", testData)

        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText("重建测试正文"))

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val node = composeTestRule.onNodeWithTag(SujianSemanticIds.EditorSaveStatus)
                node.assertExists()
                true
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        val scenario = composeTestRule.activityRule.scenario
        scenario.recreate()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("重建测试正文")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
    }

    @Test
    fun commitText_unicodeAndMultiline_persists() {
        val testData = initTestData()
        openTestChapter("编辑器持久化章节C", testData)

        val testText = "你好，素笺。\n第二行🙂"
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .perform(EditorCommitTextAction.commitText(testText))

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("素笺") && view.getDisplayText().contains("🙂")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        val scenario = composeTestRule.activityRule.scenario
        scenario.recreate()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.getDisplayText().contains("素笺") && view.getDisplayText().contains("🙂")
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)
    }

    private fun openTestChapter(chapterTitle: String, testData: AndroidTestEnvironment.TestProjectData): String {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList)

        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceCreateChapter, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.WorkspaceCreateChapter).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput(chapterTitle)

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle(chapterTitle, testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        ComposeWait.waitUntil(composeTestRule, {
            try {
                val view = composeTestRule.activity.findViewById<com.xiwei.sujian.editor.v2.host.SujianEditorView>(R.id.editor_content)
                view != null && view.visibility == android.view.View.VISIBLE
            } catch (_: Exception) {
                false
            }
        }, timeoutMs = 15_000)

        return chapterId
    }

    private fun waitForChapterByTitle(title: String, testData: AndroidTestEnvironment.TestProjectData): String {
        val ctx = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        val provider = com.xiwei.sujian.runtime.SujianAppDependencies.getTestProvider()
        val testDeps = provider?.invoke(ctx) as? com.xiwei.sujian.support.TestSujianAppDependencies
            ?: com.xiwei.sujian.support.TestSujianAppDependencies(ctx)
        val repo = testDeps.workspaceRepository
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
                val projectTitle = testData.projectTitle
                try {
                    composeTestRule.onNodeWithText(projectTitle).performClick()
                } catch (_: Exception) { }
                Thread.sleep(500)
                try {
                    composeTestRule.onNodeWithTag(volumeTag).assertExists()
                    true
                } catch (_: AssertionError) {
                    false
                }
            }
        }, timeoutMs = 15_000)
        try {
            composeTestRule.onNodeWithTag(volumeTag).performClick()
        } catch (_: Exception) { }
    }
}
