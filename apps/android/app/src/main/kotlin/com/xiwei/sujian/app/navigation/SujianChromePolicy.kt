package com.xiwei.sujian.app.navigation

import com.xiwei.sujian.feature.home.ui.WorkspaceLocation

/**
 * 顶栏/底栏外观策略 — 手机 UI 结构的唯一事实来源（纯函数，可单测）。
 *
 * #597 正文确定的手机 UI 结构：
 * - 一级导航只保留：作品、星图、统计；设置从顶栏进入，不是一级入口；
 * - 作品页顶栏右侧产品顺序（从右往左）为：设置 / 搜索 / 同步状态，
 *   Material3 actions 按代码顺序从左往右摆，因此代码顺序为 同步 → 搜索 → 设置；
 * - 进入设置后顶栏只保留左上返回，不创建一级导航（底栏/侧栏）；
 * - 进入正文（Editor）后隐藏一级导航，顶栏透明、不显示标题，只保留需要的图标层；
 * - 宽窗口与手机同一套规则：只在 Root 时把底栏换成侧栏（NavigationRail），
 *   不重新引入另一套页面结构。
 *
 * 返回箭头与点击动作必须来自同一个值（#597 评论问题三）：策略给出 showBack 决策，
 * 调用方用同一输入构造唯一返回动作，图标只在动作存在时渲染。
 */
internal enum class SujianChromeAction {
    /** 手动触发同步并显示同步状态图标（视觉最左）。 */
    Sync,

    /** 搜索（当前未实现，保留入口占位）。 */
    Search,

    /** 打开设置一级页面（视觉最右）。 */
    Settings,
}

internal data class SujianChromeSpec(
    /** 是否显示返回箭头（与唯一返回动作同源，评论问题三）。 */
    val showBack: Boolean,
    /** 顶栏透明背景（写作区专用，正文一/二）。 */
    val appBarTransparent: Boolean,
    /** 是否显示顶栏标题（写作区不显示标题，只保留图标层）。 */
    val showTitle: Boolean,
    /** 顶栏右侧操作，按显示顺序排列（从左往右）。 */
    val actions: List<SujianChromeAction>,
    /** 一级导航是否可见：compact 渲染为底栏，宽窗口渲染为侧栏；设置页与正文隐藏。 */
    val showPrimaryNavigation: Boolean,
)

internal object SujianChromePolicy {
    fun resolve(
        route: SujianRoute,
        workspaceLocation: WorkspaceLocation,
        canWorkspaceNavigateBack: Boolean,
    ): SujianChromeSpec =
        when (route) {
            is SujianRoute.Settings ->
                SujianChromeSpec(
                    showBack = true,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showPrimaryNavigation = false,
                )
            is SujianRoute.StarMap ->
                SujianChromeSpec(
                    showBack = false,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showPrimaryNavigation = true,
                )
            is SujianRoute.Stats ->
                SujianChromeSpec(
                    showBack = false,
                    appBarTransparent = false,
                    showTitle = true,
                    actions = emptyList(),
                    showPrimaryNavigation = true,
                )
            is SujianRoute.Works -> {
                val isEditor = workspaceLocation is WorkspaceLocation.Editor
                SujianChromeSpec(
                    showBack = canWorkspaceNavigateBack,
                    appBarTransparent = isEditor,
                    showTitle = !isEditor,
                    actions =
                        if (isEditor) {
                            // 写作区只保留需要的图标层：同步、设置；搜索（未实现）不进入写作区。
                            listOf(SujianChromeAction.Sync, SujianChromeAction.Settings)
                        } else {
                            // 作品页产品顺序从右往左：设置 / 搜索 / 同步状态。
                            listOf(
                                SujianChromeAction.Sync,
                                SujianChromeAction.Search,
                                SujianChromeAction.Settings,
                            )
                        },
                    showPrimaryNavigation = !isEditor,
                )
            }
        }
}
