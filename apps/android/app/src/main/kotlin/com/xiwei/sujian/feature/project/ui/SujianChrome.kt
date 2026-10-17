package com.xiwei.sujian.feature.project.ui

/*
 * 工作区顶栏 chrome 模型 — Android 本地产品语义。
 *
 * Core 不再返回页面动作/导航可见性契约，Android 顶层导航套件按当前路由与工作区位置
 * 自行决定顶栏标题、返回、透明背景与一级导航可见性（见导航层的 chrome 解析）。
 * 本文件只描述"顶栏有哪些动作、是否显示一级导航"，不包含窗口尺寸判断。
 */

/** 顶栏右侧动作（Material3 actions 从左往右的顺序）。 */
internal enum class SujianChromeAction {
    /** 手动触发同步并显示同步状态图标。 */
    Sync,

    /** 打开搜索入口。 */
    Search,

    /** 打开设置一级页面。 */
    Settings,
}

/** 当前页面的顶栏与一级导航决策。 */
internal data class SujianChromeSpec(
    /** 是否显示返回箭头。 */
    val showBack: Boolean,
    /** 顶栏透明背景（写作区专用）。 */
    val appBarTransparent: Boolean,
    /** 是否显示顶栏标题（写作区不显示标题，只保留图标层）。 */
    val showTitle: Boolean,
    /** 顶栏右侧操作，从左往右。 */
    val actions: List<SujianChromeAction>,
    /** 一级导航（底栏 / 侧栏）是否可见。 */
    val showPrimaryNavigation: Boolean,
)
