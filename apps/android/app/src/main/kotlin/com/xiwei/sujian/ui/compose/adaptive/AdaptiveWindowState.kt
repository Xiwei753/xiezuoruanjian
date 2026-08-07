package com.xiwei.sujian.ui.compose.adaptive

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.model.AvoidRegionKind
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.WorkspacePaneMode

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun rememberCoreLayoutDirective(layoutPlan: LayoutPlan?): PaneScaffoldDirective {
    val windowAdaptiveInfo = androidx.compose.material3.adaptive.currentWindowAdaptiveInfo()
    val defaultDirective =
        remember(windowAdaptiveInfo) {
            calculatePaneScaffoldDirective(windowAdaptiveInfo)
        }
    val density = LocalDensity.current.density

    if (layoutPlan == null) return defaultDirective

    return remember(layoutPlan, density) {
        val maxHorizontalPartitions =
            when (layoutPlan.workspacePaneMode) {
                WorkspacePaneMode.SinglePane -> 1
                WorkspacePaneMode.ListDetail -> 2
                WorkspacePaneMode.ThreePane -> 3
            }

        val hingeBounds =
            layoutPlan.avoidRegions
                .filter { it.kind == AvoidRegionKind.VerticalHinge || it.kind == AvoidRegionKind.HorizontalHinge }
                .map { region ->
                    Rect(
                        left = region.leftDp * density,
                        top = region.topDp * density,
                        right = region.rightDp * density,
                        bottom = region.bottomDp * density,
                    )
                }

        val verticalHingeBounds =
            layoutPlan.avoidRegions
                .filter { it.kind == AvoidRegionKind.VerticalHinge }
                .map { region ->
                    Rect(
                        left = region.leftDp * density,
                        top = region.topDp * density,
                        right = region.rightDp * density,
                        bottom = region.bottomDp * density,
                    )
                }

        val hasHorizontalHinge = layoutPlan.avoidRegions.any { it.kind == AvoidRegionKind.HorizontalHinge }

        val excludedBounds =
            if (hasHorizontalHinge) {
                verticalHingeBounds
            } else {
                hingeBounds
            }

        val preferredWidth =
            if (layoutPlan.listPaneWidth.preferredDp > 0f) {
                layoutPlan.listPaneWidth.preferredDp.dp
            } else {
                defaultDirective.defaultPanePreferredWidth
            }

        defaultDirective.copy(
            maxHorizontalPartitions = maxHorizontalPartitions,
            defaultPanePreferredWidth = preferredWidth,
            excludedBounds = excludedBounds,
        )
    }
}

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
private fun calculatePaneScaffoldDirective(
    windowAdaptiveInfo: androidx.compose.material3.adaptive.WindowAdaptiveInfo,
): PaneScaffoldDirective {
    return androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective(windowAdaptiveInfo)
}
