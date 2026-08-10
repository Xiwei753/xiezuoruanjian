package com.xiwei.sujian.app.presentation

import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/**
 * Android Chrome Policy（#610 第 3 节）— 只做一件事：
 *
 * ```text
 * Core ActionSlot → Android 顶栏/列表/菜单位置 → 对应 Material3 控件
 * ```
 *
 * 图标、颜色、TopAppBar、NavigationBar、NavigationRail 全留 Android
 * （SujianNavigationSuite 渲染层）。产品设计语言（#597）的唯一事实来源是
 * Core screen_contract：本策略只消费 [ScreenPolicyDto]，不再自建第二份
 * “同步→搜索→设置”或“哪些页面显示顶栏/底栏”的规则。
 *
 * Android 独有的动态状态（工作区返回历史、正文位置）在这里与静态产品契约
 * 合成最终 chrome 决策。
 */
internal enum class SujianChromeAction {
    /** 手动触发同步并显示同步状态图标（Material3 视觉最左）。 */
    Sync,

    /** 搜索（当前未实现，保留入口占位）。 */
    Search,

    /** 打开设置一级页面（Material3 视觉最右）。 */
    Settings,
}

internal data class SujianChromeSpec(
    /** 是否显示返回箭头（与唯一返回动作同源，评论问题三）。 */
    val showBack: Boolean,
    /** 顶栏透明背景（写作区专用，正文一/二）。 */
    val appBarTransparent: Boolean,
    /** 是否显示顶栏标题（写作区不显示标题，只保留图标层）。 */
    val showTitle: Boolean,
    /** 顶栏右侧操作，按 Core order 排序（从左往右）。 */
    val actions: List<SujianChromeAction>,
    /** 一级导航是否可见：由 Core 布局契约 + 路由规则共同决定。 */
    val showPrimaryNavigation: Boolean,
)

internal object AndroidChromePolicy {
    /**
     * 路由 + 工作区位置 → Core ScreenRole（产品页面角色）。
     *
     * - Works 非正文 → ProjectWorkspace（作品页顶栏：同步/搜索/设置）
     * - Works 正文 → Writing（写作区只保留同步/设置，搜索不进入写作区，#597）
     * - Settings/StarMap/Stats → 对应角色
     */
    fun screenRoleFor(
        route: com.xiwei.sujian.app.navigation.SujianRoute,
        workspaceLocation: WorkspaceLocation,
    ): ScreenRoleDto =
        when (route) {
            com.xiwei.sujian.app.navigation.SujianRoute.Settings -> ScreenRoleDto.SETTINGS
            com.xiwei.sujian.app.navigation.SujianRoute.StarMap -> ScreenRoleDto.STAR_MAP
            com.xiwei.sujian.app.navigation.SujianRoute.Stats -> ScreenRoleDto.STATS
            com.xiwei.sujian.app.navigation.SujianRoute.Works ->
                if (workspaceLocation is WorkspaceLocation.Editor) {
                    ScreenRoleDto.WRITING
                } else {
                    ScreenRoleDto.PROJECT_WORKSPACE
                }
        }

    /**
     * Core ActionSlot（HeaderTrailing，按 order 升序）→ Android 顶栏动作。
     *
     * 只映射 Android 当前有控件的角色；Save（自动保存）/Sort（未实现）等
     * 不渲染为顶栏图标——这是 Android 呈现决策，不是产品规则。
     */
    fun headerActions(screenPolicy: ScreenPolicyDto?): List<SujianChromeAction> =
        PresentationContractBridge.headerTrailingSlots(screenPolicy).mapNotNull { slot ->
            when (slot.role) {
                ActionRoleDto.SYNC -> SujianChromeAction.Sync
                ActionRoleDto.SEARCH -> SujianChromeAction.Search
                ActionRoleDto.SETTINGS -> SujianChromeAction.Settings
                else -> null
            }
        }

    /**
     * 合成最终 chrome 决策。
     *
     * @param screenRole 当前页面角色（[screenRoleFor]）
     * @param screenPolicy Core 页面契约（PresentationContractBridge.resolveScreenPolicy）
     * @param workspaceLocation 工作区位置（正文/章节树/作品列表）
     * @param canWorkspaceNavigateBack 工作区返回历史是否可回退（Android 动态状态）
     * @param contractShowsPrimaryNavigation Core 布局契约的 show_primary_navigation
     */
    fun resolve(
        screenRole: ScreenRoleDto,
        screenPolicy: ScreenPolicyDto?,
        workspaceLocation: WorkspaceLocation,
        canWorkspaceNavigateBack: Boolean,
        contractShowsPrimaryNavigation: Boolean,
    ): SujianChromeSpec {
        val isEditor = workspaceLocation is WorkspaceLocation.Editor
        val showBack =
            when (screenRole) {
                ScreenRoleDto.SETTINGS ->
                    // 设置页返回来自 Core 的 Back 槽位（HeaderLeading）。
                    PresentationContractBridge.hasRoleAtLeading(screenPolicy, ActionRoleDto.BACK)
                ScreenRoleDto.PROJECT_WORKSPACE, ScreenRoleDto.WRITING ->
                    // 工作区返回是导航历史状态（Android 动态），由唯一 navigator 决定。
                    canWorkspaceNavigateBack
                else -> false
            }
        val showPrimaryNavigation =
            when (screenRole) {
                // #597：设置页从顶栏进入，不创建一级导航；正文隐藏一级导航。
                ScreenRoleDto.SETTINGS, ScreenRoleDto.WRITING -> false
                else -> contractShowsPrimaryNavigation
            }
        return SujianChromeSpec(
            showBack = showBack,
            appBarTransparent = isEditor,
            showTitle = !isEditor,
            actions = headerActions(screenPolicy),
            showPrimaryNavigation = showPrimaryNavigation,
        )
    }
}
