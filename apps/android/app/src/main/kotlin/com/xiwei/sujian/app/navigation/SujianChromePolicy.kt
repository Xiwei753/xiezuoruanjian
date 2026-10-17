package com.xiwei.sujian.app.navigation

import com.xiwei.sujian.feature.project.ui.SujianChromeAction
import com.xiwei.sujian.feature.project.ui.SujianChromeSpec
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation

/*
 * 顶层导航 chrome 解析 — Android 本地产品语义。
 *
 * Core 不再返回页面动作/导航可见性契约，顶栏归属由 Android 顶层导航套件按当前路由与
 * 工作区位置自行决定。规则集中在此处，不在多个 Composable 里各写一份：
 *
 * - 作品：作品列表 / 章节树 / 正文三个业务位置；
 * - 设置：沉浸二级页，只保留左上返回，一级导航（底栏/侧栏）隐藏；
 * - 星图 / 统计：一级根页，保留一级导航，无返回；
 * - 正文：透明顶栏、无标题、隐藏一级导航，右侧仍保留 同步 / 搜索 / 设置（从左往右）。
 */

/** 全局页头右侧动作（从左往右顺序）。 */
private val GlobalHeaderTrailingActions: List<SujianChromeAction> =
    listOf(
        SujianChromeAction.Sync,
        SujianChromeAction.Search,
        SujianChromeAction.Settings,
    )

internal fun resolveSujianChrome(
    currentRoute: SujianRoute,
    workspaceLocation: WorkspaceLocation,
    canWorkspaceNavigateBack: Boolean,
): SujianChromeSpec {
    val isEditor = currentRoute is SujianRoute.Works && workspaceLocation is WorkspaceLocation.Editor
    val isChapterTreeOrEditor =
        workspaceLocation is WorkspaceLocation.ChapterTree || workspaceLocation is WorkspaceLocation.Editor
    val showBack =
        when {
            currentRoute is SujianRoute.Settings -> true
            currentRoute is SujianRoute.Works && isChapterTreeOrEditor -> canWorkspaceNavigateBack
            else -> false
        }
    val showPrimaryNavigation =
        when {
            currentRoute is SujianRoute.Settings -> false
            isEditor -> false
            else -> true
        }
    val actions =
        when (currentRoute) {
            is SujianRoute.Settings -> emptyList()
            is SujianRoute.Works, is SujianRoute.StarMap, is SujianRoute.Stats -> GlobalHeaderTrailingActions
        }
    return SujianChromeSpec(
        showBack = showBack,
        appBarTransparent = isEditor,
        showTitle = !isEditor,
        actions = actions,
        showPrimaryNavigation = showPrimaryNavigation,
    )
}
