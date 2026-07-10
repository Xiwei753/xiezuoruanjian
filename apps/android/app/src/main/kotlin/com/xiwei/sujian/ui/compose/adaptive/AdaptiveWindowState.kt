package com.xiwei.sujian.ui.compose.adaptive

import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.calculateListDetailPaneScaffoldDirective
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.window.layout.DisplayFeature
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo
import kotlinx.coroutines.flow.StateFlow

data class AdaptiveWindowState(
    val scaffoldDirective: androidx.compose.material3.adaptive.PaneScaffoldDirective,
    val foldingFeatures: List<FoldingFeature> = emptyList()
)

@Composable
fun rememberAdaptiveWindowState(): AdaptiveWindowState {
    val windowAdaptiveInfo = currentWindowAdaptiveInfo()
    val directive = remember(windowAdaptiveInfo) {
        calculateListDetailPaneScaffoldDirective(windowAdaptiveInfo)
    }
    return remember(directive) {
        AdaptiveWindowState(
            scaffoldDirective = directive
        )
    }
}

data class AndroidFoldFeatureInfo(
    val state: FoldState,
    val orientation: FoldOrientation,
    val isSeparating: Boolean,
    val occlusionType: FoldOcclusionType,
    val boundsLeft: Int,
    val boundsTop: Int,
    val boundsRight: Int,
    val boundsBottom: Int
)

enum class FoldState {
    None, Flat, HalfOpened
}

enum class FoldOrientation {
    Horizontal, Vertical
}

enum class FoldOcclusionType {
    None, Full
}
