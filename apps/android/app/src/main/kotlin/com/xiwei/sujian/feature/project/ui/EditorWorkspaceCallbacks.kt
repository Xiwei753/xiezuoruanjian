package com.xiwei.sujian.feature.project.ui

/**
 * #641 评论 5457777142 问题6：写作区动作回调 —
 * 由导航套件层（[com.xiwei.sujian.app.navigation.SujianNavigationSuite]）创建，
 * 传进 [ProjectWorkspaceScreen]。
 *
 * 不让 project UI 反向依赖 navigation route 类型 —
 * 这里只暴露 `() -> Unit` 回调，具体导航/同步实现由创建方注入。
 *
 * @param onSync 用户点击同步按钮 — 触发手动全量同步。
 * @param onSettings 用户点击设置按钮 — 打开设置页。
 * @param onSearch 用户点击搜索按钮 — 打开全局搜索入口。
 */
data class EditorWorkspaceCallbacks(
    val onSync: () -> Unit,
    val onSettings: () -> Unit,
    val onSearch: () -> Unit,
)
