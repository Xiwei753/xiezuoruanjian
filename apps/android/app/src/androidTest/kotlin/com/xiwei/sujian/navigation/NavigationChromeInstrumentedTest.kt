package com.xiwei.sujian.navigation

import android.content.pm.ActivityInfo
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.RestartableMainActivityRule
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

/**
 * #597 九：只检查当前已实现 UI 结构的页面测试。
 *
 * 覆盖正文确定的手机 UI 基线（不测星图功能，不为同一行为按手机/平板各写一套）：
 * 1. 启动作品页：底栏存在 Works/StarMap/Stats，顶栏存在 Search/Settings/Sync，
 *    设置不是底栏入口；
 * 2. 点击 Settings：设置页出现，底栏消失，顶栏操作消失（只剩返回）；
 * 3. 打开章节进入正文：EditorContent 出现，底栏消失；
 * 4. 从正文返回章节列表：底栏重新出现；
 * 5. 强制宽窗口：Root 使用 NavigationRail（同一套 Works/StarMap/Stats），
 *    不创建另一套页面结构。
 *
 * 窗口宽度只检查导航外壳变化，不重复运行章节新建、保存、输入等已有功能测试。
 */
@RunWith(AndroidJUnit4::class)
class NavigationChromeInstrumentedTest {
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

    private fun initTestData(): AndroidTestEnvironment.TestProjectData {
        return AndroidTestEnvironment.ensureTestProjectAndVolume(
            androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext,
        )
    }

    // ---- 场景 1：启动作品页（Root chrome）----

    @Test
    fun rootShowsBottomBarWithThreeDestinationsAndTopBarActions() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)

        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationBar).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStarMap).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStats).assertIsDisplayed()

        // 顶栏存在 设置/搜索/同步 入口。
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSearch).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSync).assertIsDisplayed()

        // 设置不是底栏入口：全树中 NavigationSettings 只出现一次（顶栏按钮）。
        val settingsNodes =
            composeTestRule.onAllNodes(hasTestTag(SujianSemanticIds.NavigationSettings))
                .fetchSemanticsNodes()
        assertEquals("设置只能存在于顶栏，不能是底栏入口", 1, settingsNodes.size)
    }

    // ---- 场景 2：设置页（Settings chrome）----

    @Test
    fun settingsHidesBottomBarAndKeepsOnlyBackTopBar() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen, timeoutMs = 15_000)

        // 底栏消失；设置不是一级入口。
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationBar).assertDoesNotExist()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).assertDoesNotExist()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStarMap).assertDoesNotExist()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStats).assertDoesNotExist()

        // 顶栏只剩返回：设置/搜索/同步操作全部消失。
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).assertDoesNotExist()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSearch).assertDoesNotExist()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSync).assertDoesNotExist()
    }

    // ---- 场景 3 & 4：正文隐藏底栏，返回后底栏恢复 ----

    @Test
    fun editorHidesBottomBarAndBackRestoresIt() {
        val testData = initTestData()
        navigateToTestVolume(testData)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.createChapter(testData.volumeId), timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.createChapter(testData.volumeId)).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.ChapterTitleInput)
        composeTestRule.onNodeWithTag(SujianSemanticIds.ChapterTitleInput).performTextInput("导航壳测试章节")

        composeTestRule.onNodeWithTag(SujianSemanticIds.DialogConfirm).performClick()

        val chapterId = waitForChapterByTitle("导航壳测试章节", testData)
        composeTestRule.onNodeWithTag(SujianSemanticIds.chapter(testData.volumeId, chapterId)).performClick()

        // 进入正文：EditorContent 出现，底栏消失。
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.EditorContent, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationBar).assertDoesNotExist()

        // 从正文返回章节列表：底栏重新出现。
        Espresso.closeSoftKeyboard()
        Espresso.pressBack()
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.WorkspaceVolumeList, timeoutMs = 15_000)
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationBar, timeoutMs = 10_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationBar).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).assertIsDisplayed()
    }

    // ---- 场景 5：宽窗口 Root 使用 NavigationRail，同一套目的地 ----

    @Test
    fun wideWindowRootUsesNavigationRailWithSameDestinations() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationWorks, timeoutMs = 15_000)

        activityRule.onActivity { activity ->
            activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
        }

        // 等待配置变化生效：宽窗口 Root 换成 NavigationRail。
        ComposeWait.waitUntil(
            composeTestRule,
            {
                composeTestRule.onAllNodes(hasTestTag(SujianSemanticIds.NavigationRail))
                    .fetchSemanticsNodes().isNotEmpty()
            },
            timeoutMs = 15_000,
            message = { "Rotated to landscape should show NavigationRail" },
        )

        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationRail).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationBar).assertDoesNotExist()

        // 仍然是同一套 Works/StarMap/Stats，不创建另一套页面结构。
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationWorks).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStarMap).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationStats).assertIsDisplayed()

        // 顶栏操作仍然存在（设置/搜索/同步）。
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSearch).assertIsDisplayed()
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSync).assertIsDisplayed()
    }

    // ---- 复用工作区导航（与 ChapterLifecycleTest 相同的进入路径）----

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

    private fun waitForChapterByTitle(
        title: String,
        testData: AndroidTestEnvironment.TestProjectData,
    ): String {
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
}
