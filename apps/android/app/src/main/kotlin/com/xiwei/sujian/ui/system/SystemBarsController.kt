package com.xiwei.sujian.ui.system

import android.app.Activity
import android.view.View
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/**
 * SystemBarsController — 统一管理 WindowInsets 和系统栏行为
 *
 * 职责：
 * 1. 启用 edge-to-edge（setDecorFitsSystemWindows = false）
 * 2. 设置状态栏/导航栏透明或 colorSurface
 * 3. 根据 night mode 设置 isAppearanceLightStatusBars / isAppearanceLightNavigationBars
 * 4. 给 AppBarLayout / Toolbar 所在容器应用 systemBars.top padding
 * 5. 给 BottomNavigationView / FAB / 正文底部容器应用 max(systemBars.bottom, ime.bottom) 避让
 * 6. 支持实验室全屏模式：hide/show systemBars + BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
 *
 * 所有 WindowInsets 操作只放此类，不允许 Activity 直接操作。
 */
class SystemBarsController(private val activity: Activity) {

    private var appBarInsetTarget: View? = null
    private var bottomInsetTarget: View? = null

    /**
     * 设置 edge-to-edge + insets 监听。
     * 必须在 setContentView 之后调用。
     */
    fun setupEdgeToEdge() {
        val window = activity.window
        WindowCompat.setDecorFitsSystemWindows(window, false)

        // 设置状态栏/导航栏透明
        window.statusBarColor = android.graphics.Color.TRANSPARENT
        window.navigationBarColor = android.graphics.Color.TRANSPARENT

        // 根据 night mode 设置 light status/navigation bars
        val insetsController = WindowInsetsControllerCompat(window, window.decorView)
        val isNightMode = (activity.resources.configuration.uiMode and
            android.content.res.Configuration.UI_MODE_NIGHT_MASK) ==
            android.content.res.Configuration.UI_MODE_NIGHT_YES
        insetsController.isAppearanceLightStatusBars = !isNightMode
        insetsController.isAppearanceLightNavigationBars = !isNightMode

        // 监听 insets 变化
        ViewCompat.setOnApplyWindowInsetsListener(window.decorView) { _, insets ->
            val systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())

            // AppBarLayout: top padding
            appBarInsetTarget?.setPadding(
                systemBars.left,
                systemBars.top,
                systemBars.right,
                0
            )

            // Bottom container: max(systemBars.bottom, ime.bottom)
            val bottomPadding = maxOf(systemBars.bottom, ime.bottom)
            bottomInsetTarget?.setPadding(
                systemBars.left,
                0,
                systemBars.right,
                bottomPadding
            )

            insets
        }
    }

    /**
     * 全屏切换。
     * enabled=true: hide systemBars + BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
     * enabled=false: show systemBars 恢复默认 edge-to-edge
     */
    fun applyFullscreen(enabled: Boolean) {
        val window = activity.window
        val insetsController = WindowInsetsControllerCompat(window, window.decorView)
        if (enabled) {
            insetsController.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            insetsController.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            insetsController.show(WindowInsetsCompat.Type.systemBars())
            insetsController.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_DEFAULT
        }
    }

    /**
     * 设置需要 top inset 的 AppBarLayout / Toolbar 容器。
     */
    fun setAppBarInsetTarget(view: View) {
        appBarInsetTarget = view
    }

    /**
     * 设置需要 bottom inset 的底部容器（BottomNavigationView / FAB 容器 / 正文底部）。
     */
    fun setBottomInsetTarget(view: View) {
        bottomInsetTarget = view
    }
}
