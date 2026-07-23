package com.xiwei.sujian.ui.compose.adaptive

import androidx.activity.ComponentActivity
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.window.layout.FoldingFeature
import com.xiwei.sujian.model.AvoidRegionKind
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.WorkspacePaneMode
import com.xiwei.sujian.platform.api.FoldOrientation
import com.xiwei.sujian.platform.api.FoldPosture
import com.xiwei.sujian.platform.api.OcclusionType

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun rememberCoreLayoutDirective(layoutPlan: LayoutPlan?): PaneScaffoldDirective {
    val windowAdaptiveInfo = androidx.compose.material3.adaptive.currentWindowAdaptiveInfo()
    val defaultDirective = remember(windowAdaptiveInfo) {
        calculatePaneScaffoldDirective(windowAdaptiveInfo)
    }
    val density = LocalDensity.current.density

    if (layoutPlan == null) return defaultDirective

    return remember(layoutPlan, density) {
        val maxHorizontalPartitions = when (layoutPlan.workspacePaneMode) {
            WorkspacePaneMode.SinglePane -> 1
            WorkspacePaneMode.ListDetail -> 2
            WorkspacePaneMode.ThreePane -> 3
        }

        val hingeBounds = layoutPlan.avoidRegions
            .filter { it.kind == AvoidRegionKind.VerticalHinge || it.kind == AvoidRegionKind.HorizontalHinge }
            .map { region ->
                Rect(
                    left = region.leftDp * density,
                    top = region.topDp * density,
                    right = region.rightDp * density,
                    bottom = region.bottomDp * density
                )
            }

        val verticalHingeBounds = layoutPlan.avoidRegions
            .filter { it.kind == AvoidRegionKind.VerticalHinge }
            .map { region ->
                Rect(
                    left = region.leftDp * density,
                    top = region.topDp * density,
                    right = region.rightDp * density,
                    bottom = region.bottomDp * density
                )
            }

        val hasHorizontalHinge = layoutPlan.avoidRegions.any { it.kind == AvoidRegionKind.HorizontalHinge }

        val excludedBounds = if (hasHorizontalHinge) {
            verticalHingeBounds
        } else {
            hingeBounds
        }

        val preferredWidth = if (layoutPlan.listPaneWidth.preferredDp > 0f)
            layoutPlan.listPaneWidth.preferredDp.dp else defaultDirective.defaultPanePreferredWidth

        defaultDirective.copy(
            maxHorizontalPartitions = maxHorizontalPartitions,
            defaultPanePreferredWidth = preferredWidth,
            excludedBounds = excludedBounds
        )
    }
}

data class AdaptiveWindowState(
    val scaffoldDirective: PaneScaffoldDirective,
    val foldingFeatures: List<FoldingFeature> = emptyList()
)

@Composable
fun rememberAdaptiveWindowState(): AdaptiveWindowState {
    val activity = LocalContext.current as? ComponentActivity
    var foldingFeatures by remember { mutableStateOf<List<FoldingFeature>>(emptyList()) }

    if (activity != null) {
        val adapter = remember { AndroidAdaptiveWindowAdapter(activity) }
        DisposableEffect(adapter) {
            adapter.startCollecting { features ->
                foldingFeatures = features
            }
            onDispose {
                adapter.stopCollecting()
            }
        }
    }

    val windowAdaptiveInfo = androidx.compose.material3.adaptive.currentWindowAdaptiveInfo()
    val directive = remember(windowAdaptiveInfo) {
        calculatePaneScaffoldDirective(windowAdaptiveInfo)
    }

    return remember(directive, foldingFeatures) {
        AdaptiveWindowState(
            scaffoldDirective = directive,
            foldingFeatures = foldingFeatures
        )
    }
}

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
private fun calculatePaneScaffoldDirective(
    windowAdaptiveInfo: androidx.compose.material3.adaptive.WindowAdaptiveInfo
): PaneScaffoldDirective {
    return androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective(windowAdaptiveInfo)
}

data class AndroidFoldFeatureInfo(
    val state: FoldPosture,
    val orientation: FoldOrientation,
    val isSeparating: Boolean,
    val occlusionType: OcclusionType,
    val boundsLeft: Int,
    val boundsTop: Int,
    val boundsRight: Int,
    val boundsBottom: Int
)
