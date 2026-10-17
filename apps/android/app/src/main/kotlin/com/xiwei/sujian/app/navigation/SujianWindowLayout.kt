package com.xiwei.sujian.app.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo

/*
 * Android 窗口布局决策 — 完全由本端 Compose 决定，不再请求 Core 返回布局模式。
 *
 * 宽/窄（工作台 / 单栏）与一级导航放置（底栏 / 侧栏）都只依据当前 Compose 宿主的
 * 窗口尺寸；折叠态作为本端窗口几何输入（分离铰链宽度从可用宽度中扣除），
 * 不构造跨平台共享布局抽象，也不把窗口尺寸送进 Core。
 *
 * 断点沿用旧产品规则（等价迁移）：
 *
 * ```text
 * width  < 600                          -> SinglePane
 * 600 <= width < 840 且 height < 480    -> SinglePane
 * 其余（width >= 600）                  -> Workbench
 *
 * width < 840  -> 底栏（NavigationBar）
 * width >= 840 -> 侧栏（NavigationRail）
 * ```
 */

/** 跨端结构尺寸的 Android 映射 — 各端各自持有本端数值，不再由 Core 下发。 */
internal object SujianWorkbenchDimensions {
    const val projectCardMinWidthDp: Float = 180f
    const val listPaneWidthDp: Float = 320f
    const val toolPaneWidthDp: Float = 240f
    const val toolRailWidthDp: Float = 56f
    const val toolbarHeightDp: Float = 64f
    const val toolbarLeadingWidthDp: Float = 200f
    const val toolbarTrailingWidthDp: Float = 200f
}

private const val WORKBENCH_MIN_WIDTH_DP: Float = 600f
private const val WORKBENCH_COMPACT_HEIGHT_DP: Float = 480f
private const val BOTTOM_NAVIGATION_MAX_WIDTH_DP: Float = 840f

/** 当前窗口（扣除分离铰链）的可用尺寸，派生出工作区布局模式与一级导航放置。 */
internal data class SujianWindowLayout(
    val widthDp: Float,
    val heightDp: Float,
) {
    /** 是否使用大屏工作台：窄屏或"中宽但过矮"回落单栏。 */
    val isWorkbench: Boolean
        get() =
            widthDp >= WORKBENCH_MIN_WIDTH_DP &&
                !(widthDp < BOTTOM_NAVIGATION_MAX_WIDTH_DP && heightDp < WORKBENCH_COMPACT_HEIGHT_DP)

    /** 一级导航是否用底栏（否则用侧栏）。 */
    val useBottomNavigation: Boolean
        get() = widthDp < BOTTOM_NAVIGATION_MAX_WIDTH_DP
}

/**
 * 读取当前 Compose 宿主窗口尺寸（dp），扣掉分离铰链占用的宽度后给出 [SujianWindowLayout]。
 *
 * - 尺寸来自 [LocalWindowInfo.current].containerDpSize（Compose 标准窗口尺寸入口）；
 * - 折叠态 [foldingFeatures] 中 `isSeparating` 的铰链宽度从可用宽度中扣除，
 *   近似旧"free region"语义，避免跨铰链硬塞工作台；
 * - 不读取/比较平台 WindowSizeClass，也不把尺寸传给 Core。
 */
@Composable
internal fun rememberSujianWindowLayout(foldingFeatures: List<AospFoldFeatureInfo> = emptyList()): SujianWindowLayout {
    val density = LocalDensity.current
    val containerDpSize = LocalWindowInfo.current.containerDpSize
    val separatingHingeWidthDp =
        remember(foldingFeatures, density) {
            var totalDp = 0f
            for (feature in foldingFeatures) {
                if (!feature.isSeparating) continue
                val widthPx = (feature.boundsRight - feature.boundsLeft).coerceAtLeast(0)
                if (widthPx == 0) continue
                totalDp += with(density) { widthPx.toDp().value }
            }
            totalDp
        }
    return remember(containerDpSize, separatingHingeWidthDp) {
        SujianWindowLayout(
            widthDp = (containerDpSize.width.value - separatingHingeWidthDp).coerceAtLeast(0f),
            heightDp = containerDpSize.height.value,
        )
    }
}
