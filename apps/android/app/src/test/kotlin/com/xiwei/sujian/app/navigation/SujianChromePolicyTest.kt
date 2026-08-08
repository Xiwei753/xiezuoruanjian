package com.xiwei.sujian.app.navigation

import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #597 正文一/三的 UI 结构策略测试：
 * - 一级导航只保留 作品/星图/统计；设置从顶栏进入，不是一级入口；
 * - 作品页顶栏右侧产品顺序（从右往左）为 设置/搜索/同步状态，
 *   代码顺序为 同步 → 搜索 → 设置；
 * - 进入设置后顶栏只保留左上返回，一级导航（底栏/侧栏）消失；
 * - 进入正文后一级导航消失；写作区顶栏透明、不显示标题、只保留需要的图标层；
 * - 宽窗口与手机同一套规则：只在 Root 时把底栏换成侧栏，Editor 同样
 *   隐藏侧栏并透明化顶栏，不重新引入另一套结构；
 * - 星图根页没有返回动作（占位页无编辑态顶栏状态，正文四）。
 */
class SujianChromePolicyTest {
    private fun resolve(
        route: SujianRoute,
        location: WorkspaceLocation = WorkspaceLocation.ProjectList,
        canBack: Boolean = false,
    ): SujianChromeSpec =
        SujianChromePolicy.resolve(
            route = route,
            workspaceLocation = location,
            canWorkspaceNavigateBack = canBack,
        )

    // ---- 一级导航：作品 / 星图 / 统计，设置不是一级入口 ----

    @Test
    fun `works root keeps primary navigation with sync search settings in order`() {
        val spec = resolve(SujianRoute.Works)
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
        assertFalse(spec.showBack)
        // 正文三：产品顺序从右往左为 设置/搜索/同步状态，代码顺序为 同步 → 搜索 → 设置。
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Search, SujianChromeAction.Settings),
            spec.actions,
        )
    }

    @Test
    fun `starmap root keeps primary navigation and has no actions or back`() {
        val spec = resolve(SujianRoute.StarMap)
        assertTrue(spec.showPrimaryNavigation)
        assertTrue(spec.actions.isEmpty())
        assertFalse("星图占位根页没有返回动作（正文四）", spec.showBack)
        assertTrue(spec.showTitle)
    }

    @Test
    fun `stats root keeps primary navigation and never shows back arrow`() {
        val spec = resolve(SujianRoute.Stats)
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.showBack)
        assertTrue(spec.actions.isEmpty())
    }

    // ---- 设置：顶栏只保留左上返回，无一级导航 ----

    @Test
    fun `settings shows only back and hides primary navigation`() {
        val spec = resolve(SujianRoute.Settings)
        assertTrue(spec.showBack)
        assertFalse(spec.showPrimaryNavigation)
        assertTrue(spec.actions.isEmpty())
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
    }

    // ---- 正文：隐藏一级导航、透明顶栏、无标题、只保留需要的图标层 ----

    @Test
    fun `editor hides primary navigation and shows transparent titleless top bar`() {
        val spec =
            resolve(
                SujianRoute.Works,
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        assertFalse("进入正文后隐藏一级导航（底栏/侧栏）", spec.showPrimaryNavigation)
        assertTrue("写作区顶栏透明背景", spec.appBarTransparent)
        assertFalse("写作区顶栏不显示标题", spec.showTitle)
        assertTrue(spec.showBack)
        // 写作区只保留需要的图标层：同步、设置（设置视觉最右）；搜索（未实现）不进入写作区。
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Settings),
            spec.actions,
        )
    }

    @Test
    fun `editor wide window follows the same chrome rules`() {
        val spec =
            resolve(
                SujianRoute.Works,
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        // 正文一：宽窗口同一套规则 — Editor 不创建 NavigationRail，顶栏透明。
        // 宽屏只是在 Root 时把底栏换成侧栏，不重新引入另一套页面结构。
        assertFalse("宽窗口正文同样隐藏一级导航", spec.showPrimaryNavigation)
        assertTrue("宽窗口正文顶栏同样透明", spec.appBarTransparent)
        assertFalse(spec.showTitle)
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Settings),
            spec.actions,
        )
        assertTrue(spec.showBack)
    }

    // ---- 返回箭头与动作同源（评论问题三）----

    @Test
    fun `works chapter tree shows back when workspace can navigate back`() {
        val spec =
            resolve(
                SujianRoute.Works,
                location = WorkspaceLocation.ChapterTree("p1"),
                canBack = true,
            )
        assertTrue(spec.showBack)
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.appBarTransparent)
    }

    @Test
    fun `stats root never shows a back arrow even if workspace could go back`() {
        // 统计根页是独立一级入口，不继承作品工作区的返回能力。
        val spec =
            resolve(
                SujianRoute.Stats,
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        assertFalse(spec.showBack)
    }

    @Test
    fun `works project list shows no back arrow`() {
        val spec = resolve(SujianRoute.Works)
        assertFalse(spec.showBack)
    }
}
