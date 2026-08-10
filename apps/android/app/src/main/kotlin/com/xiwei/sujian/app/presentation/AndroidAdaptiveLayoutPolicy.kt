@file:OptIn(ExperimentalMaterial3AdaptiveApi::class, ExperimentalLayoutApi::class)

package com.xiwei.sujian.app.presentation

import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.WindowAdaptiveInfo
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.window.core.layout.WindowWidthSizeClass
import com.xiwei.sujian.app.LocalAndroidCapabilities
import com.xiwei.sujian.core.platform.api.PointerKind
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import uniffi.writer_core.LayoutContractDto
import uniffi.writer_core.PointerClassDto
import uniffi.writer_core.WindowCapabilitiesDto
import uniffi.writer_core.WorkspacePaneModeDto

/**
 * Android 自适应布局策略（#610 第 3 节）— Android presentation/render 层。
 *
 * 职责：把 Android 窗口系统（WindowSizeClass / FoldingFeature / 指针 / IME）
 * 判断成窗口能力，交给 Core presentation contract 解析产品壳层契约，
 * 再叠加 Android 平台值（NavigationBar/NavigationRail、pane preferred width、
 * hinge excludedBounds）合成最终 Android UI spec。
 *
 * 分层（#610）：
 * - Core 不再包含 Material 断点 / dp / NavigationPresentation；
 * - 断点与 dp 是 Android 平台决策，在本文件计算；
 * - 产品壳层语义（ShellMode/WorkspacePaneMode）来自 Core。
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
internal fun rememberAndroidLayoutSpec(foldingFeatures: List<AospFoldFeatureInfo>): AndroidLayoutSpec {
    val context = LocalContext.current
    val windowAdaptiveInfo = currentWindowAdaptiveInfo()
    val defaultDirective =
        remember(windowAdaptiveInfo) {
            calculatePaneScaffoldDirective(windowAdaptiveInfo)
        }

    val activePointerKind = LocalAndroidCapabilities.current.activePointerKind
    val keyboardVisible = WindowInsets.isImeVisible

    // Android 平台决策：窗口能力 → Core 输入。
    val capabilities =
        remember(windowAdaptiveInfo, foldingFeatures, activePointerKind, keyboardVisible) {
            buildCapabilities(
                windowAdaptiveInfo = windowAdaptiveInfo,
                foldingFeatures = foldingFeatures,
                activePointerKind = activePointerKind,
                keyboardVisible = keyboardVisible,
            )
        }
    val contract =
        remember(capabilities) {
            PresentationContractBridge.resolveLayoutContract(context, capabilities)
        }

    // Android 平台决策：NavigationBar vs NavigationRail（Material3 WindowSizeClass）。
    val navigationPresentation =
        if (windowAdaptiveInfo.windowSizeClass.windowWidthSizeClass == WindowWidthSizeClass.COMPACT) {
            AndroidNavigationPresentation.BottomBar
        } else {
            AndroidNavigationPresentation.NavigationRail
        }

    // Android 平台值：pane preferred width（列表栏 320dp）。
    val preferredListPaneWidth = 320.dp

    // Android 平台值：hinge excludedBounds（分隔式折叠铰链区域，px 坐标）。
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
        remember(contract, defaultDirective, excludedBounds) {
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
        navigationPresentation = navigationPresentation,
        scaffoldDirective = scaffoldDirective,
    )
}

/** 把 Android 窗口状态换算成 Core WindowCapabilities 输入（Android 平台决策）。 */
internal fun buildCapabilities(
    windowAdaptiveInfo: WindowAdaptiveInfo,
    foldingFeatures: List<AospFoldFeatureInfo>,
    activePointerKind: PointerKind,
    keyboardVisible: Boolean,
): WindowCapabilitiesDto {
    val paneCount =
        when (windowAdaptiveInfo.windowSizeClass.windowWidthSizeClass) {
            WindowWidthSizeClass.COMPACT -> 1
            WindowWidthSizeClass.MEDIUM -> 2
            else -> 3
        }
    val pointerClass =
        when (activePointerKind) {
            PointerKind.Mouse, PointerKind.Trackpad -> PointerClassDto.MOUSE
            PointerKind.Stylus -> PointerClassDto.STYLUS
            PointerKind.Touch -> PointerClassDto.TOUCH
            else -> PointerClassDto.UNKNOWN
        }
    return WindowCapabilitiesDto(
        availablePaneCount = paneCount.toUByte(),
        hasSeparatingFold = foldingFeatures.any { it.isSeparating },
        pointerClass = pointerClass,
        keyboardVisible = keyboardVisible,
    )
}

/** Core WorkspacePaneMode → Material3 maxHorizontalPartitions。 */
internal fun maxHorizontalPartitionsFor(mode: WorkspacePaneModeDto): Int =
    when (mode) {
        WorkspacePaneModeDto.SINGLE_PANE -> 1
        WorkspacePaneModeDto.LIST_DETAIL -> 2
        else -> 3
    }

/** Android 一级导航呈现（#610：平台决策，不在 Core）。 */
internal enum class AndroidNavigationPresentation {
    BottomBar,
    NavigationRail,
}

/**
 * Android UI spec — AndroidAdaptiveLayoutPolicy 的最终输出。
 *
 * [contract] 是 Core presentation contract（产品壳层语义）；
 * 其余字段是 Android 平台自己的呈现决策。
 */
internal data class AndroidLayoutSpec(
    val contract: LayoutContractDto?,
    val navigationPresentation: AndroidNavigationPresentation,
    val scaffoldDirective: PaneScaffoldDirective,
)
