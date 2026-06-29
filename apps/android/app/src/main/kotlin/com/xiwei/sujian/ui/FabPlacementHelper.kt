package com.xiwei.sujian.ui

import android.view.ViewGroup
import com.google.android.material.floatingactionbutton.FloatingActionButton

/**
 * FabPlacementHelper — FAB 底栏避让辅助工具
 *
 * 根据 BottomNavigationView 是否存在及高度，动态调整 FAB 的 bottomMargin，
 * 避免与底栏重叠。
 *
 * ## 规则
 * - 有底栏时：bottomMargin = bottomNavHeight + safeBottomInset + 24dp
 * - 无底栏/TwoPane 时：bottomMargin = 24dp
 *
 * ## 使用场景
 * - MainActivity 的三种 ShellMode 布局切换
 * - ChapterListActivity（无底栏）
 */
object FabPlacementHelper {

    /**
     * 调整 FAB 的 bottomMargin 以避让底栏。
     *
     * @param fab 要调整的 FloatingActionButton
     * @param hasBottomNav 是否存在底部导航栏
     * @param bottomNavHeight 底部导航栏高度（px），仅在 hasBottomNav=true 时有效
     * @param safeBottomInset 安全区域底部 inset（px），用于适配手势导航等
     * @param density 屏幕密度，用于 dp 换算
     */
    fun adjustFabBottomMargin(
        fab: FloatingActionButton,
        hasBottomNav: Boolean,
        bottomNavHeight: Int,
        safeBottomInset: Int,
        density: Float
    ) {
        val spacingPx = (24 * density).toInt()
        val bottomMargin = if (hasBottomNav) {
            bottomNavHeight + safeBottomInset + spacingPx
        } else {
            spacingPx
        }

        val params = fab.layoutParams
        if (params is ViewGroup.MarginLayoutParams) {
            params.bottomMargin = bottomMargin
            fab.layoutParams = params
        }
    }
}
