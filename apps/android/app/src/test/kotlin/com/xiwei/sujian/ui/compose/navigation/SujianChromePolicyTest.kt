package com.xiwei.sujian.ui.compose.navigation

import com.xiwei.sujian.ui.compose.workspace.WorkspaceLocation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #597 评论问题一/三的 UI 结构策略测试：
 * - 一级底栏只保留 作品/星图/统计；设置从顶栏进入；
 * - 作品页顶栏右侧依次提供 设置、搜索、同步状态；
 * - 进入设置后顶栏只保留左上返回；
 * - 进入正文后隐藏底栏；写作区顶栏透明、不显示标题、只保留需要的图标层；
 * - 统计/星图根页无返回动作时不显示返回箭头（图标与动作同源）；
 * - 宽窗口只展开同一套结构，不重新引入另一套。
 */
class SujianChromePolicyTest {
    private fun resolve(
        route: SujianRoute,
        location: WorkspaceLocation = WorkspaceLocation.ProjectList,
        canBack: Boolean = false,
        starMapHasBack: Boolean = false,
        isCompact: Boolean = true,
    ): SujianChromeSpec =
        SujianChromePolicy.resolve(
            route = route,
            workspaceLocation = location,
            canWorkspaceNavigateBack = canBack,
            starMapHasBack = starMapHasBack,
            isCompact = isCompact,
        )

    // ---- 一级底栏：作品 / 星图 / 统计，设置不再是底栏入口 ----

    @Test
    fun `works root keeps bottom bar with settings search sync in order`() {
        val spec = resolve(SujianRoute.Works)
        assertTrue(spec.showBottomBar)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
        assertFalse(spec.showBack)
        // 评论问题一：作品页顶栏右侧依次提供 设置、搜索、同步状态。
        assertEquals(
            listOf(SujianChromeAction.Settings, SujianChromeAction.Search, SujianChromeAction.Sync),
            spec.actions,
        )
    }

    @Test
    fun `starmap root keeps bottom bar and has no actions`() {
        val spec = resolve(SujianRoute.StarMap)
        assertTrue(spec.showBottomBar)
        assertTrue(spec.actions.isEmpty())
        assertFalse(spec.showBack)
        assertTrue(spec.showTitle)
    }

    @Test
    fun `stats root keeps bottom bar and never shows back arrow`() {
        val spec = resolve(SujianRoute.Stats)
        assertTrue(spec.showBottomBar)
        assertFalse(spec.showBack)
        assertTrue(spec.actions.isEmpty())
    }

    // ---- 设置：顶栏只保留左上返回，无底栏 ----

    @Test
    fun `settings shows only back and hides bottom bar`() {
        val spec = resolve(SujianRoute.Settings)
        assertTrue(spec.showBack)
        assertFalse(spec.showBottomBar)
        assertTrue(spec.actions.isEmpty())
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
    }

    // ---- 正文：隐藏底栏、透明顶栏、无标题、只保留需要的图标层 ----

    @Test
    fun `editor hides bottom bar and shows transparent titleless top bar`() {
        val spec =
            resolve(
                SujianRoute.Works,
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        assertFalse("进入正文后隐藏底栏", spec.showBottomBar)
        assertTrue("写作区顶栏透明背景", spec.appBarTransparent)
        assertFalse("写作区顶栏不显示标题", spec.showTitle)
        assertTrue(spec.showBack)
        // 写作区只保留需要的图标层：设置、同步；搜索（未实现）不进入写作区。
        assertEquals(
            listOf(SujianChromeAction.Settings, SujianChromeAction.Sync),
            spec.actions,
        )
    }

    @Test
    fun `editor wide window does not switch to another chrome structure`() {
        val spec =
            resolve(
                SujianRoute.Works,
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
                isCompact = false,
            )
        // 宽窗口只展开同一套手机 UI：没有底栏（改侧边导航），顶栏保持页面级
        // 标题与完整操作（设置、搜索、同步），不重新引入另一套结构。
        assertFalse(spec.showBottomBar)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
        assertEquals(
            listOf(SujianChromeAction.Settings, SujianChromeAction.Search, SujianChromeAction.Sync),
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
        assertTrue(spec.showBottomBar)
        assertFalse(spec.appBarTransparent)
    }

    @Test
    fun `starmap back arrow only when a real back action exists`() {
        assertFalse(resolve(SujianRoute.StarMap, starMapHasBack = false).showBack)
        assertTrue(resolve(SujianRoute.StarMap, starMapHasBack = true).showBack)
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
