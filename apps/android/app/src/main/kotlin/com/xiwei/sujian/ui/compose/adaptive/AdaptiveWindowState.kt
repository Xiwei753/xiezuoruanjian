package com.xiwei.sujian.ui.compose.adaptive

import androidx.activity.ComponentActivity
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.window.layout.FoldingFeature

data class AdaptiveWindowState(
    val scaffoldDirective: PaneScaffoldDirective,
    val foldingFeatures: List<FoldingFeature> = emptyList()
)

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun rememberAdaptiveWindowState(): AdaptiveWindowState {
    val windowAdaptiveInfo = currentWindowAdaptiveInfo()
    val directive = remember(windowAdaptiveInfo) {
        calculatePaneScaffoldDirective(windowAdaptiveInfo)
    }

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

    return remember(directive, foldingFeatures) {
        AdaptiveWindowState(
            scaffoldDirective = directive,
            foldingFeatures = foldingFeatures
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
