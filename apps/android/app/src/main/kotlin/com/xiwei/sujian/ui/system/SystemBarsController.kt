package com.xiwei.sujian.ui.system

import android.app.Activity
import android.view.View
import android.view.ViewGroup
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

    // 多 target 列表
    private val appBarTargets = mutableListOf<View>()
    private val bottomPaddingTargets = mutableListOf<View>()
    private val bottomMarginTargets = mutableListOf<View>()

    // 保存原始 padding/margin
    private val originalPaddings = mutableMapOf<View, OriginalPadding>()
    private val originalMargins = mutableMapOf<View, OriginalMargin>()

    private data class OriginalPadding(val left: Int, val top: Int, val right: Int, val bottom: Int)
    private data class OriginalMargin(val left: Int, val top: Int, val right: Int, val bottom: Int)

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

            // AppBarLayout: top padding — 在原始值基础上叠加
            for (view in appBarTargets) {
                val orig = originalPaddings[view] ?: continue
                view.setPadding(
                    orig.left + systemBars.left,
                    orig.top + systemBars.top,
                    orig.right + systemBars.right,
                    orig.bottom
                )
            }

            // Bottom padding targets — 在原始值基础上叠加
            val bottomPadding = maxOf(systemBars.bottom, ime.bottom)
            for (view in bottomPaddingTargets) {
                val orig = originalPaddings[view] ?: continue
                view.setPadding(
                    orig.left + systemBars.left,
                    orig.top,
                    orig.right + systemBars.right,
                    orig.bottom + bottomPadding
                )
            }

            // Bottom margin targets — 在原始值基础上叠加
            for (view in bottomMarginTargets) {
                val orig = originalMargins[view] ?: continue
                val lp = view.layoutParams
                if (lp is ViewGroup.MarginLayoutParams) {
                    lp.leftMargin = orig.left + systemBars.left
                    lp.rightMargin = orig.right + systemBars.right
                    lp.bottomMargin = orig.bottom + bottomPadding
                    view.layoutParams = lp
                }
            }

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
     * 添加需要 top inset 的 AppBarLayout / Toolbar 容器。
     * 保存原始 padding，insets 叠加在原始值之上。
     */
    fun addAppBarTarget(view: View) {
        if (!appBarTargets.contains(view)) {
            appBarTargets.add(view)
            originalPaddings[view] = OriginalPadding(view.paddingLeft, view.paddingTop, view.paddingRight, view.paddingBottom)
        }
    }

    /**
     * 添加需要 bottom inset 的底部容器（BottomNavigationView / 正文底部）。
     * 保存原始 padding，insets 叠加在原始值之上。
     */
    fun addBottomPaddingTarget(view: View) {
        if (!bottomPaddingTargets.contains(view)) {
            bottomPaddingTargets.add(view)
            originalPaddings[view] = OriginalPadding(view.paddingLeft, view.paddingTop, view.paddingRight, view.paddingBottom)
        }
    }

    /**
     * 添加需要 bottom margin inset 的 View（如 editorStatusBar）。
     * 保存原始 margin，insets 叠加在原始值之上。
     */
    fun addBottomMarginTarget(view: View) {
        if (!bottomMarginTargets.contains(view)) {
            bottomMarginTargets.add(view)
            val lp = view.layoutParams
            if (lp is ViewGroup.MarginLayoutParams) {
                originalMargins[view] = OriginalMargin(lp.leftMargin, lp.topMargin, lp.rightMargin, lp.bottomMargin)
            }
        }
    }

    // ── 旧方法兼容（标记 @Deprecated） ──

    @Deprecated("Use addAppBarTarget", ReplaceWith("addAppBarTarget(view)"))
    fun setAppBarInsetTarget(view: View) = addAppBarTarget(view)

    @Deprecated("Use addBottomPaddingTarget", ReplaceWith("addBottomPaddingTarget(view)"))
    fun setBottomInsetTarget(view: View) = addBottomPaddingTarget(view)
}
