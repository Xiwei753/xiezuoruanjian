package com.xiwei.sujian.core.platform.window

// #617 评论四：沉浸式全屏的窗口执行层 — 系统栏控制唯一入口。
// 只封装 Android 系统能力（WindowInsetsControllerCompat），不读业务状态；
// 开关值由调用方（SujianApp）从 SettingsRepository.immersiveFullscreenEnabled 收集后传入。

import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner

/**
 * 沉浸式全屏系统栏控制：
 *
 * - [enabled] 为 true 时隐藏系统栏，滑动边缘临时显示（BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE）；
 * - [enabled] 为 false 时恢复系统栏；
 * - 每次 ON_RESUME 重放当前模式（系统栏状态不跨 Activity 生命周期保持）；
 * - 组合退出时恢复系统栏，避免页面离开后窗口停留在沉浸状态。
 */
@Composable
fun ImmersiveSystemBarsEffect(
    activity: ComponentActivity?,
    enabled: Boolean,
) {
    if (activity == null) return

    val lifecycleOwner = LocalLifecycleOwner.current

    DisposableEffect(activity, lifecycleOwner, enabled) {
        val window = activity.window
        val controller = WindowCompat.getInsetsController(window, window.decorView)

        fun applyMode() {
            if (enabled) {
                controller.systemBarsBehavior =
                    WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                controller.hide(WindowInsetsCompat.Type.systemBars())
            } else {
                controller.show(WindowInsetsCompat.Type.systemBars())
            }
        }

        val observer =
            LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) {
                    applyMode()
                }
            }

        lifecycleOwner.lifecycle.addObserver(observer)
        applyMode()

        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            controller.show(WindowInsetsCompat.Type.systemBars())
        }
    }
}
