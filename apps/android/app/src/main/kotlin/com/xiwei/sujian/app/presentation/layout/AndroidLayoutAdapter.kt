@file:OptIn(ExperimentalMaterial3AdaptiveApi::class)

package com.xiwei.sujian.app.presentation.layout

import android.annotation.SuppressLint
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import uniffi.writer_core.LayoutContractDto
import uniffi.writer_core.PrimaryNavigationPlacementDto
import uniffi.writer_core.WindowViewportDto
import uniffi.writer_core.WorkspacePaneModeDto

/**
 * Android Layout Adapter（#610 / #628）— Android presentation/layout 层。
 *
 * 职责只剩两件事（#628 评论第 2 节）：
 * 1. 用 [LocalConfiguration] 读取当前 Compose 宿主窗口的原始 dp 宽高，
 *    构造 [WindowViewportDto] 交给 Core `presentation/layout` 解析；
 * 2. 把 Rust [LayoutContractDto] 映射成 Material3 [PaneScaffoldDirective] 与具体控件。
 *
 * #628 删除的内容（改由 Rust 决定）：
 * - `WindowWidthSizeClass` / `buildCapabilities()` / `availablePaneCount` —
 *   断点与 paneCount 不再由 Android 判断；
 * - `Compact/Medium/Expanded -> 1/2/3` 的 when — 壳层模式由 Rust `breakpoints` 决定；
 * - `AndroidNavigationPresentation` 枚举 — 底栏/侧栏改读
 *   [LayoutContractDto.primaryNavigationPlacement]（Bottom/Side）；
 * - 列表栏写死 `320.dp` — 改读 [LayoutContractDto.metrics.listPaneWidthDp]
 *   （Core `presentation/layout/metrics` 决定，Android 只做 `.dp` 映射）。
 *
 * 调用链（#628 评论第 4 节）：
 * ```text
 * Android LocalConfiguration.screenWidthDp/screenHeightDp
 *         ↓
 * WindowViewportDto(width, height)
 *         ↓
 * Rust presentation/layout (breakpoints/metrics/resolver)
 *         ↓
 * LayoutContractDto
 *         ↓
 * AndroidLayoutAdapter
 *         ↓
 * Material3 PaneScaffoldDirective / NavigationBar / NavigationRail
 * ```
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@SuppressLint("ConfigurationScreenWidthHeight")
@Composable
internal fun rememberAndroidLayoutSpec(
    foldingFeatures: List<AospFoldFeatureInfo>,
    resolveLayoutContract: (WindowViewportDto) -> LayoutContractDto?,
): AndroidLayoutSpec {
    val windowAdaptiveInfo = currentWindowAdaptiveInfo()
    val defaultDirective =
        remember(windowAdaptiveInfo) {
            calculatePaneScaffoldDirective(windowAdaptiveInfo)
        }

    // #628：读取当前 Compose 宿主窗口的原始 dp 宽高（Android 平台测量）。
    // LocalConfiguration.screenWidthDp/screenHeightDp 是 Android 平台测量窗口尺寸
    // 的标准方式（Compose 1.7 的 LocalWindowInfo.containerDpSize 在本项目未使用，
    // 沿用现有 LocalConfiguration 路径保持一致）。
    val configuration = LocalConfiguration.current
    val viewport =
        remember(configuration.screenWidthDp, configuration.screenHeightDp) {
            WindowViewportDto(
                widthDp = configuration.screenWidthDp.toFloat(),
                heightDp = configuration.screenHeightDp.toFloat(),
            )
        }
    val contract =
        remember(viewport) {
            resolveLayoutContract(viewport)
        }

    // Android 平台值：列表栏 preferred width — 优先用 Core metrics 返回的共用尺寸，
    // 仅在契约缺失（桥失败/空契约）时 fallback 到 320.dp。
    val preferredListPaneWidth = contract?.metrics?.listPaneWidthDp?.dp ?: 320.dp

    // Android 平台值：hinge excludedBounds（分隔式折叠铰链区域，px 坐标）。
    // 这是 Android 折叠屏独有平台值，不进入 Core。
    val excludedBounds =
        foldingFeatures
            .filter { it.isSeparating }
            .map { feature ->
                Rect(
                    left = feature.boundsLeft.toFloat(),
                    top = feature.boundsTop.toFloat(),
                    right = feature.boundsRight.toFloat(),
                    bottom = feature.boundsBottom.toFloat(),
                )
            }

    val scaffoldDirective =
        remember(contract, defaultDirective, excludedBounds, preferredListPaneWidth) {
            val maxHorizontalPartitions =
                contract?.workspacePaneMode?.let(::maxHorizontalPartitionsFor)
                    ?: defaultDirective.maxHorizontalPartitions
            defaultDirective.copy(
                maxHorizontalPartitions = maxHorizontalPartitions,
                defaultPanePreferredWidth = preferredListPaneWidth,
                excludedBounds = excludedBounds,
            )
        }

    return AndroidLayoutSpec(
        contract = contract,
        scaffoldDirective = scaffoldDirective,
    )
}

/**
 * #625 第二段：工作区窗格模式 — Kotlin 侧枚举，避免 UI 层直接引用 uniffi DTO
 * （遵守 ui-no-uniffi-jna-bridge 架构门禁）。
 *
 * 由 Core `LayoutContractDto.workspacePaneMode` 决定（#628：窗口尺寸→布局决策唯一在 Rust）。
 */
internal enum class WorkspacePaneMode {
    SINGLE_PANE,
    LIST_DETAIL,
    THREE_PANE,
}

/** Core [WorkspacePaneModeDto] → Kotlin [WorkspacePaneMode]（interop 映射，非断点判断）。 */
internal fun WorkspacePaneModeDto.toWorkspacePaneMode(): WorkspacePaneMode =
    when (this) {
        WorkspacePaneModeDto.SINGLE_PANE -> WorkspacePaneMode.SINGLE_PANE
        WorkspacePaneModeDto.LIST_DETAIL -> WorkspacePaneMode.LIST_DETAIL
        else -> WorkspacePaneMode.THREE_PANE
    }

/** Core WorkspacePaneMode → Material3 maxHorizontalPartitions（控件映射，非断点判断）。 */
internal fun maxHorizontalPartitionsFor(mode: WorkspacePaneModeDto): Int =
    when (mode) {
        WorkspacePaneModeDto.SINGLE_PANE -> 1
        WorkspacePaneModeDto.LIST_DETAIL -> 2
        else -> 3
    }

/**
 * Android UI spec — AndroidLayoutAdapter 的最终输出。
 *
 * [contract] 是 Core presentation contract（产品壳层语义，含一级导航放置与共用尺寸）；
 * [scaffoldDirective] 是 Android 平台自己的 Material3 呈现决策。
 *
 * #628：删除 `navigationPresentation` 字段 — 底栏/侧栏改读
 * `contract?.primaryNavigationPlacement`（PrimaryNavigationPlacementDto.Bottom/Side）。
 * [useBottomNavigation] 是该决策的便捷布尔视图，供 navigation 层消费，
 * 避免上层直接引用 uniffi DTO（遵守 ui-no-uniffi-jna-bridge 架构门禁）。
 *
 * #625 第二段：[workspacePaneMode] 是工作区窗格模式的便捷 Kotlin 枚举视图，供 feature/ui 层消费，
 * 避免上层直接引用 uniffi DTO（遵守 ui-no-uniffi-jna-bridge 架构门禁）。
 */
internal data class AndroidLayoutSpec(
    val contract: LayoutContractDto?,
    val scaffoldDirective: PaneScaffoldDirective,
) {
    /**
     * 一级导航是否用底栏（NavigationBar）而非侧栏（NavigationRail）。
     *
     * 由 Core `LayoutContractDto.primaryNavigationPlacement` 决定（#628 评论第 4 节）：
     * - `Bottom` → true（手机/小平板）；
     * - `Side` → false（桌面/大平板）；
     * - 契约缺失（桥失败/空契约）→ true（默认底栏，与窄窗口基线一致）。
     */
    val useBottomNavigation: Boolean
        get() = contract?.primaryNavigationPlacement == PrimaryNavigationPlacementDto.BOTTOM

    /**
     * 工作区窗格模式（#625 第二段）— 供 feature/ui 层判断窄屏/大屏布局。
     *
     * 由 Core `LayoutContractDto.workspacePaneMode` 决定（#628：窗口尺寸→布局决策唯一在 Rust）。
     * 契约缺失（桥失败/空契约）→ [WorkspacePaneMode.SINGLE_PANE]（默认窄屏，与基线一致）。
     */
    val workspacePaneMode: WorkspacePaneMode
        get() = contract?.workspacePaneMode?.toWorkspacePaneMode() ?: WorkspacePaneMode.SINGLE_PANE
}
