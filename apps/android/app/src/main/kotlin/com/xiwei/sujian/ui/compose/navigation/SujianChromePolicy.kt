package com.xiwei.sujian.ui.compose.navigation

import com.xiwei.sujian.ui.compose.workspace.WorkspaceLocation

/**
 * 顶栏/底栏外观策略 — 手机 UI 结构的唯一事实来源（纯函数，可单测）。
 *
 * #597 评论问题一确定的手机 UI 结构：
 * - 一级底栏只保留：作品、星图、统计；设置从顶栏进入，不再是底栏一级入口；
 * - 作品页顶栏右侧依次提供：设置、搜索、同步状态；
 * - 进入设置后顶栏只保留左上返回；
 * - 进入正文后隐藏底栏；
 * - 写作区顶栏透明背景，只保留需要的图标层（返回、设置、同步），不显示标题；
 * - 宽窗口只把同一套手机 UI 的导航位置和内容排列展开，不重新引入另一套结构。
 *
 * 返回箭头与点击动作必须来自同一个值（评论问题三）：策略给出 showBack 决策，
 * 调用方用同一输入构造唯一返回动作，图标只在动作存在时渲染。
 */
internal enum class SujianChromeAction {
    /** 打开设置一级页面。 */
    Settings,

    /** 搜索（当前未实现，保留入口占位）。 */
    Search,

    /** 手动触发同步并显示同步状态图标。 */
    Sync,
}

internal data class SujianChromeSpec(
    /** 是否显示返回箭头（与唯一返回动作同源，评论问题三）。 */
    val showBack: Boolean,
    /** 顶栏透明背景（写作区专用，评论问题一）。 */
    val appBarTransparent: Boolean,
    /** 是否显示顶栏标题（写作区不显示标题，只保留图标层）。 */
    val showTitle: Boolean,
    /** 顶栏右侧操作，按显示顺序排列。 */
    val actions: List<SujianChromeAction>,
    /** 底栏是否可见（compact 模式；正文内与设置页隐藏）。 */
    val showBottomBar: Boolean,
)

internal object SujianChromePolicy {
    fun resolve(
        route: SujianRoute,
        workspaceLocation: WorkspaceLocation,
        canWorkspaceNavigateBack: Boolean,
        starMapHasBack: Boolean,
        isCompact: Boolean,
    ): SujianChromeSpec =
        when (route) {
            is SujianRoute.Settings ->
                SujianChromeSpec(
                    showBack = true,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showBottomBar = false,
                )
            is SujianRoute.StarMap ->
                SujianChromeSpec(
                    showBack = starMapHasBack,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showBottomBar = isCompact,
                )
            is SujianRoute.Stats ->
                SujianChromeSpec(
                    showBack = false,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showBottomBar = isCompact,
                )
            is SujianRoute.Works -> {
                val isEditor = workspaceLocation is WorkspaceLocation.Editor
                // 写作区专属 chrome（隐藏底栏、透明顶栏、无标题、只保留需要的图标层）
                // 只在 compact 模式生效；宽窗口把同一套页面结构展开到旁边，
                // 顶栏保持页面级标题与完整操作（#597 评论问题一）。
                val editorChrome = isCompact && isEditor
                SujianChromeSpec(
                    showBack = canWorkspaceNavigateBack,
                    appBarTransparent = editorChrome,
                    showTitle = !editorChrome,
                    actions =
                        if (editorChrome) {
                            listOf(SujianChromeAction.Settings, SujianChromeAction.Sync)
                        } else {
                            listOf(
                                SujianChromeAction.Settings,
                                SujianChromeAction.Search,
                                SujianChromeAction.Sync,
                            )
                        },
                    showBottomBar = isCompact && !isEditor,
                )
            }
        }
}
